//! Owned Core optimization boundary.

mod canonicalize;
mod cse;
mod dce;
mod fusion;
mod pipeline;
mod sccp;

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::ir::core::{CoreProgram, DebugDumper, FunctionId, verify_program};
use crate::source::SourceContext;

pub use pipeline::CoreOptimizationRemark;
pub use pipeline::{
    BASELINE_PIPELINE_STEP_COUNT, CanonicalizationStatistics, CorePassLimitReason,
    CorePassStatistics, CorePipelineStep, CorePipelineStepSummary, CoreRemarkPolicy,
    CoreRemarkStream, CoreStepSkipReason, CseStatistics, DceStatistics, FusionStatistics,
    SccpStatistics, StatisticCount,
};

/// A reviewed Core optimization pipeline exposed by the compiler.
///
/// Only complete, reviewed, closed pipelines are exposed as optimization levels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CoreOptimizationLevel {
    /// Verify the input and otherwise preserve it exactly.
    None,
    /// Run the complete bounded baseline Core pipeline.
    Baseline,
}

impl fmt::Display for CoreOptimizationLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Baseline => formatter.write_str("baseline"),
        }
    }
}

/// Bounded configuration for one owned Core optimization run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreOptimizationOptions {
    level: CoreOptimizationLevel,
    failure_snapshot_byte_limit: usize,
    remarks: CoreRemarkPolicy,
}

impl CoreOptimizationOptions {
    /// Default maximum retained bytes in a failure snapshot.
    pub const DEFAULT_FAILURE_SNAPSHOT_BYTE_LIMIT: usize = 64 * 1024;

    /// Hard maximum retained bytes in a failure snapshot.
    ///
    /// Requested limits are capped here so diagnostic configuration cannot retain
    /// an accidentally unbounded malformed-program dump.
    pub const MAX_FAILURE_SNAPSHOT_BYTE_LIMIT: usize = 1024 * 1024;

    /// Creates options for one reviewed optimization level.
    #[must_use]
    pub const fn new(level: CoreOptimizationLevel) -> Self {
        Self {
            level,
            failure_snapshot_byte_limit: Self::DEFAULT_FAILURE_SNAPSHOT_BYTE_LIMIT,
            remarks: CoreRemarkPolicy::disabled(),
        }
    }

    /// Returns the selected closed pipeline level.
    #[must_use]
    pub const fn level(&self) -> CoreOptimizationLevel {
        self.level
    }

    /// Returns the effective retained-byte limit for failure snapshots.
    #[must_use]
    pub const fn failure_snapshot_byte_limit(&self) -> usize {
        self.failure_snapshot_byte_limit
    }

    /// Returns the optional bounded-detail policy.
    #[must_use]
    pub const fn remark_policy(&self) -> CoreRemarkPolicy {
        self.remarks
    }

    /// Sets the retained-byte limit for failure snapshots.
    ///
    /// Values above [`Self::MAX_FAILURE_SNAPSHOT_BYTE_LIMIT`] are capped to that
    /// hard maximum. Zero is valid and retains only omitted-byte metadata.
    #[must_use]
    pub const fn with_failure_snapshot_byte_limit(mut self, requested: usize) -> Self {
        self.failure_snapshot_byte_limit = if requested > Self::MAX_FAILURE_SNAPSHOT_BYTE_LIMIT {
            Self::MAX_FAILURE_SNAPSHOT_BYTE_LIMIT
        } else {
            requested
        };
        self
    }

    /// Selects optional filtered and capped optimization detail.
    ///
    /// The default is [`CoreRemarkPolicy::disabled`]. Remarks never affect which
    /// transformations run, and the ordinary disabled path creates no remark stream.
    #[must_use]
    pub const fn with_remark_policy(mut self, policy: CoreRemarkPolicy) -> Self {
        self.remarks = policy;
        self
    }
}

impl Default for CoreOptimizationOptions {
    fn default() -> Self {
        Self::new(CoreOptimizationLevel::None)
    }
}

/// Deterministic aggregate report for one successful Core optimization run.
///
/// `None` retains no pass summaries. `Baseline` immediately aggregates every
/// invocation into seven fixed step summaries; it never retains a function-by-step
/// record matrix or wall-clock timing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreOptimizationReport {
    level: CoreOptimizationLevel,
    steps: Option<[CorePipelineStepSummary; BASELINE_PIPELINE_STEP_COUNT]>,
    remarks: Option<CoreRemarkStream>,
}

impl CoreOptimizationReport {
    const fn none() -> Self {
        Self {
            level: CoreOptimizationLevel::None,
            steps: None,
            remarks: None,
        }
    }

    fn baseline(output: pipeline::PipelineOutput) -> Self {
        Self {
            level: CoreOptimizationLevel::Baseline,
            steps: Some(output.summaries),
            remarks: output.remarks,
        }
    }

    /// Returns the closed pipeline level that produced this report.
    #[must_use]
    pub const fn level(&self) -> CoreOptimizationLevel {
        self.level
    }

    /// Returns fixed summaries in stable pipeline order.
    ///
    /// The slice is empty for `None` and has exactly
    /// [`BASELINE_PIPELINE_STEP_COUNT`] entries for `Baseline`.
    #[must_use]
    pub fn steps(&self) -> &[CorePipelineStepSummary] {
        self.steps.as_ref().map_or(&[], |steps| steps.as_slice())
    }

    /// Returns explicitly requested bounded detail.
    ///
    /// This is `None` when remarks were disabled or when the selected optimization
    /// level ran no remark-producing pipeline (currently [`CoreOptimizationLevel::None`]).
    #[must_use]
    pub const fn remarks(&self) -> Option<&CoreRemarkStream> {
        self.remarks.as_ref()
    }

