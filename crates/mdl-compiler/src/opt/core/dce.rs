//! Bounded, deterministic dead-instruction elimination for verified Core.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use crate::entity::EntityId;
use crate::ir::core::{
    CoreProgram, FunctionBody, FunctionEditor, FunctionId, InstId, TerminatorKind, ValueDef,
    ValueId,
};
use crate::source::SourceContext;

const ATTACHED: u8 = 1 << 0;
const QUEUED: u8 = 1 << 1;
const SELECTED: u8 = 1 << 2;

/// Whether one pass-local limit is derived or explicitly supplied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DceLimit<T> {
    Derived,
    Explicit(T),
}

/// Maximum combined allocated value and instruction scratch entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DceTableLimit(usize);

impl DceTableLimit {
    #[must_use]
    pub(super) const fn new(entries: usize) -> Self {
        Self(entries)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Maximum complete candidate transitions admitted from the FIFO.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DceCandidateLimit(usize);

impl DceCandidateLimit {
    #[must_use]
    pub(super) const fn new(candidates: usize) -> Self {
        Self(candidates)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Pass-local typed limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DceLimits {
    table_entries: DceLimit<DceTableLimit>,
    candidate_visits: DceLimit<DceCandidateLimit>,
}

impl DceLimits {
    pub(super) const fn derived() -> Self {
        Self {
            table_entries: DceLimit::Derived,
            candidate_visits: DceLimit::Derived,
        }
    }

    pub(super) const fn with_table_limit(mut self, limit: DceTableLimit) -> Self {
        self.table_entries = DceLimit::Explicit(limit);
        self
    }

    pub(super) const fn with_candidate_limit(mut self, limit: DceCandidateLimit) -> Self {
        self.candidate_visits = DceLimit::Explicit(limit);
        self
    }
}

/// The typed reason DCE stopped conservatively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DceLimitReason {
    DenseTables,
    CandidateVisits,
}

/// Whether DCE reached its instruction-only fixed point.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DceCompletion {
    Complete,
    StoppedAtLimit(DceLimitReason),
}

/// Deterministic pass counters without retained site records.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct DceStatistics {
    pub(super) attached_uses_counted: usize,
    pub(super) candidate_visits: usize,
    pub(super) instructions_erased: usize,
}

/// Result of one DCE invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DceOutcome {
    pub(super) changed: bool,
    pub(super) completion: DceCompletion,
    pub(super) statistics: DceStatistics,
}

/// Failure of mandatory checked arithmetic, a verified-body invariant, or editing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DceError {
    SizeOverflow,
    InconsistentVerifiedBody(&'static str),
    Edit(crate::ir::core::EditError),
}

impl fmt::Display for DceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for DceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Edit(error) => Some(error),
            Self::SizeOverflow | Self::InconsistentVerifiedBody(_) => None,
        }
    }
}

impl From<crate::ir::core::EditError> for DceError {
    fn from(error: crate::ir::core::EditError) -> Self {
        Self::Edit(error)
    }
}

/// Removes a bounded closed set of unused `Pure + Always` instructions.
pub(super) fn run_dce(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    limits: DceLimits,
) -> Result<DceOutcome, DceError> {
    let instruction_count = body.instructions.len();
    let value_count = body.values.len();
    let dense_entries = value_count
        .checked_add(instruction_count)
        .ok_or(DceError::SizeOverflow)?;
    if matches!(
        limits.table_entries,
        DceLimit::Explicit(limit) if dense_entries > limit.get()
    ) {
        return Ok(DceOutcome {
            changed: false,
            completion: DceCompletion::StoppedAtLimit(DceLimitReason::DenseTables),
            statistics: DceStatistics::default(),
        });
    }

    let mut remaining_uses = vec![0_usize; value_count];
    let mut instruction_state = vec![0_u8; instruction_count];
    let mut statistics = DceStatistics::default();
    let attached_instruction_count = count_attached_uses(
        body,
        &mut remaining_uses,
        &mut instruction_state,
        &mut statistics,
    )?;

    let mut worklist = VecDeque::new();
    seed_dead_instructions(body, &remaining_uses, &mut instruction_state, &mut worklist)?;
    let candidate_limit = match limits.candidate_visits {
        DceLimit::Derived => attached_instruction_count,
        DceLimit::Explicit(limit) => limit.get(),
    };
    let (selected, completion) = discover_dead_instructions(
        body,
        remaining_uses,
        instruction_state,
        worklist,
        candidate_limit,
        &mut statistics,
    )?;
    statistics.instructions_erased = selected.len();
    let changed = !selected.is_empty();
    if changed {
        FunctionEditor::from_trusted_body(program, sources, function, body)
            .erase_discardable_inst_set(selected)?;
    }
    Ok(DceOutcome {
        changed,
        completion,
        statistics,
    })
}

