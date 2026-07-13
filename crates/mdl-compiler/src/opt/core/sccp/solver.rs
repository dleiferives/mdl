use std::collections::VecDeque;

use super::lattice::{
    LatticeTypeError, LatticeValue, bool_not, i32_add_overflowing, i32_add_wrapping, i32_compare,
};
use super::{
    SccpDecision, SccpInvariantError, SccpLimit, SccpLimitReason, SccpLimits, SccpSolveResult,
    SccpStatistics,
};
use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, BlockParam, BlockTarget, CoreOp, FunctionBody, InstId, PlacementIndex, Terminator,
    TerminatorKind, TypedCoreConstant, UseIndex, UseSite, ValueDef, ValueId, ValueReplacement,
};

const BLOCK_EXECUTABLE: u8 = 1 << 0;
const BLOCK_VISIT_QUEUED: u8 = 1 << 1;
const BLOCK_TERMINATOR_QUEUED: u8 = 1 << 2;
const BLOCK_FINALIZED: u8 = 1 << 3;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::opt::core) struct SccpEdgeId {
    source: BlockId,
    successor_index: u8,
}

#[cfg(test)]
impl SccpEdgeId {
    pub(super) const fn parts(self) -> (BlockId, u8) {
        (self.source, self.successor_index)
    }
}

#[derive(Debug)]
struct EdgeData<'a> {
    id: SccpEdgeId,
    destination: BlockId,
    arguments: &'a [ValueId],
    argument_base: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    VisitBlock(BlockId),
    VisitInstruction(InstId),
    VisitTerminator(BlockId),
    ActivateEdge(SccpEdgeId),
    PropagateEdgeArgument(SccpEdgeId, u32),
}

#[derive(Clone, Copy, Debug, Default)]
struct BlockState(u8);

impl BlockState {
    const fn has(self, flag: u8) -> bool {
        self.0 & flag != 0
    }

    fn insert(&mut self, flag: u8) {
        self.0 |= flag;
    }

    fn remove(&mut self, flag: u8) {
        self.0 &= !flag;
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct EdgeState {
    executable: bool,
    queued: bool,
}

#[derive(Clone, Copy, Debug)]
struct Preflight {
    allocated_values: usize,
    attached_values: usize,
    allocated_blocks: usize,
    allocated_instructions: usize,
    attached_blocks: usize,
    attached_instructions: usize,
    instruction_operand_uses: u64,
    branch_conditions: u64,
    uses: usize,
    edges: usize,
    edge_arguments: usize,
    table_entries: u64,
    table_admission_requirement: u64,
    event_upper_bound: u64,
    queue_slots: usize,
}

impl Preflight {
    #[allow(
        clippy::too_many_lines,
        reason = "one read-only scan keeps every SCCP allocation count in one checked transaction"
    )]
    fn scan(body: &FunctionBody) -> Result<Self, PreflightError> {
        let value_counts = body.value_counts();
        let allocated_values = value_counts.allocated;
        let attached_values = value_counts.attached;
        let allocated_blocks = body.block_counts().allocated;
        let allocated_instructions = body.instruction_counts().allocated;
        let mut attached_blocks = 0_usize;
        let mut attached_instructions = 0_usize;
        let mut instruction_operand_uses = 0_u64;
        let mut branch_conditions = 0_u64;
        let mut uses = 0_usize;
        let mut edges = 0_usize;
        let mut edge_arguments = 0_usize;

        for block in body.block_order().iter().copied() {
            attached_blocks = attached_blocks
                .checked_add(1)
                .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
            let Some(data) = body.block(block) else {
                return Err(SccpInvariantError::InvalidBlock { block }.into());
            };
            for instruction in data.instructions().iter().copied() {
                let instruction_id = instruction;
                let Some(instruction) = body.instruction(instruction_id) else {
                    return Err(SccpInvariantError::InvalidInstruction {
                        instruction: instruction_id,
                    }
                    .into());
                };
                attached_instructions = attached_instructions
                    .checked_add(1)
                    .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
                let operand_count = instruction.operands().len();
                uses = uses
                    .checked_add(operand_count)
                    .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
                instruction_operand_uses = instruction_operand_uses
                    .checked_add(
                        u64::try_from(operand_count)
                            .map_err(|_| SccpLimitReason::DerivedBoundOverflow)?,
                    )
                    .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
            }

            let Some(terminator) = data.terminator() else {
                return Err(SccpInvariantError::MissingTerminator { block }.into());
            };
            match terminator.kind() {
                TerminatorKind::Jump(target) => {
                    add_target_counts(target, &mut uses, &mut edges, &mut edge_arguments)?;
                }
                TerminatorKind::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    uses = uses
                        .checked_add(1)
                        .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
                    branch_conditions = branch_conditions
                        .checked_add(1)
                        .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
                    add_target_counts(then_target, &mut uses, &mut edges, &mut edge_arguments)?;
                    add_target_counts(else_target, &mut uses, &mut edges, &mut edge_arguments)?;
                }
                TerminatorKind::Return(values) => {
                    uses = uses
                        .checked_add(values.len())
                        .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
                }
                TerminatorKind::Unreachable => {}
            }
        }

        let table_entries = checked_sum_u64(&[
            allocated_values,
            allocated_blocks,
            allocated_instructions,
            uses,
            edges,
            edge_arguments,
        ])?;

        // With per-kind queued bits, the queue can hold at most one block event and
        // one terminator event per block, plus one event per instruction, edge, and
        // edge argument.
        let queue_slots = attached_instructions
            .checked_add(
                attached_blocks
                    .checked_mul(2)
                    .ok_or(SccpLimitReason::DerivedBoundOverflow)?,
            )
            .and_then(|value| value.checked_add(edges))
            .and_then(|value| value.checked_add(edge_arguments))
            .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
        // `table_entries` is the documented aggregate and remains stable for
        // reporting.  Admission separately covers the largest single allocation:
        // the deduplicated FIFO can be larger than that aggregate for a tiny body
        // with no values, instructions, uses, or edges.
        let table_admission_requirement = table_entries.max(u64_from_usize(queue_slots)?);

