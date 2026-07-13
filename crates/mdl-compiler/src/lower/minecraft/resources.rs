//! Immutable ownership and naming of generated Minecraft function resources.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{BlockId, CoreProgram, FunctionId};
use crate::ir::minecraft::FunctionResourceId;
use crate::source::OriginId;

use super::analysis::{CoreEdgeKind, SemanticInventory};
use super::edge_transfer::EdgeTransferPlan;
use super::plan::{BranchEdge, PlannedFunctionId, PlannedFunctionRole};
use super::{GeneratedNames, LoweringOptions, MinecraftOptimizationLevel};

const SCAFFOLDING_RESOURCE_COUNT: u64 = 2;
const MAX_PLANNED_FUNCTION_COUNT: u64 = u32::MAX as u64 + 1;

/// Immutable identity, target resource, provenance, and ownership for all generated
/// Minecraft functions.
#[derive(Clone, Debug)]
pub(crate) struct ResourceInventory {
    level: MinecraftOptimizationLevel,
    load: PlannedFunctionId,
    init_try_create: PlannedFunctionId,
    planned_functions: EntityVec<PlannedFunctionId, ResourceFunction>,
    functions: EntityVec<FunctionId, FunctionResourceInventory>,
}

/// One generated function resource in final global `PlannedFunctionId` order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResourceFunction {
    resource: FunctionResourceId,
    origin: OriginId,
    role: PlannedFunctionRole,
}

/// Dense resource lookups owned by one Core function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionResourceInventory {
    block_functions: Box<[Option<PlannedFunctionId>]>,
    branch_helpers: Box<[Option<PlannedFunctionId>]>,
}

impl ResourceInventory {
    /// Allocates every Stage 5E function resource after edge transfers have frozen.
    ///
    /// Construction and local verification are expected-linear in Core functions,
    /// allocated block slots, semantic edges, and generated resources. Hash-table
    /// iteration never determines an ID or output order; hashing is used only for
    /// resource-name uniqueness membership.
    pub(crate) fn new(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        transfers: &EdgeTransferPlan,
        options: &LoweringOptions,
    ) -> Result<Self, ResourceInventoryError> {
        let level = options.optimization_level();
        if !transfers.matches_level(level) {
            return Err(ResourceInventoryError::OptimizationLevelMismatch {
                expected: level,
                actual: transfers.level(),
            });
        }
        validate_prerequisite_function_count(
            ResourcePrerequisite::SemanticInventory,
            core.len(),
            inventory.len(),
        )?;
        validate_prerequisite_function_count(
            ResourcePrerequisite::EdgeTransferPlan,
            core.len(),
            transfers.len(),
        )?;
        let required = required_resource_count(core, inventory, transfers)?;
        validate_resource_capacity(required)?;

        let mut builder = ResourceInventoryBuilder::new(options.generated_names());
        let load = builder.allocate(PlannedFunctionRole::Load, OriginId::UNKNOWN)?;
        let init_try_create =
            builder.allocate(PlannedFunctionRole::InitTryCreate, OriginId::UNKNOWN)?;
        let mut functions = EntityVec::new();

        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(ResourceInventoryError::MissingDefinition { function })?;
            let semantic = inventory
                .function(function)
                .ok_or(ResourceInventoryError::MissingInventory { function })?;
            let transfer = transfers
                .function(function)
                .ok_or(ResourceInventoryError::MissingTransferPlan { function })?;
            validate_edge_alignment(function, semantic.edges(), transfer.edges())?;

            let mut block_functions = vec![None; body.block_counts().allocated];
            for block in semantic.reachable_blocks().iter().copied() {
                let origin = body
                    .block(block)
                    .ok_or(ResourceInventoryError::InvalidBlock { function, block })?
                    .origin();
                let planned =
                    builder.allocate(PlannedFunctionRole::Block { function, block }, origin)?;
                set_block_resource(&mut block_functions, function, block, planned)?;
            }

            let mut branch_helpers = vec![None; transfer.edges().len()];
            for (edge_index, edge) in transfer.edges().iter().enumerate() {
                let CoreEdgeKind::Branch(arm) = edge.kind() else {
                    continue;
                };
                if edge.is_empty() {
                    continue;
                }
                let source = edge.source();
                let origin = body
                    .block(source)
                    .ok_or(ResourceInventoryError::InvalidBlock {
                        function,
                        block: source,
                    })?
                    .terminator()
                    .ok_or(ResourceInventoryError::MissingTerminator {
                        function,
                        block: source,
                    })?
                    .origin();
                let planned = builder.allocate(
                    PlannedFunctionRole::BranchHelper {
                        function,
                        edge: BranchEdge::new(source, arm),
                    },
                    origin,
                )?;
                set_branch_helper(&mut branch_helpers, function, edge_index, planned)?;
            }

            let assigned = functions
                .push(FunctionResourceInventory {
                    block_functions: block_functions.into_boxed_slice(),
                    branch_helpers: branch_helpers.into_boxed_slice(),
                })
                .map_err(|EntityLimitError| ResourceInventoryError::EntityLimit)?;
            if assigned != function {
                return Err(ResourceInventoryError::FunctionAlignment { function });
            }
        }

