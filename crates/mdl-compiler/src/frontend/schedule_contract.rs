//! Stage 9B: post-lowering enforcement for `tick`/`schedule`-target functions.
//!
//! Structurally mirrors [`super::target_contract::check_one_tick_contracts`]
//! (root lookup, `ProvenWithin` enforcement) but is a deliberately separate,
//! sibling module — see the 9B dossier's "Reusing, not modifying, 9A's check
//! primitive" section. Two independent obligations are checked for the union
//! of every `tick_handler` and `is_schedule_target` Core function:
//!
//! 1. self-rooting: [`crate::lower::minecraft::LoweredFunction::generated_entry_requirement`]
//!    must reduce to [`AmbientContextRequirements::NONE`];
//! 2. `ProvenWithin`: an independent schedule-target function is checked
//!    against its own root exactly like 9A's `one_tick`; a `tick_handler`
//!    function is checked as part of the `#minecraft:tick` tag's *aggregate*
//!    cost, not independently — see "Aggregating a function tag's per-entry
//!    roots" below for why this needs its own summation rather than reading
//!    one pre-aggregated summary.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::analysis::minecraft::{
    CommandLimitStatus, CountBound, CountUpperKind, ResolvedTargetExecutionRoot,
    RootExecutionSummary, TargetExecutionAnalysisFailure, TargetExecutionCostReport,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::ir::core::{CoreProgram, FunctionId};
use crate::ir::minecraft::FunctionTagResourceId;
use crate::ir::semantic::AmbientContextRequirements;
use crate::lower::minecraft::LoweringOutput;

const SCHEDULE_TARGET_NOT_PROVEN: &str = "target-cost.schedule-target-not-proven";
const TICK_TAG_NOT_PROVEN: &str = "target-cost.tick-tag-not-proven";
const SCHEDULE_ANALYSIS_INCOMPLETE: &str = "target-cost.schedule-analysis-incomplete";
const SCHEDULE_MISSING_ROOT: &str = "target-cost.schedule-missing-root";
const SCHEDULE_ENTRY_NOT_SELF_ROOTED: &str = "lower.schedule-entry-not-self-rooted";

/// Checks every `tick_handler`/`is_schedule_target` Core function against the
/// report already produced by [`LoweringOutput::analyze_target_execution`].
///
/// Returns `None` when neither marker is set anywhere in the program (an
/// analysis failure remains purely advisory in that case) or every marked
/// function is self-rooted and proven within budget. Every failing function
/// is reported, not just the first.
pub(super) fn check_schedule_contracts(
    core: &CoreProgram,
    lowering: &LoweringOutput,
    analysis: Result<&TargetExecutionCostReport, &TargetExecutionAnalysisFailure>,
) -> Option<Diagnostics> {
    let marked = core
        .functions()
        .filter(|(_, function)| function.tick_handler() || function.is_schedule_target())
        .collect::<Vec<_>>();
    if marked.is_empty() {
        return None;
    }

    let mut findings = Vec::new();

    // Self-rooting is a static semantic-model fact independent of the target
    // analysis report, so it is checked unconditionally, even when the
    // report itself failed to complete.
    for (function_id, function) in &marked {
        let name = function.name_hint().unwrap_or("<unnamed>");
        let origin = function.origin();
        let Some(lowered) = lowering.map().function(*function_id) else {
            findings.push(Diagnostic::new(
                SCHEDULE_MISSING_ROOT,
                format!("`{name}` has no generated lowering entry"),
                origin,
            ));
            continue;
        };
        let requirement = lowered.generated_entry_requirement();
        if requirement != AmbientContextRequirements::NONE {
            findings.push(
                Diagnostic::new(
                    SCHEDULE_ENTRY_NOT_SELF_ROOTED,
                    format!(
                        "`{name}` is a tick/schedule-target entry but is not self-rooted: {}",
                        describe_non_self_rooted(requirement)
                    ),
                    origin,
                )
                .with_primary_label("this function's entry context is not self-rooting"),
            );
        }
    }

    let report = match analysis {
        Ok(report) => report,
        Err(failure) => {
            for (_, function) in &marked {
                let name = function.name_hint().unwrap_or("<unnamed>");
                findings.push(
                    Diagnostic::new(
                        SCHEDULE_ANALYSIS_INCOMPLETE,
                        format!(
                            "target-execution analysis did not complete, so the tick/schedule \
                             contract on `{name}` cannot be proven: {failure}"
                        ),
                        function.origin(),
                    )
                    .with_primary_label("this function's tick/schedule contract cannot be checked"),
                );
            }
            return Diagnostics::from_findings(findings);
        }
    };

    check_schedule_targets(core, lowering, report, &mut findings);
    check_tick_tag_aggregate(core, lowering, report, &mut findings);

    Diagnostics::from_findings(findings)
}

