//! Bounded, deterministic sparse conditional constant propagation for Core.

mod lattice;
mod solver;

use std::error::Error;
use std::fmt;

use lattice::LatticeTypeError;
use solver::SccpEdgeId;

use crate::ir::core::{
    BlockId, EditError, FunctionEditor, InstId, Terminator, ValueId, ValueReplacement,
};

/// Whether one SCCP resource limit is derived from immutable input metadata or
/// explicitly selected by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SccpLimit<T> {
    Derived,
    Explicit(T),
}

/// Maximum logical SCCP table entries.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct SccpTableLimit(u64);

impl SccpTableLimit {
    #[must_use]
    pub(super) const fn new(entries: u64) -> Self {
        Self(entries)
    }

    const fn get(self) -> u64 {
        self.0
    }
}

/// Maximum dequeued events plus pessimistic lattice resolutions.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct SccpEventLimit(u64);

impl SccpEventLimit {
    #[must_use]
    pub(super) const fn new(events: u64) -> Self {
        Self(events)
    }

    const fn get(self) -> u64 {
        self.0
    }
}

/// Resource policy for one SCCP invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SccpLimits {
    tables: SccpLimit<SccpTableLimit>,
    events: SccpLimit<SccpEventLimit>,
}

impl SccpLimits {
    #[must_use]
    pub(super) const fn derived() -> Self {
        Self {
            tables: SccpLimit::Derived,
            events: SccpLimit::Derived,
        }
    }

    #[must_use]
    pub(super) const fn with_table_limit(mut self, limit: SccpTableLimit) -> Self {
        self.tables = SccpLimit::Explicit(limit);
        self
    }

    #[must_use]
    pub(super) const fn with_event_limit(mut self, limit: SccpEventLimit) -> Self {
        self.events = SccpLimit::Explicit(limit);
        self
    }
}

impl Default for SccpLimits {
    fn default() -> Self {
        Self::derived()
    }
}

/// Stable reason an optional SCCP analysis produced no decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SccpLimitReason {
    Tables,
    Events,
    DerivedBoundOverflow,
}

/// Bounded counters for one SCCP invocation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct SccpStatistics {
    pub(super) table_entries: u64,
    pub(super) event_fuel_used: u64,
    pub(super) events_dequeued: u64,
    pub(super) lattice_transitions: u64,
    pub(super) edges_activated: u64,
    pub(super) edge_arguments_propagated: u64,
    pub(super) pessimistic_resolutions: u64,
    pub(super) maximum_queue_length: u64,
    pub(super) constants_replaced: u64,
    pub(super) branches_folded: u64,
    pub(super) blocks_detached: u64,
}

/// Completion state for one SCCP invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SccpCompletion {
    Complete,
    StoppedAtLimit(SccpLimitReason),
}

/// Result of solving and, only on complete facts, applying SCCP once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SccpOutcome {
    changed: bool,
    completion: SccpCompletion,
    statistics: SccpStatistics,
}

impl SccpOutcome {
    #[must_use]
    pub(super) const fn changed(self) -> bool {
        self.changed
    }

    #[must_use]
    pub(super) const fn completion(self) -> SccpCompletion {
        self.completion
    }

    #[must_use]
    pub(super) const fn statistics(self) -> SccpStatistics {
        self.statistics
    }
}

/// Runs SCCP against an immutable body snapshot and applies one compact decision.
///
/// Limit exhaustion is a successful unchanged outcome.  A completed solution applies
/// branch folds before value materialization and detaches unreachable blocks last.
pub(super) fn run(
    editor: &mut FunctionEditor<'_>,
    limits: SccpLimits,
) -> Result<SccpOutcome, SccpError> {
    let solution = solver::solve(editor.body(), limits)?;
    let decision = match solution {
        SccpSolveResult::Stopped { reason, statistics } => {
            return Ok(SccpOutcome {
                changed: false,
                completion: SccpCompletion::StoppedAtLimit(reason),
                statistics,
            });
        }
        SccpSolveResult::Complete(decision) => decision,
    };

    let SccpDecision {
        terminators,
        values,
        dead_blocks,
        statistics,
    } = decision;
    let changed = !terminators.is_empty() || !values.is_empty() || dead_blocks != 0;
    editor.set_terminators_batch(terminators)?;
    editor.replace_values_batch(values)?;
    let detached = editor.detach_unreachable_blocks()?;
    if detached != dead_blocks {
        return Err(SccpInvariantError::DetachedBlockMismatch {
            planned: dead_blocks,
            actual: detached,
        }
        .into());
    }
    Ok(SccpOutcome {
        changed,
        completion: SccpCompletion::Complete,
        statistics,
    })
}

#[derive(Debug)]
pub(super) enum SccpError {
    Invariant(SccpInvariantError),
    Edit(EditError),
}