        // A flat cell widens at most twice.  A block is seeded once; each operand
        // transition can revisit its instruction, each branch-condition transition
        // can revisit its terminator, each edge activates once, and each edge
        // argument propagates once at activation plus once per source transition.
        // One final fuel unit per value covers pessimistic completion.
        let event_upper_bound = u64_from_usize(attached_instructions)?
            .checked_add(
                u64_from_usize(attached_blocks)?
                    .checked_mul(2)
                    .ok_or(SccpLimitReason::DerivedBoundOverflow)?,
            )
            .and_then(|value| value.checked_add(u64_from_usize(edges).ok()?))
            .and_then(|value| value.checked_add(u64_from_usize(attached_values).ok()?))
            .and_then(|value| value.checked_add(instruction_operand_uses.checked_mul(2)?))
            .and_then(|value| value.checked_add(branch_conditions.checked_mul(2)?))
            .and_then(|value| {
                value.checked_add(u64_from_usize(edge_arguments).ok()?.checked_mul(3)?)
            })
            .ok_or(SccpLimitReason::DerivedBoundOverflow)?;

        Ok(Self {
            allocated_values,
            attached_values,
            allocated_blocks,
            allocated_instructions,
            attached_blocks,
            attached_instructions,
            instruction_operand_uses,
            branch_conditions,
            uses,
            edges,
            edge_arguments,
            table_entries,
            table_admission_requirement,
            event_upper_bound,
            queue_slots,
        })
    }
}

#[derive(Debug)]
enum PreflightError {
    Limit(SccpLimitReason),
    Invariant(SccpInvariantError),
}

impl From<SccpLimitReason> for PreflightError {
    fn from(reason: SccpLimitReason) -> Self {
        Self::Limit(reason)
    }
}

impl From<SccpInvariantError> for PreflightError {
    fn from(error: SccpInvariantError) -> Self {
        Self::Invariant(error)
    }
}

fn add_target_counts(
    target: &BlockTarget,
    uses: &mut usize,
    edges: &mut usize,
    edge_arguments: &mut usize,
) -> Result<(), SccpLimitReason> {
    *edges = edges
        .checked_add(1)
        .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
    *edge_arguments = edge_arguments
        .checked_add(target.arguments().len())
        .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
    *uses = uses
        .checked_add(target.arguments().len())
        .ok_or(SccpLimitReason::DerivedBoundOverflow)?;
    Ok(())
}

fn checked_sum_u64(values: &[usize]) -> Result<u64, SccpLimitReason> {
    values.iter().try_fold(0_u64, |sum, value| {
        sum.checked_add(u64_from_usize(*value)?)
            .ok_or(SccpLimitReason::DerivedBoundOverflow)
    })
}

fn u64_from_usize(value: usize) -> Result<u64, SccpLimitReason> {
    u64::try_from(value).map_err(|_| SccpLimitReason::DerivedBoundOverflow)
}

pub(super) fn solve(
    body: &FunctionBody,
    limits: SccpLimits,
) -> Result<SccpSolveResult, SccpInvariantError> {
    let preflight = match Preflight::scan(body) {
        Ok(preflight) => preflight,
        Err(PreflightError::Limit(reason)) => {
            return Ok(SccpSolveResult::Stopped {
                reason,
                statistics: SccpStatistics::default(),
            });
        }
        Err(PreflightError::Invariant(error)) => return Err(error),
    };

    let mut statistics = SccpStatistics {
        table_entries: preflight.table_entries,
        ..SccpStatistics::default()
    };
    let table_limit = match limits.tables {
        SccpLimit::Derived => preflight.table_admission_requirement,
        SccpLimit::Explicit(limit) => limit.get(),
    };
    if preflight.table_admission_requirement > table_limit {
        return Ok(SccpSolveResult::Stopped {
            reason: SccpLimitReason::Tables,
            statistics,
        });
    }
    let event_limit = match limits.events {
        SccpLimit::Derived => preflight.event_upper_bound,
        SccpLimit::Explicit(limit) => limit.get(),
    };

    let mut solver = Solver::new(body, preflight, event_limit, statistics)?;
    solver.initialize()?;
    if !solver.run_to_completion()? {
        statistics = solver.statistics;
        return Ok(SccpSolveResult::Stopped {
            reason: SccpLimitReason::Events,
            statistics,
        });
    }
    let completed = solver.into_completed()?;
    Ok(SccpSolveResult::Complete(completed.into_decision()?))
}

/// Type-level boundary proving that no optimistic or unfinalized solver state can
/// reach decision derivation.
struct CompletedSccpSolution<'a> {
    solver: Solver<'a>,
}

impl CompletedSccpSolution<'_> {
    fn into_decision(self) -> Result<SccpDecision, SccpInvariantError> {
        self.solver.into_decision_after_completion()
    }
}

struct Solver<'a> {
    body: &'a FunctionBody,
    placement: PlacementIndex<'a>,
    uses: UseIndex<'a>,
    edges: Vec<EdgeData<'a>>,
    edge_lookup: Vec<[Option<usize>; 2]>,
    values: Vec<LatticeValue>,
    blocks: Vec<BlockState>,
    instructions_queued: Vec<bool>,
    edge_states: Vec<EdgeState>,
    edge_arguments_queued: Vec<bool>,
    queue: VecDeque<Event>,
    event_limit: u64,
    event_fuel_used: u64,
    statistics: SccpStatistics,
}

