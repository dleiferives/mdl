use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{BlockId, BlockTarget, CoreProgram, CoreType, FunctionId, ValueId};

use super::MinecraftOptimizationLevel;
use super::analysis::{CoreEdge, CoreEdgeKind, FunctionSemanticInventory, SemanticInventory};
use super::assignment::{
    AssignedHomeId, FunctionHomeAssignment, HomeAssignment, PHYSICAL_TYPE_ORDER, ValueAssignment,
};
use super::transfer::{CopyLocation, ParallelCopyResolver, SymbolicMove};

/// Immutable transfers for every Core function, aligned with `FunctionId`.
#[derive(Clone, Debug)]
pub(crate) struct EdgeTransferPlan {
    level: MinecraftOptimizationLevel,
    functions: EntityVec<FunctionId, FunctionEdgeTransferPlan>,
}

/// Function-local ordered edges, allocated-block lookup, and edge scratch ownership.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionEdgeTransferPlan {
    block_transfers: Box<[Option<BlockTransfer>]>,
    edges: Box<[PlannedEdgeTransfer]>,
    temporaries: Box<[EdgeTemporaryDescriptor]>,
}

/// Edge indices owned by one reachable control-flow terminator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BlockTransfer {
    Jump {
        edge_index: usize,
    },
    Branch {
        then_edge_index: usize,
        else_edge_index: usize,
    },
}

/// One frozen semantic edge and its ordered physical copy schedule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlannedEdgeTransfer {
    source: BlockId,
    kind: CoreEdgeKind,
    destination: BlockId,
    steps: Box<[TransferStep]>,
}

/// One physical move implementing part of a simultaneous semantic edge assignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TransferStep {
    destination: TransferLocation,
    source: TransferLocation,
}

/// A function-local assigned home or an edge-owned temporary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransferLocation {
    Home(AssignedHomeId),
    Temporary(EdgeTemporaryKind),
}

/// One function-local edge temporary materialized by final plan flattening.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EdgeTemporaryDescriptor {
    kind: EdgeTemporaryKind,
}

/// Stable identity and naming class of one edge temporary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EdgeTemporaryKind {
    LegacyUntyped,
    Typed(CoreType),
}

#[derive(Clone, Copy, Debug, Default)]
struct TemporaryUsage {
    legacy: bool,
    boolean: bool,
    integer: bool,
}

#[derive(Clone, Copy, Debug)]
struct SemanticCopy {
    destination: AssignedHomeId,
    source: AssignedHomeId,
    ty: CoreType,
}

impl EdgeTransferPlan {
    /// Preserves the exact Stage 4 policy: resolve every edge's copies together.
    pub(crate) fn for_none(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        assignment: &HomeAssignment,
    ) -> Result<Self, EdgeTransferError> {
        Self::build(
            core,
            inventory,
            assignment,
            MinecraftOptimizationLevel::None,
        )
    }

    /// Groups retained Baseline copies in fixed `Bool`, then `I32` order.
    pub(crate) fn for_baseline(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        assignment: &HomeAssignment,
    ) -> Result<Self, EdgeTransferError> {
        Self::build(
            core,
            inventory,
            assignment,
            MinecraftOptimizationLevel::Baseline,
        )
    }

    fn build(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        assignment: &HomeAssignment,
        level: MinecraftOptimizationLevel,
    ) -> Result<Self, EdgeTransferError> {
        if !assignment.matches_level(level) {
            return Err(EdgeTransferError::AssignmentPolicyMismatch {
                expected: level,
                actual: assignment.level(),
            });
        }
        let mut functions = EntityVec::new();
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(EdgeTransferError::MissingDefinition { function })?;
            let semantic = inventory
                .function(function)
                .ok_or(EdgeTransferError::MissingInventory { function })?;
            let assigned = assignment
                .function(function)
                .ok_or(EdgeTransferError::MissingAssignment { function })?;
            let transfer =
                FunctionEdgeTransferPlan::build(function, body, semantic, assigned, level)?;
            let planned = functions
                .push(transfer)
                .map_err(|EntityLimitError| EdgeTransferError::EntityLimit)?;
            debug_assert_eq!(planned, function);
        }
        Ok(Self { level, functions })
    }

    pub(crate) const fn level(&self) -> MinecraftOptimizationLevel {
        self.level
    }

    pub(crate) fn matches_level(&self, level: MinecraftOptimizationLevel) -> bool {
        self.level == level
    }

    pub(crate) fn function(&self, function: FunctionId) -> Option<&FunctionEdgeTransferPlan> {
        self.functions.get(function)
    }

    pub(crate) fn len(&self) -> usize {
        self.functions.len()
    }

    #[cfg(test)]
    pub(crate) fn push_test_function(
        &mut self,
        function: FunctionEdgeTransferPlan,
    ) -> Result<FunctionId, EntityLimitError> {
        self.functions.push(function)
    }
}