fn discover_dead_instructions(
    body: &FunctionBody,
    mut remaining_uses: Vec<usize>,
    mut instruction_state: Vec<u8>,
    mut worklist: VecDeque<InstId>,
    candidate_limit: usize,
    statistics: &mut DceStatistics,
) -> Result<(Vec<InstId>, DceCompletion), DceError> {
    let mut selected = vec![];
    while let Some(instruction) = worklist.front().copied() {
        if statistics.candidate_visits == candidate_limit {
            break;
        }
        worklist.pop_front();
        statistics.candidate_visits += 1;
        let candidate_index = instruction_index(instruction).ok_or(
            DceError::InconsistentVerifiedBody("DCE candidate has an invalid instruction ID"),
        )?;
        let state = instruction_state.get_mut(candidate_index).ok_or(
            DceError::InconsistentVerifiedBody("DCE candidate is outside dense instruction state"),
        )?;
        if *state & ATTACHED == 0 || *state & SELECTED != 0 {
            return Err(DceError::InconsistentVerifiedBody(
                "DCE worklist contains a detached or repeated candidate",
            ));
        }
        *state |= SELECTED;
        selected.push(instruction);

        let data = body
            .instruction(instruction)
            .ok_or(DceError::InconsistentVerifiedBody(
                "DCE candidate instruction is absent",
            ))?;
        if !data.op().is_trivially_discardable() || !all_results_unused(data, &remaining_uses)? {
            return Err(DceError::InconsistentVerifiedBody(
                "DCE candidate ceased to satisfy its virtual deadness proof",
            ));
        }
        for operand in data.operands() {
            let count = value_index(*operand)
                .and_then(|index| remaining_uses.get_mut(index))
                .ok_or(DceError::InconsistentVerifiedBody(
                    "DCE candidate references an invalid operand",
                ))?;
            *count = count
                .checked_sub(1)
                .ok_or(DceError::InconsistentVerifiedBody(
                    "DCE virtual use count underflowed",
                ))?;
        }
        enqueue_newly_dead_producers(
            body,
            data.operands(),
            &remaining_uses,
            &mut instruction_state,
            &mut worklist,
        )?;
    }
    let completion = if worklist.is_empty() {
        DceCompletion::Complete
    } else {
        DceCompletion::StoppedAtLimit(DceLimitReason::CandidateVisits)
    };
    Ok((selected, completion))
}

fn enqueue_newly_dead_producers(
    body: &FunctionBody,
    operands: &[ValueId],
    remaining_uses: &[usize],
    instruction_state: &mut [u8],
    worklist: &mut VecDeque<InstId>,
) -> Result<(), DceError> {
    // Every decrement is complete before producers are considered in first-
    // operand order, including duplicate operand occurrences.
    for operand in operands {
        let Some(producer) = defining_instruction(body, *operand) else {
            continue;
        };
        let producer_index = instruction_index(producer).ok_or(
            DceError::InconsistentVerifiedBody("DCE producer has an invalid instruction ID"),
        )?;
        let producer_state =
            instruction_state
                .get_mut(producer_index)
                .ok_or(DceError::InconsistentVerifiedBody(
                    "DCE producer is outside dense state",
                ))?;
        if *producer_state & (ATTACHED | QUEUED) != ATTACHED {
            continue;
        }
        let producer_data =
            body.instruction(producer)
                .ok_or(DceError::InconsistentVerifiedBody(
                    "DCE producer instruction is absent",
                ))?;
        if producer_data.op().is_trivially_discardable()
            && all_results_unused(producer_data, remaining_uses)?
        {
            *producer_state |= QUEUED;
            worklist.push_back(producer);
        }
    }
    Ok(())
}