        let result = Self {
            level,
            load,
            init_try_create,
            planned_functions: builder.finish(),
            functions,
        };
        if usize_to_u64(result.planned_functions.len()) != Some(required) {
            return Err(ResourceInventoryError::ResourceCountMismatch {
                expected: required,
                actual: result.planned_functions.len(),
            });
        }
        result.validate(core, inventory, transfers, options)?;
        Ok(result)
    }

    pub(crate) const fn level(&self) -> MinecraftOptimizationLevel {
        self.level
    }

    pub(crate) fn matches_level(&self, level: MinecraftOptimizationLevel) -> bool {
        self.level == level
    }

    pub(crate) const fn load(&self) -> PlannedFunctionId {
        self.load
    }

    pub(crate) const fn init_try_create(&self) -> PlannedFunctionId {
        self.init_try_create
    }

    pub(crate) fn planned_function(
        &self,
        function: PlannedFunctionId,
    ) -> Option<&ResourceFunction> {
        self.planned_functions.get(function)
    }

    pub(crate) fn planned_functions(
        &self,
    ) -> impl ExactSizeIterator<Item = (PlannedFunctionId, &ResourceFunction)> + '_ {
        self.planned_functions.iter()
    }

    pub(crate) fn len(&self) -> usize {
        self.functions.len()
    }

    pub(crate) fn function(&self, function: FunctionId) -> Option<&FunctionResourceInventory> {
        self.functions.get(function)
    }

    #[allow(
        dead_code,
        reason = "resource unit tests query one block identity while production assembly consumes dense slots"
    )]
    pub(crate) fn block_function(
        &self,
        function: FunctionId,
        block: BlockId,
    ) -> Option<PlannedFunctionId> {
        self.function(function)?.block_function(block)
    }

    #[allow(
        dead_code,
        reason = "resource unit tests query one helper identity while production assembly consumes function records"
    )]
    pub(crate) fn branch_helper(
        &self,
        function: FunctionId,
        edge_index: usize,
    ) -> Option<PlannedFunctionId> {
        self.function(function)?.branch_helper(edge_index)
    }

    fn validate(
        &self,
        core: &CoreProgram,
        inventory: &SemanticInventory,
        transfers: &EdgeTransferPlan,
        options: &LoweringOptions,
    ) -> Result<(), ResourceInventoryError> {
        if !self.matches_level(options.optimization_level()) || !transfers.matches_level(self.level)
        {
            return Err(ResourceInventoryError::OptimizationLevelMismatch {
                expected: options.optimization_level(),
                actual: self.level,
            });
        }
        if self.functions.len() != core.len() {
            return Err(ResourceInventoryError::FunctionCountMismatch {
                expected: core.len(),
                actual: self.functions.len(),
            });
        }
        validate_prerequisite_function_count(
            ResourcePrerequisite::SemanticInventory,
            core.len(),
            inventory.len(),
        )?;
        validate_prerequisite_function_count(
            ResourcePrerequisite::EdgeTransferPlan,
            core.len(),
            transfers.len(),
        )?;

        let names = options.generated_names();
        let mut resources = HashMap::with_capacity(self.planned_functions.len());
        for (planned, record) in self.planned_functions.iter() {
            if let Some(first) = resources.insert(record.resource.clone(), planned) {
                return Err(ResourceInventoryError::DuplicateResource {
                    first,
                    second: planned,
                    resource: record.resource.clone(),
                });
            }
        }
        let mut owners = vec![0_u8; self.planned_functions.len()];
        self.validate_owner(
            self.load,
            PlannedFunctionRole::Load,
            OriginId::UNKNOWN,
            names.load_function(),
            &mut owners,
        )?;
        self.validate_owner(
            self.init_try_create,
            PlannedFunctionRole::InitTryCreate,
            OriginId::UNKNOWN,
            names.init_try_create_function(),
            &mut owners,
        )?;

        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(ResourceInventoryError::MissingDefinition { function })?;
            let semantic = inventory
                .function(function)
                .ok_or(ResourceInventoryError::MissingInventory { function })?;
            let transfer = transfers
                .function(function)
                .ok_or(ResourceInventoryError::MissingTransferPlan { function })?;
            self.validate_function(function, body, semantic, transfer, names, &mut owners)?;
        }

        for (index, owners) in owners.into_iter().enumerate() {
            let planned = indexed_planned_function(index)?;
            match owners {
                1 => {}
                0 => return Err(ResourceInventoryError::MissingOwnership { planned }),
                count => {
                    return Err(ResourceInventoryError::DuplicateOwnership { planned, count });
                }
            }
        }
        Ok(())
    }

    fn validate_function(
        &self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        semantic: &super::analysis::FunctionSemanticInventory,
        transfer: &super::edge_transfer::FunctionEdgeTransferPlan,
        names: GeneratedNames<'_>,
        owners: &mut [u8],
    ) -> Result<(), ResourceInventoryError> {
        let function_resources = self
            .function(function)
            .ok_or(ResourceInventoryError::MissingFunctionResources { function })?;
        if function_resources.block_functions.len() != body.block_counts().allocated {
            return Err(ResourceInventoryError::BlockSlotCountMismatch {
                function,
                expected: body.block_counts().allocated,
                actual: function_resources.block_functions.len(),
            });
        }
        validate_edge_alignment(function, semantic.edges(), transfer.edges())?;
        if function_resources.branch_helpers.len() != transfer.edges().len() {
            return Err(ResourceInventoryError::HelperSlotCountMismatch {
                function,
                expected: transfer.edges().len(),
                actual: function_resources.branch_helpers.len(),
            });
        }

        for (block_index, planned) in function_resources
            .block_functions
            .iter()
            .copied()
            .enumerate()
        {
            let block = indexed_block(block_index)?;
            let expected = semantic.contains_block(block);
            if expected != planned.is_some() {
                return Err(ResourceInventoryError::BlockOwnershipMismatch { function, block });
            }
            if let Some(planned) = planned {
                let origin = body
                    .block(block)
                    .ok_or(ResourceInventoryError::InvalidBlock { function, block })?
                    .origin();
                self.validate_owner(
                    planned,
                    PlannedFunctionRole::Block { function, block },
                    origin,
                    names.block_function(function, block),
                    owners,
                )?;
            }
        }

        for (edge_index, (edge, helper)) in transfer
            .edges()
            .iter()
            .zip(function_resources.branch_helpers.iter().copied())
            .enumerate()
        {
            let arm = match edge.kind() {
                CoreEdgeKind::Jump => None,
                CoreEdgeKind::Branch(_) if edge.is_empty() => None,
                CoreEdgeKind::Branch(arm) => Some(arm),
            };
            if arm.is_some() != helper.is_some() {
                return Err(ResourceInventoryError::HelperOwnershipMismatch {
                    function,
                    edge_index,
                });
            }
            if let (Some(arm), Some(planned)) = (arm, helper) {
                let source = edge.source();
                let origin = body
                    .block(source)
                    .ok_or(ResourceInventoryError::InvalidBlock {
                        function,
                        block: source,
                    })?
                    .terminator()
                    .ok_or(ResourceInventoryError::MissingTerminator {
                        function,
                        block: source,
                    })?
                    .origin();
                self.validate_owner(
                    planned,
                    PlannedFunctionRole::BranchHelper {
                        function,
                        edge: BranchEdge::new(source, arm),
                    },
                    origin,
                    names.branch_helper_function(function, source, arm),
                    owners,
                )?;
            }
        }
        Ok(())
    }

    fn validate_owner(
        &self,
        planned: PlannedFunctionId,
        expected_role: PlannedFunctionRole,
        expected_origin: OriginId,
        expected_resource: FunctionResourceId,
        owners: &mut [u8],
    ) -> Result<(), ResourceInventoryError> {
        let record = self
            .planned_function(planned)
            .ok_or(ResourceInventoryError::InvalidPlannedFunction { planned })?;
        if record.role != expected_role {
            return Err(ResourceInventoryError::RoleMismatch {
                planned,
                expected: expected_role,
                actual: record.role,
            });
        }
        if record.origin != expected_origin {
            return Err(ResourceInventoryError::OriginMismatch {
                planned,
                expected: expected_origin,
                actual: record.origin,
            });
        }
        if record.resource != expected_resource {
            return Err(ResourceInventoryError::ResourceMismatch {
                planned,
                expected: expected_resource,
                actual: record.resource.clone(),
            });
        }
        let count = usize::try_from(planned.index())
            .ok()
            .and_then(|index| owners.get_mut(index))
            .ok_or(ResourceInventoryError::InvalidPlannedFunction { planned })?;
        *count = count
            .checked_add(1)
            .ok_or(ResourceInventoryError::OwnershipCountOverflow { planned })?;
        Ok(())
    }
}