impl fmt::Display for SccpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invariant(error) => write!(formatter, "SCCP invariant failed: {error}"),
            Self::Edit(error) => write!(formatter, "SCCP application failed: {error}"),
        }
    }
}

impl Error for SccpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invariant(error) => Some(error),
            Self::Edit(error) => Some(error),
        }
    }
}

impl From<SccpInvariantError> for SccpError {
    fn from(error: SccpInvariantError) -> Self {
        Self::Invariant(error)
    }
}

impl From<EditError> for SccpError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}

#[derive(Debug)]
pub(super) enum SccpInvariantError {
    InvalidBlock {
        block: BlockId,
    },
    InvalidInstruction {
        instruction: InstId,
    },
    InvalidValue {
        value: ValueId,
    },
    MissingTerminator {
        block: BlockId,
    },
    InvalidEdge {
        edge: SccpEdgeId,
    },
    DuplicateEdge {
        edge: SccpEdgeId,
    },
    InvalidEdgeArgument {
        edge: SccpEdgeId,
        argument_index: usize,
    },
    InvalidSuccessorIndex,
    EdgeArgumentIndexOverflow,
    CountOverflow,
    PreflightMismatch,
    IncompleteSolution,
    LatticeType(LatticeTypeError),
    DetachedBlockMismatch {
        planned: usize,
        actual: usize,
    },
}

impl fmt::Display for SccpInvariantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBlock { block } => write!(formatter, "invalid block {block:?}"),
            Self::InvalidInstruction { instruction } => {
                write!(formatter, "invalid instruction {instruction:?}")
            }
            Self::InvalidValue { value } => write!(formatter, "invalid value {value:?}"),
            Self::MissingTerminator { block } => {
                write!(formatter, "attached block {block:?} has no terminator")
            }
            Self::InvalidEdge { edge } => write!(formatter, "invalid semantic edge {edge:?}"),
            Self::DuplicateEdge { edge } => {
                write!(formatter, "duplicate semantic edge {edge:?}")
            }
            Self::InvalidEdgeArgument {
                edge,
                argument_index,
            } => write!(
                formatter,
                "invalid argument {argument_index} on semantic edge {edge:?}"
            ),
            Self::InvalidSuccessorIndex => formatter.write_str("invalid successor occurrence"),
            Self::EdgeArgumentIndexOverflow => {
                formatter.write_str("edge argument index is not representable")
            }
            Self::CountOverflow => formatter.write_str("SCCP count overflowed"),
            Self::PreflightMismatch => {
                formatter.write_str("SCCP preflight and constructed tables disagree")
            }
            Self::IncompleteSolution => {
                formatter.write_str("SCCP solution did not satisfy completion invariants")
            }
            Self::LatticeType(error) => write!(
                formatter,
                "lattice constant has type {}, expected {}",
                error.actual, error.expected
            ),
            Self::DetachedBlockMismatch { planned, actual } => write!(
                formatter,
                "planned {planned} dead blocks but detached {actual}"
            ),
        }
    }
}

impl Error for SccpInvariantError {}

enum SccpSolveResult {
    Complete(SccpDecision),
    Stopped {
        reason: SccpLimitReason,
        statistics: SccpStatistics,
    },
}

