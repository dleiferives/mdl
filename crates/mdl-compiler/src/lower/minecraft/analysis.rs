use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{
    BlockId, ControlFlowGraph, CoreOp, CoreProgram, FunctionId, InstId, Reachability,
    TerminatorKind, ValueId,
};
use crate::source::OriginId;

/// One reachable internal call in deterministic Core layout order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CallSite {
    block: BlockId,
    instruction: InstId,
    callee: FunctionId,
    origin: OriginId,
}

impl CallSite {
    pub(crate) const fn block(self) -> BlockId {
        self.block
    }

    pub(crate) const fn instruction(self) -> InstId {
        self.instruction
    }

    pub(crate) const fn callee(self) -> FunctionId {
        self.callee
    }

    pub(crate) const fn origin(self) -> OriginId {
        self.origin
    }
}

/// Semantic identity of one conditional successor arm.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum BranchArm {
    Then,
    Else,
}

impl BranchArm {
    pub(super) const fn ordinal(self) -> u8 {
        match self {
            Self::Then => 0,
            Self::Else => 1,
        }
    }
}

/// Semantic identity of one reachable outgoing Core edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoreEdgeKind {
    Jump,
    Branch(BranchArm),
}

/// One reachable Core edge and its simultaneous argument assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CoreEdge {
    source: BlockId,
    kind: CoreEdgeKind,
    destination: BlockId,
    arguments: Box<[ValueId]>,
}

impl CoreEdge {
    pub(crate) const fn source(&self) -> BlockId {
        self.source
    }

    pub(crate) const fn kind(&self) -> CoreEdgeKind {
        self.kind
    }

    pub(crate) const fn destination(&self) -> BlockId {
        self.destination
    }

    pub(crate) fn arguments(&self) -> &[ValueId] {
        &self.arguments
    }
}

/// Checked semantic incidences needed to bound backward runtime demand.
///
/// Allocated slots include detached history because demand uses dense tables. All
/// other fields count only entry-reachable semantics from the authoritative Core
/// layout walk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DemandIncidenceCounts {
    allocated_values: usize,
    allocated_instructions: usize,
    reachable_values: usize,
    reachable_instructions: usize,
    instruction_operands: usize,
    edge_arguments: usize,
    return_operands: usize,
    branch_conditions: usize,
    non_discardable_instructions: usize,
}

impl DemandIncidenceCounts {
    pub(crate) const fn allocated_values(self) -> usize {
        self.allocated_values
    }

    pub(crate) const fn allocated_instructions(self) -> usize {
        self.allocated_instructions
    }

    pub(crate) const fn reachable_values(self) -> usize {
        self.reachable_values
    }

    pub(crate) const fn reachable_instructions(self) -> usize {
        self.reachable_instructions
    }

    pub(crate) const fn instruction_operands(self) -> usize {
        self.instruction_operands
    }

    pub(crate) const fn edge_arguments(self) -> usize {
        self.edge_arguments
    }

    pub(crate) const fn return_operands(self) -> usize {
        self.return_operands
    }

    pub(crate) const fn branch_conditions(self) -> usize {
        self.branch_conditions
    }

    pub(crate) const fn non_discardable_instructions(self) -> usize {
        self.non_discardable_instructions
    }
}

#[derive(Default)]
struct DemandIncidenceBuilder {
    instruction_operands: usize,
    edge_arguments: usize,
    return_operands: usize,
    branch_conditions: usize,
    non_discardable_instructions: usize,
}

impl DemandIncidenceBuilder {
    fn add_instruction(
        &mut self,
        function: FunctionId,
        operands: usize,
        discardable: bool,
    ) -> Result<(), AnalysisError> {
        checked_add(&mut self.instruction_operands, operands, function)?;
        if !discardable {
            checked_add(&mut self.non_discardable_instructions, 1, function)?;
        }
        Ok(())
    }

    fn add_edge(&mut self, function: FunctionId, arguments: usize) -> Result<(), AnalysisError> {
        checked_add(&mut self.edge_arguments, arguments, function)
    }

    fn add_return(&mut self, function: FunctionId, operands: usize) -> Result<(), AnalysisError> {
        checked_add(&mut self.return_operands, operands, function)
    }

    fn add_branch(&mut self, function: FunctionId) -> Result<(), AnalysisError> {
        checked_add(&mut self.branch_conditions, 1, function)
    }