fn check_schedule_targets(
    core: &CoreProgram,
    lowering: &LoweringOutput,
    report: &TargetExecutionCostReport,
    findings: &mut Vec<Diagnostic>,
) {
    for (function_id, function) in core
        .functions()
        .filter(|(_, function)| function.is_schedule_target())
    {
        let name = function.name_hint().unwrap_or("<unnamed>");
        let origin = function.origin();
        let Some(lowered) = lowering.map().function(function_id) else {
            // Already reported by the self-rooting pass above.
            continue;
        };
        let Some((mc_function, _)) = lowering
            .program()
            .functions()
            .find(|(_, data)| data.resource() == lowered.entry_resource())
        else {
            findings.push(Diagnostic::new(
                SCHEDULE_MISSING_ROOT,
                format!("schedule-target function `{name}` has no generated target function"),
                origin,
            ));
            continue;
        };
        let Some(summary) = report.roots().iter().find(|root| {
            matches!(root.root(), ResolvedTargetExecutionRoot::Function(id) if *id == mc_function)
        }) else {
            findings.push(Diagnostic::new(
                SCHEDULE_MISSING_ROOT,
                format!("schedule-target function `{name}` is not an analyzed target-execution root"),
                origin,
            ));
            continue;
        };
        report_if_not_proven(
            findings,
            SCHEDULE_TARGET_NOT_PROVEN,
            &format!("schedule-target function `{name}`"),
            origin,
            summary.sequence_limit_status(),
            summary.fork_limit_status(),
            report,
        );
    }
}