impl ResourceFunction {
    pub(crate) const fn resource(&self) -> &FunctionResourceId {
        &self.resource
    }

    pub(crate) const fn origin(&self) -> OriginId {
        self.origin
    }

    pub(crate) const fn role(&self) -> PlannedFunctionRole {
        self.role
    }
}

impl FunctionResourceInventory {
    #[allow(
        dead_code,
        reason = "resource unit tests query individual block identities in addition to dense production slots"
    )]
    pub(crate) fn block_function(&self, block: BlockId) -> Option<PlannedFunctionId> {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| self.block_functions.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn branch_helper(&self, edge_index: usize) -> Option<PlannedFunctionId> {
        self.branch_helpers.get(edge_index).copied().flatten()
    }

    pub(crate) fn block_slots(&self) -> &[Option<PlannedFunctionId>] {
        &self.block_functions
    }

    pub(crate) fn branch_helper_slots(&self) -> &[Option<PlannedFunctionId>] {
        &self.branch_helpers
    }
}

struct ResourceInventoryBuilder<'a> {
    names: GeneratedNames<'a>,
    planned_functions: EntityVec<PlannedFunctionId, ResourceFunction>,
}

impl<'a> ResourceInventoryBuilder<'a> {
    fn new(names: GeneratedNames<'a>) -> Self {
        Self {
            names,
            planned_functions: EntityVec::new(),
        }
    }