    fn finish(
        self,
        body: &crate::ir::core::FunctionBody,
        reachable_values: usize,
        reachable_instructions: usize,
    ) -> DemandIncidenceCounts {
        DemandIncidenceCounts {
            allocated_values: body.value_counts().allocated,
            allocated_instructions: body.instruction_counts().allocated,
            reachable_values,
            reachable_instructions,
            instruction_operands: self.instruction_operands,
            edge_arguments: self.edge_arguments,
            return_operands: self.return_operands,
            branch_conditions: self.branch_conditions,
            non_discardable_instructions: self.non_discardable_instructions,
        }
    }
}

/// Reusable semantic facts about the reachable part of one function.
///
/// This record deliberately owns IDs and small edge argument lists rather than
/// borrowing a CFG. The CFG and its reachability scratch are constructed once and
/// dropped after these deterministic facts have been derived.
#[derive(Clone, Debug)]
pub(crate) struct FunctionSemanticInventory {
    reachable_blocks: Box<[BlockId]>,
    reachable_block_bits: Box<[bool]>,
    reachable_instructions: Box<[InstId]>,
    reachable_instruction_bits: Box<[bool]>,
    reachable_values: Box<[ValueId]>,
    reachable_value_bits: Box<[bool]>,
    call_sites: Box<[CallSite]>,
    edges: Box<[CoreEdge]>,
    outgoing_edge_indices: Box<[Box<[usize]>]>,
    incoming_edge_indices: Box<[Box<[usize]>]>,
    demand_incidences: DemandIncidenceCounts,
}