impl FunctionEdgeTransferPlan {
    #[allow(
        clippy::too_many_lines,
        reason = "one authoritative layout-order walk pairs terminators with semantic edges and freezes each block slot atomically"
    )]
    fn build(
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        semantic: &FunctionSemanticInventory,
        assignment: &FunctionHomeAssignment,
        level: MinecraftOptimizationLevel,
    ) -> Result<Self, EdgeTransferError> {
        if assignment.abi().entry_block() != body.entry() {
            return Err(EdgeTransferError::AssignmentMismatch { function });
        }
        let home_types = assignment
            .homes()
            .map(|(_, home)| home.ty())
            .collect::<Vec<_>>();
        let mut resolver = ParallelCopyResolver::new();
        let mut usage = TemporaryUsage::default();
        let mut edges = Vec::with_capacity(semantic.edges().len());
        let mut edge_cursor = 0;
        let mut block_transfers = vec![None; body.block_counts().allocated];

        for block in semantic.reachable_blocks().iter().copied() {
            let data = body
                .block(block)
                .ok_or(EdgeTransferError::InvalidBlock { function, block })?;
            let terminator = data
                .terminator()
                .ok_or(EdgeTransferError::MissingTerminator { function, block })?;
            let transfer = match terminator.kind() {
                crate::ir::core::TerminatorKind::Jump(target) => {
                    let edge = checked_edge(
                        semantic.edges(),
                        &mut edge_cursor,
                        function,
                        block,
                        CoreEdgeKind::Jump,
                        target,
                    )?;
                    let edge_index = edges.len();
                    edges.push(plan_edge(
                        function,
                        body,
                        assignment,
                        &home_types,
                        edge,
                        level,
                        &mut resolver,
                        &mut usage,
                    )?);
                    Some(BlockTransfer::Jump { edge_index })
                }
                crate::ir::core::TerminatorKind::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    let then_edge = checked_edge(
                        semantic.edges(),
                        &mut edge_cursor,
                        function,
                        block,
                        CoreEdgeKind::Branch(super::analysis::BranchArm::Then),
                        then_target,
                    )?;
                    let then_edge_index = edges.len();
                    edges.push(plan_edge(
                        function,
                        body,
                        assignment,
                        &home_types,
                        then_edge,
                        level,
                        &mut resolver,
                        &mut usage,
                    )?);

                    let else_edge = checked_edge(
                        semantic.edges(),
                        &mut edge_cursor,
                        function,
                        block,
                        CoreEdgeKind::Branch(super::analysis::BranchArm::Else),
                        else_target,
                    )?;
                    let else_edge_index = edges.len();
                    edges.push(plan_edge(
                        function,
                        body,
                        assignment,
                        &home_types,
                        else_edge,
                        level,
                        &mut resolver,
                        &mut usage,
                    )?);
                    Some(BlockTransfer::Branch {
                        then_edge_index,
                        else_edge_index,
                    })
                }
                crate::ir::core::TerminatorKind::Return(_)
                | crate::ir::core::TerminatorKind::Unreachable => None,
            };
            if let Some(transfer) = transfer {
                set_block_transfer(&mut block_transfers, function, block, transfer)?;
            }
        }
        if edge_cursor != semantic.edges().len() {
            return Err(EdgeTransferError::SemanticEdgeMismatch {
                function,
                block: body.entry(),
            });
        }

        Ok(Self {
            block_transfers: block_transfers.into_boxed_slice(),
            edges: edges.into_boxed_slice(),
            temporaries: usage.descriptors(level),
        })
    }

    #[allow(
        dead_code,
        reason = "edge-transfer unit tests query individual block slots in addition to the production dense table"
    )]
    pub(crate) fn block_transfer(&self, block: BlockId) -> Option<&BlockTransfer> {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| self.block_transfers.get(index))
            .and_then(Option::as_ref)
    }

    pub(crate) fn block_slots(&self) -> &[Option<BlockTransfer>] {
        &self.block_transfers
    }

    pub(crate) fn edges(&self) -> &[PlannedEdgeTransfer] {
        &self.edges
    }

    pub(crate) fn temporaries(&self) -> &[EdgeTemporaryDescriptor] {
        &self.temporaries
    }
}

