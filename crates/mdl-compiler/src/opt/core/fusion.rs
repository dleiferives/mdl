//! Atomic maximal straight-line jump-region fusion for verified Core.

use std::error::Error;
use std::fmt;

use crate::ir::core::{
    CoreProgram, EditError, FunctionBody, FunctionEditor, FunctionId,
    JumpFusionApplicationStatistics, JumpFusionPreparation,
};
use crate::source::SourceContext;

/// Whether one fusion resource limit is derived from immutable input metadata or
/// explicitly supplied by the caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FusionLimit<T> {
    Derived,
    Explicit(T),
}

/// Maximum stable-ID fact entries plus flattened parameter substitution slots.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct FusionFactTableEntryLimit(usize);

impl FusionFactTableEntryLimit {
    #[must_use]
    pub(super) const fn new(entries: usize) -> Self {
        Self(entries)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Maximum complete block visits admitted during head-first region discovery.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct FusionBlockVisitLimit(usize);

impl FusionBlockVisitLimit {
    #[must_use]
    pub(super) const fn new(blocks: usize) -> Self {
        Self(blocks)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Typed limits for one frozen fusion snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FusionLimits {
    fact_table_entries: FusionLimit<FusionFactTableEntryLimit>,
    block_visits: FusionLimit<FusionBlockVisitLimit>,
}

impl FusionLimits {
    #[must_use]
    pub(super) const fn derived() -> Self {
        Self {
            fact_table_entries: FusionLimit::Derived,
            block_visits: FusionLimit::Derived,
        }
    }

    #[must_use]
    pub(super) const fn with_fact_table_limit(mut self, limit: FusionFactTableEntryLimit) -> Self {
        self.fact_table_entries = FusionLimit::Explicit(limit);
        self
    }

    #[must_use]
    pub(super) const fn with_block_visit_limit(mut self, limit: FusionBlockVisitLimit) -> Self {
        self.block_visits = FusionLimit::Explicit(limit);
        self
    }
}

impl Default for FusionLimits {
    fn default() -> Self {
        Self::derived()
    }
}

/// Stable reason fusion stopped conservatively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FusionLimitReason {
    FactTables,
    BlockVisits,
}

/// Whether every maximal region in the frozen snapshot was considered.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FusionCompletion {
    Complete,
    StoppedAtLimit(FusionLimitReason),
}

/// Deterministic counters for fact construction, discovery, and one atomic batch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FusionStatistics {
    pub(super) fact_builds: usize,
    pub(super) required_fact_table_entries: usize,
    pub(super) attached_edges_counted: usize,
    pub(super) raw_instruction_memberships: usize,
    pub(super) block_visits: usize,
    pub(super) regions_fused: usize,
    pub(super) blocks_consumed: usize,
    pub(super) selected_instruction_memberships_preflighted: usize,
    pub(super) parameter_mapping_slots_used: usize,
    pub(super) replacement_links_visited: usize,
    pub(super) attached_values_scanned: usize,
    pub(super) attached_rewrite_scans: usize,
    pub(super) head_reservations: usize,
    pub(super) layout_retain_scans: usize,
}

/// Result of one bounded fusion invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FusionOutcome {
    pub(super) changed: bool,
    pub(super) completion: FusionCompletion,
    pub(super) statistics: FusionStatistics,
}

/// Failure of mandatory frozen facts or the consumer-specific atomic editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FusionError {
    Edit(EditError),
}

impl fmt::Display for FusionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for FusionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Edit(error) => Some(error),
        }
    }
}

impl From<EditError> for FusionError {
    fn from(error: EditError) -> Self {
        Self::Edit(error)
    }
}