impl FunctionSemanticInventory {
    #[allow(
        clippy::too_many_lines,
        reason = "one layout-order walk derives the complete reachable semantic census"
    )]
    fn new(core: &CoreProgram, function: FunctionId) -> Result<Self, AnalysisError> {
        let body = core
            .function(function)
            .and_then(|declaration| declaration.body())
            .ok_or(AnalysisError::MissingDefinition { function })?;
        let cfg = ControlFlowGraph::new(body);
        let reachability = Reachability::new(&cfg);

        let mut reachable_blocks = Vec::new();
        let mut reachable_block_bits = vec![false; body.block_counts().allocated];
        let mut reachable_instructions = Vec::new();
        let mut reachable_instruction_bits = vec![false; body.instruction_counts().allocated];
        let mut reachable_values = Vec::new();
        let mut reachable_value_bits = vec![false; body.values.len()];
        let mut call_sites = Vec::new();
        let mut edges = Vec::new();
        let mut outgoing_edge_indices = vec![Vec::new(); body.block_counts().allocated];
        let mut incoming_edge_indices = vec![Vec::new(); body.block_counts().allocated];
        let mut demand_incidences = DemandIncidenceBuilder::default();

        for block in body.block_order().iter().copied() {
            if !reachability.contains(block) {
                continue;
            }
            reachable_blocks.push(block);
            set_bit(&mut reachable_block_bits, block);
            let data = body
                .block(block)
                .expect("verified attached block must have data");

            for parameter in data.parameters() {
                record_value(
                    &mut reachable_values,
                    &mut reachable_value_bits,
                    parameter.value(),
                );
            }
            for instruction in data.instructions().iter().copied() {
                let instruction_data = body
                    .instruction(instruction)
                    .expect("verified attached instruction must have data");
                reachable_instructions.push(instruction);
                set_bit(&mut reachable_instruction_bits, instruction);
                demand_incidences.add_instruction(
                    function,
                    instruction_data.operands().len(),
                    instruction_data.op().is_trivially_discardable(),
                )?;
                for result in instruction_data.results().iter().copied() {
                    record_value(&mut reachable_values, &mut reachable_value_bits, result);
                }
                if let CoreOp::Call(callee) = instruction_data.op() {
                    call_sites.push(CallSite {
                        block,
                        instruction,
                        callee: *callee,
                        origin: instruction_data.origin(),
                    });
                }
            }

            let terminator = data
                .terminator()
                .expect("verified reachable block must have a terminator");
            match terminator.kind() {
                TerminatorKind::Jump(target) => record_edge(
                    function,
                    &mut edges,
                    &mut outgoing_edge_indices,
                    &mut incoming_edge_indices,
                    &mut demand_incidences,
                    CoreEdge {
                        source: block,
                        kind: CoreEdgeKind::Jump,
                        destination: target.block(),
                        arguments: target.arguments().into(),
                    },
                )?,
                TerminatorKind::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    demand_incidences.add_branch(function)?;
                    record_edge(
                        function,
                        &mut edges,
                        &mut outgoing_edge_indices,
                        &mut incoming_edge_indices,
                        &mut demand_incidences,
                        CoreEdge {
                            source: block,
                            kind: CoreEdgeKind::Branch(BranchArm::Then),
                            destination: then_target.block(),
                            arguments: then_target.arguments().into(),
                        },
                    )?;
                    record_edge(
                        function,
                        &mut edges,
                        &mut outgoing_edge_indices,
                        &mut incoming_edge_indices,
                        &mut demand_incidences,
                        CoreEdge {
                            source: block,
                            kind: CoreEdgeKind::Branch(BranchArm::Else),
                            destination: else_target.block(),
                            arguments: else_target.arguments().into(),
                        },
                    )?;
                }
                TerminatorKind::Return(values) => {
                    demand_incidences.add_return(function, values.len())?;
                }
                TerminatorKind::Unreachable => {}
            }
        }

        let demand_incidences =
            demand_incidences.finish(body, reachable_values.len(), reachable_instructions.len());

        Ok(Self {
            reachable_blocks: reachable_blocks.into_boxed_slice(),
            reachable_block_bits: reachable_block_bits.into_boxed_slice(),
            reachable_instructions: reachable_instructions.into_boxed_slice(),
            reachable_instruction_bits: reachable_instruction_bits.into_boxed_slice(),
            reachable_values: reachable_values.into_boxed_slice(),
            reachable_value_bits: reachable_value_bits.into_boxed_slice(),
            call_sites: call_sites.into_boxed_slice(),
            edges: edges.into_boxed_slice(),
            outgoing_edge_indices: outgoing_edge_indices
                .into_iter()
                .map(Vec::into_boxed_slice)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            incoming_edge_indices: incoming_edge_indices
                .into_iter()
                .map(Vec::into_boxed_slice)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            demand_incidences,
        })
    }

    pub(crate) fn contains_block(&self, block: BlockId) -> bool {
        get_bit(&self.reachable_block_bits, block)
    }

    pub(crate) fn contains_value(&self, value: ValueId) -> bool {
        get_bit(&self.reachable_value_bits, value)
    }

    pub(crate) fn contains_instruction(&self, instruction: InstId) -> bool {
        get_bit(&self.reachable_instruction_bits, instruction)
    }

    pub(crate) fn reachable_blocks(&self) -> &[BlockId] {
        &self.reachable_blocks
    }

    pub(crate) fn reachable_values(&self) -> &[ValueId] {
        &self.reachable_values
    }

    pub(crate) fn reachable_instructions(&self) -> &[InstId] {
        &self.reachable_instructions
    }

    pub(crate) fn call_sites(&self) -> &[CallSite] {
        &self.call_sites
    }

    pub(crate) fn edges(&self) -> &[CoreEdge] {
        &self.edges
    }

    pub(crate) fn outgoing_edge_indices(&self, block: BlockId) -> Option<&[usize]> {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| self.outgoing_edge_indices.get(index))
            .map(AsRef::as_ref)
    }

    pub(crate) fn incoming_edge_indices(&self, block: BlockId) -> Option<&[usize]> {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| self.incoming_edge_indices.get(index))
            .map(AsRef::as_ref)
    }

    pub(crate) const fn demand_incidences(&self) -> DemandIncidenceCounts {
        self.demand_incidences
    }
}

/// Dense per-function semantic inventories, aligned with Core `FunctionId`s.
#[derive(Clone, Debug)]
pub(crate) struct SemanticInventory {
    functions: EntityVec<FunctionId, FunctionSemanticInventory>,
}

impl SemanticInventory {
    pub(crate) fn new(core: &CoreProgram) -> Result<Self, AnalysisError> {
        let mut functions = EntityVec::new();
        for (function, _) in core.functions() {
            let inventory = FunctionSemanticInventory::new(core, function)?;
            let inventoried = functions
                .push(inventory)
                .map_err(|EntityLimitError| AnalysisError::EntityLimit)?;
            debug_assert_eq!(inventoried, function);
        }
        Ok(Self { functions })
    }

    pub(crate) fn function(&self, function: FunctionId) -> Option<&FunctionSemanticInventory> {
        self.functions.get(function)
    }

    pub(crate) fn len(&self) -> usize {
        self.functions.len()
    }
}