    fn allocate(
        &mut self,
        role: PlannedFunctionRole,
        origin: OriginId,
    ) -> Result<PlannedFunctionId, ResourceInventoryError> {
        let resource = match role {
            PlannedFunctionRole::Load => self.names.load_function(),
            PlannedFunctionRole::InitTryCreate => self.names.init_try_create_function(),
            PlannedFunctionRole::Block { function, block } => {
                self.names.block_function(function, block)
            }
            PlannedFunctionRole::BranchHelper { function, edge } => self
                .names
                .branch_helper_function(function, edge.source(), edge.arm()),
        };
        let planned = self
            .planned_functions
            .push(ResourceFunction {
                resource: resource.clone(),
                origin,
                role,
            })
            .map_err(|EntityLimitError| ResourceInventoryError::EntityLimit)?;
        Ok(planned)
    }

    fn finish(self) -> EntityVec<PlannedFunctionId, ResourceFunction> {
        self.planned_functions
    }
}

fn required_resource_count(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    transfers: &EdgeTransferPlan,
) -> Result<u64, ResourceInventoryError> {
    let mut required = SCAFFOLDING_RESOURCE_COUNT;
    for (function, declaration) in core.functions() {
        declaration
            .body()
            .ok_or(ResourceInventoryError::MissingDefinition { function })?;
        let semantic = inventory
            .function(function)
            .ok_or(ResourceInventoryError::MissingInventory { function })?;
        let transfer = transfers
            .function(function)
            .ok_or(ResourceInventoryError::MissingTransferPlan { function })?;
        validate_edge_alignment(function, semantic.edges(), transfer.edges())?;
        let helpers = transfer
            .edges()
            .iter()
            .filter(|edge| matches!(edge.kind(), CoreEdgeKind::Branch(_)) && !edge.is_empty())
            .count();
        required = checked_resource_count(required, semantic.reachable_blocks().len(), helpers)?;
    }
    Ok(required)
}

fn checked_resource_count(
    current: u64,
    blocks: usize,
    helpers: usize,
) -> Result<u64, ResourceInventoryError> {
    let blocks = usize_to_u64(blocks).ok_or(ResourceInventoryError::CapacityOverflow)?;
    let helpers = usize_to_u64(helpers).ok_or(ResourceInventoryError::CapacityOverflow)?;
    current
        .checked_add(blocks)
        .and_then(|count| count.checked_add(helpers))
        .ok_or(ResourceInventoryError::CapacityOverflow)
}

fn validate_resource_capacity(required: u64) -> Result<(), ResourceInventoryError> {
    let maximum = maximum_resource_count();
    if required > maximum {
        Err(ResourceInventoryError::CapacityExceeded { required, maximum })
    } else {
        Ok(())
    }
}

fn maximum_resource_count() -> u64 {
    MAX_PLANNED_FUNCTION_COUNT.min(u64::try_from(usize::MAX).unwrap_or(u64::MAX))
}

fn validate_prerequisite_function_count(
    prerequisite: ResourcePrerequisite,
    expected: usize,
    actual: usize,
) -> Result<(), ResourceInventoryError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ResourceInventoryError::PrerequisiteFunctionCountMismatch {
            prerequisite,
            expected,
            actual,
        })
    }
}

fn validate_edge_alignment(
    function: FunctionId,
    semantic: &[super::analysis::CoreEdge],
    transfers: &[super::edge_transfer::PlannedEdgeTransfer],
) -> Result<(), ResourceInventoryError> {
    if semantic.len() != transfers.len() {
        return Err(ResourceInventoryError::EdgeCountMismatch {
            function,
            expected: semantic.len(),
            actual: transfers.len(),
        });
    }
    for (edge_index, (semantic, transfer)) in semantic.iter().zip(transfers).enumerate() {
        if semantic.source() != transfer.source()
            || semantic.kind() != transfer.kind()
            || semantic.destination() != transfer.destination()
        {
            return Err(ResourceInventoryError::EdgeMismatch {
                function,
                edge_index,
            });
        }
    }
    Ok(())
}

