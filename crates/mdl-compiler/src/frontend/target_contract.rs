//! Stage 9A: post-lowering enforcement of the `one_tick` source contract.
//!
//! Promotes the existing target-execution cost analysis (already computed for
//! every reporting purpose) into a hard compile error for any function marked
//! `one_tick` whose command sequence or fork expansion is not
//! [`CommandLimitStatus::ProvenWithin`] under the configured limits. A program
//! with no `one_tick`-marked function is completely unaffected: this pass reads
//! only [`crate::ir::core::Function::one_tick_contract`] and produces no
//! diagnostics at all when no function sets it.

use std::fmt::Write as _;

use crate::analysis::minecraft::{
    CommandLimitStatus, ResolvedTargetExecutionRoot, TargetExecutionAnalysisFailure,
    TargetExecutionCostReport,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::ir::core::CoreProgram;
use crate::lower::minecraft::LoweringOutput;

const ONE_TICK_NOT_PROVEN: &str = "target-cost.one-tick-not-proven";
const ONE_TICK_ANALYSIS_INCOMPLETE: &str = "target-cost.one-tick-analysis-incomplete";
const ONE_TICK_MISSING_ROOT: &str = "target-cost.one-tick-missing-root";

/// Checks every `one_tick`-marked Core function against the report already
/// produced by [`LoweringOutput::analyze_target_execution`].
///
/// Returns `None` when no function is marked `one_tick` (an analysis failure
/// remains purely advisory in that case, exactly as it is for an unmarked
/// program today) or when every marked function is proven within its
/// configured sequence and fork budget. Every marked function that fails is
/// reported, not just the first.
pub(super) fn check_one_tick_contracts(
    core: &CoreProgram,
    lowering: &LoweringOutput,
    analysis: Result<&TargetExecutionCostReport, &TargetExecutionAnalysisFailure>,
) -> Option<Diagnostics> {
    let marked = core
        .functions()
        .filter(|(_, function)| function.one_tick_contract())
        .collect::<Vec<_>>();
    if marked.is_empty() {
        return None;
    }
    let report = match analysis {
        Ok(report) => report,
        Err(failure) => {
            let findings = marked
                .into_iter()
                .map(|(_, function)| {
                    Diagnostic::new(
                        ONE_TICK_ANALYSIS_INCOMPLETE,
                        format!(
                            "target-execution analysis did not complete, so the `one_tick` \
                             contract on `{}` cannot be proven: {failure}",
                            function.name_hint().unwrap_or("<unnamed>"),
                        ),
                        function.origin(),
                    )
                    .with_primary_label("this function's `one_tick` contract cannot be checked")
                })
                .collect();
            return Diagnostics::from_findings(findings);
        }
    };
    let mut findings = Vec::with_capacity(marked.len());
    for (function_id, function) in marked {
        let name = function.name_hint().unwrap_or("<unnamed>");
        let origin = function.origin();
        let Some(lowered) = lowering.map().function(function_id) else {
            findings.push(Diagnostic::new(
                ONE_TICK_MISSING_ROOT,
                format!("`one_tick` function `{name}` has no generated lowering entry"),
                origin,
            ));
            continue;
        };
        let Some((mc_function, _)) = lowering
            .program()
            .functions()
            .find(|(_, data)| data.resource() == lowered.entry_resource())
        else {
            findings.push(Diagnostic::new(
                ONE_TICK_MISSING_ROOT,
                format!("`one_tick` function `{name}` has no generated target function"),
                origin,
            ));
            continue;
        };
        let Some(summary) = report.roots().iter().find(|root| {
            matches!(root.root(), ResolvedTargetExecutionRoot::Function(id) if *id == mc_function)
        }) else {
            findings.push(Diagnostic::new(
                ONE_TICK_MISSING_ROOT,
                format!("`one_tick` function `{name}` is not an analyzed target-execution root"),
                origin,
            ));
            continue;
        };
        let sequence = summary.sequence_limit_status();
        let forks = summary.fork_limit_status();
        if matches!(sequence, CommandLimitStatus::ProvenWithin)
            && matches!(forks, CommandLimitStatus::ProvenWithin)
        {
            continue;
        }
        let assumptions = report.assumptions();
        let mut message = format!(
            "`one_tick` function `{name}` is not proven to complete within one tick under the \
             configured limits (sequence <= {}, forks <= {})",
            assumptions.max_command_sequence_length(),
            assumptions.max_command_forks(),
        );
        if !matches!(sequence, CommandLimitStatus::ProvenWithin) {
            write!(message, "; sequence status: {sequence:?}")
                .expect("writing to a String cannot fail");
        }
        if !matches!(forks, CommandLimitStatus::ProvenWithin) {
            write!(message, "; fork status: {forks:?}").expect("writing to a String cannot fail");
        }
        findings.push(
            Diagnostic::new(ONE_TICK_NOT_PROVEN, message, origin)
                .with_primary_label("this function's `one_tick` contract is not proven"),
        );
    }
    Diagnostics::from_findings(findings)
}