    /// Renders a byte-stable report without timing or process-local data.
    #[must_use]
    pub fn dump(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for CoreOptimizationReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "core-optimization level={}", self.level)?;
        if let Some(steps) = &self.steps {
            for summary in steps {
                pipeline::write_step_summary(formatter, summary)?;
            }
        }
        Ok(())
    }
}

/// A successfully verified owned Core program and its deterministic report.
#[derive(Debug)]
pub struct CoreOptimizationOutput {
    program: CoreProgram,
    report: CoreOptimizationReport,
}

impl CoreOptimizationOutput {
    /// Returns the verified optimized program.
    #[must_use]
    pub const fn program(&self) -> &CoreProgram {
        &self.program
    }

    /// Returns the deterministic aggregate optimization report.
    #[must_use]
    pub const fn report(&self) -> &CoreOptimizationReport {
        &self.report
    }

    /// Consumes the output into its owned program and report.
    #[must_use]
    pub fn into_parts(self) -> (CoreProgram, CoreOptimizationReport) {
        (self.program, self.report)
    }
}

/// Pipeline phase that rejected an owned Core program.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum CoreOptimizationPhase {
    /// Verification of the consumed input before any optimization.
    InputVerification,
    /// Execution of one concrete Core pass.
    PassExecution,
    /// Debug/test/CI verification immediately after one executed pass.
    PassVerification,
    /// Whole-program verification at the mutating pipeline boundary.
    OutputVerification,
}

impl fmt::Display for CoreOptimizationPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputVerification => formatter.write_str("input verification"),
            Self::PassExecution => formatter.write_str("pass execution"),
            Self::PassVerification => formatter.write_str("pass verification"),
            Self::OutputVerification => formatter.write_str("output verification"),
        }
    }
}

/// Count of complete dump bytes omitted after a retained UTF-8 prefix.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OmittedByteCount {
    /// The exact number of omitted bytes.
    Exact(u64),
    /// The complete byte count exceeded the reportable `u64` domain.
    Saturated,
}

/// One deterministic bounded dump of a rejected Core program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreOptimizationSnapshot {
    text: String,
    omitted_bytes: OmittedByteCount,
}

impl CoreOptimizationSnapshot {
    fn capture(program: &CoreProgram, byte_limit: usize) -> Self {
        let rendered = DebugDumper::new(program).render_bounded(byte_limit);
        Self::from_render_parts(
            rendered.text,
            rendered.total_bytes,
            rendered.total_bytes_saturated,
        )
    }

    fn from_render_parts(text: String, total_bytes: u64, total_bytes_saturated: bool) -> Self {
        let omitted_bytes = if total_bytes_saturated {
            OmittedByteCount::Saturated
        } else {
            u64::try_from(text.len())
                .ok()
                .and_then(|retained| total_bytes.checked_sub(retained))
                .map_or(OmittedByteCount::Saturated, OmittedByteCount::Exact)
        };
        Self {
            text,
            omitted_bytes,
        }
    }

    /// Returns the retained UTF-8 prefix of the malformed-safe debug dump.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the retained prefix length in UTF-8 bytes.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.text.len()
    }

    /// Returns exact-or-saturated metadata for bytes after the retained prefix.
    #[must_use]
    pub const fn omitted_bytes(&self) -> OmittedByteCount {
        self.omitted_bytes
    }

    /// Returns whether any complete dump bytes were omitted.
    #[must_use]
    pub const fn is_truncated(&self) -> bool {
        !matches!(self.omitted_bytes, OmittedByteCount::Exact(0))
    }

    /// Consumes the snapshot into its retained text and omitted-byte metadata.
    #[must_use]
    pub fn into_parts(self) -> (String, OmittedByteCount) {
        (self.text, self.omitted_bytes)
    }
}

/// Phase-aware optimization failure that never retains the consumed Core program.
#[derive(Clone, Debug)]
pub struct CoreOptimizationFailure {
    phase: CoreOptimizationPhase,
    function: Option<FunctionId>,
    step: Option<CorePipelineStep>,
    diagnostics: Diagnostics,
    snapshot: CoreOptimizationSnapshot,
    pass_error: Option<Arc<pipeline::CorePassError>>,
}

impl PartialEq for CoreOptimizationFailure {
    fn eq(&self, other: &Self) -> bool {
        self.phase == other.phase
            && self.function == other.function
            && self.step == other.step
            && self.diagnostics == other.diagnostics
            && self.snapshot == other.snapshot
    }
}

impl Eq for CoreOptimizationFailure {}

impl CoreOptimizationFailure {
    /// Returns the phase that rejected the consumed program.
    #[must_use]
    pub const fn phase(&self) -> CoreOptimizationPhase {
        self.phase
    }

    /// Returns the function active at failure, when the phase is function-local.
    #[must_use]
    pub const fn function(&self) -> Option<FunctionId> {
        self.function
    }

    /// Returns the stable pipeline invocation active at failure, when applicable.
    #[must_use]
    pub const fn step(&self) -> Option<CorePipelineStep> {
        self.step
    }

    /// Returns deterministic structured diagnostics for the rejection.
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Returns the one bounded malformed-safe input snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &CoreOptimizationSnapshot {
        &self.snapshot
    }

    /// Consumes the failure into its phase, diagnostics, and one snapshot.
    ///
    /// Use [`Self::into_detailed_parts`] when function/pass context is also needed.
    #[must_use]
    pub fn into_parts(self) -> (CoreOptimizationPhase, Diagnostics, CoreOptimizationSnapshot) {
        (self.phase, self.diagnostics, self.snapshot)
    }