/// Checks the `#minecraft:tick` tag's *aggregate* cost — the sum of every
/// handler's own sequence/fork cost, not each handler independently.
///
/// # Aggregating a function tag's per-entry roots
///
/// The 9B dossier's design assumed `TargetExecutionRoot::FunctionTag`
/// resolves to one combined `RootExecutionSummary` for the whole tag. Re-
/// verified against current source (`analysis/minecraft/analyze.rs`'s
/// `resolve_roots`): a function-tag root instead resolves to **one
/// `RootExecutionSummary` per tag entry**
/// (`ResolvedTargetExecutionRoot::FunctionTagFunction { tag, entry_index,
/// function }`), each already compared against the *per-entry* configured
/// limit independently — there is no pre-aggregated summary anywhere in the
/// analysis module to read. This function performs the summation itself:
/// summing every handler's lower bound and (when every handler's upper bound
/// is finite) upper bound, then comparing the combined range against the
/// same configured limits `RootExecutionSummary` itself compares against,
/// mirroring `CommandLimitStatus`'s own (private) `compare_first_rejected`
/// classification. A non-finite handler (`Unknown`/`NoFiniteBoundProven`)
/// makes the aggregate non-finite too, at that reason.
fn check_tick_tag_aggregate(
    core: &CoreProgram,
    lowering: &LoweringOutput,
    report: &TargetExecutionCostReport,
    findings: &mut Vec<Diagnostic>,
) {
    let tick_handlers = core
        .functions()
        .filter(|(_, function)| function.tick_handler())
        .collect::<Vec<_>>();
    if tick_handlers.is_empty() {
        return;
    }
    let Ok(tick_resource) = FunctionTagResourceId::parse("minecraft:tick") else {
        findings.push(Diagnostic::new(
            SCHEDULE_MISSING_ROOT,
            "the compiler's static vanilla tick-tag resource is invalid",
            crate::source::OriginId::UNKNOWN,
        ));
        return;
    };
    let Some((tick_tag, _)) = lowering
        .program()
        .function_tags()
        .find(|(_, data)| data.resource() == &tick_resource)
    else {
        findings.push(Diagnostic::new(
            SCHEDULE_MISSING_ROOT,
            "no generated `#minecraft:tick` tag despite at least one `tick`-marked function",
            crate::source::OriginId::UNKNOWN,
        ));
        return;
    };

    let mut resolved: BTreeSet<FunctionId> = BTreeSet::new();
    let mut summaries: Vec<&RootExecutionSummary> = Vec::new();
    let mut missing = Vec::new();
    for (function_id, function) in &tick_handlers {
        let name = function.name_hint().unwrap_or("<unnamed>");
        let Some(lowered) = lowering.map().function(*function_id) else {
            missing.push(name);
            continue;
        };
        let Some((mc_function, _)) = lowering
            .program()
            .functions()
            .find(|(_, data)| data.resource() == lowered.entry_resource())
        else {
            missing.push(name);
            continue;
        };
        let Some(summary) = report.roots().iter().find(|root| {
            matches!(
                root.root(),
                ResolvedTargetExecutionRoot::FunctionTagFunction { tag, function, .. }
                    if *tag == tick_tag && *function == mc_function
            )
        }) else {
            missing.push(name);
            continue;
        };
        resolved.insert(*function_id);
        summaries.push(summary);
    }
    for name in missing {
        findings.push(Diagnostic::new(
            SCHEDULE_MISSING_ROOT,
            format!("tick handler `{name}` is not an analyzed `#minecraft:tick` tag entry"),
            crate::source::OriginId::UNKNOWN,
        ));
    }
    if summaries.is_empty() {
        return;
    }

    let assumptions = report.assumptions();
    let sequence = aggregate_status(
        summaries.iter().map(|summary| summary.sequence_operations()),
        u64::from(assumptions.max_command_sequence_length().max(1)) + 1,
    );
    let forks = aggregate_status(
        summaries
            .iter()
            .map(|summary| summary.maximum_chain_expansion()),
        u64::from(assumptions.max_command_forks().max(1)) + 1,
    );
    report_if_not_proven(
        findings,
        TICK_TAG_NOT_PROVEN,
        "the `#minecraft:tick` tag's aggregate cost",
        crate::source::OriginId::UNKNOWN,
        sequence,
        forks,
        report,
    );
}

/// Sums a set of per-entry `CountBound`s and classifies the result against
/// `first_rejected`, mirroring `CommandLimitStatus`'s own (private)
/// classification rule.
fn aggregate_status(bounds: impl Iterator<Item = CountBound>, first_rejected: u64) -> CommandLimitStatus {
    let mut lower_sum: u64 = 0;
    let mut finite_sum: Option<u64> = Some(0);
    let mut worst_reason: Option<CommandLimitStatus> = None;
    for bound in bounds {
        lower_sum = lower_sum.saturating_add(bound.lower());
        match bound.upper().kind() {
            CountUpperKind::Finite(value) => {
                finite_sum = finite_sum.map(|sum| sum.saturating_add(value));
            }
            CountUpperKind::AboveAnalysisCap => {
                finite_sum = None;
                worst_reason = worst_reason.or(Some(CommandLimitStatus::MayExceed));
            }
            CountUpperKind::NoFiniteBoundProven(reason) => {
                finite_sum = None;
                worst_reason = Some(rank_reason(
                    worst_reason,
                    CommandLimitStatus::NoFiniteBoundProven(reason),
                ));
            }
            CountUpperKind::Unknown(reason) => {
                finite_sum = None;
                worst_reason = Some(rank_reason(
                    worst_reason,
                    CommandLimitStatus::Unknown(reason),
                ));
            }
        }
    }
    if lower_sum >= first_rejected {
        return CommandLimitStatus::ProvenExceeds;
    }
    match (finite_sum, worst_reason) {
        (Some(sum), _) if sum < first_rejected => CommandLimitStatus::ProvenWithin,
        (Some(_), _) => CommandLimitStatus::MayExceed,
        (None, Some(status)) => status,
        (None, None) => CommandLimitStatus::MayExceed,
    }
}