/// Fuses one complete-prefix set of maximal unconditional-jump regions.
pub(super) fn run_fusion(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    limits: FusionLimits,
) -> Result<FusionOutcome, FusionError> {
    let maximum_fact_table_entries = match limits.fact_table_entries {
        FusionLimit::Derived => None,
        FusionLimit::Explicit(limit) => Some(limit.get()),
    };
    let maximum_block_visits = match limits.block_visits {
        FusionLimit::Derived => body.block_counts().attached,
        FusionLimit::Explicit(limit) => limit.get(),
    };

    let mut editor = FunctionEditor::from_trusted_body(program, sources, function, body);
    let mut prepared = match editor.prepare_jump_fusion(maximum_fact_table_entries)? {
        JumpFusionPreparation::Ready(prepared) => prepared,
        JumpFusionPreparation::StoppedAtFactTableLimit { statistics } => {
            return Ok(FusionOutcome {
                changed: false,
                completion: FusionCompletion::StoppedAtLimit(FusionLimitReason::FactTables),
                statistics: FusionStatistics {
                    fact_builds: statistics.fact_builds,
                    required_fact_table_entries: statistics.required_fact_table_entries,
                    attached_edges_counted: statistics.attached_edges_counted,
                    raw_instruction_memberships: statistics.raw_instruction_memberships,
                    ..FusionStatistics::default()
                },
            });
        }
    };

    let facts = prepared.fact_statistics();
    let discovery = prepared.discover_maximal_regions(maximum_block_visits)?;
    let completion = if discovery.stopped_at_block_visit_limit {
        FusionCompletion::StoppedAtLimit(FusionLimitReason::BlockVisits)
    } else {
        FusionCompletion::Complete
    };
    let block_visits = discovery.block_visits;
    let application = prepared.fuse_jump_regions_batch(&discovery.regions)?;
    let statistics = combine_statistics(facts, block_visits, application);
    Ok(FusionOutcome {
        changed: statistics.regions_fused != 0,
        completion,
        statistics,
    })
}