    /// Consumes the failure into all typed context, diagnostics, and one snapshot.
    #[must_use]
    pub fn into_detailed_parts(
        self,
    ) -> (
        CoreOptimizationPhase,
        Option<FunctionId>,
        Option<CorePipelineStep>,
        Diagnostics,
        CoreOptimizationSnapshot,
    ) {
        (
            self.phase,
            self.function,
            self.step,
            self.diagnostics,
            self.snapshot,
        )
    }
}

impl fmt::Display for CoreOptimizationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Core optimization failed during {}", self.phase)?;
        if let Some(function) = self.function {
            write!(formatter, " for {function:?}")?;
        }
        if let Some(step) = self.step {
            write!(formatter, " at {step}")?;
        }
        write!(formatter, ": {}", self.diagnostics)
    }
}

impl Error for CoreOptimizationFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.pass_error
            .as_ref()
            .map_or(Some(&self.diagnostics as &(dyn Error + 'static)), |error| {
                Some(error.as_ref() as &(dyn Error + 'static))
            })
    }
}

/// Verifies and runs one closed Core optimization pipeline on an owned program.
///
/// The `None` level verifies exactly once. Because it performs no mutation, that
/// successful input proof is also the output proof. Failure consumes and drops the
/// program after constructing one bounded malformed-safe snapshot; no partial Core
/// program is returned.
///
/// # Errors
///
/// Returns phase/function/step-aware diagnostics and one bounded current-program
/// snapshot. The consumed program is never returned on failure.
pub fn optimize_core(
    program: CoreProgram,
    sources: &SourceContext,
    options: &CoreOptimizationOptions,
) -> Result<CoreOptimizationOutput, CoreOptimizationFailure> {
    let mut configuration = pipeline::PipelineConfiguration::for_build();
    configuration.remarks = options.remark_policy();
    optimize_core_with_configuration(
        program,
        sources,
        options,
        &configuration,
        &mut pipeline::NoInstrumentation,
        CoreOptimizationSnapshot::capture,
    )
}

fn optimize_core_with_configuration<I: pipeline::PipelineInstrumentation>(
    mut program: CoreProgram,
    sources: &SourceContext,
    options: &CoreOptimizationOptions,
    configuration: &pipeline::PipelineConfiguration,
    instrumentation: &mut I,
    capture_snapshot: impl Fn(&CoreProgram, usize) -> CoreOptimizationSnapshot,
) -> Result<CoreOptimizationOutput, CoreOptimizationFailure> {
    if let Err(diagnostics) = verify_program(&program, sources) {
        let snapshot = capture_snapshot(&program, options.failure_snapshot_byte_limit());
        return Err(CoreOptimizationFailure {
            phase: CoreOptimizationPhase::InputVerification,
            function: None,
            step: None,
            diagnostics,
            snapshot,
            pass_error: None,
        });
    }

    match options.level() {
        CoreOptimizationLevel::None => Ok(CoreOptimizationOutput {
            program,
            report: CoreOptimizationReport::none(),
        }),
        CoreOptimizationLevel::Baseline => {
            let pipeline_output = match pipeline::run_baseline_with(
                &mut program,
                sources,
                configuration,
                instrumentation,
            ) {
                Ok(output) => output,
                Err(failure) => {
                    let (phase, function, step, diagnostics, pass_error) =
                        convert_pipeline_failure(&program, failure);
                    let snapshot =
                        capture_snapshot(&program, options.failure_snapshot_byte_limit());
                    return Err(CoreOptimizationFailure {
                        phase,
                        function: Some(function),
                        step: Some(step),
                        diagnostics,
                        snapshot,
                        pass_error,
                    });
                }
            };

            if let Err(diagnostics) = verify_program(&program, sources) {
                let snapshot = capture_snapshot(&program, options.failure_snapshot_byte_limit());
                return Err(CoreOptimizationFailure {
                    phase: CoreOptimizationPhase::OutputVerification,
                    function: None,
                    step: None,
                    diagnostics,
                    snapshot,
                    pass_error: None,
                });
            }

            Ok(CoreOptimizationOutput {
                program,
                report: CoreOptimizationReport::baseline(pipeline_output),
            })
        }
    }
}

fn convert_pipeline_failure(
    program: &CoreProgram,
    failure: pipeline::PipelineFailure,
) -> (
    CoreOptimizationPhase,
    FunctionId,
    CorePipelineStep,
    Diagnostics,
    Option<Arc<pipeline::CorePassError>>,
) {
    match failure {
        pipeline::PipelineFailure::Verification {
            function,
            step,
            diagnostics,
        } => (
            CoreOptimizationPhase::PassVerification,
            function,
            step,
            diagnostics,
            None,
        ),
        pipeline::PipelineFailure::Pass {
            function,
            step,
            error,
        } => {
            let diagnostics = one_pass_failure_diagnostic(
                program,
                function,
                format!("{step} failed for {function:?}: {error}"),
            );
            (
                CoreOptimizationPhase::PassExecution,
                function,
                step,
                diagnostics,
                Some(Arc::new(error)),
            )
        }
        pipeline::PipelineFailure::MissingVerifiedBody { function, step } => {
            let diagnostics = one_pass_failure_diagnostic(
                program,
                function,
                format!("verified function {function:?} lost its body before {step}"),
            );
            (
                CoreOptimizationPhase::PassExecution,
                function,
                step,
                diagnostics,
                None,
            )
        }
        #[cfg(test)]
        pipeline::PipelineFailure::Injected { function, step } => {
            let diagnostics = one_pass_failure_diagnostic(
                program,
                function,
                format!("injected {step} failure for {function:?}"),
            );
            (
                CoreOptimizationPhase::PassExecution,
                function,
                step,
                diagnostics,
                None,
            )
        }
    }
}

fn one_pass_failure_diagnostic(
    program: &CoreProgram,
    function: FunctionId,
    message: String,
) -> Diagnostics {
    let origin = program
        .function(function)
        .map_or(crate::source::OriginId::UNKNOWN, |declaration| {
            declaration.origin()
        });
    Diagnostics::from_findings(vec![Diagnostic::new(
        "core.optimizer-pass-failed",
        message,
        origin,
    )])
    .expect("one optimizer failure diagnostic was constructed")
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{
        CoreOptimizationLevel, CoreOptimizationOptions, CoreOptimizationPhase,
        CoreOptimizationSnapshot, CorePassLimitReason, CorePipelineStep, CoreStepSkipReason,
        OmittedByteCount, StatisticCount, optimize_core, optimize_core_with_configuration,
    };
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockTarget, CanonicalPrinter, CoreOp, CoreProgram, CoreType, ExternalSemanticBinding,
        FunctionBuilder, FunctionId, I32Predicate, MinecraftOperationAttributes,
        MinecraftOperationOrigins, TargetFragment, Terminator, TerminatorKind,
        reset_verifier_counters, verifier_counters,
    };
    use crate::ir::semantic::{EntityKind, MessageLiteral, MinecraftSemanticKey};
    use crate::source::{OriginId, SourceContext};

    fn empty_return_program(sources: &SourceContext) -> (CoreProgram, FunctionId) {
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("empty"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        (program, function)
    }

    #[test]
    fn baseline_preserves_ordered_opaque_external_operations_and_inventories() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let fragment = program
            .declare_target_fragment(
                TargetFragment::unsafe_minecraft_command("say barrier").unwrap(),
            )
            .unwrap();
        let external = program
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let semantic_operation = program
            .declare_minecraft_operation(
                MinecraftSemanticKey::Say,
                EntityKind::ArmorStand,
                MinecraftOperationAttributes::Say {
                    message: MessageLiteral::new("typed barrier").unwrap(),
                    message_origin: OriginId::UNKNOWN,
                },
                MinecraftOperationOrigins::new(
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                ),
            )
            .unwrap();
        let semantic_external = program
            .declare_external_op(
                ExternalSemanticBinding::MinecraftOperation(semantic_operation),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = program
            .declare_function(Some("barriers"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let body = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(body, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(body).unwrap();
        builder
            .external(external, vec![], OriginId::UNKNOWN)
            .unwrap();
        builder
            .external(semantic_external, vec![], OriginId::UNKNOWN)
            .unwrap();
        builder
            .external(semantic_external, vec![], OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let output = optimize_core(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        )
        .unwrap();
        assert_eq!(output.program().target_fragments().len(), 1);
        assert_eq!(output.program().minecraft_operations().len(), 1);
        assert_eq!(output.program().external_ops().len(), 2);
        let body = output.program().function(function).unwrap().body().unwrap();
        assert_eq!(body.block_order().len(), 1);
        let operations = body
            .block_order()
            .iter()
            .flat_map(|block| body.block(*block).unwrap().instructions())
            .map(|instruction| body.instruction(*instruction).unwrap().op().clone())
            .collect::<Vec<_>>();
        assert_eq!(
            operations,
            [
                CoreOp::External(external),
                CoreOp::External(semantic_external),
                CoreOp::External(semantic_external),
            ]
        );
    }

    #[test]
    fn none_verifies_once_and_success_never_captures_a_dump() {
        let snapshot_calls = Cell::new(0);
        let program = CoreProgram::new();
        let options = CoreOptimizationOptions::default();
        reset_verifier_counters();

        let output = optimize_core_with_configuration(
            program,
            &SourceContext::new(),
            &options,
            &super::pipeline::PipelineConfiguration::for_build(),
            &mut super::pipeline::NoInstrumentation,
            |_, _| {
                snapshot_calls.set(snapshot_calls.get() + 1);
                panic!("successful None optimization must not capture a dump")
            },
        )
        .unwrap();

        assert!(output.program().is_empty());
        assert_eq!(verifier_counters(), (0, 1));
        assert_eq!(snapshot_calls.get(), 0);
    }

    #[test]
    fn invalid_input_captures_exactly_one_snapshot() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        program
            .declare_function(Some("undefined"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let snapshot_calls = Cell::new(0);

        let failure = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::default(),
            &super::pipeline::PipelineConfiguration::for_build(),
            &mut super::pipeline::NoInstrumentation,
            |candidate, limit| {
                snapshot_calls.set(snapshot_calls.get() + 1);
                CoreOptimizationSnapshot::capture(candidate, limit)
            },
        )
        .unwrap_err();

        assert!(
            failure
                .diagnostics()
                .contains_code("core.undefined-function")
        );
        assert_eq!(snapshot_calls.get(), 1);
    }

    #[test]
    fn baseline_runs_five_steps_skips_both_conditional_cleanups_and_never_dumps() {
        let sources = SourceContext::new();
        let (program, _) = empty_return_program(&sources);
        let snapshot_calls = Cell::new(0);
        reset_verifier_counters();

        let output = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
            &super::pipeline::PipelineConfiguration::for_build(),
            &mut super::pipeline::NoInstrumentation,
            |_, _| {
                snapshot_calls.set(snapshot_calls.get() + 1);
                panic!("successful Baseline optimization must not capture a dump")
            },
        )
        .unwrap();

        assert_eq!(snapshot_calls.get(), 0);
        assert_eq!(output.report().level(), CoreOptimizationLevel::Baseline);
        assert_eq!(output.report().steps().len(), 7);
        for summary in &output.report().steps()[..5] {
            assert_eq!(summary.functions_seen(), StatisticCount::Exact(1));
            assert_eq!(summary.functions_completed(), StatisticCount::Exact(1));
            assert_eq!(summary.functions_skipped(), StatisticCount::Exact(0));
        }
        for summary in &output.report().steps()[5..] {
            assert_eq!(summary.functions_seen(), StatisticCount::Exact(1));
            assert_eq!(summary.functions_skipped(), StatisticCount::Exact(1));
            assert_eq!(summary.no_enabling_change_count(), StatisticCount::Exact(1));
            assert_eq!(summary.functions_completed(), StatisticCount::Exact(0));
        }
        assert!(output.report().remarks.is_none());
        let dump = output.report().dump();
        assert!(dump.starts_with("core-optimization level=baseline\n"));
        assert!(dump.contains("step canonicalize.initial"));
        assert!(dump.contains("step canonicalize.final"));
        assert!(!dump.contains("time"));
        assert!(!dump.contains("duration"));
        assert_eq!(verifier_counters(), (7, 2));
    }

    #[test]
    fn verification_policy_changes_real_function_scans_not_program_boundaries() {
        let sources = SourceContext::new();
        let (program, _) = empty_return_program(&sources);
        let options = CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline);

        for (policy, expected_function_calls) in [
            (super::pipeline::VerificationPolicy::AfterEachPass, 7),
            (super::pipeline::VerificationPolicy::PipelineBoundaries, 2),
        ] {
            let mut configuration = super::pipeline::PipelineConfiguration::for_build();
            configuration.verification = policy;
            reset_verifier_counters();
            optimize_core_with_configuration(
                program.clone(),
                &sources,
                &options,
                &configuration,
                &mut super::pipeline::NoInstrumentation,
                |_, _| panic!("successful optimization must not capture a dump"),
            )
            .unwrap();
            assert_eq!(verifier_counters(), (expected_function_calls, 2));
        }
    }

    #[test]
    #[ignore = "non-gating comparative Core verification-policy benchmark; run with --release"]
    fn reports_both_verification_policies_on_the_same_scale_corpus() {
        use std::time::Instant;

        const INSTRUCTIONS: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("verification_scale"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = builder.body().block(entry).unwrap().parameters()[0].value();
        let mut current = input;
        for _ in 0..INSTRUCTIONS {
            current = builder
                .i32_add_wrapping(input, current, OriginId::UNKNOWN)
                .unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![current]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let options = CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline);
        for (policy, expected_function_verifications) in [
            (super::pipeline::VerificationPolicy::PipelineBoundaries, 2),
            (super::pipeline::VerificationPolicy::AfterEachPass, 7),
        ] {
            let mut configuration = super::pipeline::PipelineConfiguration::for_build();
            configuration.verification = policy;
            reset_verifier_counters();
            let candidate = program.clone();
            let started = Instant::now();
            let output = optimize_core_with_configuration(
                candidate,
                &sources,
                &options,
                &configuration,
                &mut super::pipeline::NoInstrumentation,
                |_, _| panic!("successful scale optimization must not capture a dump"),
            )
            .unwrap();
            eprintln!(
                "core-verification-policy policy={policy:?} instructions={INSTRUCTIONS} elapsed={:?}",
                started.elapsed()
            );
            assert_eq!(verifier_counters(), (expected_function_verifications, 2));
            assert_eq!(output.report().level(), CoreOptimizationLevel::Baseline);
        }
    }

    #[test]
    fn fusion_enables_exactly_one_final_cleanup_round() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("fuse"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = builder.body().block(entry).unwrap().parameters()[0].value();
        let tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        let tail_value = builder
            .append_block_parameter(tail, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(tail, vec![input])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(tail).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![tail_value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let output = optimize_core(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        )
        .unwrap();

        let steps = output.report().steps();
        assert_eq!(
            steps[CorePipelineStep::Fusion.index()].functions_changed(),
            StatisticCount::Exact(1)
        );
        for step in [
            CorePipelineStep::CanonicalizeFinal,
            CorePipelineStep::DceFinal,
        ] {
            let summary = &steps[step.index()];
            assert_eq!(summary.functions_completed(), StatisticCount::Exact(1));
            assert_eq!(summary.functions_skipped(), StatisticCount::Exact(0));
        }
    }

    #[test]
    fn cse_exposes_useful_final_canonicalization_and_dce_but_no_second_round() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("cleanup"),
                vec![CoreType::I32],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = builder.body().block(entry).unwrap().parameters()[0].value();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let representative = builder
            .i32_add_wrapping(input, one, OriginId::UNKNOWN)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(input, one, OriginId::UNKNOWN)
            .unwrap();
        let equal = builder
            .i32_compare(
                I32Predicate::Eq,
                representative,
                duplicate,
                OriginId::UNKNOWN,
            )
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![equal]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let first = optimize_core(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        )
        .unwrap();
        for step in [
            CorePipelineStep::Cse,
            CorePipelineStep::CanonicalizeFinal,
            CorePipelineStep::DceFinal,
        ] {
            assert_eq!(
                first.report().steps()[step.index()].functions_changed(),
                StatisticCount::Exact(1),
                "{step} should make the one reviewed cleanup round useful"
            );
        }

        let second = optimize_core(
            first.into_parts().0,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        )
        .unwrap();
        for summary in second.report().steps() {
            assert_eq!(
                summary.functions_changed(),
                StatisticCount::Exact(0),
                "a shadow second baseline must be unchanged at {}",
                summary.step()
            );
        }
        assert_eq!(
            second.report().steps()[CorePipelineStep::CanonicalizeFinal.index()]
                .functions_skipped(),
            StatisticCount::Exact(1)
        );
    }

    #[test]
    fn typed_limit_fallback_is_reported_and_remark_retention_is_capped() {
        let sources = SourceContext::new();
        let (program, _) = empty_return_program(&sources);
        let mut configuration = super::pipeline::PipelineConfiguration::for_build();
        configuration.limits =
            super::pipeline::PipelineLimits::for_step_limit_test(CorePipelineStep::Sccp);
        configuration.remarks = super::pipeline::CoreRemarkPolicy::limit_misses(0);

        let output = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
            &configuration,
            &mut super::pipeline::NoInstrumentation,
            |_, _| panic!("limit fallback is successful and must not capture a dump"),
        )
        .unwrap();

        let summary = &output.report().steps()[CorePipelineStep::Sccp.index()];
        assert_eq!(
            summary.functions_stopped_at_limit(),
            StatisticCount::Exact(1)
        );
        assert_eq!(
            summary.limit_count(CorePassLimitReason::SccpEvents),
            StatisticCount::Exact(1)
        );
        let remarks = output.report().remarks.as_ref().unwrap();
        assert!(remarks.records.is_empty());
        assert_eq!(remarks.omitted, StatisticCount::Exact(1));
    }

    #[test]
    fn cse_correspondence_is_submitted_directly_only_when_explicitly_requested() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("cse_detail"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = builder.body().block(entry).unwrap().parameters()[0].value();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let representative = builder
            .i32_add_wrapping(input, one, OriginId::UNKNOWN)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(input, one, OriginId::UNKNOWN)
            .unwrap();
        let second_duplicate = builder
            .i32_add_wrapping(input, one, OriginId::UNKNOWN)
            .unwrap();
        let partial = builder
            .i32_add_wrapping(representative, duplicate, OriginId::UNKNOWN)
            .unwrap();
        let result = builder
            .i32_add_wrapping(partial, second_duplicate, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let mut configuration = super::pipeline::PipelineConfiguration::for_build();
        configuration.remarks =
            super::pipeline::CoreRemarkPolicy::cse_applied(1).with_function_filter(function);
        let output = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
            &configuration,
            &mut super::pipeline::NoInstrumentation,
            |_, _| panic!("successful detailed run must not capture a failure dump"),
        )
        .unwrap();

        let stream = output.report().remarks.as_ref().unwrap();
        assert_eq!(stream.cse_applied, StatisticCount::Exact(2));
        assert_eq!(stream.omitted, StatisticCount::Exact(1));
        assert_eq!(stream.records.len(), 1);
        let super::pipeline::CoreOptimizationRemark::CseApplied {
            function: recorded_function,
            step,
            duplicate_results,
            representative_results,
            duplicate_origin,
            representative_origin,
            explanation,
            ..
        } = &stream.records[0]
        else {
            panic!("expected one truthful CSE correspondence")
        };
        assert_eq!(*recorded_function, function);
        assert_eq!(*step, CorePipelineStep::Cse);
        assert_eq!(duplicate_results.len(), 1);
        assert_eq!(representative_results.len(), 1);
        assert_eq!(*duplicate_origin, OriginId::UNKNOWN);
        assert_eq!(*representative_origin, OriginId::UNKNOWN);
        assert!(explanation.contains("dominating"));
    }

    #[derive(Default)]
    struct StepRecorder {
        before: Vec<CorePipelineStep>,
        after: Vec<CorePipelineStep>,
    }

    impl super::pipeline::PipelineInstrumentation for StepRecorder {
        fn before_step(
            &mut self,
            _program: &CoreProgram,
            _function: FunctionId,
            _body: &crate::ir::core::FunctionBody,
            step: CorePipelineStep,
        ) {
            self.before.push(step);
        }

        fn after_step(
            &mut self,
            _program: &CoreProgram,
            _function: FunctionId,
            _body: &crate::ir::core::FunctionBody,
            step: CorePipelineStep,
            _changed: bool,
        ) {
            self.after.push(step);
        }
    }

    #[derive(Debug)]
    enum TimedStepOutcome {
        Completed { elapsed_nanoseconds: u128 },
        Skipped { reason: CoreStepSkipReason },
    }

    #[derive(Debug)]
    struct TimedStepSample {
        function: FunctionId,
        step: CorePipelineStep,
        outcome: TimedStepOutcome,
    }

    #[derive(Default)]
    struct TimedStepRecorder {
        active: Option<(FunctionId, CorePipelineStep, std::time::Instant)>,
        samples: Vec<TimedStepSample>,
    }

    impl super::pipeline::PipelineInstrumentation for TimedStepRecorder {
        fn before_step(
            &mut self,
            _program: &CoreProgram,
            function: FunctionId,
            _body: &crate::ir::core::FunctionBody,
            step: CorePipelineStep,
        ) {
            assert!(
                self.active
                    .replace((function, step, std::time::Instant::now()))
                    .is_none(),
                "Core pass instrumentation must not overlap"
            );
        }

        fn after_step(
            &mut self,
            _program: &CoreProgram,
            function: FunctionId,
            _body: &crate::ir::core::FunctionBody,
            step: CorePipelineStep,
            _changed: bool,
        ) {
            let (active_function, active_step, started) = self
                .active
                .take()
                .expect("every completed Core pass has a matching start event");
            assert_eq!((active_function, active_step), (function, step));
            self.samples.push(TimedStepSample {
                function,
                step,
                outcome: TimedStepOutcome::Completed {
                    elapsed_nanoseconds: started.elapsed().as_nanos(),
                },
            });
        }

        fn skipped(
            &mut self,
            function: FunctionId,
            step: CorePipelineStep,
            reason: CoreStepSkipReason,
        ) {
            assert!(self.active.is_none());
            self.samples.push(TimedStepSample {
                function,
                step,
                outcome: TimedStepOutcome::Skipped { reason },
            });
        }
    }

    struct CanonicalPrefixRecorder<'a> {
        sources: &'a SourceContext,
        snapshots: Vec<(CorePipelineStep, String)>,
    }

    impl<'a> CanonicalPrefixRecorder<'a> {
        const fn new(sources: &'a SourceContext) -> Self {
            Self {
                sources,
                snapshots: vec![],
            }
        }

        fn render(&self) -> String {
            let mut output = String::new();
            for (step, snapshot) in &self.snapshots {
                output.push_str("=== ");
                output.push_str(&step.to_string());
                output.push_str(" ===\n");
                output.push_str(snapshot);
            }
            output
        }
    }

    impl super::pipeline::PipelineInstrumentation for CanonicalPrefixRecorder<'_> {
        fn after_step(
            &mut self,
            program: &CoreProgram,
            function: FunctionId,
            body: &crate::ir::core::FunctionBody,
            step: CorePipelineStep,
            _changed: bool,
        ) {
            if !matches!(
                step,
                CorePipelineStep::CanonicalizeInitial
                    | CorePipelineStep::Sccp
                    | CorePipelineStep::DceAfterSccp
            ) {
                return;
            }
            let mut snapshot = program.clone();
            snapshot.restore_function_body(function, body.clone());
            let rendered = CanonicalPrinter::new(&snapshot, self.sources)
                .expect("a pass prefix must remain valid")
                .render();
            self.snapshots.push((step, rendered));
        }
    }

    fn baseline_snapshot_program(sources: &SourceContext) -> CoreProgram {
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("baseline_snapshot"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, sources, function).unwrap();
        let entry = builder.entry_block();
        let input = builder.body().block(entry).unwrap().parameters()[0].value();
        let gate = builder.create_block(OriginId::UNKNOWN).unwrap();
        let gate_condition = builder
            .append_block_parameter(gate, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        let forwarded = builder
            .append_block_parameter(gate, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let selected = builder.create_block(OriginId::UNKNOWN).unwrap();
        let rejected = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();

        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let identity = builder
            .i32_add_wrapping(input, zero, OriginId::UNKNOWN)
            .unwrap();
        let truth = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(gate, vec![truth, identity])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(gate).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: gate_condition,
                    then_target: BlockTarget::new(selected, vec![]),
                    else_target: BlockTarget::new(rejected, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(selected).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let _dead = builder
            .i32_add_wrapping(forwarded, one, OriginId::UNKNOWN)
            .unwrap();
        let first = builder
            .i32_add_wrapping(forwarded, forwarded, OriginId::UNKNOWN)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(forwarded, forwarded, OriginId::UNKNOWN)
            .unwrap();
        let combined = builder
            .i32_add_wrapping(first, duplicate, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![combined])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(rejected).unwrap();
        let rejected_value = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![rejected_value])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        program
    }

    #[test]
    #[ignore = "environment-specific per-pass measurements; inspect a --release run"]
    fn reports_every_core_step_as_a_raw_completed_or_skipped_sample() {
        let sources = SourceContext::new();
        let program = baseline_snapshot_program(&sources);
        let options = CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline);
        let expected = optimize_core(program.clone(), &sources, &options).unwrap();
        let expected_program = CanonicalPrinter::new(expected.program(), &sources)
            .unwrap()
            .render();

        for policy in [
            super::pipeline::VerificationPolicy::PipelineBoundaries,
            super::pipeline::VerificationPolicy::AfterEachPass,
        ] {
            let mut configuration = super::pipeline::PipelineConfiguration::for_build();
            configuration.verification = policy;
            let mut recorder = TimedStepRecorder::default();
            let started = std::time::Instant::now();
            let output = optimize_core_with_configuration(
                program.clone(),
                &sources,
                &options,
                &configuration,
                &mut recorder,
                |_, _| panic!("a successful measurement must not capture a failure dump"),
            )
            .unwrap();
            eprintln!(
                "core-pipeline policy={policy:?} elapsed_nanoseconds={}",
                started.elapsed().as_nanos()
            );
            for sample in &recorder.samples {
                match sample.outcome {
                    TimedStepOutcome::Completed {
                        elapsed_nanoseconds,
                    } => eprintln!(
                        "core-step policy={policy:?} function={} step={} status=completed elapsed_nanoseconds={elapsed_nanoseconds}",
                        sample.function.index(),
                        sample.step,
                    ),
                    TimedStepOutcome::Skipped { reason } => eprintln!(
                        "core-step policy={policy:?} function={} step={} status=skipped reason={reason}",
                        sample.function.index(),
                        sample.step,
                    ),
                }
            }

            assert_eq!(recorder.samples.len(), super::BASELINE_PIPELINE_STEP_COUNT);
            assert!(
                recorder
                    .samples
                    .iter()
                    .all(|sample| matches!(sample.outcome, TimedStepOutcome::Completed { .. }))
            );
            assert_eq!(output.report(), expected.report());
            assert_eq!(
                CanonicalPrinter::new(output.program(), &sources)
                    .unwrap()
                    .render(),
                expected_program
            );
            assert!(!output.report().dump().contains("elapsed"));
        }
    }

    #[test]
    fn closed_pipeline_prefixes_and_complete_output_have_exact_canonical_snapshots() {
        let sources = SourceContext::new();
        let program = baseline_snapshot_program(&sources);
        let mut recorder = CanonicalPrefixRecorder::new(&sources);
        let output = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
            &super::pipeline::PipelineConfiguration::for_build(),
            &mut recorder,
            |_, _| panic!("successful optimization must not capture a failure dump"),
        )
        .unwrap();

        assert_eq!(recorder.snapshots.len(), 3);
        for step in [
            CorePipelineStep::CanonicalizeInitial,
            CorePipelineStep::Sccp,
            CorePipelineStep::DceAfterSccp,
            CorePipelineStep::Fusion,
            CorePipelineStep::Cse,
            CorePipelineStep::DceFinal,
        ] {
            assert_eq!(
                output.report().steps()[step.index()].functions_changed(),
                StatisticCount::Exact(1),
                "fixture must exercise {step}"
            );
        }
        assert_eq!(
            recorder.render(),
            include_str!("../../../tests/golden/core-baseline-prefixes.txt")
        );
        assert_eq!(
            CanonicalPrinter::new(output.program(), &sources)
                .unwrap()
                .render(),
            include_str!("../../../tests/golden/core-baseline-full.txt")
        );
    }

    #[test]
    fn pass_failure_is_consumed_bounded_and_stops_later_steps() {
        let sources = SourceContext::new();
        let (program, function) = empty_return_program(&sources);
        let mut configuration = super::pipeline::PipelineConfiguration::for_build();
        configuration.fault = Some(super::pipeline::PipelineTestFault::FailBefore(
            CorePipelineStep::DceAfterSccp,
        ));
        let mut recorder = StepRecorder::default();
        let snapshot_calls = Cell::new(0);

        let failure = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline)
                .with_failure_snapshot_byte_limit(8),
            &configuration,
            &mut recorder,
            |candidate, limit| {
                snapshot_calls.set(snapshot_calls.get() + 1);
                CoreOptimizationSnapshot::capture(candidate, limit)
            },
        )
        .unwrap_err();

        assert_eq!(failure.phase(), CoreOptimizationPhase::PassExecution);
        assert_eq!(failure.function(), Some(function));
        assert_eq!(failure.step(), Some(CorePipelineStep::DceAfterSccp));
        assert!(
            failure
                .diagnostics()
                .contains_code("core.optimizer-pass-failed")
        );
        assert_eq!(failure.snapshot().retained_bytes(), 8);
        assert_eq!(snapshot_calls.get(), 1);
        assert_eq!(
            recorder.before,
            vec![
                CorePipelineStep::CanonicalizeInitial,
                CorePipelineStep::Sccp
            ]
        );
        assert_eq!(recorder.after, recorder.before);
    }

    #[test]
    fn verify_each_reports_the_exact_corrupting_step_with_current_snapshot() {
        let sources = SourceContext::new();
        let (program, function) = empty_return_program(&sources);
        let mut configuration = super::pipeline::PipelineConfiguration::for_build();
        configuration.verification = super::pipeline::VerificationPolicy::AfterEachPass;
        configuration.fault = Some(super::pipeline::PipelineTestFault::CorruptAfter(
            CorePipelineStep::Sccp,
        ));

        let failure = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
            &configuration,
            &mut super::pipeline::NoInstrumentation,
            CoreOptimizationSnapshot::capture,
        )
        .unwrap_err();

        assert_eq!(failure.phase(), CoreOptimizationPhase::PassVerification);
        assert_eq!(failure.function(), Some(function));
        assert_eq!(failure.step(), Some(CorePipelineStep::Sccp));
        assert!(
            failure
                .diagnostics()
                .contains_code("core.missing-terminator")
        );
        assert!(failure.snapshot().text().contains("term=None"));
    }

    #[test]
    fn boundary_policy_still_performs_final_whole_program_verification() {
        let sources = SourceContext::new();
        let (program, _) = empty_return_program(&sources);
        let mut configuration = super::pipeline::PipelineConfiguration::for_build();
        configuration.verification = super::pipeline::VerificationPolicy::PipelineBoundaries;
        configuration.fault = Some(super::pipeline::PipelineTestFault::CorruptAfter(
            CorePipelineStep::Cse,
        ));

        let failure = optimize_core_with_configuration(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
            &configuration,
            &mut super::pipeline::NoInstrumentation,
            CoreOptimizationSnapshot::capture,
        )
        .unwrap_err();

        assert_eq!(failure.phase(), CoreOptimizationPhase::OutputVerification);
        assert_eq!(failure.function(), None);
        assert_eq!(failure.step(), None);
        assert!(
            failure
                .diagnostics()
                .contains_code("core.missing-terminator")
        );
    }

    #[test]
    fn malformed_body_is_rejected_and_safely_snapshotted() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("malformed"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let mut body = program.take_function_body(function).unwrap();
        let entry = body.entry();
        body.block_mut(entry).unwrap().terminator = None;
        program.restore_function_body(function, body);

        let failure = optimize_core(
            program,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::None)
                .with_failure_snapshot_byte_limit(1_024),
        )
        .unwrap_err();

        assert!(
            failure
                .diagnostics()
                .contains_code("core.missing-terminator")
        );
        assert!(failure.snapshot().text().contains("term=None"));
    }

    #[test]
    fn saturated_render_metadata_never_claims_an_exact_omission() {
        let snapshot =
            CoreOptimizationSnapshot::from_render_parts("prefix".to_owned(), u64::MAX, true);
        assert_eq!(snapshot.omitted_bytes(), OmittedByteCount::Saturated);

        let inconsistent =
            CoreOptimizationSnapshot::from_render_parts("longer".to_owned(), 1, false);
        assert_eq!(inconsistent.omitted_bytes(), OmittedByteCount::Saturated);
    }
}