fn count_attached_uses(
    body: &FunctionBody,
    remaining_uses: &mut [usize],
    instruction_state: &mut [u8],
    statistics: &mut DceStatistics,
) -> Result<usize, DceError> {
    let mut attached_instruction_count = 0_usize;
    let mut count = |value: ValueId| -> Result<(), DceError> {
        let uses = value_index(value)
            .and_then(|index| remaining_uses.get_mut(index))
            .ok_or(DceError::InconsistentVerifiedBody(
                "attached use references an invalid value",
            ))?;
        *uses = uses.checked_add(1).ok_or(DceError::SizeOverflow)?;
        statistics.attached_uses_counted = statistics
            .attached_uses_counted
            .checked_add(1)
            .ok_or(DceError::SizeOverflow)?;
        Ok(())
    };

    for block in body.block_order() {
        let data = body
            .block(*block)
            .ok_or(DceError::InconsistentVerifiedBody(
                "attached DCE block is absent",
            ))?;
        for instruction in data.instructions() {
            let index = instruction_index(*instruction).ok_or(
                DceError::InconsistentVerifiedBody("attached instruction has an invalid ID"),
            )?;
            let state =
                instruction_state
                    .get_mut(index)
                    .ok_or(DceError::InconsistentVerifiedBody(
                        "attached instruction is outside dense state",
                    ))?;
            if *state & ATTACHED != 0 {
                return Err(DceError::InconsistentVerifiedBody(
                    "verified instruction is attached more than once",
                ));
            }
            *state |= ATTACHED;
            attached_instruction_count = attached_instruction_count
                .checked_add(1)
                .ok_or(DceError::SizeOverflow)?;
            let instruction_data =
                body.instruction(*instruction)
                    .ok_or(DceError::InconsistentVerifiedBody(
                        "attached instruction is absent",
                    ))?;
            for operand in instruction_data.operands() {
                count(*operand)?;
            }
        }
        let terminator = data.terminator().ok_or(DceError::InconsistentVerifiedBody(
            "attached block has no terminator",
        ))?;
        for_each_terminator_value(terminator.kind(), &mut count)?;
    }
    Ok(attached_instruction_count)
}

fn seed_dead_instructions(
    body: &FunctionBody,
    remaining_uses: &[usize],
    instruction_state: &mut [u8],
    worklist: &mut VecDeque<InstId>,
) -> Result<(), DceError> {
    for block in body.block_order() {
        let data = body
            .block(*block)
            .ok_or(DceError::InconsistentVerifiedBody(
                "attached DCE block is absent",
            ))?;
        for instruction in data.instructions() {
            let instruction_data =
                body.instruction(*instruction)
                    .ok_or(DceError::InconsistentVerifiedBody(
                        "attached instruction is absent",
                    ))?;
            if !instruction_data.op().is_trivially_discardable()
                || !all_results_unused(instruction_data, remaining_uses)?
            {
                continue;
            }
            let state = instruction_index(*instruction)
                .and_then(|index| instruction_state.get_mut(index))
                .ok_or(DceError::InconsistentVerifiedBody(
                    "attached instruction is outside dense state",
                ))?;
            if *state & ATTACHED == 0 {
                return Err(DceError::InconsistentVerifiedBody(
                    "DCE seed is not attached",
                ));
            }
            *state |= QUEUED;
            worklist.push_back(*instruction);
        }
    }
    Ok(())
}