impl PlannedEdgeTransfer {
    pub(crate) const fn source(&self) -> BlockId {
        self.source
    }

    pub(crate) const fn kind(&self) -> CoreEdgeKind {
        self.kind
    }

    pub(crate) const fn destination(&self) -> BlockId {
        self.destination
    }

    pub(crate) fn steps(&self) -> &[TransferStep] {
        &self.steps
    }

    pub(crate) const fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

impl TransferStep {
    pub(crate) const fn destination(self) -> TransferLocation {
        self.destination
    }

    pub(crate) const fn source(self) -> TransferLocation {
        self.source
    }
}

impl EdgeTemporaryDescriptor {
    pub(crate) const fn kind(self) -> EdgeTemporaryKind {
        self.kind
    }
}

impl TemporaryUsage {
    fn mark(&mut self, kind: EdgeTemporaryKind) {
        match kind {
            EdgeTemporaryKind::LegacyUntyped => self.legacy = true,
            EdgeTemporaryKind::Typed(CoreType::Bool) => self.boolean = true,
            EdgeTemporaryKind::Typed(CoreType::I32) => self.integer = true,
        }
    }

    fn descriptors(self, level: MinecraftOptimizationLevel) -> Box<[EdgeTemporaryDescriptor]> {
        let mut descriptors = Vec::with_capacity(2);
        match level {
            MinecraftOptimizationLevel::None => {
                if self.legacy {
                    descriptors.push(EdgeTemporaryDescriptor {
                        kind: EdgeTemporaryKind::LegacyUntyped,
                    });
                }
            }
            MinecraftOptimizationLevel::Baseline => {
                if self.boolean {
                    descriptors.push(EdgeTemporaryDescriptor {
                        kind: EdgeTemporaryKind::Typed(CoreType::Bool),
                    });
                }
                if self.integer {
                    descriptors.push(EdgeTemporaryDescriptor {
                        kind: EdgeTemporaryKind::Typed(CoreType::I32),
                    });
                }
            }
        }
        descriptors.into_boxed_slice()
    }
}

fn checked_edge<'a>(
    edges: &'a [CoreEdge],
    cursor: &mut usize,
    function: FunctionId,
    block: BlockId,
    kind: CoreEdgeKind,
    target: &BlockTarget,
) -> Result<&'a CoreEdge, EdgeTransferError> {
    let edge = edges
        .get(*cursor)
        .ok_or(EdgeTransferError::SemanticEdgeMismatch { function, block })?;
    *cursor += 1;
    if edge.source() != block
        || edge.kind() != kind
        || edge.destination() != target.block()
        || edge.arguments() != target.arguments()
    {
        return Err(EdgeTransferError::SemanticEdgeMismatch { function, block });
    }
    Ok(edge)
}