const fn combine_statistics(
    facts: crate::ir::core::JumpFusionFactStatistics,
    block_visits: usize,
    application: JumpFusionApplicationStatistics,
) -> FusionStatistics {
    FusionStatistics {
        fact_builds: facts.fact_builds,
        required_fact_table_entries: facts.required_fact_table_entries,
        attached_edges_counted: facts.attached_edges_counted,
        raw_instruction_memberships: facts.raw_instruction_memberships,
        block_visits,
        regions_fused: application.regions_fused,
        blocks_consumed: application.blocks_consumed,
        selected_instruction_memberships_preflighted: application
            .selected_instruction_memberships_preflighted,
        parameter_mapping_slots_used: application.parameter_mapping_slots_used,
        replacement_links_visited: application.replacement_links_visited,
        attached_values_scanned: application.attached_values_scanned,
        attached_rewrite_scans: application.attached_rewrite_scans,
        head_reservations: application.head_reservations,
        layout_retain_scans: application.layout_retain_scans,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityVec;
    use crate::ir::core::{
        BlockData, BlockId, BlockTarget, CoreType, FunctionBuilder, InstId, Terminator,
        TerminatorKind, ValueDef, ValueId, verify_function,
    };
    use crate::source::{Origin, OriginId};

    struct ParameterizedFixture {
        sources: SourceContext,
        program: CoreProgram,
        function: FunctionId,
        body: FunctionBody,
        entry: BlockId,
        middle: BlockId,
        tail: BlockId,
        exit: BlockId,
        head: BlockId,
        side: BlockId,
        left: ValueId,
        right: ValueId,
        middle_sum: ValueId,
        tail_sum: ValueId,
        middle_instruction: InstId,
        tail_instruction: InstId,
        head_origin: OriginId,
        final_terminator_origin: OriginId,
    }

    fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
        builder.body().block(block).unwrap().parameters()[index].value()
    }

    fn defining_instruction(body: &FunctionBody, value: ValueId) -> InstId {
        match body.value(value).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => panic!("expected instruction result"),
        }
    }

    fn distinct_origin(sources: &mut SourceContext, reason: &str) -> OriginId {
        sources
            .add_origin(Origin::Fused {
                inputs: vec![OriginId::UNKNOWN],
                reason: Some(reason.into()),
            })
            .unwrap()
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the fixture keeps one adversarial typed CFG and its provenance expectations together"
    )]
    fn parameterized_fixture() -> ParameterizedFixture {
        let mut sources = SourceContext::new();
        let head_origin = distinct_origin(&mut sources, "head");
        let head_jump_origin = distinct_origin(&mut sources, "head jump");
        let middle_origin = distinct_origin(&mut sources, "middle");
        let middle_jump_origin = distinct_origin(&mut sources, "middle jump");
        let tail_origin = distinct_origin(&mut sources, "tail");
        let tail_jump_origin = distinct_origin(&mut sources, "tail jump");
        let final_terminator_origin = distinct_origin(&mut sources, "final return");

        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("parameterized-fusion"),
                vec![CoreType::Bool, CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let left = parameter(&builder, entry, 1);
        let right = parameter(&builder, entry, 2);

        // Deliberately allocate the middle of the CFG chain before its head.
        let middle = builder.create_block(middle_origin).unwrap();
        let tail = builder.create_block(tail_origin).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let head = builder.create_block(head_origin).unwrap();
        let side = builder.create_block(OriginId::UNKNOWN).unwrap();

        for ty in [CoreType::I32, CoreType::I32] {
            builder
                .append_block_parameter(middle, ty, middle_origin)
                .unwrap();
        }
        for ty in [CoreType::I32, CoreType::I32, CoreType::I32] {
            builder
                .append_block_parameter(tail, ty, tail_origin)
                .unwrap();
            builder
                .append_block_parameter(exit, ty, OriginId::UNKNOWN)
                .unwrap();
        }

        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(head, vec![]),
                    else_target: BlockTarget::new(side, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(head).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(middle, vec![left, right])),
                head_jump_origin,
            ))
            .unwrap();

        builder.switch_to_block(middle).unwrap();
        let middle_left = parameter(&builder, middle, 0);
        let middle_right = parameter(&builder, middle, 1);
        let middle_sum = builder
            .i32_add_wrapping(middle_left, middle_right, middle_origin)
            .unwrap();
        let middle_instruction = defining_instruction(builder.body(), middle_sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    tail,
                    vec![middle_right, middle_left, middle_sum],
                )),
                middle_jump_origin,
            ))
            .unwrap();

        builder.switch_to_block(tail).unwrap();
        let tail_first = parameter(&builder, tail, 0);
        let tail_second = parameter(&builder, tail, 1);
        let tail_third = parameter(&builder, tail, 2);
        let tail_sum = builder
            .i32_add_wrapping(tail_second, tail_third, tail_origin)
            .unwrap();
        let tail_instruction = defining_instruction(builder.body(), tail_sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    exit,
                    vec![tail_first, tail_second, tail_sum],
                )),
                tail_jump_origin,
            ))
            .unwrap();

        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![
                    parameter(&builder, exit, 0),
                    parameter(&builder, exit, 1),
                    parameter(&builder, exit, 2),
                ]),
                final_terminator_origin,
            ))
            .unwrap();

        builder.switch_to_block(side).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![left, right, left]),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        let body = builder.finish().unwrap();
        assert_eq!(body.block_order(), &[entry, middle, tail, exit, head, side]);
        ParameterizedFixture {
            sources,
            program,
            function,
            body,
            entry,
            middle,
            tail,
            exit,
            head,
            side,
            left,
            right,
            middle_sum,
            tail_sum,
            middle_instruction,
            tail_instruction,
            head_origin,
            final_terminator_origin,
        }
    }

    #[test]
    fn fuses_adversarial_layout_with_simultaneous_parameters_and_stable_provenance() {
        let mut fixture = parameterized_fixture();
        let outcome = run_fusion(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut fixture.body,
            FusionLimits::derived(),
        )
        .unwrap();

        assert!(outcome.changed);
        assert_eq!(outcome.completion, FusionCompletion::Complete);
        assert_eq!(outcome.statistics.fact_builds, 1);
        // 6 dense block tables * 6 allocated blocks + 2 raw-instruction
        // owners + exactly 8 consumed-parameter substitution slots.
        assert_eq!(outcome.statistics.required_fact_table_entries, 46);
        assert_eq!(outcome.statistics.attached_edges_counted, 5);
        assert_eq!(outcome.statistics.raw_instruction_memberships, 2);
        assert_eq!(outcome.statistics.block_visits, 4);
        assert_eq!(outcome.statistics.regions_fused, 1);
        assert_eq!(outcome.statistics.blocks_consumed, 3);
        assert_eq!(
            outcome
                .statistics
                .selected_instruction_memberships_preflighted,
            2
        );
        assert_eq!(outcome.statistics.parameter_mapping_slots_used, 8);
        assert_eq!(outcome.statistics.attached_values_scanned, 19);
        assert_eq!(outcome.statistics.attached_rewrite_scans, 1);
        assert_eq!(outcome.statistics.head_reservations, 1);
        assert_eq!(outcome.statistics.layout_retain_scans, 1);

        assert_eq!(
            fixture.body.block_order(),
            &[fixture.entry, fixture.head, fixture.side]
        );
        let head = fixture.body.block(fixture.head).unwrap();
        assert_eq!(head.origin(), fixture.head_origin);
        assert_eq!(
            head.instructions(),
            &[fixture.middle_instruction, fixture.tail_instruction]
        );
        assert_eq!(
            head.terminator().unwrap().origin(),
            fixture.final_terminator_origin
        );
        assert_eq!(
            head.terminator().unwrap().kind(),
            &TerminatorKind::Return(vec![fixture.right, fixture.left, fixture.tail_sum])
        );
        assert_eq!(
            fixture
                .body
                .instruction(fixture.middle_instruction)
                .unwrap()
                .operands(),
            &[fixture.left, fixture.right]
        );
        assert_eq!(
            fixture
                .body
                .instruction(fixture.tail_instruction)
                .unwrap()
                .operands(),
            &[fixture.left, fixture.middle_sum]
        );
        for consumed in [fixture.middle, fixture.tail, fixture.exit] {
            let data = fixture.body.block(consumed).unwrap();
            assert!(data.instructions().is_empty());
            assert!(data.terminator().is_none());
            assert!(!data.parameters().is_empty());
        }
        assert_eq!(
            fixture
                .body
                .instruction(fixture.middle_instruction)
                .unwrap()
                .origin(),
            fixture.body.block(fixture.middle).unwrap().origin()
        );
        verify_function(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &fixture.body,
        )
        .unwrap();
    }

    #[test]
    fn second_stage_fact_limit_reports_exact_eligible_parameter_slots_unchanged() {
        let mut fixture = parameterized_fixture();
        let before = format!("{:#?}", fixture.body);
        let outcome = run_fusion(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut fixture.body,
            FusionLimits::derived().with_fact_table_limit(FusionFactTableEntryLimit::new(45)),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(
            outcome.completion,
            FusionCompletion::StoppedAtLimit(FusionLimitReason::FactTables)
        );
        assert_eq!(outcome.statistics.fact_builds, 1);
        assert_eq!(outcome.statistics.required_fact_table_entries, 46);
        assert_eq!(outcome.statistics.attached_edges_counted, 5);
        assert_eq!(outcome.statistics.raw_instruction_memberships, 2);
        assert_eq!(format!("{:#?}", fixture.body), before);

        let admitted = run_fusion(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut fixture.body,
            FusionLimits::derived().with_fact_table_limit(FusionFactTableEntryLimit::new(46)),
        )
        .unwrap();
        assert!(admitted.changed);
        assert_eq!(admitted.completion, FusionCompletion::Complete);
        assert_eq!(admitted.statistics.required_fact_table_entries, 46);
    }

    fn nonmatch_fixture() -> (SourceContext, CoreProgram, FunctionId, FunctionBody) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("edge-count-nonmatches"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let source = builder.create_block(OriginId::UNKNOWN).unwrap();
        let destination = builder.create_block(OriginId::UNKNOWN).unwrap();
        let unreachable_predecessor = builder.create_block(OriginId::UNKNOWN).unwrap();
        let side = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(source, vec![]),
                    else_target: BlockTarget::new(side, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(source).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(destination, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(destination).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(unreachable_predecessor).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(destination, vec![]),
                    else_target: BlockTarget::new(destination, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(side).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        (sources, program, function, body)
    }

    #[test]
    fn exact_duplicate_edges_and_attached_unreachable_predecessors_prevent_fusion() {
        let (sources, program, function, mut body) = nonmatch_fixture();
        let before = format!("{body:#?}");
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(outcome.completion, FusionCompletion::Complete);
        assert_eq!(outcome.statistics.attached_edges_counted, 5);
        assert_eq!(outcome.statistics.block_visits, 0);
        assert_eq!(format!("{body:#?}"), before);
    }

    fn two_region_fixture() -> (SourceContext, CoreProgram, FunctionId, FunctionBody) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("two-regions"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let first_head = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first_tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        let second_head = builder.create_block(OriginId::UNKNOWN).unwrap();
        let second_tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(first_head, vec![]),
                    else_target: BlockTarget::new(second_head, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for (head, tail) in [(first_head, first_tail), (second_head, second_tail)] {
            builder.switch_to_block(head).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(tail, vec![])),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            builder.switch_to_block(tail).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        let body = builder.finish().unwrap();
        (sources, program, function, body)
    }

    #[test]
    fn typed_limits_reject_tables_unchanged_and_apply_only_complete_region_prefixes() {
        let (sources, program, function, body) = two_region_fixture();
        let required_entries = 5 * 6;

        let mut rejected = body.clone();
        let before = format!("{rejected:#?}");
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut rejected,
            FusionLimits::derived()
                .with_fact_table_limit(FusionFactTableEntryLimit::new(required_entries - 1)),
        )
        .unwrap();
        assert_eq!(
            outcome.completion,
            FusionCompletion::StoppedAtLimit(FusionLimitReason::FactTables)
        );
        assert_eq!(
            outcome.statistics.required_fact_table_entries,
            required_entries
        );
        assert!(!outcome.changed);
        assert_eq!(format!("{rejected:#?}"), before);

        let mut prefix = body;
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut prefix,
            FusionLimits::derived()
                .with_fact_table_limit(FusionFactTableEntryLimit::new(required_entries))
                .with_block_visit_limit(FusionBlockVisitLimit::new(2)),
        )
        .unwrap();
        assert_eq!(
            outcome.completion,
            FusionCompletion::StoppedAtLimit(FusionLimitReason::BlockVisits)
        );
        assert!(outcome.changed);
        assert_eq!(outcome.statistics.block_visits, 2);
        assert_eq!(outcome.statistics.regions_fused, 1);
        assert_eq!(outcome.statistics.blocks_consumed, 1);
        assert_eq!(outcome.statistics.head_reservations, 1);
        assert_eq!(outcome.statistics.layout_retain_scans, 1);
        assert_eq!(prefix.block_order().len(), 4);
        verify_function(&program, &sources, function, &prefix).unwrap();
    }

    #[test]
    fn visit_limit_inside_region_discards_the_whole_region_and_never_starts_at_a_suffix() {
        const BLOCKS: usize = 5;
        let (sources, program, function, body) = large_chain_body(BLOCKS);

        let mut interrupted = body.clone();
        let before = format!("{interrupted:#?}");
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut interrupted,
            FusionLimits::derived().with_block_visit_limit(FusionBlockVisitLimit::new(BLOCKS - 1)),
        )
        .unwrap();
        assert_eq!(
            outcome.completion,
            FusionCompletion::StoppedAtLimit(FusionLimitReason::BlockVisits)
        );
        assert!(!outcome.changed);
        assert_eq!(outcome.statistics.block_visits, BLOCKS - 1);
        assert_eq!(outcome.statistics.regions_fused, 0);
        assert_eq!(outcome.statistics.blocks_consumed, 0);
        assert_eq!(format!("{interrupted:#?}"), before);

        let mut admitted = body;
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut admitted,
            FusionLimits::derived().with_block_visit_limit(FusionBlockVisitLimit::new(BLOCKS)),
        )
        .unwrap();
        assert_eq!(outcome.completion, FusionCompletion::Complete);
        assert!(outcome.changed);
        assert_eq!(outcome.statistics.block_visits, BLOCKS);
        assert_eq!(outcome.statistics.blocks_consumed, BLOCKS - 1);
        assert_eq!(admitted.block_order(), &[admitted.entry()]);
        verify_function(&program, &sources, function, &admitted).unwrap();
    }

    #[test]
    fn consumed_parameters_are_rewritten_in_all_surviving_descendants() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("fusion-surviving-descendants"),
                vec![CoreType::I32, CoreType::Bool],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = parameter(&builder, entry, 0);
        let input_condition = parameter(&builder, entry, 1);
        let middle = builder.create_block(OriginId::UNKNOWN).unwrap();
        let left = builder.create_block(OriginId::UNKNOWN).unwrap();
        let right = builder.create_block(OriginId::UNKNOWN).unwrap();
        let middle_input = builder
            .append_block_parameter(middle, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let middle_condition = builder
            .append_block_parameter(middle, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();

        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(middle, vec![input, input_condition])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(middle).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: middle_condition,
                    then_target: BlockTarget::new(left, vec![]),
                    else_target: BlockTarget::new(right, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for descendant in [left, right] {
            builder.switch_to_block(descendant).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![middle_input]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        let mut body = builder.finish().unwrap();

        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived(),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.statistics.blocks_consumed, 1);
        assert_eq!(body.block_order(), &[entry, left, right]);
        let TerminatorKind::Branch {
            condition: fused_condition,
            ..
        } = body.block(entry).unwrap().terminator().unwrap().kind()
        else {
            panic!("fused entry should retain the middle branch");
        };
        assert_eq!(*fused_condition, input_condition);
        for descendant in [left, right] {
            assert_eq!(
                body.block(descendant).unwrap().terminator().unwrap().kind(),
                &TerminatorKind::Return(vec![input])
            );
        }
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn disjoint_region_tail_may_branch_to_another_surviving_region_head() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("fusion-cross-region-successor"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let first_head = builder.entry_block();
        let condition = parameter(&builder, first_head, 0);
        let first_tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        let second_head = builder.create_block(OriginId::UNKNOWN).unwrap();
        let second_tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();

        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(first_tail, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(first_tail).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(second_head, vec![]),
                    else_target: BlockTarget::new(exit, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(second_head).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(second_tail, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for terminal in [second_tail, exit] {
            builder.switch_to_block(terminal).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        let mut body = builder.finish().unwrap();

        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived(),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.statistics.regions_fused, 2);
        assert_eq!(outcome.statistics.blocks_consumed, 2);
        assert_eq!(body.block_order(), &[first_head, second_head, exit]);
        assert_eq!(
            body.block(first_head).unwrap().terminator().unwrap().kind(),
            &TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(second_head, vec![]),
                else_target: BlockTarget::new(exit, vec![]),
            }
        );
        assert_eq!(
            body.block(second_head)
                .unwrap()
                .terminator()
                .unwrap()
                .kind(),
            &TerminatorKind::Return(vec![])
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn final_backedge_to_surviving_head_becomes_a_valid_self_loop() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("fusion-loop"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let head = builder.create_block(OriginId::UNKNOWN).unwrap();
        let body_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let side = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(head, vec![]),
                    else_target: BlockTarget::new(side, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(head).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(body_block, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(body_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(head, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(side).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived(),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.statistics.blocks_consumed, 1);
        assert_eq!(
            body.block(head).unwrap().terminator().unwrap().kind(),
            &TerminatorKind::Jump(BlockTarget::new(head, vec![]))
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn existing_nonentry_self_edge_is_not_a_fusion_region() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("existing-self-edge"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let looping = builder.create_block(OriginId::UNKNOWN).unwrap();
        let side = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(looping, vec![]),
                    else_target: BlockTarget::new(side, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(looping).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(looping, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(side).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let before = format!("{body:#?}");
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(outcome.statistics.block_visits, 0);
        assert_eq!(format!("{body:#?}"), before);
    }

    #[test]
    fn duplicate_raw_instruction_membership_is_rejected_before_mutation() {
        let mut fixture = parameterized_fixture();
        fixture
            .body
            .block_mut(fixture.side)
            .unwrap()
            .instructions
            .push(fixture.middle_instruction);
        let before = format!("{:#?}", fixture.body);
        let error = run_fusion(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut fixture.body,
            FusionLimits::derived(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FusionError::Edit(EditError::InconsistentJumpFusion(message))
                if message.contains("more than one raw block-container")
        ));
        assert_eq!(format!("{:#?}", fixture.body), before);
    }

    #[test]
    fn fact_admission_counts_dense_detached_block_instruction_and_value_history() {
        const DETACHED_BLOCKS: usize = 1_000;
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("fusion-detached-history"),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        let mut detached = Vec::with_capacity(DETACHED_BLOCKS);
        for value in 0..DETACHED_BLOCKS {
            let block = builder.create_block(OriginId::UNKNOWN).unwrap();
            detached.push(block);
            builder.switch_to_block(block).unwrap();
            builder
                .i32_constant(i32::try_from(value).unwrap(), OriginId::UNKNOWN)
                .unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        builder.switch_to_block(entry).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(tail, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(tail).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        body.block_order
            .retain(|block| *block == entry || *block == tail);
        verify_function(&program, &sources, function, &body).unwrap();

        let required_entries = (DETACHED_BLOCKS + 2) * 6 + DETACHED_BLOCKS;
        let before = format!("{body:#?}");
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived()
                .with_fact_table_limit(FusionFactTableEntryLimit::new(required_entries - 1)),
        )
        .unwrap();
        assert_eq!(
            outcome.completion,
            FusionCompletion::StoppedAtLimit(FusionLimitReason::FactTables)
        );
        assert!(!outcome.changed);
        assert_eq!(
            outcome.statistics.required_fact_table_entries,
            required_entries
        );
        assert_eq!(outcome.statistics.fact_builds, 0);
        assert_eq!(outcome.statistics.raw_instruction_memberships, 0);
        assert_eq!(format!("{body:#?}"), before);

        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived()
                .with_fact_table_limit(FusionFactTableEntryLimit::new(required_entries)),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(
            outcome.statistics.required_fact_table_entries,
            required_entries
        );
        assert_eq!(
            outcome.statistics.raw_instruction_memberships,
            DETACHED_BLOCKS
        );
        assert_eq!(body.block_order(), &[entry]);
        verify_function(&program, &sources, function, &body).unwrap();
    }

    fn large_chain_body(
        block_count: usize,
    ) -> (SourceContext, CoreProgram, FunctionId, FunctionBody) {
        assert!(block_count >= 2);
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("large-fusion-chain"),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut blocks = EntityVec::new();
        let mut block_order = Vec::with_capacity(block_count);
        for _ in 0..block_count {
            let block = blocks
                .push(BlockData {
                    origin: OriginId::UNKNOWN,
                    parameters: vec![],
                    instructions: vec![],
                    terminator: None,
                })
                .unwrap();
            block_order.push(block);
        }
        for edge in block_order.windows(2) {
            blocks.get_mut(edge[0]).unwrap().terminator = Some(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(edge[1], vec![])),
                OriginId::UNKNOWN,
            ));
        }
        blocks
            .get_mut(*block_order.last().unwrap())
            .unwrap()
            .terminator = Some(Terminator::new(
            TerminatorKind::Return(vec![]),
            OriginId::UNKNOWN,
        ));
        let entry = block_order[0];
        block_order.rotate_left(1);
        let body = FunctionBody {
            blocks,
            instructions: EntityVec::new(),
            values: EntityVec::new(),
            block_order,
            entry,
        };
        verify_function(&program, &sources, function, &body).unwrap();
        (sources, program, function, body)
    }

    #[test]
    fn twenty_thousand_block_adversarial_chain_is_iterative_and_one_batch() {
        const BLOCKS: usize = 20_000;
        let (sources, program, function, mut body) = large_chain_body(BLOCKS);
        let entry = body.entry();
        let outcome = run_fusion(
            &program,
            &sources,
            function,
            &mut body,
            FusionLimits::derived(),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.completion, FusionCompletion::Complete);
        assert_eq!(outcome.statistics.block_visits, BLOCKS);
        assert_eq!(outcome.statistics.regions_fused, 1);
        assert_eq!(outcome.statistics.blocks_consumed, BLOCKS - 1);
        assert_eq!(outcome.statistics.head_reservations, 1);
        assert_eq!(outcome.statistics.layout_retain_scans, 1);
        assert_eq!(body.block_order(), &[entry]);
        verify_function(&program, &sources, function, &body).unwrap();
    }
}