impl<'a> Solver<'a> {
    fn new(
        body: &'a FunctionBody,
        preflight: Preflight,
        event_limit: u64,
        statistics: SccpStatistics,
    ) -> Result<Self, SccpInvariantError> {
        let placement = PlacementIndex::new(body);
        let uses = UseIndex::from_placement(&placement);
        let mut edges = Vec::with_capacity(preflight.edges);
        let mut edge_lookup = vec![[None; 2]; preflight.allocated_blocks];
        let mut next_argument = 0_usize;

        for source in body.block_order().iter().copied() {
            let terminator = body
                .block(source)
                .and_then(|block| block.terminator())
                .ok_or(SccpInvariantError::MissingTerminator { block: source })?;
            match terminator.kind() {
                TerminatorKind::Jump(target) => add_edge(
                    &mut edges,
                    &mut edge_lookup,
                    source,
                    0,
                    target,
                    &mut next_argument,
                )?,
                TerminatorKind::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    add_edge(
                        &mut edges,
                        &mut edge_lookup,
                        source,
                        0,
                        then_target,
                        &mut next_argument,
                    )?;
                    add_edge(
                        &mut edges,
                        &mut edge_lookup,
                        source,
                        1,
                        else_target,
                        &mut next_argument,
                    )?;
                }
                TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
            }
        }
        if edges.len() != preflight.edges || next_argument != preflight.edge_arguments {
            return Err(SccpInvariantError::PreflightMismatch);
        }

        // Retain the otherwise diagnostic-only counts in debug builds so additions to
        // the preflight formula cannot silently stop matching its intended units.
        debug_assert_eq!(preflight.attached_blocks, body.block_order().len());
        debug_assert_eq!(preflight.attached_values, body.value_counts().attached);
        debug_assert_eq!(
            preflight.attached_instructions,
            body.instruction_counts().attached
        );
        debug_assert_eq!(preflight.uses, count_uses(&uses, body));
        debug_assert!(preflight.instruction_operand_uses <= preflight.uses as u64);
        debug_assert!(preflight.branch_conditions <= preflight.attached_blocks as u64);

