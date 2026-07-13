//! Closed orchestration and bounded aggregate reporting for the baseline Core passes.

use std::error::Error;
use std::fmt;

use super::{canonicalize, cse, dce, fusion, sccp};
use crate::diagnostic::Diagnostics;
use crate::entity::EntityId;
use crate::ir::core::{
    CoreProgram, FunctionBody, FunctionEditor, FunctionId, InstId, ValueId, verify_function,
};
use crate::source::{OriginId, SourceContext};

/// Number of stable invocations in the baseline per-function pipeline.
pub const BASELINE_PIPELINE_STEP_COUNT: usize = 7;

const LIMIT_REASON_COUNT: usize = 12;

/// One stable invocation in the closed baseline Core pipeline.
///
/// Invocation identity is deliberately separate from pass kind so initial and final
/// cleanup remain distinguishable in reports and developer instrumentation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum CorePipelineStep {
    /// Initial cheap local canonicalization.
    CanonicalizeInitial,
    /// Sparse conditional constant propagation and its atomic application.
    Sccp,
    /// Dead-instruction cleanup immediately after SCCP.
    DceAfterSccp,
    /// Maximal straight-line jump-region fusion.
    Fusion,
    /// Dominance-scoped common-subexpression elimination.
    Cse,
    /// The single conditional post-fusion/CSE canonicalization cleanup.
    CanonicalizeFinal,
    /// The single conditional post-fusion/CSE dead-instruction cleanup.
    DceFinal,
}

impl CorePipelineStep {
    const ORDERED: [Self; BASELINE_PIPELINE_STEP_COUNT] = [
        Self::CanonicalizeInitial,
        Self::Sccp,
        Self::DceAfterSccp,
        Self::Fusion,
        Self::Cse,
        Self::CanonicalizeFinal,
        Self::DceFinal,
    ];

    pub(super) const fn index(self) -> usize {
        match self {
            Self::CanonicalizeInitial => 0,
            Self::Sccp => 1,
            Self::DceAfterSccp => 2,
            Self::Fusion => 3,
            Self::Cse => 4,
            Self::CanonicalizeFinal => 5,
            Self::DceFinal => 6,
        }
    }

    const fn kind(self) -> CorePassKind {
        match self {
            Self::CanonicalizeInitial | Self::CanonicalizeFinal => CorePassKind::Canonicalize,
            Self::Sccp => CorePassKind::Sccp,
            Self::DceAfterSccp | Self::DceFinal => CorePassKind::Dce,
            Self::Fusion => CorePassKind::Fusion,
            Self::Cse => CorePassKind::Cse,
        }
    }
}

impl fmt::Display for CorePipelineStep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CanonicalizeInitial => "canonicalize.initial",
            Self::Sccp => "sccp",
            Self::DceAfterSccp => "dce.after-sccp",
            Self::Fusion => "fusion",
            Self::Cse => "cse",
            Self::CanonicalizeFinal => "canonicalize.final",
            Self::DceFinal => "dce.final",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CorePassKind {
    Canonicalize,
    Sccp,
    Dce,
    Fusion,
    Cse,
}

/// Why a configured pipeline invocation did not execute.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CoreStepSkipReason {
    /// Neither fusion nor CSE changed the function, so cleanup could expose nothing
    /// that those passes enabled.
    NoEnablingChange,
}

impl fmt::Display for CoreStepSkipReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEnablingChange => formatter.write_str("no-enabling-change"),
        }
    }
}

/// Stable typed reason an optional pass stopped conservatively.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum CorePassLimitReason {
    /// Canonicalization's allocated-entity tables exceeded their configured bound.
    CanonicalizationDenseTables,
    /// Canonicalization reached its complete-root visit bound.
    CanonicalizationRootVisits,
    /// SCCP's logical table requirement exceeded its configured bound.
    SccpTables,
    /// SCCP reached its propagation-event bound.
    SccpEvents,
    /// SCCP could not represent a safe derived event bound.
    SccpDerivedBoundOverflow,
    /// DCE's allocated-entity tables exceeded their configured bound.
    DceDenseTables,
    /// DCE reached its whole-candidate visit bound.
    DceCandidateVisits,
    /// Fusion's allocated fact-table requirement exceeded its configured bound.
    FusionFactTables,
    /// Fusion reached its complete-block visit bound.
    FusionBlockVisits,
    /// CSE's allocated analysis-slot requirement exceeded its configured bound.
    CseAnalysisSize,
    /// CSE reached its maximum live expressions on one dominator path.
    CseActiveEntries,
    /// CSE reached its complete-root visit bound.
    CseRootVisits,
}

impl CorePassLimitReason {
    const fn index(self) -> usize {
        match self {
            Self::CanonicalizationDenseTables => 0,
            Self::CanonicalizationRootVisits => 1,
            Self::SccpTables => 2,
            Self::SccpEvents => 3,
            Self::SccpDerivedBoundOverflow => 4,
            Self::DceDenseTables => 5,
            Self::DceCandidateVisits => 6,
            Self::FusionFactTables => 7,
            Self::FusionBlockVisits => 8,
            Self::CseAnalysisSize => 9,
            Self::CseActiveEntries => 10,
            Self::CseRootVisits => 11,
        }
    }
}

impl fmt::Display for CorePassLimitReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CanonicalizationDenseTables => "canonicalization.dense-tables",
            Self::CanonicalizationRootVisits => "canonicalization.root-visits",
            Self::SccpTables => "sccp.tables",
            Self::SccpEvents => "sccp.events",
            Self::SccpDerivedBoundOverflow => "sccp.derived-bound-overflow",
            Self::DceDenseTables => "dce.dense-tables",
            Self::DceCandidateVisits => "dce.candidate-visits",
            Self::FusionFactTables => "fusion.fact-tables",
            Self::FusionBlockVisits => "fusion.block-visits",
            Self::CseAnalysisSize => "cse.analysis-size",
            Self::CseActiveEntries => "cse.active-entries",
            Self::CseRootVisits => "cse.root-visits",
        })
    }
}

/// An aggregate statistic whose exact mathematical value may exceed `u64`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StatisticCount {
    /// The exact aggregate count.
    Exact(u64),
    /// At least one source or aggregate addition exceeded `u64`.
    Saturated,
}

impl StatisticCount {
    const ZERO: Self = Self::Exact(0);

    const fn from_u64(value: u64) -> Self {
        Self::Exact(value)
    }

    fn from_usize(value: usize) -> Self {
        u64::try_from(value).map_or(Self::Saturated, Self::Exact)
    }

    fn add_assign(&mut self, other: Self) {
        *self = match (*self, other) {
            (Self::Exact(left), Self::Exact(right)) => {
                left.checked_add(right).map_or(Self::Saturated, Self::Exact)
            }
            (Self::Saturated, _) | (_, Self::Saturated) => Self::Saturated,
        };
    }

    fn max_assign(&mut self, other: Self) {
        *self = match (*self, other) {
            (Self::Exact(left), Self::Exact(right)) => Self::Exact(left.max(right)),
            (Self::Saturated, _) | (_, Self::Saturated) => Self::Saturated,
        };
    }