fn all_results_unused(
    data: &crate::ir::core::InstData,
    remaining_uses: &[usize],
) -> Result<bool, DceError> {
    for result in data.results() {
        let uses = value_index(*result)
            .and_then(|index| remaining_uses.get(index))
            .copied()
            .ok_or(DceError::InconsistentVerifiedBody(
                "instruction result is outside dense use counts",
            ))?;
        if uses != 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn defining_instruction(body: &FunctionBody, value: ValueId) -> Option<InstId> {
    match body.value(value)?.definition() {
        ValueDef::InstResult { instruction, .. } => Some(instruction),
        ValueDef::BlockParam { .. } => None,
    }
}

fn for_each_terminator_value(
    kind: &TerminatorKind,
    visit: &mut impl FnMut(ValueId) -> Result<(), DceError>,
) -> Result<(), DceError> {
    match kind {
        TerminatorKind::Jump(target) => {
            for value in target.arguments() {
                visit(*value)?;
            }
        }
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => {
            visit(*condition)?;
            for value in then_target.arguments() {
                visit(*value)?;
            }
            for value in else_target.arguments() {
                visit(*value)?;
            }
        }
        TerminatorKind::Return(values) => {
            for value in values {
                visit(*value)?;
            }
        }
        TerminatorKind::Unreachable => {}
    }
    Ok(())
}

fn value_index(value: ValueId) -> Option<usize> {
    usize::try_from(value.index()).ok()
}

fn instruction_index(instruction: InstId) -> Option<usize> {
    usize::try_from(instruction.index()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreType, FunctionBuilder, PlacementIndex, Terminator,
        TerminatorKind, ValueId, verify_function,
    };
    use crate::source::OriginId;

    fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
        builder.body().block(block).unwrap().parameters()[index].value()
    }

    fn defining_instruction(body: &FunctionBody, value: ValueId) -> InstId {
        match body.value(value).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => panic!("expected instruction result"),
        }
    }

    fn dead_duplicate_operand_chain() -> (
        SourceContext,
        CoreProgram,
        FunctionId,
        FunctionBody,
        [InstId; 3],
    ) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("dead-chain"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let duplicate = builder
            .i32_add_wrapping(one, one, OriginId::UNKNOWN)
            .unwrap();
        let leaf = builder
            .i32_add_wrapping(duplicate, one, OriginId::UNKNOWN)
            .unwrap();
        let instructions =
            [one, duplicate, leaf].map(|value| defining_instruction(builder.body(), value));
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        (sources, program, function, body, instructions)
    }

    fn naive_use_counts(body: &FunctionBody) -> Vec<usize> {
        let mut counts = vec![0_usize; body.value_counts().allocated];
        let mut count = |value: ValueId| {
            counts[usize::try_from(value.index()).unwrap()] += 1;
        };
        for block in body.block_order() {
            let data = body.block(*block).unwrap();
            for instruction in data.instructions() {
                for operand in body.instruction(*instruction).unwrap().operands() {
                    count(*operand);
                }
            }
            match data.terminator().unwrap().kind() {
                TerminatorKind::Jump(target) => {
                    for value in target.arguments() {
                        count(*value);
                    }
                }
                TerminatorKind::Branch {
                    condition,
                    then_target,
                    else_target,
                } => {
                    count(*condition);
                    for value in then_target.arguments() {
                        count(*value);
                    }
                    for value in else_target.arguments() {
                        count(*value);
                    }
                }
                TerminatorKind::Return(values) => {
                    for value in values {
                        count(*value);
                    }
                }
                TerminatorKind::Unreachable => {}
            }
        }
        counts
    }

    fn run_naive_oracle(
        program: &CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
        body: &mut FunctionBody,
    ) {
        loop {
            let counts = naive_use_counts(body);
            let mut candidate = None;
            'search: for block in body.block_order() {
                for instruction in body.block(*block).unwrap().instructions() {
                    let data = body.instruction(*instruction).unwrap();
                    if data.op().is_trivially_discardable()
                        && data
                            .results()
                            .iter()
                            .all(|result| counts[usize::try_from(result.index()).unwrap()] == 0)
                    {
                        candidate = Some(*instruction);
                        break 'search;
                    }
                }
            }
            let Some(candidate) = candidate else {
                break;
            };
            FunctionEditor::from_trusted_body(program, sources, function, body)
                .erase_discardable_inst_set(vec![candidate])
                .unwrap();
        }
    }

    #[test]
    fn removes_duplicate_operand_chains_and_retains_stable_raw_data() {
        let (sources, program, function, mut body, instructions) = dead_duplicate_operand_chain();
        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.completion, DceCompletion::Complete);
        assert_eq!(outcome.statistics.attached_uses_counted, 4);
        assert_eq!(outcome.statistics.candidate_visits, 3);
        assert_eq!(outcome.statistics.instructions_erased, 3);
        let placement = PlacementIndex::new(&body);
        for instruction in instructions {
            assert!(!placement.is_instruction_attached(instruction));
            assert!(body.instruction(instruction).is_some());
        }
        verify_function(&program, &sources, function, &body).unwrap();

        let second = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert!(!second.changed);
        assert_eq!(second.completion, DceCompletion::Complete);
    }

    #[test]
    fn candidate_limit_applies_one_whole_closed_transition() {
        let (sources, program, function, original, instructions) = dead_duplicate_operand_chain();
        let mut body = original.clone();
        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived().with_candidate_limit(DceCandidateLimit::new(1)),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(
            outcome.completion,
            DceCompletion::StoppedAtLimit(DceLimitReason::CandidateVisits)
        );
        assert_eq!(outcome.statistics.candidate_visits, 1);
        let placement = PlacementIndex::new(&body);
        assert!(placement.is_instruction_attached(instructions[0]));
        assert!(placement.is_instruction_attached(instructions[1]));
        assert!(!placement.is_instruction_attached(instructions[2]));
        verify_function(&program, &sources, function, &body).unwrap();

        let remainder = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert_eq!(remainder.statistics.instructions_erased, 2);
        verify_function(&program, &sources, function, &body).unwrap();

        let mut exact = original;
        let exact_outcome = run_dce(
            &program,
            &sources,
            function,
            &mut exact,
            DceLimits::derived().with_candidate_limit(DceCandidateLimit::new(3)),
        )
        .unwrap();
        assert!(exact_outcome.changed);
        assert_eq!(exact_outcome.completion, DceCompletion::Complete);
        assert_eq!(exact_outcome.statistics.candidate_visits, 3);
        verify_function(&program, &sources, function, &exact).unwrap();
    }

    #[test]
    fn scratch_limit_returns_the_exact_unchanged_body() {
        let (sources, program, function, original, _) = dead_duplicate_operand_chain();
        let dense_entries =
            original.value_counts().allocated + original.instruction_counts().allocated;
        let mut body = original.clone();
        let before = format!("{body:#?}");
        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived().with_table_limit(DceTableLimit::new(0)),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(
            outcome.completion,
            DceCompletion::StoppedAtLimit(DceLimitReason::DenseTables)
        );
        assert_eq!(format!("{body:#?}"), before);

        let mut exact = original;
        let exact_outcome = run_dce(
            &program,
            &sources,
            function,
            &mut exact,
            DceLimits::derived().with_table_limit(DceTableLimit::new(dense_entries)),
        )
        .unwrap();
        assert!(exact_outcome.changed);
        assert_eq!(exact_outcome.completion, DceCompletion::Complete);
        assert_eq!(exact_outcome.statistics.instructions_erased, 3);
        verify_function(&program, &sources, function, &exact).unwrap();
    }

    #[test]
    fn calls_remain_and_keep_argument_producers_live() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let callee = program
            .declare_function(
                Some("callee"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = program
            .declare_function(Some("caller"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let argument = builder.i32_constant(11, OriginId::UNKNOWN).unwrap();
        let argument_instruction = defining_instruction(builder.body(), argument);
        let call_result = builder
            .call(callee, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
        let call_instruction = defining_instruction(builder.body(), call_result);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        let placement = PlacementIndex::new(&body);
        assert!(placement.is_instruction_attached(argument_instruction));
        assert!(placement.is_instruction_attached(call_instruction));
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn one_live_overflow_result_keeps_the_complete_instruction_and_operands() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("overflow"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let left = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (sum, overflow) = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap();
        let instructions = [
            defining_instruction(builder.body(), left),
            defining_instruction(builder.body(), right),
            defining_instruction(builder.body(), sum),
        ];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![overflow]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        let placement = PlacementIndex::new(&body);
        assert!(
            instructions
                .into_iter()
                .all(|instruction| placement.is_instruction_attached(instruction))
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn branch_condition_and_each_edge_occurrence_are_liveness_roots() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("terminator-roots"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let condition = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let argument = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let condition_instruction = defining_instruction(builder.body(), condition);
        let argument_instruction = defining_instruction(builder.body(), argument);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(join, vec![argument]),
                    else_target: BlockTarget::new(join, vec![argument]),
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
        let mut body = builder.finish().unwrap();

        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(outcome.statistics.attached_uses_counted, 4);
        let placement = PlacementIndex::new(&body);
        assert!(placement.is_instruction_attached(condition_instruction));
        assert!(placement.is_instruction_attached(argument_instruction));
        assert_eq!(body.entry(), entry);
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn worklist_fixed_point_matches_an_independent_naive_rescan_oracle() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("oracle"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = parameter(&builder, entry, 0);
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let live = builder
            .i32_add_wrapping(input, zero, OriginId::UNKNOWN)
            .unwrap();
        let dead_left = builder
            .i32_add_wrapping(input, input, OriginId::UNKNOWN)
            .unwrap();
        let dead_right = builder
            .i32_add_wrapping(input, input, OriginId::UNKNOWN)
            .unwrap();
        let shared = builder
            .i32_add_wrapping(dead_left, dead_right, OriginId::UNKNOWN)
            .unwrap();
        let _leaf = builder
            .i32_add_wrapping(shared, dead_left, OriginId::UNKNOWN)
            .unwrap();
        let _independent = builder.i32_constant(99, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![live]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let original = builder.finish().unwrap();
        let mut worklist = original.clone();
        let mut naive = original;

        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut worklist,
            DceLimits::derived(),
        )
        .unwrap();
        run_naive_oracle(&program, &sources, function, &mut naive);
        assert!(outcome.changed);
        assert_eq!(format!("{worklist:#?}"), format!("{naive:#?}"));
        verify_function(&program, &sources, function, &worklist).unwrap();
    }

    #[test]
    fn twenty_thousand_instruction_chain_uses_no_host_recursion() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("deep"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let mut value = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        for _ in 0..20_000 {
            value = builder.bool_not(value, OriginId::UNKNOWN).unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.completion, DceCompletion::Complete);
        assert_eq!(outcome.statistics.instructions_erased, 20_001);
        assert_eq!(body.instruction_counts().attached, 0);
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn block_parameters_are_not_instruction_producers() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("parameter"),
                vec![CoreType::I32],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = parameter(&builder, entry, 0);
        let dead = builder
            .i32_add_wrapping(input, input, OriginId::UNKNOWN)
            .unwrap();
        let dead_instruction = defining_instruction(builder.body(), dead);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        run_dce(
            &program,
            &sources,
            function,
            &mut body,
            DceLimits::derived(),
        )
        .unwrap();
        assert!(!PlacementIndex::new(&body).is_instruction_attached(dead_instruction));
        verify_function(&program, &sources, function, &body).unwrap();
    }
}