fn record_edge(
    function: FunctionId,
    edges: &mut Vec<CoreEdge>,
    outgoing_edge_indices: &mut [Vec<usize>],
    incoming_edge_indices: &mut [Vec<usize>],
    demand_incidences: &mut DemandIncidenceBuilder,
    edge: CoreEdge,
) -> Result<(), AnalysisError> {
    demand_incidences.add_edge(function, edge.arguments().len())?;
    let source = usize::try_from(edge.source().index())
        .ok()
        .and_then(|index| outgoing_edge_indices.get_mut(index))
        .ok_or(AnalysisError::InvalidEdgeSource {
            function,
            source: edge.source(),
        })?;
    let destination = usize::try_from(edge.destination().index())
        .ok()
        .and_then(|index| incoming_edge_indices.get_mut(index))
        .ok_or(AnalysisError::InvalidEdgeDestination {
            function,
            destination: edge.destination(),
        })?;
    let edge_index = edges.len();
    source.push(edge_index);
    destination.push(edge_index);
    edges.push(edge);
    Ok(())
}

fn checked_add(
    total: &mut usize,
    amount: usize,
    function: FunctionId,
) -> Result<(), AnalysisError> {
    *total = total
        .checked_add(amount)
        .ok_or(AnalysisError::IncidenceOverflow { function })?;
    Ok(())
}

fn record_value(values: &mut Vec<ValueId>, bits: &mut [bool], value: ValueId) {
    if !get_bit(bits, value) {
        values.push(value);
        set_bit(bits, value);
    }
}

fn get_bit<I: EntityId>(bits: &[bool], id: I) -> bool {
    usize::try_from(id.index())
        .ok()
        .and_then(|index| bits.get(index))
        .copied()
        .unwrap_or(false)
}