    /// Returns the exact count, or `None` when it saturated.
    #[must_use]
    pub const fn exact(self) -> Option<u64> {
        match self {
            Self::Exact(value) => Some(value),
            Self::Saturated => None,
        }
    }

    /// Returns whether the count saturated.
    #[must_use]
    pub const fn is_saturated(self) -> bool {
        matches!(self, Self::Saturated)
    }
}

impl fmt::Display for StatisticCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(value) => value.fmt(formatter),
            Self::Saturated => formatter.write_str("saturated"),
        }
    }
}

macro_rules! statistics_struct {
    ($name:ident { $($(#[$field_doc:meta])* $field:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[non_exhaustive]
        pub struct $name {
            $($(#[$field_doc])* pub $field: StatisticCount,)+
        }

        impl $name {
            const ZERO: Self = Self {
                $($field: StatisticCount::ZERO,)+
            };

            fn merge_sum(&mut self, other: Self) {
                $(self.$field.add_assign(other.$field);)+
            }
        }
    };
}

statistics_struct!(CanonicalizationStatistics {
    /// Complete reachable roots inspected.
    roots_visited,
    /// SSA value mappings applied.
    value_replacements,
    /// Typed constants materialized at definition sites.
    constants_materialized,
    /// Commutative operand pairs put in canonical order.
    operands_reordered,
    /// Branch terminators folded.
    branches_folded,
    /// Discardable instructions erased.
    instructions_erased,
});

statistics_struct!(SccpStatistics {
    /// Logical solver table entries admitted.
    table_entries,
    /// Propagation fuel consumed.
    event_fuel_used,
    /// Events dequeued.
    events_dequeued,
    /// Lattice transitions committed.
    lattice_transitions,
    /// Semantic CFG edges activated.
    edges_activated,
    /// Edge arguments propagated.
    edge_arguments_propagated,
    /// Unknown executable definitions resolved pessimistically.
    pessimistic_resolutions,
    /// Maximum queue length across all function invocations.
    maximum_queue_length,
    /// Proven constants replaced.
    constants_replaced,
    /// Branch terminators folded.
    branches_folded,
    /// Newly unreachable blocks detached.
    blocks_detached,
});

statistics_struct!(DceStatistics {
    /// Attached use occurrences counted.
    attached_uses_counted,
    /// Complete candidate transitions visited.
    candidate_visits,
    /// Discardable instructions erased.
    instructions_erased,
});

statistics_struct!(FusionStatistics {
    /// Immutable fusion fact snapshots built.
    fact_builds,
    /// Fact-table entries required.
    required_fact_table_entries,
    /// Attached semantic edges counted.
    attached_edges_counted,
    /// Raw instruction memberships inspected.
    raw_instruction_memberships,
    /// Complete blocks visited during region discovery.
    block_visits,
    /// Maximal regions fused.
    regions_fused,
    /// Blocks consumed into region heads.
    blocks_consumed,
    /// Selected-region raw instruction memberships preflighted before mutation.
    selected_instruction_memberships_preflighted,
    /// Parameter-mapping slots used.
    parameter_mapping_slots_used,
    /// Replacement links traversed.
    replacement_links_visited,
    /// Attached values scanned during application.
    attached_values_scanned,
    /// Whole attached-use rewrite scans.
    attached_rewrite_scans,
    /// Region heads reserved before mutation.
    head_reservations,
    /// Whole layout-retain scans.
    layout_retain_scans,
});

statistics_struct!(CseStatistics {
    /// Immutable planner fact snapshots built.
    planning_fact_snapshots_built,
    /// Dominator structures built by the CSE planner.
    planning_dominator_builds,
    /// Dominator structures rebuilt by atomic replacement validation.
    replacement_validation_dominator_builds,
    /// Whole attached-use audits built after replacement and before erasure.
    post_replacement_use_audits,
    /// Complete instruction roots inspected.
    roots_visited,
    /// Structural expression keys built.
    keys_built,
    /// Scoped table insertions.
    table_insertions,
    /// Scoped table hits.
    table_hits,
    /// Scoped table removals during dominance exit.
    table_removals,
    /// Maximum active expressions on any dominator path.
    maximum_active_entries,
    /// SSA values replaced.
    value_replacements,
    /// Redundant instructions erased.
    instructions_erased,
    /// Atomic replacement batches applied.
    replacement_batches,
    /// Atomic erasure batches applied.
    erasure_batches,
});

/// Fixed pass-specific structural counters for one pipeline step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CorePassStatistics {
    /// Cheap canonicalization counters.
    Canonicalization(CanonicalizationStatistics),
    /// Sparse conditional constant propagation counters.
    Sccp(SccpStatistics),
    /// Dead-instruction elimination counters.
    Dce(DceStatistics),
    /// Straight-line fusion counters.
    Fusion(FusionStatistics),
    /// Dominance-scoped CSE counters.
    Cse(CseStatistics),
}

impl CorePassStatistics {
    const fn zero(kind: CorePassKind) -> Self {
        match kind {
            CorePassKind::Canonicalize => Self::Canonicalization(CanonicalizationStatistics::ZERO),
            CorePassKind::Sccp => Self::Sccp(SccpStatistics::ZERO),
            CorePassKind::Dce => Self::Dce(DceStatistics::ZERO),
            CorePassKind::Fusion => Self::Fusion(FusionStatistics::ZERO),
            CorePassKind::Cse => Self::Cse(CseStatistics::ZERO),
        }
    }

    fn merge(&mut self, other: Self) {
        match (self, other) {
            (Self::Canonicalization(left), Self::Canonicalization(right)) => {
                left.merge_sum(right);
            }
            (Self::Sccp(left), Self::Sccp(right)) => {
                let left_maximum = left.maximum_queue_length;
                let right_maximum = right.maximum_queue_length;
                left.merge_sum(right);
                left.maximum_queue_length = left_maximum;
                left.maximum_queue_length.max_assign(right_maximum);
            }
            (Self::Dce(left), Self::Dce(right)) => left.merge_sum(right),
            (Self::Fusion(left), Self::Fusion(right)) => left.merge_sum(right),
            (Self::Cse(left), Self::Cse(right)) => {
                let left_maximum = left.maximum_active_entries;
                let right_maximum = right.maximum_active_entries;
                left.merge_sum(right);
                left.maximum_active_entries = left_maximum;
                left.maximum_active_entries.max_assign(right_maximum);
            }
            _ => unreachable!("closed pipeline step and statistics kind must agree"),
        }
    }
}

/// Fixed aggregate outcome for one stable pipeline invocation across all functions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorePipelineStepSummary {
    step: CorePipelineStep,
    functions_seen: StatisticCount,
    functions_changed: StatisticCount,
    functions_skipped: StatisticCount,
    functions_completed: StatisticCount,
    functions_stopped_at_limit: StatisticCount,
    no_enabling_change: StatisticCount,
    limit_counts: [StatisticCount; LIMIT_REASON_COUNT],
    statistics: CorePassStatistics,
}

impl CorePipelineStepSummary {
    const fn new(step: CorePipelineStep) -> Self {
        Self {
            step,
            functions_seen: StatisticCount::ZERO,
            functions_changed: StatisticCount::ZERO,
            functions_skipped: StatisticCount::ZERO,
            functions_completed: StatisticCount::ZERO,
            functions_stopped_at_limit: StatisticCount::ZERO,
            no_enabling_change: StatisticCount::ZERO,
            limit_counts: [StatisticCount::ZERO; LIMIT_REASON_COUNT],
            statistics: CorePassStatistics::zero(step.kind()),
        }
    }

    /// Returns this invocation's stable identity.
    #[must_use]
    pub const fn step(&self) -> CorePipelineStep {
        self.step
    }

    /// Returns the number of functions accounted for, including skipped invocations.
    #[must_use]
    pub const fn functions_seen(&self) -> StatisticCount {
        self.functions_seen
    }

    /// Returns the number of executed invocations that changed their function.
    #[must_use]
    pub const fn functions_changed(&self) -> StatisticCount {
        self.functions_changed
    }

    /// Returns the number of configured invocations that did not execute.
    #[must_use]
    pub const fn functions_skipped(&self) -> StatisticCount {
        self.functions_skipped
    }

    /// Returns the number of invocations that completed their full bounded analysis.
    #[must_use]
    pub const fn functions_completed(&self) -> StatisticCount {
        self.functions_completed
    }

    /// Returns the number of invocations that took a conservative typed-limit fallback.
    #[must_use]
    pub const fn functions_stopped_at_limit(&self) -> StatisticCount {
        self.functions_stopped_at_limit
    }

    /// Returns the count for a particular typed pass-limit fallback.
    #[must_use]
    pub const fn limit_count(&self, reason: CorePassLimitReason) -> StatisticCount {
        self.limit_counts[reason.index()]
    }

    /// Returns the number skipped because fusion and CSE exposed no cleanup work.
    #[must_use]
    pub const fn no_enabling_change_count(&self) -> StatisticCount {
        self.no_enabling_change
    }

    /// Returns fixed pass-specific structural counters.
    #[must_use]
    pub const fn statistics(&self) -> &CorePassStatistics {
        &self.statistics
    }

    fn record_ran(&mut self, outcome: PassOutcome) {
        self.functions_seen.add_assign(StatisticCount::from_u64(1));
        if outcome.changed {
            self.functions_changed
                .add_assign(StatisticCount::from_u64(1));
        }
        match outcome.completion {
            PassCompletion::Complete => self
                .functions_completed
                .add_assign(StatisticCount::from_u64(1)),
            PassCompletion::StoppedAtLimit(reason) => {
                self.functions_stopped_at_limit
                    .add_assign(StatisticCount::from_u64(1));
                self.limit_counts[reason.index()].add_assign(StatisticCount::from_u64(1));
            }
        }
        self.statistics.merge(outcome.statistics);
    }

    fn record_skip(&mut self, reason: CoreStepSkipReason) {
        self.functions_seen.add_assign(StatisticCount::from_u64(1));
        self.functions_skipped
            .add_assign(StatisticCount::from_u64(1));
        match reason {
            CoreStepSkipReason::NoEnablingChange => self
                .no_enabling_change
                .add_assign(StatisticCount::from_u64(1)),
        }
    }
}

pub(super) fn empty_summaries() -> [CorePipelineStepSummary; BASELINE_PIPELINE_STEP_COUNT] {
    CorePipelineStep::ORDERED.map(CorePipelineStepSummary::new)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PassCompletion {
    Complete,
    StoppedAtLimit(CorePassLimitReason),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PassOutcome {
    changed: bool,
    completion: PassCompletion,
    statistics: CorePassStatistics,
}

/// Internal validation policy. This is instrumentation, not an optimization level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum VerificationPolicy {
    PipelineBoundaries,
    AfterEachPass,
}

impl VerificationPolicy {
    pub(super) const fn default_for_build() -> Self {
        if cfg!(any(test, debug_assertions))
            || option_env!("CI").is_some()
            || option_env!("MDL_VERIFY_CORE_EACH_PASS").is_some()
        {
            Self::AfterEachPass
        } else {
            Self::PipelineBoundaries
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PipelineLimits {
    canonicalize_initial: canonicalize::CanonicalizationLimits,
    sccp: sccp::SccpLimits,
    dce_after_sccp: dce::DceLimits,
    fusion: fusion::FusionLimits,
    cse: cse::CseLimits,
    canonicalize_final: canonicalize::CanonicalizationLimits,
    dce_final: dce::DceLimits,
}

impl PipelineLimits {
    pub(super) const fn derived() -> Self {
        Self {
            canonicalize_initial: canonicalize::CanonicalizationLimits::derived(),
            sccp: sccp::SccpLimits::derived(),
            dce_after_sccp: dce::DceLimits::derived(),
            fusion: fusion::FusionLimits::derived(),
            cse: cse::CseLimits::derived(),
            canonicalize_final: canonicalize::CanonicalizationLimits::derived(),
            dce_final: dce::DceLimits::derived(),
        }
    }

    #[cfg(test)]
    pub(super) fn for_step_limit_test(step: CorePipelineStep) -> Self {
        let mut limits = Self::derived();
        match step {
            CorePipelineStep::CanonicalizeInitial => {
                limits.canonicalize_initial = canonicalize::CanonicalizationLimits::derived()
                    .with_root_limit(canonicalize::CanonicalizationRootLimit::new(0));
            }
            CorePipelineStep::Sccp => {
                limits.sccp =
                    sccp::SccpLimits::derived().with_event_limit(sccp::SccpEventLimit::new(0));
            }
            CorePipelineStep::DceAfterSccp => {
                limits.dce_after_sccp =
                    dce::DceLimits::derived().with_candidate_limit(dce::DceCandidateLimit::new(0));
            }
            CorePipelineStep::Fusion => {
                limits.fusion = fusion::FusionLimits::derived()
                    .with_block_visit_limit(fusion::FusionBlockVisitLimit::new(0));
            }
            CorePipelineStep::Cse => {
                limits.cse = cse::CseLimits::derived().with_root_limit(cse::CseRootLimit::new(0));
            }
            CorePipelineStep::CanonicalizeFinal => {
                limits.canonicalize_final = canonicalize::CanonicalizationLimits::derived()
                    .with_root_limit(canonicalize::CanonicalizationRootLimit::new(0));
            }
            CorePipelineStep::DceFinal => {
                limits.dce_final =
                    dce::DceLimits::derived().with_candidate_limit(dce::DceCandidateLimit::new(0));
            }
        }
        limits
    }

    // Keeps every typed explicit-limit path compiled in ordinary builds. Public
    // optimization does not expose this developer override yet, but the distinction
    // must not exist only under `cfg(test)`.
    #[allow(dead_code)]
    fn all_explicit_zero_for_developer() -> Self {
        Self {
            canonicalize_initial: canonicalize::CanonicalizationLimits::derived()
                .with_table_limit(canonicalize::CanonicalizationTableLimit::new(0))
                .with_root_limit(canonicalize::CanonicalizationRootLimit::new(0)),
            sccp: sccp::SccpLimits::derived()
                .with_table_limit(sccp::SccpTableLimit::new(0))
                .with_event_limit(sccp::SccpEventLimit::new(0)),
            dce_after_sccp: dce::DceLimits::derived()
                .with_table_limit(dce::DceTableLimit::new(0))
                .with_candidate_limit(dce::DceCandidateLimit::new(0)),
            fusion: fusion::FusionLimits::derived()
                .with_fact_table_limit(fusion::FusionFactTableEntryLimit::new(0))
                .with_block_visit_limit(fusion::FusionBlockVisitLimit::new(0)),
            cse: cse::CseLimits::derived()
                .with_analysis_slot_limit(cse::CseAnalysisSlotLimit::new(0))
                .with_active_entry_limit(cse::CseActiveEntryLimit::new(0))
                .with_root_limit(cse::CseRootLimit::new(0)),
            canonicalize_final: canonicalize::CanonicalizationLimits::derived()
                .with_table_limit(canonicalize::CanonicalizationTableLimit::new(0))
                .with_root_limit(canonicalize::CanonicalizationRootLimit::new(0)),
            dce_final: dce::DceLimits::derived()
                .with_table_limit(dce::DceTableLimit::new(0))
                .with_candidate_limit(dce::DceCandidateLimit::new(0)),
        }
    }
}

impl Default for PipelineLimits {
    fn default() -> Self {
        Self::derived()
    }
}

/// Developer-only observation boundary. Production uses the zero-sized no-op sink.
///
/// Timing or complete diagnostic dumps can be implemented by a caller of the private
/// runner without retaining either in the deterministic optimization report.
pub(super) trait PipelineInstrumentation {
    fn before_step(
        &mut self,
        _program: &CoreProgram,
        _function: FunctionId,
        _body: &FunctionBody,
        _step: CorePipelineStep,
    ) {
    }

    fn after_step(
        &mut self,
        _program: &CoreProgram,
        _function: FunctionId,
        _body: &FunctionBody,
        _step: CorePipelineStep,
        _changed: bool,
    ) {
    }

    fn skipped(
        &mut self,
        _function: FunctionId,
        _step: CorePipelineStep,
        _reason: CoreStepSkipReason,
    ) {
    }
}

pub(super) struct NoInstrumentation;

impl PipelineInstrumentation for NoInstrumentation {}

/// Bounded selection policy for optional Core optimization remarks.
///
/// Remarks are disabled by default. Enabling a policy never changes optimization;
/// it only retains matching records up to one compilation-wide cap while continuing
/// to count matching omitted records exactly or with explicit saturation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CoreRemarkPolicy {
    kinds: u8,
    function_filter: Option<FunctionId>,
    step_filter: Option<CorePipelineStep>,
    record_limit: usize,
}

impl CoreRemarkPolicy {
    const LIMIT_MISSES: u8 = 1 << 0;
    const CSE_APPLIED: u8 = 1 << 1;

    /// Disables all detailed remarks and their backing stream.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            kinds: 0,
            function_filter: None,
            step_filter: None,
            record_limit: 0,
        }
    }

    /// Requests typed optional-limit misses with at most `record_limit` records.
    #[must_use]
    pub const fn limit_misses(record_limit: usize) -> Self {
        Self {
            kinds: Self::LIMIT_MISSES,
            function_filter: None,
            step_filter: None,
            record_limit,
        }
    }

    /// Requests applied CSE correspondences with at most `record_limit` records.
    #[must_use]
    pub const fn cse_applied(record_limit: usize) -> Self {
        Self {
            kinds: Self::CSE_APPLIED,
            function_filter: None,
            step_filter: Some(CorePipelineStep::Cse),
            record_limit,
        }
    }

    /// Requests both currently supported remark kinds with one shared record cap.
    #[must_use]
    pub const fn all(record_limit: usize) -> Self {
        Self {
            kinds: Self::LIMIT_MISSES | Self::CSE_APPLIED,
            function_filter: None,
            step_filter: None,
            record_limit,
        }
    }

    /// Restricts matching records and aggregate counts to one function.
    #[must_use]
    pub const fn with_function_filter(mut self, function: FunctionId) -> Self {
        self.function_filter = Some(function);
        self
    }

    /// Restricts matching records and aggregate counts to one stable pipeline step.
    #[must_use]
    pub const fn with_step_filter(mut self, step: CorePipelineStep) -> Self {
        self.step_filter = Some(step);
        self
    }

    fn accepts(self, kind: u8, function: FunctionId, step: CorePipelineStep) -> bool {
        self.kinds & kind != 0
            && match self.function_filter {
                Some(expected) => expected.index() == function.index(),
                None => true,
            }
            && match self.step_filter {
                Some(expected) => expected.index() == step.index(),
                None => true,
            }
    }
}

impl Default for CoreRemarkPolicy {
    fn default() -> Self {
        Self::disabled()
    }
}

/// One truthful retained Core optimization event.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CoreOptimizationRemark {
    /// A pass stopped conservatively at a configured optional limit.
    LimitMiss {
        /// Function whose invocation stopped.
        function: FunctionId,
        /// Stable pipeline invocation.
        step: CorePipelineStep,
        /// Typed resource whose limit was reached.
        reason: CorePassLimitReason,
        /// Deterministic human-readable explanation.
        explanation: String,
    },
    /// One redundant instruction eliminated by dominance-scoped CSE.
    CseApplied {
        /// Function containing both sites.
        function: FunctionId,
        /// Stable CSE pipeline invocation.
        step: CorePipelineStep,
        /// Eliminated instruction identity.
        duplicate: InstId,
        /// Surviving dominating instruction identity.
        representative: InstId,
        /// Complete ordered results of the eliminated instruction.
        duplicate_results: Box<[ValueId]>,
        /// Complete corresponding ordered results of the representative.
        representative_results: Box<[ValueId]>,
        /// Provenance of the eliminated instruction.
        duplicate_origin: OriginId,
        /// Provenance of the surviving representative.
        representative_origin: OriginId,
        /// Deterministic human-readable explanation.
        explanation: String,
    },
}

/// Filtered, capped optional Core optimization detail for one compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreRemarkStream {
    pub(super) records: Vec<CoreOptimizationRemark>,
    pub(super) reason_counts: [StatisticCount; LIMIT_REASON_COUNT],
    pub(super) cse_applied: StatisticCount,
    pub(super) omitted: StatisticCount,
}

impl CoreRemarkStream {
    /// Returns retained records in their deterministic submission order.
    #[must_use]
    pub fn records(&self) -> &[CoreOptimizationRemark] {
        &self.records
    }

    /// Returns matching limit misses for one typed reason, including omitted records.
    #[must_use]
    pub const fn limit_miss_count(&self, reason: CorePassLimitReason) -> StatisticCount {
        self.reason_counts[reason.index()]
    }

    /// Returns matching applied CSE sites, including omitted records.
    #[must_use]
    pub const fn cse_applied_count(&self) -> StatisticCount {
        self.cse_applied
    }

    /// Returns matching records excluded by the shared retention cap.
    #[must_use]
    pub const fn omitted_count(&self) -> StatisticCount {
        self.omitted
    }
}

pub(super) struct CoreRemarkSink {
    policy: CoreRemarkPolicy,
    stream: Option<CoreRemarkStream>,
}

impl CoreRemarkSink {
    fn new(policy: CoreRemarkPolicy) -> Self {
        Self {
            policy,
            stream: (policy.kinds != 0).then(|| CoreRemarkStream {
                records: Vec::new(),
                reason_counts: [StatisticCount::ZERO; LIMIT_REASON_COUNT],
                cse_applied: StatisticCount::ZERO,
                omitted: StatisticCount::ZERO,
            }),
        }
    }

    fn submit_limit_miss(
        &mut self,
        function: FunctionId,
        step: CorePipelineStep,
        reason: CorePassLimitReason,
        explanation: impl FnOnce() -> String,
    ) {
        if !self
            .policy
            .accepts(CoreRemarkPolicy::LIMIT_MISSES, function, step)
        {
            return;
        }
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        stream.reason_counts[reason.index()].add_assign(StatisticCount::from_u64(1));
        if stream.records.len() >= self.policy.record_limit {
            stream.omitted.add_assign(StatisticCount::from_u64(1));
            return;
        }
        stream.records.push(CoreOptimizationRemark::LimitMiss {
            function,
            step,
            reason,
            explanation: explanation(),
        });
    }

    fn retains_cse_applied(&self, function: FunctionId, step: CorePipelineStep) -> bool {
        self.policy
            .accepts(CoreRemarkPolicy::CSE_APPLIED, function, step)
    }

    fn submit_cse_applied(
        &mut self,
        function: FunctionId,
        step: CorePipelineStep,
        rewrite: cse::CseAppliedRewrite<'_>,
    ) {
        if !self.retains_cse_applied(function, step) {
            return;
        }
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        stream.cse_applied.add_assign(StatisticCount::from_u64(1));
        if stream.records.len() >= self.policy.record_limit {
            stream.omitted.add_assign(StatisticCount::from_u64(1));
            return;
        }
        stream.records.push(CoreOptimizationRemark::CseApplied {
            function,
            step,
            duplicate: rewrite.duplicate,
            representative: rewrite.representative,
            duplicate_results: rewrite.duplicate_results.into(),
            representative_results: rewrite.representative_results.into(),
            duplicate_origin: rewrite.duplicate_origin,
            representative_origin: rewrite.representative_origin,
            explanation: format!(
                "eliminated {:?} in favor of dominating {:?}",
                rewrite.duplicate, rewrite.representative
            ),
        });
    }

    fn finish(self) -> Option<CoreRemarkStream> {
        self.stream
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PipelineConfiguration {
    pub(super) verification: VerificationPolicy,
    pub(super) limits: PipelineLimits,
    pub(super) remarks: CoreRemarkPolicy,
    #[cfg(test)]
    pub(super) fault: Option<PipelineTestFault>,
}

impl PipelineConfiguration {
    pub(super) const fn for_build() -> Self {
        Self {
            verification: VerificationPolicy::default_for_build(),
            limits: PipelineLimits::derived(),
            remarks: CoreRemarkPolicy::disabled(),
            #[cfg(test)]
            fault: None,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PipelineTestFault {
    FailBefore(CorePipelineStep),
    CorruptAfter(CorePipelineStep),
}

pub(super) struct PipelineOutput {
    pub(super) summaries: [CorePipelineStepSummary; BASELINE_PIPELINE_STEP_COUNT],
    pub(super) remarks: Option<CoreRemarkStream>,
}

#[derive(Debug)]
pub(super) enum PipelineFailure {
    MissingVerifiedBody {
        function: FunctionId,
        step: CorePipelineStep,
    },
    Pass {
        function: FunctionId,
        step: CorePipelineStep,
        error: CorePassError,
    },
    Verification {
        function: FunctionId,
        step: CorePipelineStep,
        diagnostics: Diagnostics,
    },
    #[cfg(test)]
    Injected {
        function: FunctionId,
        step: CorePipelineStep,
    },
}

impl fmt::Display for PipelineFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingVerifiedBody { function, step } => write!(
                formatter,
                "verified function {function:?} lost its body before {step}"
            ),
            Self::Pass {
                function,
                step,
                error,
            } => write!(formatter, "{step} failed for {function:?}: {error}"),
            Self::Verification {
                function,
                step,
                diagnostics,
            } => write!(
                formatter,
                "verification after {step} failed for {function:?}: {diagnostics}"
            ),
            #[cfg(test)]
            Self::Injected { function, step } => {
                write!(formatter, "injected {step} failure for {function:?}")
            }
        }
    }
}

impl Error for PipelineFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Pass { error, .. } => Some(error),
            Self::Verification { diagnostics, .. } => Some(diagnostics),
            Self::MissingVerifiedBody { .. } => None,
            #[cfg(test)]
            Self::Injected { .. } => None,
        }
    }
}

#[derive(Debug)]
pub(super) enum CorePassError {
    Canonicalize(canonicalize::CanonicalizationError),
    Sccp(sccp::SccpError),
    Dce(dce::DceError),
    Fusion(fusion::FusionError),
    Cse(cse::CseError),
}

impl fmt::Display for CorePassError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canonicalize(error) => write!(formatter, "canonicalization: {error}"),
            Self::Sccp(error) => write!(formatter, "SCCP: {error}"),
            Self::Dce(error) => write!(formatter, "DCE: {error}"),
            Self::Fusion(error) => write!(formatter, "fusion: {error}"),
            Self::Cse(error) => write!(formatter, "CSE: {error}"),
        }
    }
}