struct SccpDecision {
    terminators: Vec<(BlockId, Terminator)>,
    values: Vec<(ValueId, ValueReplacement)>,
    dead_blocks: usize,
    statistics: SccpStatistics,
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use super::lattice::LatticeValue;
    use super::{SccpCompletion, SccpEventLimit, SccpLimitReason, SccpLimits, SccpTableLimit, run};
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreOp, CoreProgram, CoreType, FunctionBody, FunctionBuilder,
        FunctionEditor, FunctionId, Terminator, TerminatorKind, ValueDef, ValueId, verify_function,
    };
    use crate::source::{Origin, OriginId, SourceContext};

    trait GeneratedFixtureResult<T> {
        #[track_caller]
        fn expect_generated(self, context: &str) -> T;
    }

    impl<T, E: Debug> GeneratedFixtureResult<T> for Result<T, E> {
        #[track_caller]
        fn expect_generated(self, context: &str) -> T {
            self.unwrap_or_else(|error| {
                panic!("{context}: generated fixture construction failed: {error:?}")
            })
        }
    }

    #[track_caller]
    fn run_generated_case<T>(context: &str, case: impl FnOnce() -> T) -> T {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(case)).unwrap_or_else(|payload| {
            if let Some(message) = payload.downcast_ref::<String>() {
                panic!("{context}: generated SCCP case failed: {message}");
            }
            if let Some(message) = payload.downcast_ref::<&str>() {
                panic!("{context}: generated SCCP case failed: {message}");
            }
            panic!("{context}: generated SCCP case failed with a non-string panic payload");
        })
    }

    struct Fixture {
        program: CoreProgram,
        sources: SourceContext,
        function: FunctionId,
        body: FunctionBody,
    }

    struct SameTargetFixture {
        fixture: Fixture,
        entry: BlockId,
        join: BlockId,
        then_argument: ValueId,
        branch_origin: OriginId,
    }

    fn same_target_fixture() -> SameTargetFixture {
        let mut sources = SourceContext::new();
        let branch_origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("same-target"),
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
        let then_argument = builder.i32_constant(4, OriginId::UNKNOWN).unwrap();
        let else_argument = builder.i32_constant(9, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(join, vec![then_argument]),
                    else_target: BlockTarget::new(join, vec![else_argument]),
                },
                branch_origin,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![joined]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        SameTargetFixture {
            fixture: Fixture {
                program,
                sources,
                function,
                body,
            },
            entry,
            join,
            then_argument,
            branch_origin,
        }
    }

    fn unreachable_arm_fixture() -> (Fixture, BlockId, BlockId, BlockId) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("unreachable-arm"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
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
        for block in [then_block, else_block] {
            builder.switch_to_block(block).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        let body = builder.finish().unwrap();
        (
            Fixture {
                program,
                sources,
                function,
                body,
            },
            entry,
            then_block,
            else_block,
        )
    }

    fn run_fixture(fixture: &mut Fixture, limits: SccpLimits) -> super::SccpOutcome {
        let outcome = {
            let mut editor = FunctionEditor::new(
                &fixture.program,
                &fixture.sources,
                fixture.function,
                &mut fixture.body,
            )
            .unwrap();
            run(&mut editor, limits).unwrap()
        };
        verify_function(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &fixture.body,
        )
        .unwrap();
        outcome
    }

    #[test]
    fn same_target_arms_keep_semantic_identity_and_selected_arguments() {
        let mut case = same_target_fixture();
        let outcome = run_fixture(&mut case.fixture, SccpLimits::derived());
        assert!(outcome.changed());
        assert_eq!(outcome.completion(), SccpCompletion::Complete);
        assert_eq!(outcome.statistics().branches_folded, 1);
        assert_eq!(outcome.statistics().constants_replaced, 1);
        assert_eq!(outcome.statistics().pessimistic_resolutions, 0);

        let TerminatorKind::Jump(target) = case
            .fixture
            .body
            .block(case.entry)
            .unwrap()
            .terminator()
            .unwrap()
            .kind()
        else {
            panic!("constant branch was not folded");
        };
        assert_eq!(target.block(), case.join);
        assert_eq!(target.arguments(), &[case.then_argument]);
        assert_eq!(
            case.fixture
                .body
                .block(case.entry)
                .unwrap()
                .terminator()
                .unwrap()
                .origin(),
            case.branch_origin
        );

        let TerminatorKind::Return(values) = case
            .fixture
            .body
            .block(case.join)
            .unwrap()
            .terminator()
            .unwrap()
            .kind()
        else {
            panic!("join no longer returns");
        };
        let ValueDef::InstResult { instruction, .. } =
            case.fixture.body.value(values[0]).unwrap().definition()
        else {
            panic!("constant replacement was not materialized");
        };
        assert_eq!(
            case.fixture.body.instruction(instruction).unwrap().op(),
            &CoreOp::I32Constant(4)
        );
    }

    #[test]
    fn folded_control_flow_detaches_only_the_unselected_arm() {
        let (mut fixture, entry, then_block, else_block) = unreachable_arm_fixture();
        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert!(outcome.changed());
        assert_eq!(outcome.statistics().branches_folded, 1);
        assert_eq!(outcome.statistics().blocks_detached, 1);
        assert_eq!(fixture.body.block_order(), &[entry, then_block]);
        assert!(
            fixture.body.block(else_block).is_some(),
            "stable raw ID was lost"
        );
    }

    #[test]
    fn zero_and_too_small_limits_leave_the_body_byte_for_byte_unchanged() {
        let mut table_case = same_target_fixture().fixture;
        let before = format!("{:?}", table_case.body);
        let table_outcome = run_fixture(
            &mut table_case,
            SccpLimits::derived().with_table_limit(SccpTableLimit::new(0)),
        );
        assert_eq!(
            table_outcome.completion(),
            SccpCompletion::StoppedAtLimit(SccpLimitReason::Tables)
        );
        assert!(!table_outcome.changed());
        assert_eq!(format!("{:?}", table_case.body), before);

        let mut event_case = same_target_fixture().fixture;
        let before = format!("{:?}", event_case.body);
        let event_outcome = run_fixture(
            &mut event_case,
            SccpLimits::derived().with_event_limit(SccpEventLimit::new(0)),
        );
        assert_eq!(
            event_outcome.completion(),
            SccpCompletion::StoppedAtLimit(SccpLimitReason::Events)
        );
        assert!(!event_outcome.changed());
        assert_eq!(format!("{:?}", event_case.body), before);
    }

    #[test]
    fn exact_event_boundary_completes_but_one_less_discards_every_fact() {
        let mut reference = same_target_fixture().fixture;
        let complete = run_fixture(&mut reference, SccpLimits::derived());
        let exact = complete.statistics().event_fuel_used;
        assert!(exact > 0);

        let mut short = same_target_fixture().fixture;
        let before = format!("{:?}", short.body);
        let short_outcome = run_fixture(
            &mut short,
            SccpLimits::derived().with_event_limit(SccpEventLimit::new(exact - 1)),
        );
        assert_eq!(
            short_outcome.completion(),
            SccpCompletion::StoppedAtLimit(SccpLimitReason::Events)
        );
        assert_eq!(format!("{:?}", short.body), before);

        let mut boundary = same_target_fixture().fixture;
        let boundary_outcome = run_fixture(
            &mut boundary,
            SccpLimits::derived().with_event_limit(SccpEventLimit::new(exact)),
        );
        assert_eq!(boundary_outcome.completion(), SccpCompletion::Complete);
        assert_eq!(boundary_outcome.statistics().event_fuel_used, exact);
    }

    #[test]
    fn exact_table_boundary_completes_but_one_less_allocates_no_solver_tables() {
        let mut reference = same_target_fixture().fixture;
        let complete = run_fixture(&mut reference, SccpLimits::derived());
        let required = complete.statistics().table_entries;
        assert!(required > 0);

        let mut short = same_target_fixture().fixture;
        let before = format!("{:?}", short.body);
        let short_outcome = run_fixture(
            &mut short,
            SccpLimits::derived().with_table_limit(SccpTableLimit::new(required - 1)),
        );
        assert_eq!(
            short_outcome.completion(),
            SccpCompletion::StoppedAtLimit(SccpLimitReason::Tables)
        );
        assert_eq!(format!("{:?}", short.body), before);

        let mut boundary = same_target_fixture().fixture;
        let boundary_outcome = run_fixture(
            &mut boundary,
            SccpLimits::derived().with_table_limit(SccpTableLimit::new(required)),
        );
        assert_eq!(boundary_outcome.completion(), SccpCompletion::Complete);
        assert_eq!(boundary_outcome.statistics().table_entries, required);
    }

    #[test]
    fn table_admission_covers_a_fifo_larger_than_the_reported_aggregate() {
        let make_fixture = || {
            let sources = SourceContext::new();
            let mut program = CoreProgram::new();
            let function = program
                .declare_function(Some("resultless"), vec![], vec![], OriginId::UNKNOWN)
                .unwrap();
            let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            let body = builder.finish().unwrap();
            Fixture {
                program,
                sources,
                function,
                body,
            }
        };

        // One allocated block is the sole documented table entry.  Its separately
        // queued block and terminator visits make the exact FIFO bound two slots.
        let mut short = make_fixture();
        let before = format!("{:?}", short.body);
        let stopped = run_fixture(
            &mut short,
            SccpLimits::derived().with_table_limit(SccpTableLimit::new(1)),
        );
        assert_eq!(stopped.statistics().table_entries, 1);
        assert_eq!(stopped.statistics().event_fuel_used, 0);
        assert_eq!(stopped.statistics().maximum_queue_length, 0);
        assert_eq!(
            stopped.completion(),
            SccpCompletion::StoppedAtLimit(SccpLimitReason::Tables)
        );
        assert_eq!(format!("{:?}", short.body), before);

        let mut boundary = make_fixture();
        let complete = run_fixture(
            &mut boundary,
            SccpLimits::derived().with_table_limit(SccpTableLimit::new(2)),
        );
        assert_eq!(complete.completion(), SccpCompletion::Complete);
        assert_eq!(complete.statistics().table_entries, 1);
        assert_eq!(
            complete.statistics().maximum_queue_length,
            1,
            "the two-slot admission requirement bounds the FIFO allocation, not its observed occupancy"
        );
    }

    #[test]
    fn executable_edge_argument_repropagates_constant_then_overdefined() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("edge-widening"),
                vec![CoreType::Bool, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let parameters = builder.body().block(entry).unwrap().parameters();
        let condition = parameters[0].value();
        let unknown_i32 = parameters[1].value();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let constant = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(join, vec![constant]),
                    else_target: BlockTarget::new(join, vec![unknown_i32]),
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
        let body = builder.finish().unwrap();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };

        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert!(!outcome.changed());
        assert_eq!(outcome.statistics().edge_arguments_propagated, 2);
        assert_eq!(outcome.statistics().lattice_transitions, 5);
        assert_eq!(outcome.statistics().pessimistic_resolutions, 0);
        assert_eq!(
            outcome.statistics().event_fuel_used,
            outcome.statistics().events_dequeued
        );
    }

    #[test]
    fn folded_branch_condition_does_not_create_a_dead_constant() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("condition-only"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let literal = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_not(literal, OriginId::UNKNOWN).unwrap();
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
        for block in [then_block, else_block] {
            builder.switch_to_block(block).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        let body = builder.finish().unwrap();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };
        let allocated_before = fixture.body.instruction_counts().allocated;
        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert_eq!(outcome.statistics().branches_folded, 1);
        assert_eq!(outcome.statistics().constants_replaced, 0);
        assert_eq!(
            fixture.body.instruction_counts().allocated,
            allocated_before,
            "a condition removed by folding must not demand materialization"
        );
    }

    #[test]
    fn overflowing_add_materializes_both_results_from_one_snapshot() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("overflowing"),
                vec![],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let lhs = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let rhs = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (sum, overflowed) = builder
            .i32_add_overflowing(lhs, rhs, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum, overflowed]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let entry = body.entry();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };
        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert_eq!(outcome.statistics().constants_replaced, 2);
        let TerminatorKind::Return(results) = fixture
            .body
            .block(entry)
            .unwrap()
            .terminator()
            .unwrap()
            .kind()
        else {
            panic!("fixture no longer returns");
        };
        let expected = [CoreOp::I32Constant(i32::MIN), CoreOp::BoolConstant(true)];
        for (value, expected) in results.iter().copied().zip(expected) {
            let ValueDef::InstResult { instruction, .. } =
                fixture.body.value(value).unwrap().definition()
            else {
                panic!("replacement is not an instruction result");
            };
            assert_eq!(
                fixture.body.instruction(instruction).unwrap().op(),
                &expected
            );
        }
    }

    #[test]
    fn overflowing_add_with_zero_tracks_one_partial_result_without_demanding_the_other() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("partial-overflowing"),
                vec![CoreType::I32],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let argument = builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .parameters()[0]
            .value();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let (_unused_sum, overflowed) = builder
            .i32_add_overflowing(argument, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![overflowed]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let entry = body.entry();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };
        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert_eq!(outcome.statistics().constants_replaced, 1);
        assert_eq!(outcome.statistics().pessimistic_resolutions, 0);
        let TerminatorKind::Return(results) = fixture
            .body
            .block(entry)
            .unwrap()
            .terminator()
            .unwrap()
            .kind()
        else {
            panic!("fixture no longer returns");
        };
        let ValueDef::InstResult { instruction, .. } =
            fixture.body.value(results[0]).unwrap().definition()
        else {
            panic!("partial fact was not materialized");
        };
        assert_eq!(
            fixture.body.instruction(instruction).unwrap().op(),
            &CoreOp::BoolConstant(false)
        );
    }

    #[test]
    fn existing_and_unused_constants_allocate_nothing() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("unused-constants"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let literal = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        builder.bool_not(literal, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };
        let allocated_before = fixture.body.instruction_counts().allocated;
        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert!(!outcome.changed());
        assert_eq!(outcome.statistics().constants_replaced, 0);
        assert_eq!(
            fixture.body.instruction_counts().allocated,
            allocated_before
        );
    }

    #[test]
    fn malformed_preflight_is_an_invariant_error_not_optional_fallback() {
        let mut case = same_target_fixture();
        case.fixture.body.block_mut(case.entry).unwrap().terminator = None;
        assert!(matches!(
            super::solver::solve(&case.fixture.body, SccpLimits::derived()),
            Err(super::SccpInvariantError::MissingTerminator { block }) if block == case.entry
        ));
    }

    #[test]
    fn detached_history_is_charged_by_allocated_identity_slots() {
        const DETACHED_BLOCKS: usize = 2_048;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("detached-history"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for ordinal in 0..DETACHED_BLOCKS {
            let block = builder.create_block(OriginId::UNKNOWN).unwrap();
            builder.switch_to_block(block).unwrap();
            builder
                .i32_constant(
                    i32::try_from(ordinal).unwrap_or(i32::MAX),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        let body = builder.finish().unwrap();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };
        let cleanup = run_fixture(&mut fixture, SccpLimits::derived());
        assert_eq!(
            cleanup.statistics().blocks_detached,
            u64::try_from(DETACHED_BLOCKS).unwrap()
        );
        assert_eq!(fixture.body.block_order().len(), 1);

        let measured = run_fixture(&mut fixture, SccpLimits::derived());
        let required = measured.statistics().table_entries;
        assert!(
            required >= u64::try_from(DETACHED_BLOCKS * 3).unwrap(),
            "allocated block/instruction/value history was not preflighted"
        );
        let before = format!("{:?}", fixture.body);
        let stopped = run_fixture(
            &mut fixture,
            SccpLimits::derived().with_table_limit(SccpTableLimit::new(required - 1)),
        );
        assert_eq!(
            stopped.completion(),
            SccpCompletion::StoppedAtLimit(SccpLimitReason::Tables)
        );
        assert_eq!(format!("{:?}", fixture.body), before);
    }

    #[test]
    fn twenty_thousand_value_chain_is_iterative_and_linearly_bounded() {
        const CHAIN_LENGTH: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("deep-chain"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let mut value = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        for _ in 0..CHAIN_LENGTH {
            value = builder.bool_not(value, OriginId::UNKNOWN).unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();

        let super::SccpSolveResult::Complete(decision) =
            super::solver::solve(&body, SccpLimits::derived()).unwrap()
        else {
            panic!("derived limits must cover the solver's exact monotone bound");
        };
        assert_eq!(decision.values.len(), CHAIN_LENGTH);
        assert_eq!(
            decision.statistics.lattice_transitions,
            u64::try_from(CHAIN_LENGTH + 1).unwrap()
        );
        assert_eq!(decision.statistics.pessimistic_resolutions, 0);
        assert_eq!(
            decision.statistics.event_fuel_used,
            decision.statistics.events_dequeued
        );
        assert!(
            decision.statistics.maximum_queue_length <= u64::try_from(CHAIN_LENGTH + 2).unwrap()
        );
    }

    #[test]
    fn executable_backedge_reaches_a_nonrecursive_fixpoint() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("backedge"),
                vec![CoreType::Bool, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let entry_parameters = builder.body().block(entry).unwrap().parameters();
        let initial_condition = entry_parameters[0].value();
        let initial_value = entry_parameters[1].value();
        let loop_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let loop_condition = builder
            .append_block_parameter(loop_block, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        let loop_value = builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit_value = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    loop_block,
                    vec![initial_condition, initial_value],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(loop_block).unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let next = builder
            .i32_add_wrapping(loop_value, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: loop_condition,
                    then_target: BlockTarget::new(loop_block, vec![loop_condition, next]),
                    else_target: BlockTarget::new(exit, vec![next]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![exit_value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };
        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert!(!outcome.changed());
        assert_eq!(outcome.completion(), SccpCompletion::Complete);
        assert_eq!(outcome.statistics().edges_activated, 3);
        assert_eq!(outcome.statistics().edge_arguments_propagated, 5);
        assert_eq!(outcome.statistics().pessimistic_resolutions, 0);
    }

    #[test]
    fn calls_are_immediate_overdefined_barriers() {
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
            .declare_function(
                Some("caller"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let argument = builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .parameters()[0]
            .value();
        let result = builder
            .call(callee, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let mut fixture = Fixture {
            program,
            sources,
            function,
            body,
        };
        let outcome = run_fixture(&mut fixture, SccpLimits::derived());
        assert!(!outcome.changed());
        assert_eq!(outcome.completion(), SccpCompletion::Complete);
        assert_eq!(outcome.statistics().pessimistic_resolutions, 0);
        assert_eq!(outcome.statistics().constants_replaced, 0);
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum OracleFact {
        Unknown,
        Bool(bool),
        Overdefined,
    }

    impl OracleFact {
        const fn lattice(self) -> LatticeValue {
            match self {
                Self::Unknown => LatticeValue::Unknown,
                Self::Bool(value) => LatticeValue::BoolConstant(value),
                Self::Overdefined => LatticeValue::Overdefined,
            }
        }
    }

    #[derive(Debug)]
    struct OracleEdge {
        source: BlockId,
        successor_index: u8,
        destination: BlockId,
        arguments: Vec<ValueId>,
        executable: bool,
    }

    struct OracleSolution {
        values: Vec<OracleFact>,
        blocks: Vec<bool>,
        edges: Vec<OracleEdge>,
    }

    fn oracle_join(slot: &mut OracleFact, incoming: OracleFact) -> bool {
        let joined = match (*slot, incoming) {
            (OracleFact::Overdefined, _) | (_, OracleFact::Overdefined) => OracleFact::Overdefined,
            (OracleFact::Unknown, other) => other,
            (current, OracleFact::Unknown) => current,
            (OracleFact::Bool(left), OracleFact::Bool(right)) if left == right => {
                OracleFact::Bool(left)
            }
            (OracleFact::Bool(_), OracleFact::Bool(_)) => OracleFact::Overdefined,
        };
        let changed = joined != *slot;
        *slot = joined;
        changed
    }

    fn oracle_index<I: EntityId>(id: I) -> usize {
        usize::try_from(id.index()).unwrap()
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the test oracle stays deliberately flat and structurally unlike the production event solver"
    )]
    fn round_robin_oracle(body: &FunctionBody) -> OracleSolution {
        let mut values = vec![OracleFact::Unknown; body.value_counts().allocated];
        let mut blocks = vec![false; body.block_counts().allocated];
        let mut definition_blocks = vec![None; body.value_counts().allocated];
        let mut edges = Vec::new();

        for block in body.block_order().iter().copied() {
            let data = body.block(block).unwrap();
            for parameter in data.parameters() {
                definition_blocks[oracle_index(parameter.value())] = Some(block);
            }
            for instruction in data.instructions().iter().copied() {
                for result in body.instruction(instruction).unwrap().results() {
                    definition_blocks[oracle_index(*result)] = Some(block);
                }
            }
            match data.terminator().unwrap().kind() {
                TerminatorKind::Jump(target) => edges.push(OracleEdge {
                    source: block,
                    successor_index: 0,
                    destination: target.block(),
                    arguments: target.arguments().to_vec(),
                    executable: false,
                }),
                TerminatorKind::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    for (successor_index, target) in
                        [then_target, else_target].into_iter().enumerate()
                    {
                        edges.push(OracleEdge {
                            source: block,
                            successor_index: u8::try_from(successor_index).unwrap(),
                            destination: target.block(),
                            arguments: target.arguments().to_vec(),
                            executable: false,
                        });
                    }
                }
                TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
            }
        }

        blocks[oracle_index(body.entry())] = true;
        for parameter in body.block(body.entry()).unwrap().parameters() {
            values[oracle_index(parameter.value())] = OracleFact::Overdefined;
        }

        loop {
            let mut changed = false;
            for block in body.block_order().iter().copied() {
                if !blocks[oracle_index(block)] {
                    continue;
                }
                let data = body.block(block).unwrap();
                for instruction in data.instructions().iter().copied() {
                    let instruction = body.instruction(instruction).unwrap();
                    let result = match instruction.op() {
                        CoreOp::BoolConstant(value) => OracleFact::Bool(*value),
                        CoreOp::BoolNot => match values[oracle_index(instruction.operands()[0])] {
                            OracleFact::Unknown => OracleFact::Unknown,
                            OracleFact::Bool(value) => OracleFact::Bool(!value),
                            OracleFact::Overdefined => OracleFact::Overdefined,
                        },
                        _ => OracleFact::Overdefined,
                    };
                    for value in instruction.results() {
                        changed |= oracle_join(&mut values[oracle_index(*value)], result);
                    }
                }

                let mut activate = [false; 2];
                match data.terminator().unwrap().kind() {
                    TerminatorKind::Jump(_) => activate[0] = true,
                    TerminatorKind::Branch { condition, .. } => {
                        match values[oracle_index(*condition)] {
                            OracleFact::Unknown => {}
                            OracleFact::Bool(true) => activate[0] = true,
                            OracleFact::Bool(false) => activate[1] = true,
                            OracleFact::Overdefined => activate = [true, true],
                        }
                    }
                    TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
                }
                for edge in edges.iter_mut().filter(|edge| edge.source == block) {
                    if activate[usize::from(edge.successor_index)] && !edge.executable {
                        edge.executable = true;
                        blocks[oracle_index(edge.destination)] = true;
                        changed = true;
                    }
                }
            }

            for edge in edges.iter().filter(|edge| edge.executable) {
                let parameters = body.block(edge.destination).unwrap().parameters();
                for (argument, parameter) in edge.arguments.iter().zip(parameters) {
                    let incoming = values[oracle_index(*argument)];
                    changed |= oracle_join(&mut values[oracle_index(parameter.value())], incoming);
                }
            }
            if changed {
                continue;
            }

            let mut pessimistic_change = false;
            for (value_index, definition_block) in definition_blocks.iter().copied().enumerate() {
                if definition_block.is_some_and(|block| blocks[oracle_index(block)])
                    && values[value_index] == OracleFact::Unknown
                {
                    values[value_index] = OracleFact::Overdefined;
                    pessimistic_change = true;
                }
            }
            if !pessimistic_change {
                break;
            }
        }
        OracleSolution {
            values,
            blocks,
            edges,
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one generated CFG fixture keeps its seed-qualified construction in one replayable unit"
    )]
    fn generated_boolean_body(generator_version: u32, seed: u32, context: &str) -> FunctionBody {
        assert_eq!(
            generator_version, 1,
            "{context}: generator_version={generator_version} seed={seed} is unsupported"
        );
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let has_runtime_condition = seed % 3 == 0;
        let function = program
            .declare_function(
                Some("generated-sccp"),
                if has_runtime_condition {
                    vec![CoreType::Bool]
                } else {
                    vec![]
                },
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let mut builder =
            FunctionBuilder::new(&program, &sources, function).expect_generated(context);
        let entry = builder.entry_block();
        let condition = if has_runtime_condition {
            builder
                .body()
                .block(entry)
                .unwrap_or_else(|| panic!("{context}: generated entry block is missing"))
                .parameters()
                .first()
                .unwrap_or_else(|| panic!("{context}: generated runtime parameter is missing"))
                .value()
        } else {
            builder
                .bool_constant(seed & 1 == 0, OriginId::UNKNOWN)
                .expect_generated(context)
        };

        if seed % 2 == 0 {
            let join = builder
                .create_block(OriginId::UNKNOWN)
                .expect_generated(context);
            let parameter = builder
                .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
                .expect_generated(context);
            let then_value = builder
                .bool_constant(seed & 2 == 0, OriginId::UNKNOWN)
                .expect_generated(context);
            let else_literal = builder
                .bool_constant(seed & 4 == 0, OriginId::UNKNOWN)
                .expect_generated(context);
            let else_value = builder
                .bool_not(else_literal, OriginId::UNKNOWN)
                .expect_generated(context);
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Branch {
                        condition,
                        then_target: BlockTarget::new(join, vec![then_value]),
                        else_target: BlockTarget::new(join, vec![else_value]),
                    },
                    OriginId::UNKNOWN,
                ))
                .expect_generated(context);
            builder.switch_to_block(join).expect_generated(context);
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![parameter]),
                    OriginId::UNKNOWN,
                ))
                .expect_generated(context);
        } else {
            let then_block = builder
                .create_block(OriginId::UNKNOWN)
                .expect_generated(context);
            let else_block = builder
                .create_block(OriginId::UNKNOWN)
                .expect_generated(context);
            let join = builder
                .create_block(OriginId::UNKNOWN)
                .expect_generated(context);
            let parameter = builder
                .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
                .expect_generated(context);
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Branch {
                        condition,
                        then_target: BlockTarget::new(then_block, vec![]),
                        else_target: BlockTarget::new(else_block, vec![]),
                    },
                    OriginId::UNKNOWN,
                ))
                .expect_generated(context);
            for (block, value) in [(then_block, seed & 2 == 0), (else_block, seed & 4 == 0)] {
                builder.switch_to_block(block).expect_generated(context);
                let argument = builder
                    .bool_constant(value, OriginId::UNKNOWN)
                    .expect_generated(context);
                builder
                    .terminate(Terminator::new(
                        TerminatorKind::Jump(BlockTarget::new(join, vec![argument])),
                        OriginId::UNKNOWN,
                    ))
                    .expect_generated(context);
            }
            builder.switch_to_block(join).expect_generated(context);
            let returned = if seed & 8 == 0 {
                builder
                    .bool_not(parameter, OriginId::UNKNOWN)
                    .expect_generated(context)
            } else {
                parameter
            };
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![returned]),
                    OriginId::UNKNOWN,
                ))
                .expect_generated(context);
        }
        builder.finish().expect_generated(context)
    }

    #[test]
    fn sparse_fifo_matches_independent_dense_round_robin_oracle() {
        const GENERATOR_VERSION: u32 = 1;

        for seed in 0..64 {
            let condition_shape = if seed % 3 == 0 { "runtime" } else { "constant" };
            let cfg_shape = if seed % 2 == 0 {
                "same-target-branch"
            } else {
                "diamond"
            };
            let return_shape = if seed % 2 != 0 && seed & 8 == 0 {
                "negated"
            } else {
                "direct"
            };
            let inputs = if seed % 3 == 0 {
                "bool:[false,true]"
            } else {
                "none"
            };
            let context = format!(
                "generator_version={GENERATOR_VERSION} seed={seed} shape=condition:{condition_shape},cfg:{cfg_shape},return:{return_shape} inputs={inputs}"
            );
            run_generated_case(&context, || {
                let body = generated_boolean_body(GENERATOR_VERSION, seed, &context);
                let sparse = super::solver::solve_facts_for_test(&body)
                    .unwrap_or_else(|error| panic!("{context}: sparse solver failed: {error:?}"));
                let dense = round_robin_oracle(&body);
                assert_eq!(
                    sparse.values,
                    dense
                        .values
                        .iter()
                        .copied()
                        .map(OracleFact::lattice)
                        .collect::<Vec<_>>(),
                    "{context}: value facts differ"
                );
                assert_eq!(sparse.blocks, dense.blocks, "{context}: blocks differ");
                let sparse_edges = sparse
                    .edges
                    .iter()
                    .map(|(edge, executable)| {
                        let (source, successor_index) = edge.parts();
                        (source, successor_index, *executable)
                    })
                    .collect::<Vec<_>>();
                let dense_edges = dense
                    .edges
                    .iter()
                    .map(|edge| (edge.source, edge.successor_index, edge.executable))
                    .collect::<Vec<_>>();
                assert_eq!(sparse_edges, dense_edges, "{context}: edges differ");
                assert_eq!(
                    sparse.statistics.pessimistic_resolutions, 0,
                    "{context}: valid generated cases must not require pessimistic resolution"
                );
            });
        }
    }
}