fn set_block_resource(
    slots: &mut [Option<PlannedFunctionId>],
    function: FunctionId,
    block: BlockId,
    planned: PlannedFunctionId,
) -> Result<(), ResourceInventoryError> {
    let slot = usize::try_from(block.index())
        .ok()
        .and_then(|index| slots.get_mut(index))
        .ok_or(ResourceInventoryError::InvalidBlock { function, block })?;
    if slot.is_some() {
        return Err(ResourceInventoryError::DuplicateBlockResource { function, block });
    }
    *slot = Some(planned);
    Ok(())
}

fn set_branch_helper(
    slots: &mut [Option<PlannedFunctionId>],
    function: FunctionId,
    edge_index: usize,
    planned: PlannedFunctionId,
) -> Result<(), ResourceInventoryError> {
    let slot = slots
        .get_mut(edge_index)
        .ok_or(ResourceInventoryError::InvalidEdgeIndex {
            function,
            edge_index,
        })?;
    if slot.is_some() {
        return Err(ResourceInventoryError::DuplicateBranchHelper {
            function,
            edge_index,
        });
    }
    *slot = Some(planned);
    Ok(())
}

fn indexed_block(index: usize) -> Result<BlockId, ResourceInventoryError> {
    u32::try_from(index)
        .map(BlockId::from_index)
        .map_err(|_| ResourceInventoryError::CapacityOverflow)
}

fn indexed_planned_function(index: usize) -> Result<PlannedFunctionId, ResourceInventoryError> {
    u32::try_from(index)
        .map(PlannedFunctionId::from_index)
        .map_err(|_| ResourceInventoryError::CapacityOverflow)
}

fn usize_to_u64(value: usize) -> Option<u64> {
    u64::try_from(value).ok()
}

/// Typed failure while assigning generated function resources and ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResourcePrerequisite {
    SemanticInventory,
    EdgeTransferPlan,
}

/// Typed failure while assigning generated function resources and ownership.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResourceInventoryError {
    OptimizationLevelMismatch {
        expected: MinecraftOptimizationLevel,
        actual: MinecraftOptimizationLevel,
    },
    MissingDefinition {
        function: FunctionId,
    },
    MissingInventory {
        function: FunctionId,
    },
    MissingTransferPlan {
        function: FunctionId,
    },
    MissingFunctionResources {
        function: FunctionId,
    },
    FunctionAlignment {
        function: FunctionId,
    },
    FunctionCountMismatch {
        expected: usize,
        actual: usize,
    },
    PrerequisiteFunctionCountMismatch {
        prerequisite: ResourcePrerequisite,
        expected: usize,
        actual: usize,
    },
    EdgeCountMismatch {
        function: FunctionId,
        expected: usize,
        actual: usize,
    },
    EdgeMismatch {
        function: FunctionId,
        edge_index: usize,
    },
    InvalidEdgeIndex {
        function: FunctionId,
        edge_index: usize,
    },
    InvalidBlock {
        function: FunctionId,
        block: BlockId,
    },
    MissingTerminator {
        function: FunctionId,
        block: BlockId,
    },
    BlockSlotCountMismatch {
        function: FunctionId,
        expected: usize,
        actual: usize,
    },
    HelperSlotCountMismatch {
        function: FunctionId,
        expected: usize,
        actual: usize,
    },
    DuplicateBlockResource {
        function: FunctionId,
        block: BlockId,
    },
    DuplicateBranchHelper {
        function: FunctionId,
        edge_index: usize,
    },
    BlockOwnershipMismatch {
        function: FunctionId,
        block: BlockId,
    },
    HelperOwnershipMismatch {
        function: FunctionId,
        edge_index: usize,
    },
    InvalidPlannedFunction {
        planned: PlannedFunctionId,
    },
    RoleMismatch {
        planned: PlannedFunctionId,
        expected: PlannedFunctionRole,
        actual: PlannedFunctionRole,
    },
    OriginMismatch {
        planned: PlannedFunctionId,
        expected: OriginId,
        actual: OriginId,
    },
    ResourceMismatch {
        planned: PlannedFunctionId,
        expected: FunctionResourceId,
        actual: FunctionResourceId,
    },
    DuplicateResource {
        first: PlannedFunctionId,
        second: PlannedFunctionId,
        resource: FunctionResourceId,
    },
    MissingOwnership {
        planned: PlannedFunctionId,
    },
    DuplicateOwnership {
        planned: PlannedFunctionId,
        count: u8,
    },
    OwnershipCountOverflow {
        planned: PlannedFunctionId,
    },
    ResourceCountMismatch {
        expected: u64,
        actual: usize,
    },
    CapacityOverflow,
    CapacityExceeded {
        required: u64,
        maximum: u64,
    },
    EntityLimit,
}

impl fmt::Display for ResourceInventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ResourceInventoryError {}