impl Error for CorePassError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Canonicalize(error) => Some(error),
            Self::Sccp(error) => Some(error),
            Self::Dce(error) => Some(error),
            Self::Fusion(error) => Some(error),
            Self::Cse(error) => Some(error),
        }
    }
}

pub(super) fn run_baseline_with<I: PipelineInstrumentation>(
    program: &mut CoreProgram,
    sources: &SourceContext,
    configuration: &PipelineConfiguration,
    instrumentation: &mut I,
) -> Result<PipelineOutput, PipelineFailure> {
    let mut summaries = empty_summaries();
    let mut remarks = CoreRemarkSink::new(configuration.remarks);
    let function_count = program.len();

    for raw_function in 0..function_count {
        let raw_function =
            u32::try_from(raw_function).map_err(|_| PipelineFailure::MissingVerifiedBody {
                function: FunctionId::from_index(u32::MAX),
                step: CorePipelineStep::CanonicalizeInitial,
            })?;
        let function = FunctionId::from_index(raw_function);
        let Some(mut body) = program.take_function_body(function) else {
            return Err(PipelineFailure::MissingVerifiedBody {
                function,
                step: CorePipelineStep::CanonicalizeInitial,
            });
        };

        let function_result = run_function_pipeline(
            program,
            sources,
            function,
            &mut body,
            configuration,
            instrumentation,
            &mut summaries,
            &mut remarks,
        );
        program.restore_function_body(function, body);
        function_result?;
    }

    Ok(PipelineOutput {
        summaries,
        remarks: remarks.finish(),
    })
}