/// Ranks non-finite aggregate reasons `Unknown` > `NoFiniteBoundProven` >
/// `MayExceed` (matching `CountUpperKind`'s own declared precedence, applied
/// pointwise across the entries being summed), so summation order never
/// changes the reported reason.
const fn rank_reason(current: Option<CommandLimitStatus>, candidate: CommandLimitStatus) -> CommandLimitStatus {
    match (current, candidate) {
        (Some(CommandLimitStatus::Unknown(reason)), _) => CommandLimitStatus::Unknown(reason),
        (_, CommandLimitStatus::Unknown(reason)) => CommandLimitStatus::Unknown(reason),
        (Some(CommandLimitStatus::NoFiniteBoundProven(reason)), _) => {
            CommandLimitStatus::NoFiniteBoundProven(reason)
        }
        (_, CommandLimitStatus::NoFiniteBoundProven(reason)) => {
            CommandLimitStatus::NoFiniteBoundProven(reason)
        }
        (_, candidate) => candidate,
    }
}

#[allow(clippy::too_many_arguments)]
fn report_if_not_proven(
    findings: &mut Vec<Diagnostic>,
    code: &'static str,
    subject: &str,
    origin: crate::source::OriginId,
    sequence: CommandLimitStatus,
    forks: CommandLimitStatus,
    report: &TargetExecutionCostReport,
) {
    if matches!(sequence, CommandLimitStatus::ProvenWithin)
        && matches!(forks, CommandLimitStatus::ProvenWithin)
    {
        return;
    }
    let assumptions = report.assumptions();
    let mut message = format!(
        "{subject} is not proven to complete within one tick under the configured limits \
         (sequence <= {}, forks <= {})",
        assumptions.max_command_sequence_length(),
        assumptions.max_command_forks(),
    );
    if !matches!(sequence, CommandLimitStatus::ProvenWithin) {
        write!(message, "; sequence status: {sequence:?}").expect("writing to a String cannot fail");
    }
    if !matches!(forks, CommandLimitStatus::ProvenWithin) {
        write!(message, "; fork status: {forks:?}").expect("writing to a String cannot fail");
    }
    findings.push(
        Diagnostic::new(code, message, origin)
            .with_primary_label("this tick/schedule contract is not proven"),
    );
}

fn describe_non_self_rooted(requirement: AmbientContextRequirements) -> String {
    let mut parts = Vec::new();
    if !matches!(
        requirement.executor(),
        crate::ir::semantic::ContextRequirement::None
    ) {
        parts.push("executor");
    }
    if !matches!(
        requirement.position(),
        crate::ir::semantic::ContextRequirement::None
    ) {
        parts.push("position");
    }
    if !matches!(
        requirement.rotation(),
        crate::ir::semantic::ContextRequirement::None
    ) {
        parts.push("rotation");
    }
    if !matches!(
        requirement.dimension(),
        crate::ir::semantic::ContextRequirement::None
    ) {
        parts.push("dimension");
    }
    if !matches!(
        requirement.anchor(),
        crate::ir::semantic::ContextRequirement::None
    ) {
        parts.push("anchor");
    }
    if parts.is_empty() {
        "an unspecified ambient context component is required".to_owned()
    } else {
        format!("requires ambient {}", parts.join(", "))
    }
}