#[allow(
    clippy::too_many_arguments,
    reason = "edge planning checks each explicit immutable phase input before freezing one record"
)]
fn plan_edge(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assignment: &FunctionHomeAssignment,
    home_types: &[CoreType],
    edge: &CoreEdge,
    level: MinecraftOptimizationLevel,
    resolver: &mut ParallelCopyResolver,
    usage: &mut TemporaryUsage,
) -> Result<PlannedEdgeTransfer, EdgeTransferError> {
    if edge.destination() == assignment.abi().entry_block() {
        return Err(EdgeTransferError::EdgeTargetsEntry {
            function,
            source: edge.source(),
        });
    }
    let destination = body
        .block(edge.destination())
        .ok_or(EdgeTransferError::InvalidBlock {
            function,
            block: edge.destination(),
        })?;
    if destination.parameters().len() != edge.arguments().len() {
        return Err(EdgeTransferError::InvalidEdgeShape {
            function,
            source: edge.source(),
        });
    }

    let mut copies = Vec::with_capacity(edge.arguments().len());
    for (parameter, source) in destination.parameters().iter().zip(edge.arguments()) {
        let destination_value = parameter.value();
        let destination_assignment = assignment.value_assignment(destination_value);
        if level == MinecraftOptimizationLevel::Baseline && destination_assignment.is_none() {
            continue;
        }
        let destination_assignment =
            destination_assignment.ok_or(EdgeTransferError::MissingValueAssignment {
                function,
                value: destination_value,
            })?;
        let source_assignment = assignment.value_assignment(*source).ok_or(
            EdgeTransferError::MissingValueAssignment {
                function,
                value: *source,
            },
        )?;
        validate_assignment(
            function,
            body,
            home_types,
            destination_value,
            destination_assignment,
        )?;
        validate_assignment(function, body, home_types, *source, source_assignment)?;
        if destination_assignment.ty() != source_assignment.ty() {
            return Err(EdgeTransferError::CopyTypeMismatch {
                function,
                destination: destination_value,
                source: *source,
            });
        }
        copies.push(SemanticCopy {
            destination: destination_assignment.home(),
            source: source_assignment.home(),
            ty: destination_assignment.ty(),
        });
    }

    let steps = match level {
        MinecraftOptimizationLevel::None => resolve_group(
            function,
            edge.source(),
            &copies,
            None,
            EdgeTemporaryKind::LegacyUntyped,
            home_types.len(),
            resolver,
            usage,
        )?,
        MinecraftOptimizationLevel::Baseline => {
            let mut steps = Vec::with_capacity(copies.len() + 2);
            for ty in PHYSICAL_TYPE_ORDER {
                steps.extend(resolve_group(
                    function,
                    edge.source(),
                    &copies,
                    Some(ty),
                    EdgeTemporaryKind::Typed(ty),
                    home_types.len(),
                    resolver,
                    usage,
                )?);
            }
            steps
        }
    };
    Ok(PlannedEdgeTransfer {
        source: edge.source(),
        kind: edge.kind(),
        destination: edge.destination(),
        steps: steps.into_boxed_slice(),
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the generic resolver needs explicit function/edge diagnostics and scratch ownership"
)]
fn resolve_group(
    function: FunctionId,
    block: BlockId,
    copies: &[SemanticCopy],
    ty: Option<CoreType>,
    temporary: EdgeTemporaryKind,
    home_count: usize,
    resolver: &mut ParallelCopyResolver,
    usage: &mut TemporaryUsage,
) -> Result<Vec<TransferStep>, EdgeTransferError> {
    let assignments = copies
        .iter()
        .filter(|copy| ty.is_none_or(|ty| copy.ty == ty))
        .map(|copy| (copy.destination, copy.source))
        .collect::<Vec<_>>();
    let symbolic = resolver
        .resolve(&assignments, home_count)
        .map_err(|_| EdgeTransferError::InvalidParallelCopy { function, block })?;
    Ok(freeze_steps(symbolic, temporary, usage))
}

fn freeze_steps(
    symbolic: Vec<SymbolicMove<AssignedHomeId>>,
    temporary: EdgeTemporaryKind,
    usage: &mut TemporaryUsage,
) -> Vec<TransferStep> {
    symbolic
        .into_iter()
        .map(|step| TransferStep {
            destination: freeze_location(step.destination, temporary, usage),
            source: freeze_location(step.source, temporary, usage),
        })
        .collect()
}

fn freeze_location(
    location: CopyLocation<AssignedHomeId>,
    temporary: EdgeTemporaryKind,
    usage: &mut TemporaryUsage,
) -> TransferLocation {
    match location {
        CopyLocation::Home(home) => TransferLocation::Home(home),
        CopyLocation::Scratch => {
            usage.mark(temporary);
            TransferLocation::Temporary(temporary)
        }
    }
}

fn validate_assignment(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    home_types: &[CoreType],
    value: ValueId,
    assignment: ValueAssignment,
) -> Result<(), EdgeTransferError> {
    let value_type = body
        .value(value)
        .ok_or(EdgeTransferError::InvalidValue { function, value })?
        .ty();
    let home_type = usize::try_from(assignment.home().index())
        .ok()
        .and_then(|index| home_types.get(index))
        .copied();
    if assignment.value() != value || assignment.ty() != value_type || home_type != Some(value_type)
    {
        return Err(EdgeTransferError::InvalidValueAssignment { function, value });
    }
    Ok(())
}

fn set_block_transfer(
    slots: &mut [Option<BlockTransfer>],
    function: FunctionId,
    block: BlockId,
    transfer: BlockTransfer,
) -> Result<(), EdgeTransferError> {
    let slot = usize::try_from(block.index())
        .ok()
        .and_then(|index| slots.get_mut(index))
        .ok_or(EdgeTransferError::InvalidBlock { function, block })?;
    if slot.is_some() {
        return Err(EdgeTransferError::DuplicateBlockTransfer { function, block });
    }
    *slot = Some(transfer);
    Ok(())
}

/// Checked failure while freezing function-local semantic edge copies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EdgeTransferError {
    AssignmentPolicyMismatch {
        expected: MinecraftOptimizationLevel,
        actual: MinecraftOptimizationLevel,
    },
    MissingDefinition {
        function: FunctionId,
    },
    MissingInventory {
        function: FunctionId,
    },
    MissingAssignment {
        function: FunctionId,
    },
    AssignmentMismatch {
        function: FunctionId,
    },
    InvalidBlock {
        function: FunctionId,
        block: BlockId,
    },
    MissingTerminator {
        function: FunctionId,
        block: BlockId,
    },
    SemanticEdgeMismatch {
        function: FunctionId,
        block: BlockId,
    },
    EdgeTargetsEntry {
        function: FunctionId,
        source: BlockId,
    },
    InvalidEdgeShape {
        function: FunctionId,
        source: BlockId,
    },
    MissingValueAssignment {
        function: FunctionId,
        value: ValueId,
    },
    InvalidValue {
        function: FunctionId,
        value: ValueId,
    },
    InvalidValueAssignment {
        function: FunctionId,
        value: ValueId,
    },
    CopyTypeMismatch {
        function: FunctionId,
        destination: ValueId,
        source: ValueId,
    },
    DuplicateBlockTransfer {
        function: FunctionId,
        block: BlockId,
    },
    InvalidParallelCopy {
        function: FunctionId,
        block: BlockId,
    },
    EntityLimit,
}