#[allow(clippy::too_many_arguments)]
fn run_function_pipeline<I: PipelineInstrumentation>(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    configuration: &PipelineConfiguration,
    instrumentation: &mut I,
    summaries: &mut [CorePipelineStepSummary; BASELINE_PIPELINE_STEP_COUNT],
    remarks: &mut CoreRemarkSink,
) -> Result<(), PipelineFailure> {
    run_step(
        program,
        sources,
        function,
        body,
        CorePipelineStep::CanonicalizeInitial,
        configuration,
        instrumentation,
        summaries,
        remarks,
    )?;
    run_step(
        program,
        sources,
        function,
        body,
        CorePipelineStep::Sccp,
        configuration,
        instrumentation,
        summaries,
        remarks,
    )?;
    run_step(
        program,
        sources,
        function,
        body,
        CorePipelineStep::DceAfterSccp,
        configuration,
        instrumentation,
        summaries,
        remarks,
    )?;
    let fusion = run_step(
        program,
        sources,
        function,
        body,
        CorePipelineStep::Fusion,
        configuration,
        instrumentation,
        summaries,
        remarks,
    )?;
    let cse = run_step(
        program,
        sources,
        function,
        body,
        CorePipelineStep::Cse,
        configuration,
        instrumentation,
        summaries,
        remarks,
    )?;

    if fusion.changed || cse.changed {
        run_step(
            program,
            sources,
            function,
            body,
            CorePipelineStep::CanonicalizeFinal,
            configuration,
            instrumentation,
            summaries,
            remarks,
        )?;
        run_step(
            program,
            sources,
            function,
            body,
            CorePipelineStep::DceFinal,
            configuration,
            instrumentation,
            summaries,
            remarks,
        )?;
    } else {
        for step in [
            CorePipelineStep::CanonicalizeFinal,
            CorePipelineStep::DceFinal,
        ] {
            let reason = CoreStepSkipReason::NoEnablingChange;
            summaries[step.index()].record_skip(reason);
            instrumentation.skipped(function, step, reason);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_step<I: PipelineInstrumentation>(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    step: CorePipelineStep,
    configuration: &PipelineConfiguration,
    instrumentation: &mut I,
    summaries: &mut [CorePipelineStepSummary; BASELINE_PIPELINE_STEP_COUNT],
    remarks: &mut CoreRemarkSink,
) -> Result<PassOutcome, PipelineFailure> {
    #[cfg(test)]
    if configuration.fault == Some(PipelineTestFault::FailBefore(step)) {
        return Err(PipelineFailure::Injected { function, step });
    }
    instrumentation.before_step(program, function, body, step);
    let outcome = dispatch_step(
        program,
        sources,
        function,
        body,
        step,
        configuration.limits,
        remarks,
    )
    .map_err(|error| PipelineFailure::Pass {
        function,
        step,
        error,
    })?;
    summaries[step.index()].record_ran(outcome);
    if let PassCompletion::StoppedAtLimit(reason) = outcome.completion {
        remarks.submit_limit_miss(function, step, reason, || {
            format!("{step} stopped conservatively at {reason}")
        });
    }
    instrumentation.after_step(program, function, body, step, outcome.changed);

    #[cfg(test)]
    if configuration.fault == Some(PipelineTestFault::CorruptAfter(step)) {
        body.block_mut(body.entry())
            .expect("verified entry block exists")
            .terminator = None;
    }

    if configuration.verification == VerificationPolicy::AfterEachPass {
        verify_function(program, sources, function, body).map_err(|diagnostics| {
            PipelineFailure::Verification {
                function,
                step,
                diagnostics,
            }
        })?;
    }
    Ok(outcome)
}

fn dispatch_step(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    step: CorePipelineStep,
    limits: PipelineLimits,
    remarks: &mut CoreRemarkSink,
) -> Result<PassOutcome, CorePassError> {
    match step {
        CorePipelineStep::CanonicalizeInitial => canonicalize::run_canonicalization(
            program,
            sources,
            function,
            body,
            limits.canonicalize_initial,
        )
        .map(convert_canonicalization)
        .map_err(CorePassError::Canonicalize),
        CorePipelineStep::Sccp => {
            let mut editor = FunctionEditor::from_trusted_body(program, sources, function, body);
            sccp::run(&mut editor, limits.sccp)
                .map(convert_sccp)
                .map_err(CorePassError::Sccp)
        }
        CorePipelineStep::DceAfterSccp => {
            dce::run_dce(program, sources, function, body, limits.dce_after_sccp)
                .map(convert_dce)
                .map_err(CorePassError::Dce)
        }
        CorePipelineStep::Fusion => {
            fusion::run_fusion(program, sources, function, body, limits.fusion)
                .map(convert_fusion)
                .map_err(CorePassError::Fusion)
        }
        CorePipelineStep::Cse => {
            let result = if remarks.retains_cse_applied(function, step) {
                cse::run_cse_with_rewrite_sink(
                    program,
                    sources,
                    function,
                    body,
                    limits.cse,
                    |rewrite| remarks.submit_cse_applied(function, step, rewrite),
                )
            } else {
                // The plain entry deliberately avoids CSE's detail-only ordered-list
                // clone when applied correspondence is not requested.
                cse::run_cse(program, sources, function, body, limits.cse)
            };
            result.map(convert_cse).map_err(CorePassError::Cse)
        }
        CorePipelineStep::CanonicalizeFinal => canonicalize::run_canonicalization(
            program,
            sources,
            function,
            body,
            limits.canonicalize_final,
        )
        .map(convert_canonicalization)
        .map_err(CorePassError::Canonicalize),
        CorePipelineStep::DceFinal => {
            dce::run_dce(program, sources, function, body, limits.dce_final)
                .map(convert_dce)
                .map_err(CorePassError::Dce)
        }
    }
}

fn convert_canonicalization(outcome: canonicalize::CanonicalizationOutcome) -> PassOutcome {
    use canonicalize::{CanonicalizationCompletion, CanonicalizationLimitReason};
    let statistics = outcome.statistics;
    PassOutcome {
        changed: outcome.changed,
        completion: match outcome.completion {
            CanonicalizationCompletion::Complete => PassCompletion::Complete,
            CanonicalizationCompletion::StoppedAtLimit(reason) => {
                PassCompletion::StoppedAtLimit(match reason {
                    CanonicalizationLimitReason::DenseTables => {
                        CorePassLimitReason::CanonicalizationDenseTables
                    }
                    CanonicalizationLimitReason::RootVisits => {
                        CorePassLimitReason::CanonicalizationRootVisits
                    }
                })
            }
        },
        statistics: CorePassStatistics::Canonicalization(CanonicalizationStatistics {
            roots_visited: StatisticCount::from_usize(statistics.roots_visited),
            value_replacements: StatisticCount::from_usize(statistics.value_replacements),
            constants_materialized: StatisticCount::from_usize(statistics.constants_materialized),
            operands_reordered: StatisticCount::from_usize(statistics.operands_reordered),
            branches_folded: StatisticCount::from_usize(statistics.branches_folded),
            instructions_erased: StatisticCount::from_usize(statistics.instructions_erased),
        }),
    }
}

fn convert_sccp(outcome: sccp::SccpOutcome) -> PassOutcome {
    use sccp::{SccpCompletion, SccpLimitReason};
    let statistics = outcome.statistics();
    PassOutcome {
        changed: outcome.changed(),
        completion: match outcome.completion() {
            SccpCompletion::Complete => PassCompletion::Complete,
            SccpCompletion::StoppedAtLimit(reason) => {
                PassCompletion::StoppedAtLimit(match reason {
                    SccpLimitReason::Tables => CorePassLimitReason::SccpTables,
                    SccpLimitReason::Events => CorePassLimitReason::SccpEvents,
                    SccpLimitReason::DerivedBoundOverflow => {
                        CorePassLimitReason::SccpDerivedBoundOverflow
                    }
                })
            }
        },
        statistics: CorePassStatistics::Sccp(SccpStatistics {
            table_entries: StatisticCount::from_u64(statistics.table_entries),
            event_fuel_used: StatisticCount::from_u64(statistics.event_fuel_used),
            events_dequeued: StatisticCount::from_u64(statistics.events_dequeued),
            lattice_transitions: StatisticCount::from_u64(statistics.lattice_transitions),
            edges_activated: StatisticCount::from_u64(statistics.edges_activated),
            edge_arguments_propagated: StatisticCount::from_u64(
                statistics.edge_arguments_propagated,
            ),
            pessimistic_resolutions: StatisticCount::from_u64(statistics.pessimistic_resolutions),
            maximum_queue_length: StatisticCount::from_u64(statistics.maximum_queue_length),
            constants_replaced: StatisticCount::from_u64(statistics.constants_replaced),
            branches_folded: StatisticCount::from_u64(statistics.branches_folded),
            blocks_detached: StatisticCount::from_u64(statistics.blocks_detached),
        }),
    }
}

fn convert_dce(outcome: dce::DceOutcome) -> PassOutcome {
    use dce::{DceCompletion, DceLimitReason};
    let statistics = outcome.statistics;
    PassOutcome {
        changed: outcome.changed,
        completion: match outcome.completion {
            DceCompletion::Complete => PassCompletion::Complete,
            DceCompletion::StoppedAtLimit(reason) => PassCompletion::StoppedAtLimit(match reason {
                DceLimitReason::DenseTables => CorePassLimitReason::DceDenseTables,
                DceLimitReason::CandidateVisits => CorePassLimitReason::DceCandidateVisits,
            }),
        },
        statistics: CorePassStatistics::Dce(DceStatistics {
            attached_uses_counted: StatisticCount::from_usize(statistics.attached_uses_counted),
            candidate_visits: StatisticCount::from_usize(statistics.candidate_visits),
            instructions_erased: StatisticCount::from_usize(statistics.instructions_erased),
        }),
    }
}

fn convert_fusion(outcome: fusion::FusionOutcome) -> PassOutcome {
    use fusion::{FusionCompletion, FusionLimitReason};
    let statistics = outcome.statistics;
    PassOutcome {
        changed: outcome.changed,
        completion: match outcome.completion {
            FusionCompletion::Complete => PassCompletion::Complete,
            FusionCompletion::StoppedAtLimit(reason) => {
                PassCompletion::StoppedAtLimit(match reason {
                    FusionLimitReason::FactTables => CorePassLimitReason::FusionFactTables,
                    FusionLimitReason::BlockVisits => CorePassLimitReason::FusionBlockVisits,
                })
            }
        },
        statistics: CorePassStatistics::Fusion(FusionStatistics {
            fact_builds: StatisticCount::from_usize(statistics.fact_builds),
            required_fact_table_entries: StatisticCount::from_usize(
                statistics.required_fact_table_entries,
            ),
            attached_edges_counted: StatisticCount::from_usize(statistics.attached_edges_counted),
            raw_instruction_memberships: StatisticCount::from_usize(
                statistics.raw_instruction_memberships,
            ),
            block_visits: StatisticCount::from_usize(statistics.block_visits),
            regions_fused: StatisticCount::from_usize(statistics.regions_fused),
            blocks_consumed: StatisticCount::from_usize(statistics.blocks_consumed),
            selected_instruction_memberships_preflighted: StatisticCount::from_usize(
                statistics.selected_instruction_memberships_preflighted,
            ),
            parameter_mapping_slots_used: StatisticCount::from_usize(
                statistics.parameter_mapping_slots_used,
            ),
            replacement_links_visited: StatisticCount::from_usize(
                statistics.replacement_links_visited,
            ),
            attached_values_scanned: StatisticCount::from_usize(statistics.attached_values_scanned),
            attached_rewrite_scans: StatisticCount::from_usize(statistics.attached_rewrite_scans),
            head_reservations: StatisticCount::from_usize(statistics.head_reservations),
            layout_retain_scans: StatisticCount::from_usize(statistics.layout_retain_scans),
        }),
    }
}

fn convert_cse(outcome: cse::CseOutcome) -> PassOutcome {
    use cse::{CseCompletion, CseLimitReason};
    let statistics = outcome.statistics;
    PassOutcome {
        changed: outcome.changed,
        completion: match outcome.completion {
            CseCompletion::Complete => PassCompletion::Complete,
            CseCompletion::StoppedAtLimit(reason) => PassCompletion::StoppedAtLimit(match reason {
                CseLimitReason::AnalysisSize => CorePassLimitReason::CseAnalysisSize,
                CseLimitReason::ActiveEntries => CorePassLimitReason::CseActiveEntries,
                CseLimitReason::RootVisits => CorePassLimitReason::CseRootVisits,
            }),
        },
        statistics: CorePassStatistics::Cse(CseStatistics {
            planning_fact_snapshots_built: StatisticCount::from_usize(
                statistics.planning_fact_snapshots_built,
            ),
            planning_dominator_builds: StatisticCount::from_usize(
                statistics.planning_dominator_builds,
            ),
            replacement_validation_dominator_builds: StatisticCount::from_usize(
                statistics.replacement_validation_dominator_builds,
            ),
            post_replacement_use_audits: StatisticCount::from_usize(
                statistics.post_replacement_use_audits,
            ),
            roots_visited: StatisticCount::from_usize(statistics.roots_visited),
            keys_built: StatisticCount::from_usize(statistics.keys_built),
            table_insertions: StatisticCount::from_usize(statistics.table_insertions),
            table_hits: StatisticCount::from_usize(statistics.table_hits),
            table_removals: StatisticCount::from_usize(statistics.table_removals),
            maximum_active_entries: StatisticCount::from_usize(statistics.maximum_active_entries),
            value_replacements: StatisticCount::from_usize(statistics.value_replacements),
            instructions_erased: StatisticCount::from_usize(statistics.instructions_erased),
            replacement_batches: StatisticCount::from_usize(statistics.replacement_batches),
            erasure_batches: StatisticCount::from_usize(statistics.erasure_batches),
        }),
    }
}

pub(super) fn write_step_summary(
    formatter: &mut fmt::Formatter<'_>,
    summary: &CorePipelineStepSummary,
) -> fmt::Result {
    writeln!(
        formatter,
        "step {} seen={} changed={} skipped={} completed={} stopped-at-limit={} no-enabling-change={}",
        summary.step,
        summary.functions_seen,
        summary.functions_changed,
        summary.functions_skipped,
        summary.functions_completed,
        summary.functions_stopped_at_limit,
        summary.no_enabling_change,
    )?;
    for reason in limit_reasons_for_step(summary.step) {
        writeln!(
            formatter,
            "  limit {reason}={}",
            summary.limit_counts[reason.index()]
        )?;
    }
    write_statistics(formatter, summary.statistics)
}

fn limit_reasons_for_step(step: CorePipelineStep) -> &'static [CorePassLimitReason] {
    use CorePassLimitReason as Reason;
    match step.kind() {
        CorePassKind::Canonicalize => &[
            Reason::CanonicalizationDenseTables,
            Reason::CanonicalizationRootVisits,
        ],
        CorePassKind::Sccp => &[
            Reason::SccpTables,
            Reason::SccpEvents,
            Reason::SccpDerivedBoundOverflow,
        ],
        CorePassKind::Dce => &[Reason::DceDenseTables, Reason::DceCandidateVisits],
        CorePassKind::Fusion => &[Reason::FusionFactTables, Reason::FusionBlockVisits],
        CorePassKind::Cse => &[
            Reason::CseAnalysisSize,
            Reason::CseActiveEntries,
            Reason::CseRootVisits,
        ],
    }
}

fn write_statistics(
    formatter: &mut fmt::Formatter<'_>,
    statistics: CorePassStatistics,
) -> fmt::Result {
    macro_rules! write_fields {
        ($value:ident; $($field:ident),+ $(,)?) => {
            {
                $(writeln!(formatter, "  statistic {}={}", stringify!($field), $value.$field)?;)+
            }
        };
    }
    match statistics {
        CorePassStatistics::Canonicalization(value) => write_fields!(value;
            roots_visited,
            value_replacements,
            constants_materialized,
            operands_reordered,
            branches_folded,
            instructions_erased,
        ),
        CorePassStatistics::Sccp(value) => write_fields!(value;
            table_entries,
            event_fuel_used,
            events_dequeued,
            lattice_transitions,
            edges_activated,
            edge_arguments_propagated,
            pessimistic_resolutions,
            maximum_queue_length,
            constants_replaced,
            branches_folded,
            blocks_detached,
        ),
        CorePassStatistics::Dce(value) => write_fields!(value;
            attached_uses_counted,
            candidate_visits,
            instructions_erased,
        ),
        CorePassStatistics::Fusion(value) => write_fields!(value;
            fact_builds,
            required_fact_table_entries,
            attached_edges_counted,
            raw_instruction_memberships,
            block_visits,
            regions_fused,
            blocks_consumed,
            selected_instruction_memberships_preflighted,
            parameter_mapping_slots_used,
            replacement_links_visited,
            attached_values_scanned,
            attached_rewrite_scans,
            head_reservations,
            layout_retain_scans,
        ),
        CorePassStatistics::Cse(value) => write_fields!(value;
            planning_fact_snapshots_built,
            planning_dominator_builds,
            replacement_validation_dominator_builds,
            post_replacement_use_audits,
            roots_visited,
            keys_built,
            table_insertions,
            table_hits,
            table_removals,
            maximum_active_entries,
            value_replacements,
            instructions_erased,
            replacement_batches,
            erasure_batches,
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{
        CorePassLimitReason, CorePassStatistics, CorePipelineStep, CorePipelineStepSummary,
        CoreRemarkPolicy, CoreRemarkSink, CoreStepSkipReason, CseStatistics, PassCompletion,
        PassOutcome, StatisticCount, empty_summaries,
    };
    use crate::entity::EntityId;
    use crate::ir::core::FunctionId;

    #[test]
    fn stable_step_ids_are_distinct_and_ordered() {
        let summaries = empty_summaries();
        assert_eq!(
            summaries.map(|summary| summary.step()),
            CorePipelineStep::ORDERED
        );
        assert_eq!(
            CorePipelineStep::CanonicalizeInitial.to_string(),
            "canonicalize.initial"
        );
        assert_eq!(
            CorePipelineStep::CanonicalizeFinal.to_string(),
            "canonicalize.final"
        );
        assert_eq!(CorePipelineStep::DceAfterSccp.to_string(), "dce.after-sccp");
        assert_eq!(CorePipelineStep::DceFinal.to_string(), "dce.final");
    }

    #[test]
    fn aggregate_addition_and_maxima_saturate_explicitly() {
        let mut sum = StatisticCount::Exact(u64::MAX);
        sum.add_assign(StatisticCount::Exact(1));
        assert_eq!(sum, StatisticCount::Saturated);

        let mut maximum = StatisticCount::Exact(7);
        maximum.max_assign(StatisticCount::Exact(3));
        assert_eq!(maximum, StatisticCount::Exact(7));
        maximum.max_assign(StatisticCount::Saturated);
        assert_eq!(maximum, StatisticCount::Saturated);
    }

    #[test]
    fn fixed_step_summary_sums_work_but_keeps_true_maxima() {
        let mut summary = CorePipelineStepSummary::new(CorePipelineStep::Cse);
        for (roots, maximum) in [(3, 9), (5, 4)] {
            summary.record_ran(PassOutcome {
                changed: true,
                completion: PassCompletion::Complete,
                statistics: CorePassStatistics::Cse(CseStatistics {
                    roots_visited: StatisticCount::Exact(roots),
                    maximum_active_entries: StatisticCount::Exact(maximum),
                    ..CseStatistics::ZERO
                }),
            });
        }
        let CorePassStatistics::Cse(statistics) = summary.statistics() else {
            panic!("CSE step must retain CSE statistics")
        };
        assert_eq!(statistics.roots_visited, StatisticCount::Exact(8));
        assert_eq!(statistics.maximum_active_entries, StatisticCount::Exact(9));

        summary.functions_seen = StatisticCount::Exact(u64::MAX);
        summary.record_skip(CoreStepSkipReason::NoEnablingChange);
        assert_eq!(summary.functions_seen(), StatisticCount::Saturated);
    }

    #[test]
    fn disabled_remark_sink_never_constructs_explanations_or_a_container() {
        let explanation_calls = Cell::new(0);
        let mut sink = CoreRemarkSink::new(CoreRemarkPolicy::disabled());
        sink.submit_limit_miss(
            FunctionId::from_index(0),
            CorePipelineStep::Sccp,
            CorePassLimitReason::SccpEvents,
            || {
                explanation_calls.set(explanation_calls.get() + 1);
                "unused".to_owned()
            },
        );
        assert_eq!(explanation_calls.get(), 0);
        assert!(sink.finish().is_none());
    }

    #[test]
    fn remark_sink_filters_and_caps_at_submission_time() {
        let explanation_calls = Cell::new(0);
        let mut sink = CoreRemarkSink::new(CoreRemarkPolicy::limit_misses(1));
        for raw_function in 0..3 {
            sink.submit_limit_miss(
                FunctionId::from_index(raw_function),
                CorePipelineStep::Sccp,
                CorePassLimitReason::SccpEvents,
                || {
                    explanation_calls.set(explanation_calls.get() + 1);
                    "retained".to_owned()
                },
            );
        }
        let stream = sink.finish().unwrap();
        assert_eq!(explanation_calls.get(), 1);
        assert_eq!(stream.records.len(), 1);
        assert_eq!(stream.omitted, StatisticCount::Exact(2));
        assert_eq!(
            stream.reason_counts[CorePassLimitReason::SccpEvents.index()],
            StatisticCount::Exact(3)
        );
    }
}