fn set_bit<I: EntityId>(bits: &mut [bool], id: I) {
    if let Some(bit) = usize::try_from(id.index())
        .ok()
        .and_then(|index| bits.get_mut(index))
    {
        *bit = true;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnalysisError {
    MissingDefinition {
        function: FunctionId,
    },
    InvalidEdgeDestination {
        function: FunctionId,
        destination: BlockId,
    },
    InvalidEdgeSource {
        function: FunctionId,
        source: BlockId,
    },
    IncidenceOverflow {
        function: FunctionId,
    },
    EntityLimit,
}

#[cfg(test)]
mod tests {
    use super::{CoreEdgeKind, FunctionSemanticInventory, SemanticInventory};
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, FunctionBuilder, FunctionId, Terminator, TerminatorKind,
        ValueId,
    };
    use crate::source::{OriginId, SourceContext};

    struct ReachabilityFixture {
        program: CoreProgram,
        entry_function: FunctionId,
        live_target: FunctionId,
        entry: BlockId,
        merge: BlockId,
        left: BlockId,
        right: BlockId,
        unreachable: BlockId,
        detached: BlockId,
        condition: ValueId,
        merge_value: ValueId,
        left_value: ValueId,
        right_value: ValueId,
        unreachable_value: ValueId,
        detached_value: ValueId,
    }

    fn reachability_fixture() -> ReachabilityFixture {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let live_target = define_empty_function(&mut program, &sources, "callee");
        let dead_target = define_empty_function(&mut program, &sources, "dead");
        let entry_function = program
            .declare_function(Some("caller"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, entry_function).unwrap();
        let entry = builder.entry_block();
        let condition = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        builder
            .call(live_target, vec![], OriginId::UNKNOWN)
            .unwrap();
        let merge = builder.create_block(OriginId::UNKNOWN).unwrap();
        let left = builder.create_block(OriginId::UNKNOWN).unwrap();
        let right = builder.create_block(OriginId::UNKNOWN).unwrap();
        let unreachable = builder.create_block(OriginId::UNKNOWN).unwrap();
        let detached = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(left, vec![]),
                    else_target: BlockTarget::new(right, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        let merge_value = fill_constant_call_block(
            &mut builder,
            merge,
            0,
            live_target,
            TerminatorKind::Return(vec![]),
        );

        // Construct right before left so allocation order cannot accidentally stand
        // in for the body's authoritative block/instruction order.
        let right_value = fill_constant_call_block(
            &mut builder,
            right,
            2,
            live_target,
            TerminatorKind::Jump(BlockTarget::new(merge, vec![])),
        );
        let left_value = fill_constant_call_block(
            &mut builder,
            left,
            1,
            live_target,
            TerminatorKind::Jump(BlockTarget::new(merge, vec![])),
        );
        let unreachable_value = fill_constant_call_block(
            &mut builder,
            unreachable,
            3,
            dead_target,
            TerminatorKind::Return(vec![]),
        );
        let detached_value = fill_constant_call_block(
            &mut builder,
            detached,
            4,
            dead_target,
            TerminatorKind::Return(vec![]),
        );

        let mut body = builder.finish().unwrap();
        body.block_order.retain(|block| *block != detached);
        program.define_function(entry_function, body).unwrap();

        ReachabilityFixture {
            program,
            entry_function,
            live_target,
            entry,
            merge,
            left,
            right,
            unreachable,
            detached,
            condition,
            merge_value,
            left_value,
            right_value,
            unreachable_value,
            detached_value,
        }
    }

    #[test]
    fn records_only_reachable_entities_in_authoritative_layout_order() {
        let fixture = reachability_fixture();

        let semantic_inventory = SemanticInventory::new(&fixture.program).unwrap();
        let emitted = semantic_inventory.function(fixture.entry_function).unwrap();
        let body = fixture
            .program
            .function(fixture.entry_function)
            .unwrap()
            .body()
            .unwrap();
        let expected_instructions = [fixture.entry, fixture.merge, fixture.left, fixture.right]
            .into_iter()
            .flat_map(|block| body.block(block).unwrap().instructions().iter().copied())
            .collect::<Vec<_>>();

        assert_eq!(
            &*emitted.reachable_blocks,
            &[fixture.entry, fixture.merge, fixture.left, fixture.right]
        );
        assert_eq!(
            &*emitted.reachable_values,
            &[
                fixture.condition,
                fixture.merge_value,
                fixture.left_value,
                fixture.right_value,
            ]
        );
        assert_eq!(emitted.reachable_instructions(), expected_instructions);
        assert!(
            expected_instructions
                .iter()
                .all(|instruction| emitted.contains_instruction(*instruction))
        );
        assert!(
            [fixture.unreachable, fixture.detached]
                .into_iter()
                .all(|block| !emitted.contains_block(block))
        );
        assert!(
            [fixture.unreachable_value, fixture.detached_value]
                .into_iter()
                .all(|value| !emitted.contains_value(value))
        );
        assert!(
            [fixture.unreachable, fixture.detached]
                .into_iter()
                .flat_map(|block| body.block(block).unwrap().instructions().iter().copied())
                .all(|instruction| !emitted.contains_instruction(instruction))
        );
        assert_eq!(
            emitted
                .call_sites
                .iter()
                .map(|site| (site.block, site.callee))
                .collect::<Vec<_>>(),
            vec![
                (fixture.entry, fixture.live_target),
                (fixture.merge, fixture.live_target),
                (fixture.left, fixture.live_target),
                (fixture.right, fixture.live_target),
            ]
        );
        assert_eq!(
            emitted
                .edges
                .iter()
                .map(|edge| (edge.source, edge.kind, edge.destination))
                .collect::<Vec<_>>(),
            vec![
                (
                    fixture.entry,
                    CoreEdgeKind::Branch(super::BranchArm::Then),
                    fixture.left,
                ),
                (
                    fixture.entry,
                    CoreEdgeKind::Branch(super::BranchArm::Else),
                    fixture.right,
                ),
                (fixture.left, CoreEdgeKind::Jump, fixture.merge),
                (fixture.right, CoreEdgeKind::Jump, fixture.merge),
            ]
        );
        assert_eq!(emitted.incoming_edge_indices(fixture.entry), Some(&[][..]));
        assert_eq!(
            emitted.incoming_edge_indices(fixture.merge),
            Some(&[2, 3][..])
        );
        assert_eq!(emitted.incoming_edge_indices(fixture.left), Some(&[0][..]));
        assert_eq!(emitted.incoming_edge_indices(fixture.right), Some(&[1][..]));
        assert_eq!(
            emitted.incoming_edge_indices(fixture.unreachable),
            Some(&[][..])
        );

        assert_reachability_fixture_incidences(emitted);
    }

    fn assert_reachability_fixture_incidences(emitted: &FunctionSemanticInventory) {
        let incidences = emitted.demand_incidences();
        assert_eq!(incidences.allocated_values(), 6);
        assert_eq!(incidences.allocated_instructions(), 12);
        assert_eq!(incidences.reachable_values(), 4);
        assert_eq!(incidences.reachable_instructions(), 8);
        assert_eq!(incidences.instruction_operands(), 0);
        assert_eq!(incidences.edge_arguments(), 0);
        assert_eq!(incidences.return_operands(), 0);
        assert_eq!(incidences.branch_conditions(), 1);
        assert_eq!(incidences.non_discardable_instructions(), 4);
    }

    #[test]
    fn demand_incidences_are_exact_and_dense_capacity_includes_detached_history() {
        let (program, entry_function, join, detached) = demand_incidence_fixture();
        let inventory = SemanticInventory::new(&program).unwrap();
        let function_inventory = inventory.function(entry_function).unwrap();
        let counts = function_inventory.demand_incidences();

        assert_eq!(counts.allocated_values(), 7);
        assert_eq!(counts.allocated_instructions(), 4);
        assert_eq!(counts.reachable_values(), 6);
        assert_eq!(counts.reachable_instructions(), 3);
        assert_eq!(counts.instruction_operands(), 3);
        assert_eq!(counts.edge_arguments(), 2);
        assert_eq!(counts.return_operands(), 1);
        assert_eq!(counts.branch_conditions(), 1);
        assert_eq!(counts.non_discardable_instructions(), 1);
        assert_eq!(
            function_inventory.incoming_edge_indices(join),
            Some(&[2, 3][..])
        );
        assert_eq!(
            function_inventory.incoming_edge_indices(detached),
            Some(&[][..])
        );
    }

    fn demand_incidence_fixture() -> (CoreProgram, FunctionId, BlockId, BlockId) {
        use crate::ir::core::CoreType;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let identity_function = program
            .declare_function(
                Some("callee"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut callee_builder =
            FunctionBuilder::new(&program, &sources, identity_function).unwrap();
        let callee_parameter = callee_builder
            .body()
            .block(callee_builder.entry_block())
            .unwrap()
            .parameters()[0]
            .value();
        callee_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![callee_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(identity_function, callee_builder.finish().unwrap())
            .unwrap();

        let entry_function = program
            .declare_function(
                Some("caller"),
                vec![CoreType::Bool, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, entry_function).unwrap();
        let entry = builder.entry_block();
        let parameters = builder.body().block(entry).unwrap().parameters();
        let condition = parameters[0].value();
        let input = parameters[1].value();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(input, one, OriginId::UNKNOWN)
            .unwrap();
        let call_result = builder
            .call(identity_function, vec![sum], OriginId::UNKNOWN)
            .unwrap()[0];
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let detached = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![joined]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(then_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![call_result])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(else_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![input])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(detached).unwrap();
        builder.i32_constant(99, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![input]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        body.block_order.retain(|block| *block != detached);
        program.define_function(entry_function, body).unwrap();
        (program, entry_function, join, detached)
    }

    fn fill_constant_call_block(
        builder: &mut FunctionBuilder<'_>,
        block: BlockId,
        constant: i32,
        target: FunctionId,
        terminator: TerminatorKind,
    ) -> ValueId {
        builder.switch_to_block(block).unwrap();
        let value = builder.i32_constant(constant, OriginId::UNKNOWN).unwrap();
        builder.call(target, vec![], OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(terminator, OriginId::UNKNOWN))
            .unwrap();
        value
    }

    fn define_empty_function(
        program: &mut CoreProgram,
        sources: &SourceContext,
        name: &str,
    ) -> FunctionId {
        let function = program
            .declare_function(Some(name), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(program, sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        function
    }

    #[test]
    fn call_sites_preserve_instruction_identity_and_origin() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let target = define_empty_function(&mut program, &sources, "callee");
        let entry_function = program
            .declare_function(Some("caller"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, entry_function).unwrap();
        builder.call(target, vec![], OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let expected_instruction = builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .instructions()[0];
        program
            .define_function(entry_function, builder.finish().unwrap())
            .unwrap();

        let semantic_inventory = SemanticInventory::new(&program).unwrap();
        let site = semantic_inventory
            .function(entry_function)
            .unwrap()
            .call_sites[0];
        assert_eq!(site.instruction, expected_instruction);
        assert_eq!(site.origin, OriginId::UNKNOWN);
    }

    #[test]
    fn analysis_does_not_mutate_core() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        define_empty_function(&mut program, &sources, "entry");
        let before = format!("{program:?}");

        let _ = SemanticInventory::new(&program).unwrap();

        assert_eq!(format!("{program:?}"), before);
    }
}