#[cfg(test)]
mod tests {
    use super::{
        BlockTransfer, EdgeTemporaryDescriptor, EdgeTemporaryKind, EdgeTransferError,
        EdgeTransferPlan, TransferLocation, TransferStep,
    };
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind, ValueId,
    };
    use crate::lower::minecraft::MinecraftOptimizationLevel;
    use crate::lower::minecraft::analysis::{BranchArm, CoreEdgeKind, SemanticInventory};
    use crate::lower::minecraft::assignment::{
        AssignedHomeId, FunctionHomeAssignment, HomeAssignment,
    };
    use crate::lower::minecraft::demand::{
        RuntimeDemand, RuntimeDemandLimits, RuntimeDemandTableLimit,
    };
    use crate::source::{OriginId, SourceContext};

    #[test]
    fn none_freezes_an_exact_acyclic_jump_in_allocated_block_slots() {
        let (core, function, entry, join, integer, boolean, joined_integer, joined_boolean) =
            acyclic_jump_program();
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let function_assignment = assignment.function(function).unwrap();
        let plan = EdgeTransferPlan::for_none(&core, &inventory, &assignment).unwrap();
        let planned = plan.function(function).unwrap();

        assert_eq!(planned.block_slots().len(), 2);
        assert_eq!(
            planned.block_transfer(entry),
            Some(&BlockTransfer::Jump { edge_index: 0 })
        );
        assert_eq!(planned.block_transfer(join), None);
        assert!(planned.temporaries().is_empty());
        assert_eq!(planned.edges().len(), 1);
        let edge = &planned.edges()[0];
        assert_eq!(edge.source(), entry);
        assert_eq!(edge.kind(), CoreEdgeKind::Jump);
        assert_eq!(edge.destination(), join);
        assert_eq!(
            edge.steps(),
            &[
                ordinary(
                    function_assignment,
                    joined_integer,
                    function_assignment,
                    integer,
                ),
                ordinary(
                    function_assignment,
                    joined_boolean,
                    function_assignment,
                    boolean,
                ),
            ]
        );
    }

    #[test]
    fn none_owns_one_legacy_temporary_for_an_exact_self_loop_cycle() {
        let (core, function, entry, loop_block, first, second) = i32_cycle_program();
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let assigned = assignment.function(function).unwrap();
        let plan = EdgeTransferPlan::for_none(&core, &inventory, &assignment).unwrap();
        let planned = plan.function(function).unwrap();
        let first_home = home(assigned, first);
        let second_home = home(assigned, second);

        assert_eq!(
            planned.temporaries(),
            &[EdgeTemporaryDescriptor {
                kind: EdgeTemporaryKind::LegacyUntyped,
            }]
        );
        assert_eq!(
            planned.block_transfer(entry),
            Some(&BlockTransfer::Jump { edge_index: 0 })
        );
        assert_eq!(
            planned.block_transfer(loop_block),
            Some(&BlockTransfer::Jump { edge_index: 1 })
        );
        assert_eq!(
            planned.edges()[1].steps(),
            &[
                TransferStep {
                    destination: TransferLocation::Temporary(EdgeTemporaryKind::LegacyUntyped,),
                    source: TransferLocation::Home(first_home),
                },
                TransferStep {
                    destination: TransferLocation::Home(first_home),
                    source: TransferLocation::Home(second_home),
                },
                TransferStep {
                    destination: TransferLocation::Home(second_home),
                    source: TransferLocation::Temporary(EdgeTemporaryKind::LegacyUntyped),
                },
            ]
        );
    }

    #[test]
    fn branch_arms_retain_then_else_semantic_order_and_exact_steps() {
        let (
            core,
            function,
            entry,
            then_block,
            else_block,
            then_value,
            else_value,
            then_parameter,
            else_parameter,
        ) = branch_program();
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let assigned = assignment.function(function).unwrap();
        let plan = EdgeTransferPlan::for_none(&core, &inventory, &assignment).unwrap();
        let planned = plan.function(function).unwrap();

        assert_eq!(
            planned.block_transfer(entry),
            Some(&BlockTransfer::Branch {
                then_edge_index: 0,
                else_edge_index: 1,
            })
        );
        assert_eq!(planned.block_transfer(then_block), None);
        assert_eq!(planned.block_transfer(else_block), None);
        assert_eq!(
            planned.edges()[0].kind(),
            CoreEdgeKind::Branch(BranchArm::Then)
        );
        assert_eq!(
            planned.edges()[1].kind(),
            CoreEdgeKind::Branch(BranchArm::Else)
        );
        assert_eq!(
            planned.edges()[0].steps(),
            &[ordinary(assigned, then_parameter, assigned, then_value)]
        );
        assert_eq!(
            planned.edges()[1].steps(),
            &[ordinary(assigned, else_parameter, assigned, else_value)]
        );
    }

    #[test]
    fn baseline_groups_bool_before_i32_and_owns_only_used_typed_temporaries() {
        let (core, function, bools, integers) = mixed_cycle_program();
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = baseline_all_reachable_assignment(&core, &inventory);
        let assigned = assignment.function(function).unwrap();
        let plan = EdgeTransferPlan::for_baseline(&core, &inventory, &assignment).unwrap();
        let planned = plan.function(function).unwrap();
        let cycle = &planned.edges()[1];

        assert_eq!(plan.level(), MinecraftOptimizationLevel::Baseline);
        assert!(plan.matches_level(MinecraftOptimizationLevel::Baseline));
        assert_eq!(
            planned.temporaries(),
            &[
                EdgeTemporaryDescriptor {
                    kind: EdgeTemporaryKind::Typed(CoreType::Bool),
                },
                EdgeTemporaryDescriptor {
                    kind: EdgeTemporaryKind::Typed(CoreType::I32),
                },
            ]
        );
        assert_eq!(cycle.steps().len(), 6);
        assert_eq!(
            cycle.steps()[0],
            TransferStep {
                destination: TransferLocation::Temporary(EdgeTemporaryKind::Typed(CoreType::Bool)),
                source: TransferLocation::Home(home(assigned, bools[0])),
            }
        );
        assert_eq!(
            cycle.steps()[3],
            TransferStep {
                destination: TransferLocation::Temporary(EdgeTemporaryKind::Typed(CoreType::I32)),
                source: TransferLocation::Home(home(assigned, integers[0])),
            }
        );
    }

    #[test]
    fn baseline_coalesces_the_demanded_copy_and_omits_the_undemanded_copy() {
        let (
            core,
            function,
            demanded_source,
            demanded_parameter,
            omitted_source,
            omitted_parameter,
        ) = partially_demanded_edge_program();
        let inventory = SemanticInventory::new(&core).unwrap();
        let demand = RuntimeDemand::for_level(
            &core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let assignment =
            HomeAssignment::for_baseline_derived_liveness(&core, &inventory, &demand).unwrap();
        let assigned = assignment.function(function).unwrap();
        let plan = EdgeTransferPlan::for_baseline(&core, &inventory, &assignment).unwrap();
        let planned = plan.function(function).unwrap();

        assert!(assigned.value_assignment(demanded_source).is_some());
        assert!(assigned.value_assignment(demanded_parameter).is_some());
        assert!(assigned.value_assignment(omitted_source).is_none());
        assert!(assigned.value_assignment(omitted_parameter).is_none());
        assert_eq!(
            home(assigned, demanded_parameter),
            home(assigned, demanded_source)
        );
        assert!(planned.edges()[0].steps().is_empty());
        assert!(planned.temporaries().is_empty());
    }

    #[test]
    fn mismatched_immutable_phase_inputs_fail_without_a_partial_plan() {
        let (source_core, ..) = acyclic_jump_program();
        let source_inventory = SemanticInventory::new(&source_core).unwrap();
        let (different_core, function, entry) = two_return_blocks_program();
        let different_inventory = SemanticInventory::new(&different_core).unwrap();
        let different_assignment =
            HomeAssignment::for_none(&different_core, &different_inventory).unwrap();

        assert_eq!(
            EdgeTransferPlan::for_none(&different_core, &source_inventory, &different_assignment,)
                .unwrap_err(),
            EdgeTransferError::SemanticEdgeMismatch {
                function,
                block: entry,
            }
        );
    }

    #[test]
    fn planning_is_deterministic_for_edges_blocks_and_temporaries() {
        let (core, ..) = mixed_cycle_program();
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = baseline_all_reachable_assignment(&core, &inventory);
        let first = EdgeTransferPlan::for_baseline(&core, &inventory, &assignment).unwrap();
        let second = EdgeTransferPlan::for_baseline(&core, &inventory, &assignment).unwrap();

        assert_eq!(format!("{first:#?}"), format!("{second:#?}"));
    }

    #[test]
    fn empty_function_rejects_cross_policy_assignment_without_role_inference() {
        let (core, _) = empty_return_program();
        let inventory = SemanticInventory::new(&core).unwrap();
        let none = HomeAssignment::for_none(&core, &inventory).unwrap();
        let baseline = baseline_all_reachable_assignment(&core, &inventory);

        assert_eq!(
            EdgeTransferPlan::for_baseline(&core, &inventory, &none).unwrap_err(),
            EdgeTransferError::AssignmentPolicyMismatch {
                expected: MinecraftOptimizationLevel::Baseline,
                actual: MinecraftOptimizationLevel::None,
            }
        );
        assert_eq!(
            EdgeTransferPlan::for_none(&core, &inventory, &baseline).unwrap_err(),
            EdgeTransferError::AssignmentPolicyMismatch {
                expected: MinecraftOptimizationLevel::None,
                actual: MinecraftOptimizationLevel::Baseline,
            }
        );
    }

    fn acyclic_jump_program() -> (
        CoreProgram,
        FunctionId,
        BlockId,
        BlockId,
        ValueId,
        ValueId,
        ValueId,
        ValueId,
    ) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("acyclic"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let integer = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let boolean = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined_integer = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let joined_boolean = builder
            .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![integer, boolean])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (
            core,
            function,
            entry,
            join,
            integer,
            boolean,
            joined_integer,
            joined_boolean,
        )
    }

    fn i32_cycle_program() -> (CoreProgram, FunctionId, BlockId, BlockId, ValueId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("cycle"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let initial_first = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let initial_second = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let loop_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first = builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let second = builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    loop_block,
                    vec![initial_first, initial_second],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(loop_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(loop_block, vec![second, first])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, entry, loop_block, first, second)
    }

    #[allow(
        clippy::type_complexity,
        reason = "the fixture returns each asserted semantic ID"
    )]
    fn branch_program() -> (
        CoreProgram,
        FunctionId,
        BlockId,
        BlockId,
        BlockId,
        ValueId,
        ValueId,
        ValueId,
        ValueId,
    ) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("branch"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let then_value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let else_value = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let then_parameter = builder
            .append_block_parameter(then_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_parameter = builder
            .append_block_parameter(else_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(then_block, vec![then_value]),
                    else_target: BlockTarget::new(else_block, vec![else_value]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for block in [then_block, else_block] {
            builder.switch_to_block(block).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (
            core,
            function,
            entry,
            then_block,
            else_block,
            then_value,
            else_value,
            then_parameter,
            else_parameter,
        )
    }

    fn mixed_cycle_program() -> (CoreProgram, FunctionId, [ValueId; 2], [ValueId; 2]) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("mixed-cycle"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let initial_bools = [
            builder.bool_constant(false, OriginId::UNKNOWN).unwrap(),
            builder.bool_constant(true, OriginId::UNKNOWN).unwrap(),
        ];
        let initial_integers = [
            builder.i32_constant(1, OriginId::UNKNOWN).unwrap(),
            builder.i32_constant(2, OriginId::UNKNOWN).unwrap(),
        ];
        let loop_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let bools = [
            builder
                .append_block_parameter(loop_block, CoreType::Bool, OriginId::UNKNOWN)
                .unwrap(),
            builder
                .append_block_parameter(loop_block, CoreType::Bool, OriginId::UNKNOWN)
                .unwrap(),
        ];
        let integers = [
            builder
                .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
                .unwrap(),
            builder
                .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
                .unwrap(),
        ];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    loop_block,
                    vec![
                        initial_bools[0],
                        initial_bools[1],
                        initial_integers[0],
                        initial_integers[1],
                    ],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(loop_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    loop_block,
                    vec![bools[1], bools[0], integers[1], integers[0]],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, bools, integers)
    }

    fn partially_demanded_edge_program()
    -> (CoreProgram, FunctionId, ValueId, ValueId, ValueId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("partial-edge"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let demanded_source = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let omitted_source = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let demanded_parameter = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let omitted_parameter = builder
            .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    join,
                    vec![demanded_source, omitted_source],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![demanded_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (
            core,
            function,
            demanded_source,
            demanded_parameter,
            omitted_source,
            omitted_parameter,
        )
    }

    fn two_return_blocks_program() -> (CoreProgram, FunctionId, BlockId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("different"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let other = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(other).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, entry)
    }

    fn empty_return_program() -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("empty"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    fn baseline_all_reachable_assignment(
        core: &CoreProgram,
        inventory: &SemanticInventory,
    ) -> HomeAssignment {
        let demand = RuntimeDemand::for_level(
            core,
            inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived().with_table_limit(RuntimeDemandTableLimit::new(0)),
        )
        .unwrap();
        HomeAssignment::for_baseline_derived_liveness(core, inventory, &demand).unwrap()
    }

    fn ordinary(
        destination_assignment: &FunctionHomeAssignment,
        destination: ValueId,
        source_assignment: &FunctionHomeAssignment,
        source: ValueId,
    ) -> TransferStep {
        TransferStep {
            destination: TransferLocation::Home(home(destination_assignment, destination)),
            source: TransferLocation::Home(home(source_assignment, source)),
        }
    }

    fn home(assignment: &FunctionHomeAssignment, value: ValueId) -> AssignedHomeId {
        assignment.value_assignment(value).unwrap().home()
    }
}