        Ok(Self {
            body,
            placement,
            uses,
            edges,
            edge_lookup,
            values: vec![LatticeValue::Unknown; preflight.allocated_values],
            blocks: vec![BlockState::default(); preflight.allocated_blocks],
            instructions_queued: vec![false; preflight.allocated_instructions],
            edge_states: vec![EdgeState::default(); preflight.edges],
            edge_arguments_queued: vec![false; preflight.edge_arguments],
            queue: VecDeque::with_capacity(preflight.queue_slots),
            event_limit,
            event_fuel_used: 0,
            statistics,
        })
    }

    fn initialize(&mut self) -> Result<(), SccpInvariantError> {
        let entry = self.body.entry();
        let parameter_count = self
            .body
            .block(entry)
            .ok_or(SccpInvariantError::InvalidBlock { block: entry })?
            .parameters()
            .len();
        for parameter_index in 0..parameter_count {
            let value = self
                .body
                .block(entry)
                .and_then(|block| block.parameters().get(parameter_index))
                .map(BlockParam::value)
                .ok_or(SccpInvariantError::InvalidBlock { block: entry })?;
            self.join_value(value, LatticeValue::Overdefined)?;
        }
        self.mark_block_executable(entry)?;
        Ok(())
    }

    fn run_to_completion(&mut self) -> Result<bool, SccpInvariantError> {
        loop {
            while !self.queue.is_empty() {
                if !self.charge_fuel() {
                    return Ok(false);
                }
                let event = self.queue.pop_front().expect("queue was checked non-empty");
                self.clear_queued(event)?;
                bump(&mut self.statistics.events_dequeued);
                self.process_event(event)?;
            }

            let mut finalized_any = false;
            for block_position in 0..self.body.block_order().len() {
                let block = self.body.block_order()[block_position];
                let block_index = block_index(block)?;
                if !self.blocks[block_index].has(BLOCK_EXECUTABLE)
                    || self.blocks[block_index].has(BLOCK_FINALIZED)
                {
                    continue;
                }
                finalized_any = true;
                if !self.finalize_block(block)? {
                    return Ok(false);
                }
                self.blocks[block_index].insert(BLOCK_FINALIZED);
            }

            if self.queue.is_empty() && !finalized_any {
                return Ok(true);
            }
        }
    }

    fn process_event(&mut self, event: Event) -> Result<(), SccpInvariantError> {
        match event {
            Event::VisitBlock(block) => self.visit_block(block),
            Event::VisitInstruction(instruction) => self.visit_instruction(instruction),
            Event::VisitTerminator(block) => self.visit_terminator(block),
            Event::ActivateEdge(edge) => self.activate_edge(edge),
            Event::PropagateEdgeArgument(edge, argument) => {
                self.propagate_edge_argument(edge, argument)
            }
        }
    }

    fn visit_block(&mut self, block: BlockId) -> Result<(), SccpInvariantError> {
        if !self.is_block_executable(block)? {
            return Ok(());
        }
        let instruction_count = self
            .body
            .block(block)
            .ok_or(SccpInvariantError::InvalidBlock { block })?
            .instructions()
            .len();
        for position in 0..instruction_count {
            let instruction = self
                .body
                .block(block)
                .and_then(|data| data.instructions().get(position))
                .copied()
                .ok_or(SccpInvariantError::InvalidBlock { block })?;
            self.enqueue_instruction(instruction)?;
        }
        self.enqueue_terminator(block)
    }

    fn visit_instruction(&mut self, instruction: InstId) -> Result<(), SccpInvariantError> {
        let (block, _) = self
            .placement
            .instruction(instruction)
            .ok_or(SccpInvariantError::InvalidInstruction { instruction })?;
        if !self.is_block_executable(block)? {
            return Ok(());
        }
        let data = self
            .body
            .instruction(instruction)
            .ok_or(SccpInvariantError::InvalidInstruction { instruction })?;

        match data.op() {
            CoreOp::BoolConstant(value) => {
                let result = only_result(data.results(), instruction)?;
                self.join_value(result, LatticeValue::BoolConstant(*value))?;
            }
            CoreOp::I32Constant(value) => {
                let result = only_result(data.results(), instruction)?;
                self.join_value(result, LatticeValue::I32Constant(*value))?;
            }
            CoreOp::I32AddWrapping => {
                let [lhs, rhs] = two_operands(data.operands(), instruction)?;
                let fact = i32_add_wrapping(self.value_fact(lhs)?, self.value_fact(rhs)?)?;
                let result = only_result(data.results(), instruction)?;
                self.join_value(result, fact)?;
            }
            CoreOp::I32AddOverflowing => {
                let [lhs, rhs] = two_operands(data.operands(), instruction)?;
                let facts = i32_add_overflowing(self.value_fact(lhs)?, self.value_fact(rhs)?)?;
                let [sum, overflowed] = two_results(data.results(), instruction)?;
                // Both facts were computed from the same immutable operand snapshot.
                self.join_value(sum, facts[0])?;
                self.join_value(overflowed, facts[1])?;
            }
            CoreOp::I32Compare(predicate) => {
                let [lhs, rhs] = two_operands(data.operands(), instruction)?;
                let fact = i32_compare(
                    *predicate,
                    self.value_fact(lhs)?,
                    self.value_fact(rhs)?,
                    lhs == rhs,
                )?;
                let result = only_result(data.results(), instruction)?;
                self.join_value(result, fact)?;
            }
            CoreOp::BoolNot => {
                let operand = only_operand(data.operands(), instruction)?;
                let fact = bool_not(self.value_fact(operand)?)?;
                let result = only_result(data.results(), instruction)?;
                self.join_value(result, fact)?;
            }
            CoreOp::Call(_) => {
                let result_count = data.results().len();
                for result_index in 0..result_count {
                    let result = self
                        .body
                        .instruction(instruction)
                        .and_then(|data| data.results().get(result_index))
                        .copied()
                        .ok_or(SccpInvariantError::InvalidInstruction { instruction })?;
                    self.join_value(result, LatticeValue::Overdefined)?;
                }
            }
        }
        Ok(())
    }

    fn visit_terminator(&mut self, block: BlockId) -> Result<(), SccpInvariantError> {
        if !self.is_block_executable(block)? {
            return Ok(());
        }
        let terminator = self
            .body
            .block(block)
            .and_then(|data| data.terminator())
            .ok_or(SccpInvariantError::MissingTerminator { block })?;
        match terminator.kind() {
            TerminatorKind::Jump(_) => self.enqueue_edge(SccpEdgeId {
                source: block,
                successor_index: 0,
            })?,
            TerminatorKind::Branch { condition, .. } => match self.value_fact(*condition)? {
                LatticeValue::Unknown => {}
                LatticeValue::BoolConstant(value) => self.enqueue_edge(SccpEdgeId {
                    source: block,
                    successor_index: u8::from(!value),
                })?,
                LatticeValue::Overdefined => {
                    self.enqueue_edge(SccpEdgeId {
                        source: block,
                        successor_index: 0,
                    })?;
                    self.enqueue_edge(SccpEdgeId {
                        source: block,
                        successor_index: 1,
                    })?;
                }
                LatticeValue::I32Constant(_) => {
                    return Err(SccpInvariantError::LatticeType(LatticeTypeError {
                        expected: crate::ir::core::CoreType::Bool,
                        actual: crate::ir::core::CoreType::I32,
                    }));
                }
            },
            TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
        }
        Ok(())
    }

    fn activate_edge(&mut self, edge: SccpEdgeId) -> Result<(), SccpInvariantError> {
        let edge_index = self.edge_index(edge)?;
        if self.edge_states[edge_index].executable {
            return Ok(());
        }
        self.edge_states[edge_index].executable = true;
        bump(&mut self.statistics.edges_activated);

        let argument_count = self.edges[edge_index].arguments.len();
        for argument_index in 0..argument_count {
            self.enqueue_edge_argument(
                edge,
                u32::try_from(argument_index)
                    .map_err(|_| SccpInvariantError::EdgeArgumentIndexOverflow)?,
            )?;
        }
        let destination = self.edges[edge_index].destination;
        self.mark_block_executable(destination)
    }

    fn propagate_edge_argument(
        &mut self,
        edge: SccpEdgeId,
        argument_index: u32,
    ) -> Result<(), SccpInvariantError> {
        let edge_index = self.edge_index(edge)?;
        if !self.edge_states[edge_index].executable {
            return Ok(());
        }
        let argument_index = usize::try_from(argument_index)
            .map_err(|_| SccpInvariantError::EdgeArgumentIndexOverflow)?;
        let source = self.edges[edge_index]
            .arguments
            .get(argument_index)
            .copied()
            .ok_or(SccpInvariantError::InvalidEdgeArgument {
                edge,
                argument_index,
            })?;
        let destination = self.edges[edge_index].destination;
        let parameter = self
            .body
            .block(destination)
            .and_then(|block| block.parameters().get(argument_index))
            .map(BlockParam::value)
            .ok_or(SccpInvariantError::InvalidEdgeArgument {
                edge,
                argument_index,
            })?;
        let incoming = self.value_fact(source)?;
        self.join_value(parameter, incoming)?;
        bump(&mut self.statistics.edge_arguments_propagated);
        Ok(())
    }

    fn finalize_block(&mut self, block: BlockId) -> Result<bool, SccpInvariantError> {
        let parameter_count = self
            .body
            .block(block)
            .ok_or(SccpInvariantError::InvalidBlock { block })?
            .parameters()
            .len();
        for index in 0..parameter_count {
            let value = self
                .body
                .block(block)
                .and_then(|data| data.parameters().get(index))
                .map(BlockParam::value)
                .ok_or(SccpInvariantError::InvalidBlock { block })?;
            if !self.resolve_unknown(value)? {
                return Ok(false);
            }
        }

        let instruction_count = self
            .body
            .block(block)
            .ok_or(SccpInvariantError::InvalidBlock { block })?
            .instructions()
            .len();
        for instruction_position in 0..instruction_count {
            let instruction = self
                .body
                .block(block)
                .and_then(|data| data.instructions().get(instruction_position))
                .copied()
                .ok_or(SccpInvariantError::InvalidBlock { block })?;
            let result_count = self
                .body
                .instruction(instruction)
                .ok_or(SccpInvariantError::InvalidInstruction { instruction })?
                .results()
                .len();
            for result_index in 0..result_count {
                let value = self
                    .body
                    .instruction(instruction)
                    .and_then(|data| data.results().get(result_index))
                    .copied()
                    .ok_or(SccpInvariantError::InvalidInstruction { instruction })?;
                if !self.resolve_unknown(value)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    fn resolve_unknown(&mut self, value: ValueId) -> Result<bool, SccpInvariantError> {
        if self.value_fact(value)? != LatticeValue::Unknown {
            return Ok(true);
        }
        if !self.charge_fuel() {
            return Ok(false);
        }
        self.join_value(value, LatticeValue::Overdefined)?;
        bump(&mut self.statistics.pessimistic_resolutions);
        Ok(true)
    }

    fn join_value(
        &mut self,
        value: ValueId,
        incoming: LatticeValue,
    ) -> Result<(), SccpInvariantError> {
        let value_data = self
            .body
            .value(value)
            .ok_or(SccpInvariantError::InvalidValue { value })?;
        let index = value_index(value)?;
        let changed = self
            .values
            .get_mut(index)
            .ok_or(SccpInvariantError::InvalidValue { value })?
            .join(value_data.ty(), incoming)?;
        if !changed {
            return Ok(());
        }
        bump(&mut self.statistics.lattice_transitions);

        let use_count = self.uses.uses(value).len();
        for use_index in 0..use_count {
            let use_site = self.uses.uses(value)[use_index];
            match use_site {
                UseSite::InstructionOperand {
                    block, instruction, ..
                } if self.is_block_executable(block)? => {
                    self.enqueue_instruction(instruction)?;
                }
                UseSite::BranchCondition { block } if self.is_block_executable(block)? => {
                    self.enqueue_terminator(block)?;
                }
                UseSite::EdgeArgument {
                    block,
                    successor_index,
                    argument_index,
                } => {
                    let successor_index = u8::try_from(successor_index)
                        .map_err(|_| SccpInvariantError::InvalidSuccessorIndex)?;
                    let edge = SccpEdgeId {
                        source: block,
                        successor_index,
                    };
                    let edge_index = self.edge_index(edge)?;
                    if self.edge_states[edge_index].executable {
                        self.enqueue_edge_argument(
                            edge,
                            u32::try_from(argument_index)
                                .map_err(|_| SccpInvariantError::EdgeArgumentIndexOverflow)?,
                        )?;
                    }
                }
                UseSite::InstructionOperand { .. }
                | UseSite::BranchCondition { .. }
                | UseSite::Return { .. } => {}
            }
        }
        Ok(())
    }

    fn mark_block_executable(&mut self, block: BlockId) -> Result<(), SccpInvariantError> {
        if !self.placement.is_block_attached(block) {
            return Err(SccpInvariantError::InvalidBlock { block });
        }
        let index = block_index(block)?;
        let state = self
            .blocks
            .get_mut(index)
            .ok_or(SccpInvariantError::InvalidBlock { block })?;
        if state.has(BLOCK_EXECUTABLE) {
            return Ok(());
        }
        state.insert(BLOCK_EXECUTABLE);
        state.remove(BLOCK_FINALIZED);
        self.enqueue_block(block)
    }

    fn enqueue_block(&mut self, block: BlockId) -> Result<(), SccpInvariantError> {
        let index = block_index(block)?;
        let state = self
            .blocks
            .get_mut(index)
            .ok_or(SccpInvariantError::InvalidBlock { block })?;
        if !state.has(BLOCK_VISIT_QUEUED) {
            state.insert(BLOCK_VISIT_QUEUED);
            self.push_event(Event::VisitBlock(block));
        }
        Ok(())
    }

    fn enqueue_instruction(&mut self, instruction: InstId) -> Result<(), SccpInvariantError> {
        let index = instruction_index(instruction)?;
        let queued = self
            .instructions_queued
            .get_mut(index)
            .ok_or(SccpInvariantError::InvalidInstruction { instruction })?;
        if !*queued {
            *queued = true;
            self.push_event(Event::VisitInstruction(instruction));
        }
        Ok(())
    }

    fn enqueue_terminator(&mut self, block: BlockId) -> Result<(), SccpInvariantError> {
        let index = block_index(block)?;
        let state = self
            .blocks
            .get_mut(index)
            .ok_or(SccpInvariantError::InvalidBlock { block })?;
        if !state.has(BLOCK_TERMINATOR_QUEUED) {
            state.insert(BLOCK_TERMINATOR_QUEUED);
            self.push_event(Event::VisitTerminator(block));
        }
        Ok(())
    }

    fn enqueue_edge(&mut self, edge: SccpEdgeId) -> Result<(), SccpInvariantError> {
        let index = self.edge_index(edge)?;
        let state = &mut self.edge_states[index];
        if !state.executable && !state.queued {
            state.queued = true;
            self.push_event(Event::ActivateEdge(edge));
        }
        Ok(())
    }

    fn enqueue_edge_argument(
        &mut self,
        edge: SccpEdgeId,
        argument_index: u32,
    ) -> Result<(), SccpInvariantError> {
        let edge_index = self.edge_index(edge)?;
        let argument_index_usize = usize::try_from(argument_index)
            .map_err(|_| SccpInvariantError::EdgeArgumentIndexOverflow)?;
        if argument_index_usize >= self.edges[edge_index].arguments.len() {
            return Err(SccpInvariantError::InvalidEdgeArgument {
                edge,
                argument_index: argument_index_usize,
            });
        }
        let queued_index = self.edges[edge_index]
            .argument_base
            .checked_add(argument_index_usize)
            .ok_or(SccpInvariantError::EdgeArgumentIndexOverflow)?;
        let queued = self.edge_arguments_queued.get_mut(queued_index).ok_or(
            SccpInvariantError::InvalidEdgeArgument {
                edge,
                argument_index: argument_index_usize,
            },
        )?;
        if !*queued {
            *queued = true;
            self.push_event(Event::PropagateEdgeArgument(edge, argument_index));
        }
        Ok(())
    }

    fn push_event(&mut self, event: Event) {
        self.queue.push_back(event);
        let queue_len = u64::try_from(self.queue.len()).unwrap_or(u64::MAX);
        self.statistics.maximum_queue_length = self.statistics.maximum_queue_length.max(queue_len);
    }

    fn clear_queued(&mut self, event: Event) -> Result<(), SccpInvariantError> {
        match event {
            Event::VisitBlock(block) => {
                let index = block_index(block)?;
                self.blocks
                    .get_mut(index)
                    .ok_or(SccpInvariantError::InvalidBlock { block })?
                    .remove(BLOCK_VISIT_QUEUED);
            }
            Event::VisitInstruction(instruction) => {
                let index = instruction_index(instruction)?;
                *self
                    .instructions_queued
                    .get_mut(index)
                    .ok_or(SccpInvariantError::InvalidInstruction { instruction })? = false;
            }
            Event::VisitTerminator(block) => {
                let index = block_index(block)?;
                self.blocks
                    .get_mut(index)
                    .ok_or(SccpInvariantError::InvalidBlock { block })?
                    .remove(BLOCK_TERMINATOR_QUEUED);
            }
            Event::ActivateEdge(edge) => {
                let index = self.edge_index(edge)?;
                self.edge_states[index].queued = false;
            }
            Event::PropagateEdgeArgument(edge, argument_index) => {
                let edge_index = self.edge_index(edge)?;
                let argument_index = usize::try_from(argument_index)
                    .map_err(|_| SccpInvariantError::EdgeArgumentIndexOverflow)?;
                let index = self.edges[edge_index]
                    .argument_base
                    .checked_add(argument_index)
                    .ok_or(SccpInvariantError::EdgeArgumentIndexOverflow)?;
                *self.edge_arguments_queued.get_mut(index).ok_or(
                    SccpInvariantError::InvalidEdgeArgument {
                        edge,
                        argument_index,
                    },
                )? = false;
            }
        }
        Ok(())
    }

    fn charge_fuel(&mut self) -> bool {
        if self.event_fuel_used >= self.event_limit {
            return false;
        }
        self.event_fuel_used += 1;
        self.statistics.event_fuel_used = self.event_fuel_used;
        true
    }

    fn value_fact(&self, value: ValueId) -> Result<LatticeValue, SccpInvariantError> {
        self.values
            .get(value_index(value)?)
            .copied()
            .ok_or(SccpInvariantError::InvalidValue { value })
    }

    fn is_block_executable(&self, block: BlockId) -> Result<bool, SccpInvariantError> {
        self.blocks
            .get(block_index(block)?)
            .map(|state| state.has(BLOCK_EXECUTABLE))
            .ok_or(SccpInvariantError::InvalidBlock { block })
    }

    fn edge_index(&self, edge: SccpEdgeId) -> Result<usize, SccpInvariantError> {
        let block_index = block_index(edge.source)?;
        let successor_index = usize::from(edge.successor_index);
        let index = self
            .edge_lookup
            .get(block_index)
            .and_then(|slots| slots.get(successor_index))
            .copied()
            .flatten()
            .ok_or(SccpInvariantError::InvalidEdge { edge })?;
        if self.edges.get(index).map(|data| data.id) != Some(edge) {
            return Err(SccpInvariantError::InvalidEdge { edge });
        }
        Ok(index)
    }

    fn validate_completion(&self) -> Result<(), SccpInvariantError> {
        if !self.queue.is_empty() {
            return Err(SccpInvariantError::IncompleteSolution);
        }
        for block in self.body.block_order().iter().copied() {
            if !self.is_block_executable(block)? {
                continue;
            }
            let data = self
                .body
                .block(block)
                .ok_or(SccpInvariantError::InvalidBlock { block })?;
            for parameter in data.parameters() {
                if self.value_fact(parameter.value())? == LatticeValue::Unknown {
                    return Err(SccpInvariantError::IncompleteSolution);
                }
            }
            for instruction in data.instructions().iter().copied() {
                let instruction_data = self
                    .body
                    .instruction(instruction)
                    .ok_or(SccpInvariantError::InvalidInstruction { instruction })?;
                for result in instruction_data.results().iter().copied() {
                    if self.value_fact(result)? == LatticeValue::Unknown {
                        return Err(SccpInvariantError::IncompleteSolution);
                    }
                }
            }

            let terminator = data
                .terminator()
                .ok_or(SccpInvariantError::MissingTerminator { block })?;
            match terminator.kind() {
                TerminatorKind::Jump(_) => {
                    self.require_executable_edge(SccpEdgeId {
                        source: block,
                        successor_index: 0,
                    })?;
                }
                TerminatorKind::Branch { condition, .. } => match self.value_fact(*condition)? {
                    LatticeValue::BoolConstant(value) => {
                        self.require_executable_edge(SccpEdgeId {
                            source: block,
                            successor_index: u8::from(!value),
                        })?;
                    }
                    LatticeValue::Overdefined => {
                        self.require_executable_edge(SccpEdgeId {
                            source: block,
                            successor_index: 0,
                        })?;
                        self.require_executable_edge(SccpEdgeId {
                            source: block,
                            successor_index: 1,
                        })?;
                    }
                    LatticeValue::Unknown | LatticeValue::I32Constant(_) => {
                        return Err(SccpInvariantError::IncompleteSolution);
                    }
                },
                TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
            }
        }
        Ok(())
    }

    fn into_completed(self) -> Result<CompletedSccpSolution<'a>, SccpInvariantError> {
        self.validate_completion()?;
        Ok(CompletedSccpSolution { solver: self })
    }

    fn require_executable_edge(&self, edge: SccpEdgeId) -> Result<(), SccpInvariantError> {
        if self.edge_states[self.edge_index(edge)?].executable {
            Ok(())
        } else {
            Err(SccpInvariantError::IncompleteSolution)
        }
    }

    fn into_decision_after_completion(mut self) -> Result<SccpDecision, SccpInvariantError> {
        let mut folded_blocks = vec![false; self.blocks.len()];
        let mut terminators = Vec::new();
        for block in self.body.block_order().iter().copied() {
            if !self.is_block_executable(block)? {
                continue;
            }
            let terminator = self
                .body
                .block(block)
                .and_then(|data| data.terminator())
                .ok_or(SccpInvariantError::MissingTerminator { block })?;
            let TerminatorKind::Branch {
                condition,
                then_target,
                else_target,
            } = terminator.kind()
            else {
                continue;
            };
            let Some(TypedCoreConstant::Bool(condition)) = self.value_fact(*condition)?.constant()
            else {
                continue;
            };
            let selected = if condition { then_target } else { else_target };
            folded_blocks[block_index(block)?] = true;
            terminators.push((
                block,
                Terminator::new(TerminatorKind::Jump(selected.clone()), terminator.origin()),
            ));
        }

        let mut values = Vec::new();
        for (value, value_data) in self.body.values() {
            let Some(constant) = self.value_fact(value)?.constant() else {
                continue;
            };
            let Some(definition_block) = self.definition_block(value_data.definition()) else {
                continue;
            };
            if !self.is_block_executable(definition_block)?
                || self.is_identical_constant_definition(value, constant)?
                || !self.has_surviving_use(value, &folded_blocks)?
            {
                continue;
            }
            values.push((value, ValueReplacement::Constant(constant)));
        }

        let dead_blocks =
            self.body
                .block_order()
                .iter()
                .copied()
                .try_fold(0_usize, |count, block| {
                    if self.is_block_executable(block)? {
                        Ok(count)
                    } else {
                        count
                            .checked_add(1)
                            .ok_or(SccpInvariantError::CountOverflow)
                    }
                })?;

        self.statistics.constants_replaced = u64::try_from(values.len()).unwrap_or(u64::MAX);
        self.statistics.branches_folded = u64::try_from(terminators.len()).unwrap_or(u64::MAX);
        self.statistics.blocks_detached = u64::try_from(dead_blocks).unwrap_or(u64::MAX);
        Ok(SccpDecision {
            terminators,
            values,
            dead_blocks,
            statistics: self.statistics,
        })
    }

    fn definition_block(&self, definition: ValueDef) -> Option<BlockId> {
        match definition {
            ValueDef::BlockParam { block, .. } if self.placement.is_block_attached(block) => {
                Some(block)
            }
            ValueDef::InstResult { instruction, .. } => self
                .placement
                .instruction(instruction)
                .map(|placed| placed.0),
            ValueDef::BlockParam { .. } => None,
        }
    }

    fn is_identical_constant_definition(
        &self,
        value: ValueId,
        constant: TypedCoreConstant,
    ) -> Result<bool, SccpInvariantError> {
        let definition = self
            .body
            .value(value)
            .ok_or(SccpInvariantError::InvalidValue { value })?
            .definition();
        let ValueDef::InstResult {
            instruction,
            result_index: 0,
        } = definition
        else {
            return Ok(false);
        };
        let data = self
            .body
            .instruction(instruction)
            .ok_or(SccpInvariantError::InvalidInstruction { instruction })?;
        Ok(data.op() == &constant.op())
    }

    fn has_surviving_use(
        &self,
        value: ValueId,
        folded_blocks: &[bool],
    ) -> Result<bool, SccpInvariantError> {
        for use_site in self.uses.uses(value) {
            let survives = match *use_site {
                UseSite::InstructionOperand { block, .. } | UseSite::Return { block, .. } => {
                    self.is_block_executable(block)?
                }
                UseSite::BranchCondition { block } => {
                    self.is_block_executable(block)?
                        && !folded_blocks
                            .get(block_index(block)?)
                            .copied()
                            .ok_or(SccpInvariantError::InvalidBlock { block })?
                }
                UseSite::EdgeArgument {
                    block,
                    successor_index,
                    ..
                } => {
                    let edge = SccpEdgeId {
                        source: block,
                        successor_index: u8::try_from(successor_index)
                            .map_err(|_| SccpInvariantError::InvalidSuccessorIndex)?,
                    };
                    self.edge_states[self.edge_index(edge)?].executable
                }
            };
            if survives {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn add_edge<'a>(
    edges: &mut Vec<EdgeData<'a>>,
    lookup: &mut [[Option<usize>; 2]],
    source: BlockId,
    successor_index: u8,
    target: &'a BlockTarget,
    next_argument: &mut usize,
) -> Result<(), SccpInvariantError> {
    let id = SccpEdgeId {
        source,
        successor_index,
    };
    let source_index = block_index(source)?;
    let slot = lookup
        .get_mut(source_index)
        .and_then(|slots| slots.get_mut(usize::from(successor_index)))
        .ok_or(SccpInvariantError::InvalidEdge { edge: id })?;
    if slot.is_some() {
        return Err(SccpInvariantError::DuplicateEdge { edge: id });
    }
    *slot = Some(edges.len());
    let argument_base = *next_argument;
    *next_argument = next_argument
        .checked_add(target.arguments().len())
        .ok_or(SccpInvariantError::CountOverflow)?;
    edges.push(EdgeData {
        id,
        destination: target.block(),
        arguments: target.arguments(),
        argument_base,
    });
    Ok(())
}

fn count_uses(uses: &UseIndex<'_>, body: &FunctionBody) -> usize {
    body.values().map(|(value, _)| uses.uses(value).len()).sum()
}

fn only_operand(operands: &[ValueId], instruction: InstId) -> Result<ValueId, SccpInvariantError> {
    match operands {
        [operand] => Ok(*operand),
        _ => Err(SccpInvariantError::InvalidInstruction { instruction }),
    }
}

fn two_operands(
    operands: &[ValueId],
    instruction: InstId,
) -> Result<[ValueId; 2], SccpInvariantError> {
    match operands {
        [lhs, rhs] => Ok([*lhs, *rhs]),
        _ => Err(SccpInvariantError::InvalidInstruction { instruction }),
    }
}

fn only_result(results: &[ValueId], instruction: InstId) -> Result<ValueId, SccpInvariantError> {
    match results {
        [result] => Ok(*result),
        _ => Err(SccpInvariantError::InvalidInstruction { instruction }),
    }
}

fn two_results(
    results: &[ValueId],
    instruction: InstId,
) -> Result<[ValueId; 2], SccpInvariantError> {
    match results {
        [first, second] => Ok([*first, *second]),
        _ => Err(SccpInvariantError::InvalidInstruction { instruction }),
    }
}

fn block_index(block: BlockId) -> Result<usize, SccpInvariantError> {
    usize::try_from(block.index()).map_err(|_| SccpInvariantError::InvalidBlock { block })
}

fn instruction_index(instruction: InstId) -> Result<usize, SccpInvariantError> {
    usize::try_from(instruction.index())
        .map_err(|_| SccpInvariantError::InvalidInstruction { instruction })
}

fn value_index(value: ValueId) -> Result<usize, SccpInvariantError> {
    usize::try_from(value.index()).map_err(|_| SccpInvariantError::InvalidValue { value })
}

fn bump(counter: &mut u64) {
    *counter = counter.saturating_add(1);
}

#[cfg(test)]
pub(super) struct TestSolution {
    pub(super) values: Vec<LatticeValue>,
    pub(super) blocks: Vec<bool>,
    pub(super) edges: Vec<(SccpEdgeId, bool)>,
    pub(super) statistics: SccpStatistics,
}

#[cfg(test)]
pub(super) fn solve_facts_for_test(
    body: &FunctionBody,
) -> Result<TestSolution, SccpInvariantError> {
    let preflight = match Preflight::scan(body) {
        Ok(preflight) => preflight,
        Err(PreflightError::Invariant(error)) => return Err(error),
        Err(PreflightError::Limit(_)) => return Err(SccpInvariantError::CountOverflow),
    };
    let statistics = SccpStatistics {
        table_entries: preflight.table_entries,
        ..SccpStatistics::default()
    };
    let event_limit = preflight.event_upper_bound;
    let mut solver = Solver::new(body, preflight, event_limit, statistics)?;
    solver.initialize()?;
    if !solver.run_to_completion()? {
        return Err(SccpInvariantError::IncompleteSolution);
    }
    let completed = solver.into_completed()?;
    let solver = completed.solver;
    Ok(TestSolution {
        values: solver.values,
        blocks: solver
            .blocks
            .iter()
            .map(|state| state.has(BLOCK_EXECUTABLE))
            .collect(),
        edges: solver
            .edges
            .iter()
            .zip(&solver.edge_states)
            .map(|(edge, state)| (edge.id, state.executable))
            .collect(),
        statistics: solver.statistics,
    })
}

impl From<LatticeTypeError> for SccpInvariantError {
    fn from(error: LatticeTypeError) -> Self {
        Self::LatticeType(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BLOCK_EXECUTABLE, BLOCK_FINALIZED, LatticeValue, Preflight, SccpInvariantError,
        SccpStatistics, Solver, block_index,
    };
    use crate::ir::core::{CoreProgram, CoreType, FunctionBuilder, Terminator, TerminatorKind};
    use crate::source::{OriginId, SourceContext};

    #[test]
    fn synthetic_unknown_executable_result_requires_pessimistic_completion() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("synthetic-completion"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let result = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let preflight = Preflight::scan(&body).unwrap();

        let mut completed = Solver::new(&body, preflight, 1, SccpStatistics::default()).unwrap();
        let entry_index = block_index(body.entry()).unwrap();
        // Bypass normal block seeding to model a solver state left unresolved when
        // the ordinary queue drains.  Only the completion routine may publish it.
        completed.blocks[entry_index].insert(BLOCK_EXECUTABLE);
        assert!(completed.run_to_completion().unwrap());
        assert_eq!(
            completed.value_fact(result).unwrap(),
            LatticeValue::Overdefined
        );
        assert!(completed.blocks[entry_index].has(BLOCK_FINALIZED));
        assert_eq!(completed.statistics.pessimistic_resolutions, 1);
        completed.into_completed().unwrap();

        let mut interrupted = Solver::new(&body, preflight, 0, SccpStatistics::default()).unwrap();
        interrupted.blocks[entry_index].insert(BLOCK_EXECUTABLE);
        assert!(!interrupted.run_to_completion().unwrap());
        assert_eq!(
            interrupted.value_fact(result).unwrap(),
            LatticeValue::Unknown
        );
        assert!(matches!(
            interrupted.into_completed(),
            Err(SccpInvariantError::IncompleteSolution)
        ));
    }
}