#[cfg(test)]
mod tests {
    use super::{
        ResourceInventory, ResourceInventoryError, ResourcePrerequisite, checked_resource_count,
        maximum_resource_count, validate_prerequisite_function_count, validate_resource_capacity,
    };
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::lower::minecraft::analysis::{BranchArm, SemanticInventory};
    use crate::lower::minecraft::assignment::HomeAssignment;
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::lower::minecraft::edge_transfer::EdgeTransferPlan;
    use crate::lower::minecraft::plan::{BranchEdge, PlannedFunctionId, PlannedFunctionRole};
    use crate::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    struct MultiFunctionFixture {
        core: CoreProgram,
        first: FunctionId,
        first_entry: BlockId,
        first_moved: BlockId,
        first_empty: BlockId,
        first_detached_from_cfg: BlockId,
        first_branch_origin: OriginId,
        second: FunctionId,
        second_entry: BlockId,
        second_join: BlockId,
    }

    fn options(level: MinecraftOptimizationLevel) -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl_test").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
        .with_optimization_level(level)
    }

    fn edge_plan(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        level: MinecraftOptimizationLevel,
    ) -> EdgeTransferPlan {
        match level {
            MinecraftOptimizationLevel::None => {
                let assignment = HomeAssignment::for_none(core, inventory).unwrap();
                EdgeTransferPlan::for_none(core, inventory, &assignment).unwrap()
            }
            MinecraftOptimizationLevel::Baseline => {
                let demand = RuntimeDemand::for_level(
                    core,
                    inventory,
                    level,
                    RuntimeDemandLimits::derived(),
                )
                .unwrap();
                let assignment =
                    HomeAssignment::for_baseline_derived_liveness(core, inventory, &demand)
                        .unwrap();
                EdgeTransferPlan::for_baseline(core, inventory, &assignment).unwrap()
            }
        }
    }

    fn fresh_origin(sources: &mut SourceContext) -> OriginId {
        sources.add_origin(Origin::Unknown).unwrap()
    }

    fn multi_function_fixture() -> MultiFunctionFixture {
        let mut sources = SourceContext::new();
        let first_origin = fresh_origin(&mut sources);
        let first_moved_origin = fresh_origin(&mut sources);
        let first_empty_origin = fresh_origin(&mut sources);
        let first_dead_origin = fresh_origin(&mut sources);
        let first_branch_origin = fresh_origin(&mut sources);
        let second_origin = fresh_origin(&mut sources);
        let second_join_origin = fresh_origin(&mut sources);

        let mut core = CoreProgram::new();
        let first = core
            .declare_function(Some("first"), vec![], vec![CoreType::I32], first_origin)
            .unwrap();
        let second = core
            .declare_function(Some("second"), vec![], vec![CoreType::I32], second_origin)
            .unwrap();

        let mut first_builder = FunctionBuilder::new(&core, &sources, first).unwrap();
        let first_entry = first_builder.entry_block();
        let condition = first_builder.bool_constant(true, first_origin).unwrap();
        let source = first_builder.i32_constant(7, first_origin).unwrap();
        let first_moved = first_builder.create_block(first_moved_origin).unwrap();
        let moved_parameter = first_builder
            .append_block_parameter(first_moved, CoreType::I32, first_moved_origin)
            .unwrap();
        let first_empty = first_builder.create_block(first_empty_origin).unwrap();
        let first_detached_from_cfg = first_builder.create_block(first_dead_origin).unwrap();
        first_builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(first_moved, vec![source]),
                    else_target: BlockTarget::new(first_empty, vec![]),
                },
                first_branch_origin,
            ))
            .unwrap();
        first_builder.switch_to_block(first_moved).unwrap();
        first_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![moved_parameter]),
                first_moved_origin,
            ))
            .unwrap();
        first_builder.switch_to_block(first_empty).unwrap();
        first_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![source]),
                first_empty_origin,
            ))
            .unwrap();
        first_builder
            .switch_to_block(first_detached_from_cfg)
            .unwrap();
        let dead = first_builder.i32_constant(99, first_dead_origin).unwrap();
        first_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![dead]),
                first_dead_origin,
            ))
            .unwrap();
        core.define_function(first, first_builder.finish().unwrap())
            .unwrap();

        let mut second_builder = FunctionBuilder::new(&core, &sources, second).unwrap();
        let second_entry = second_builder.entry_block();
        let second_source = second_builder.i32_constant(11, second_origin).unwrap();
        let second_join = second_builder.create_block(second_join_origin).unwrap();
        let second_parameter = second_builder
            .append_block_parameter(second_join, CoreType::I32, second_join_origin)
            .unwrap();
        second_builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(second_join, vec![second_source])),
                second_origin,
            ))
            .unwrap();
        second_builder.switch_to_block(second_join).unwrap();
        second_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![second_parameter]),
                second_join_origin,
            ))
            .unwrap();
        core.define_function(second, second_builder.finish().unwrap())
            .unwrap();

        MultiFunctionFixture {
            core,
            first,
            first_entry,
            first_moved,
            first_empty,
            first_detached_from_cfg,
            first_branch_origin,
            second,
            second_entry,
            second_join,
        }
    }

    #[test]
    fn preserves_none_order_and_removes_a_fully_coalesced_baseline_helper() {
        let fixture = multi_function_fixture();
        for level in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let inventory = SemanticInventory::new(&fixture.core).unwrap();
            let transfers = edge_plan(&fixture.core, &inventory, level);
            let options = options(level);
            let resources =
                ResourceInventory::new(&fixture.core, &inventory, &transfers, &options).unwrap();
            let roles = resources
                .planned_functions()
                .map(|(_, function)| function.role())
                .collect::<Vec<_>>();

            assert_eq!(resources.load(), PlannedFunctionId::from_index(0));
            assert_eq!(
                resources.init_try_create(),
                PlannedFunctionId::from_index(1)
            );
            let mut expected = vec![
                PlannedFunctionRole::Load,
                PlannedFunctionRole::InitTryCreate,
                PlannedFunctionRole::Block {
                    function: fixture.first,
                    block: fixture.first_entry,
                },
                PlannedFunctionRole::Block {
                    function: fixture.first,
                    block: fixture.first_moved,
                },
                PlannedFunctionRole::Block {
                    function: fixture.first,
                    block: fixture.first_empty,
                },
                PlannedFunctionRole::Block {
                    function: fixture.second,
                    block: fixture.second_entry,
                },
                PlannedFunctionRole::Block {
                    function: fixture.second,
                    block: fixture.second_join,
                },
            ];
            if level == MinecraftOptimizationLevel::None {
                expected.insert(
                    5,
                    PlannedFunctionRole::BranchHelper {
                        function: fixture.first,
                        edge: BranchEdge::new(fixture.first_entry, BranchArm::Then),
                    },
                );
            }
            assert_eq!(roles, expected);
            assert_eq!(
                resources.planned_functions().len(),
                if level == MinecraftOptimizationLevel::None {
                    8
                } else {
                    7
                }
            );
            if level == MinecraftOptimizationLevel::None {
                assert_eq!(
                    resources
                        .planned_function(PlannedFunctionId::from_index(5))
                        .unwrap()
                        .origin(),
                    fixture.first_branch_origin
                );
                assert_eq!(
                    resources
                        .planned_function(PlannedFunctionId::from_index(5))
                        .unwrap()
                        .resource()
                        .to_string(),
                    "mdl_test:__mdl/f0/e0_0"
                );
            }
        }
    }

    #[test]
    fn helpers_exist_only_for_nonempty_post_coalescing_arms() {
        let fixture = multi_function_fixture();
        let level = MinecraftOptimizationLevel::Baseline;
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let transfers = edge_plan(&fixture.core, &inventory, level);
        let resources =
            ResourceInventory::new(&fixture.core, &inventory, &transfers, &options(level)).unwrap();

        assert_eq!(resources.branch_helper(fixture.first, 0), None);
        assert_eq!(resources.branch_helper(fixture.first, 1), None);
        assert_eq!(resources.branch_helper(fixture.second, 0), None);
        assert_eq!(
            resources
                .function(fixture.first)
                .unwrap()
                .block_slots()
                .len(),
            4
        );
        assert_eq!(
            resources.block_function(fixture.first, fixture.first_detached_from_cfg),
            None
        );
        assert!(transfers.function(fixture.first).unwrap().edges()[0].is_empty());
        assert!(transfers.function(fixture.first).unwrap().edges()[1].is_empty());
        assert!(transfers.function(fixture.second).unwrap().edges()[0].is_empty());
    }

    fn resource_snapshot(
        resources: &ResourceInventory,
    ) -> Vec<(u32, String, OriginId, PlannedFunctionRole)> {
        resources
            .planned_functions()
            .map(|(id, function)| {
                (
                    id.index(),
                    function.resource().to_string(),
                    function.origin(),
                    function.role(),
                )
            })
            .collect()
    }

    #[test]
    fn construction_is_deterministic_and_level_provenance_is_checked() {
        let fixture = multi_function_fixture();
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let transfers = edge_plan(&fixture.core, &inventory, MinecraftOptimizationLevel::None);
        let none_options = options(MinecraftOptimizationLevel::None);
        let first =
            ResourceInventory::new(&fixture.core, &inventory, &transfers, &none_options).unwrap();
        let second =
            ResourceInventory::new(&fixture.core, &inventory, &transfers, &none_options).unwrap();

        assert_eq!(resource_snapshot(&first), resource_snapshot(&second));
        assert!(first.matches_level(MinecraftOptimizationLevel::None));
        assert_eq!(first.level(), MinecraftOptimizationLevel::None);
        assert_eq!(first.len(), fixture.core.len());
        assert_eq!(
            ResourceInventory::new(
                &fixture.core,
                &inventory,
                &transfers,
                &options(MinecraftOptimizationLevel::Baseline),
            )
            .unwrap_err(),
            ResourceInventoryError::OptimizationLevelMismatch {
                expected: MinecraftOptimizationLevel::Baseline,
                actual: MinecraftOptimizationLevel::None,
            }
        );
    }

    #[test]
    fn rejects_an_extra_transfer_function_instead_of_accepting_a_matching_prefix() {
        let fixture = multi_function_fixture();
        let level = MinecraftOptimizationLevel::None;
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let mut transfers = edge_plan(&fixture.core, &inventory, level);
        let extra = transfers.function(fixture.first).unwrap().clone();
        transfers.push_test_function(extra).unwrap();

        assert_eq!(
            ResourceInventory::new(&fixture.core, &inventory, &transfers, &options(level),)
                .unwrap_err(),
            ResourceInventoryError::PrerequisiteFunctionCountMismatch {
                prerequisite: ResourcePrerequisite::EdgeTransferPlan,
                expected: fixture.core.len(),
                actual: fixture.core.len() + 1,
            }
        );
    }

    #[test]
    fn duplicate_resource_names_are_rejected_by_final_validation() {
        let fixture = multi_function_fixture();
        let level = MinecraftOptimizationLevel::None;
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let transfers = edge_plan(&fixture.core, &inventory, level);
        let options = options(level);
        let mut resources =
            ResourceInventory::new(&fixture.core, &inventory, &transfers, &options).unwrap();
        let load = resources.load();
        let init = resources.init_try_create();
        let duplicate = resources.planned_function(load).unwrap().resource.clone();
        resources.planned_functions.get_mut(init).unwrap().resource = duplicate.clone();

        assert_eq!(
            resources
                .validate(&fixture.core, &inventory, &transfers, &options)
                .unwrap_err(),
            ResourceInventoryError::DuplicateResource {
                first: load,
                second: init,
                resource: duplicate,
            }
        );
    }

    #[test]
    fn resource_capacity_arithmetic_is_checked_without_large_allocations() {
        assert_eq!(checked_resource_count(2, 5, 3), Ok(10));
        assert_eq!(
            checked_resource_count(u64::MAX, 1, 0),
            Err(ResourceInventoryError::CapacityOverflow)
        );
        let maximum = maximum_resource_count();
        assert_eq!(validate_resource_capacity(maximum), Ok(()));
        assert_eq!(
            validate_resource_capacity(maximum + 1),
            Err(ResourceInventoryError::CapacityExceeded {
                required: maximum + 1,
                maximum,
            })
        );
    }

    #[test]
    fn prerequisite_function_counts_require_exact_phase_alignment() {
        assert_eq!(
            validate_prerequisite_function_count(ResourcePrerequisite::SemanticInventory, 2, 2,),
            Ok(())
        );
        assert_eq!(
            validate_prerequisite_function_count(ResourcePrerequisite::SemanticInventory, 2, 1,),
            Err(ResourceInventoryError::PrerequisiteFunctionCountMismatch {
                prerequisite: ResourcePrerequisite::SemanticInventory,
                expected: 2,
                actual: 1,
            })
        );
        assert_eq!(
            validate_prerequisite_function_count(ResourcePrerequisite::EdgeTransferPlan, 2, 3,),
            Err(ResourceInventoryError::PrerequisiteFunctionCountMismatch {
                prerequisite: ResourcePrerequisite::EdgeTransferPlan,
                expected: 2,
                actual: 3,
            })
        );
    }

    #[test]
    fn twenty_thousand_blocks_keep_dense_linear_order_without_jump_helpers() {
        const BLOCKS: usize = 20_000;
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("large"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let first = builder.entry_block();
        let mut last = first;
        for _ in 1..BLOCKS {
            let next = builder.create_block(OriginId::UNKNOWN).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(next, vec![])),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            builder.switch_to_block(next).unwrap();
            last = next;
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        let inventory = SemanticInventory::new(&core).unwrap();
        let transfers = edge_plan(&core, &inventory, MinecraftOptimizationLevel::None);
        let resources = ResourceInventory::new(
            &core,
            &inventory,
            &transfers,
            &options(MinecraftOptimizationLevel::None),
        )
        .unwrap();

        assert_eq!(resources.planned_functions().len(), BLOCKS + 2);
        assert_eq!(
            resources.block_function(function, first),
            Some(PlannedFunctionId::from_index(2))
        );
        assert_eq!(
            resources.block_function(function, last),
            Some(PlannedFunctionId::from_index(
                u32::try_from(BLOCKS + 1).unwrap()
            ))
        );
        assert!(
            resources
                .function(function)
                .unwrap()
                .branch_helper_slots()
                .iter()
                .all(Option::is_none)
        );
    }
}
