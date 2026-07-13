use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::analysis::minecraft::{
    CommandLimitAssumptions, TargetExecutionAnalysisFailure, TargetExecutionAnalysisLimits,
    TargetExecutionAnalysisPhase, TargetExecutionCostReport, TargetExecutionRoot,
    analyze_verified_target_execution,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::EntityId;
use crate::ir::core::{CoreProgram, CoreType, FunctionId};
use crate::ir::minecraft::{
    FakeScoreHolder, FunctionResourceId, FunctionTagResourceId, MinecraftProgram, ObjectiveName,
    verify_program,
};
use crate::source::{OriginId, SourceContext};
use crate::target::JavaEditionTarget;

use super::analysis::{AnalysisError, SemanticInventory};
use super::assignment::{AssignmentError, HomeAssignment};
use super::audit::audit_legality;
use super::demand::{DemandError, RuntimeDemand, RuntimeDemandLimits};
use super::edge_transfer::{EdgeTransferError, EdgeTransferPlan};
use super::emit::construct_program;
use super::liveness::{LivenessError, LivenessLimits, LivenessResult};
use super::placement::{ControlRecipePlan, ControlRecipePlanError};
use super::plan::{
    LoweringDecisionReport, LoweringPlan, PlanBuildError, PlanFinishError, PlanTable,
    verify_constructed_control_recipes,
};
use super::resources::{ResourceInventory, ResourceInventoryError};
use super::{LoweringOptions, LoweringOptionsError, MinecraftOptimizationLevel};

/// Required activation discipline for the Stage 4 fixed-slot ABI.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ActivationContract {
    /// Invocations must not overlap, re-enter, recurse, or execute in forked contexts.
    SingleContextNonReentrant,
}

/// Runtime command-budget responsibility retained by the caller.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct CommandLimitContract {
    assumptions: CommandLimitAssumptions,
    target_defaults: CommandLimitAssumptions,
}

impl CommandLimitContract {
    /// Returns the configured server values required by retained safety proofs.
    ///
    /// Lower actual gamerule values invalidate `ProvenWithin`; this contract never
    /// configures the server itself.
    #[must_use]
    pub const fn assumptions(self) -> CommandLimitAssumptions {
        self.assumptions
    }

    /// Returns the selected target's immutable default server values.
    #[must_use]
    pub const fn target_defaults(self) -> CommandLimitAssumptions {
        self.target_defaults
    }
}

impl fmt::Debug for CommandLimitContract {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.assumptions == self.target_defaults {
            formatter
                .debug_struct("CallerBoundedToTarget")
                .field(
                    "max_command_sequence_length",
                    &self.assumptions.max_command_sequence_length(),
                )
                .field("max_command_forks", &self.assumptions.max_command_forks())
                .finish()
        } else {
            formatter
                .debug_struct("CallerBoundedToAssumptions")
                .field("assumptions", &self.assumptions)
                .field("target_defaults", &self.target_defaults)
                .finish()
        }
    }
}

/// Honest execution restrictions for one generated pack.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionContract {
    activation: ActivationContract,
    command_limits: CommandLimitContract,
}

impl ExecutionContract {
    pub(crate) fn new(target: JavaEditionTarget, assumptions: CommandLimitAssumptions) -> Self {
        let target_defaults = CommandLimitAssumptions::for_target(target);
        let command_limits = CommandLimitContract {
            assumptions,
            target_defaults,
        };
        Self {
            activation: ActivationContract::SingleContextNonReentrant,
            command_limits,
        }
    }

    /// Returns the required activation discipline.
    #[must_use]
    pub const fn activation(&self) -> ActivationContract {
        self.activation
    }

    /// Returns the caller-owned target command-limit deployment contract.
    #[must_use]
    pub const fn command_limits(&self) -> CommandLimitContract {
        self.command_limits
    }
}

/// One self-contained compiler-owned scoreboard ABI location.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RegisterSlot {
    holder: FakeScoreHolder,
    objective: ObjectiveName,
}

impl RegisterSlot {
    pub(crate) const fn new(holder: FakeScoreHolder, objective: ObjectiveName) -> Self {
        Self { holder, objective }
    }

    /// Returns the stable fake-player holder.
    #[must_use]
    pub const fn holder(&self) -> &FakeScoreHolder {
        &self.holder
    }

    /// Returns the objective exclusively owned by this lowering output.
    #[must_use]
    pub const fn objective(&self) -> &ObjectiveName {
        &self.objective
    }
}

/// Read-only target ABI for one Core function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweredFunction {
    entry_resource: FunctionResourceId,
    parameter_homes: Box<[(CoreType, RegisterSlot)]>,
    result_homes: Box<[(CoreType, RegisterSlot)]>,
}

impl LoweredFunction {
    pub(crate) fn new(
        entry_resource: FunctionResourceId,
        parameter_homes: Vec<(CoreType, RegisterSlot)>,
        result_homes: Vec<(CoreType, RegisterSlot)>,
    ) -> Self {
        Self {
            entry_resource,
            parameter_homes: parameter_homes.into_boxed_slice(),
            result_homes: result_homes.into_boxed_slice(),
        }
    }

    /// Returns the generated entry function resource.
    #[must_use]
    pub const fn entry_resource(&self) -> &FunctionResourceId {
        &self.entry_resource
    }

    /// Returns ordered `(Core type, target slot)` parameters.
    ///
    /// Before invoking [`Self::entry_resource`], the caller must write exactly `0`
    /// or `1` to every slot whose type is [`CoreType::Bool`]. Other scoreboard
    /// integers do not represent a Core Boolean and are outside the generated
    /// function's execution contract.
    #[must_use]
    pub fn parameter_homes(&self) -> &[(CoreType, RegisterSlot)] {
        &self.parameter_homes
    }

    /// Returns ordered `(Core type, target slot)` results.
    ///
    /// After a conforming invocation, every slot whose type is [`CoreType::Bool`]
    /// contains exactly `0` or `1`.
    #[must_use]
    pub fn result_homes(&self) -> &[(CoreType, RegisterSlot)] {
        &self.result_homes
    }
}

/// Small read-only correlation map retained after lowering decisions are dropped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweringMap {
    execution: ExecutionContract,
    functions: Box<[LoweredFunction]>,
}

impl LoweringMap {
    pub(crate) fn new(execution: ExecutionContract, functions: Vec<LoweredFunction>) -> Self {
        Self {
            execution,
            functions: functions.into_boxed_slice(),
        }
    }

    /// Returns the execution restrictions of this generated pack.
    #[must_use]
    pub const fn execution_contract(&self) -> &ExecutionContract {
        &self.execution
    }

    /// Looks up one Core function's generated ABI by stable identity.
    #[must_use]
    pub fn function(&self, function: FunctionId) -> Option<&LoweredFunction> {
        usize::try_from(function.index())
            .ok()
            .and_then(|index| self.functions.get(index))
    }

    /// Returns the number of mapped Core functions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Returns whether no Core functions are mapped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}

/// Complete verified result of Core-to-Minecraft lowering.
#[derive(Debug)]
pub struct LoweringOutput {
    program: MinecraftProgram,
    map: LoweringMap,
    report: LoweringDecisionReport,
}

impl LoweringOutput {
    /// Returns the complete Stage 3 target program.
    #[must_use]
    pub const fn program(&self) -> &MinecraftProgram {
        &self.program
    }

    /// Returns the public execution and ABI map.
    #[must_use]
    pub const fn map(&self) -> &LoweringMap {
        &self.map
    }

    /// Returns the frozen physical mapping and control-selection report.
    #[must_use]
    pub const fn report(&self) -> &LoweringDecisionReport {
        &self.report
    }

    /// Dumps deterministic lowering decisions without exposing mutable plan state.
    #[must_use]
    pub fn dump_lowering(&self) -> String {
        self.report.dump()
    }

    /// Consumes the output into its independently owned target, ABI map, and report.
    #[must_use]
    pub fn into_parts(self) -> (MinecraftProgram, LoweringMap, LoweringDecisionReport) {
        (self.program, self.map, self.report)
    }

    /// Analyzes every public Core entry and the generated load tag without
    /// re-verifying, mutating, consuming, or caching the verified target.
    ///
    /// # Errors
    ///
    /// Returns a target-analysis construction failure if the retained lowering map
    /// and verified target disagree, or if checked analysis construction fails.
    pub fn analyze_target_execution(
        &self,
        limits: TargetExecutionAnalysisLimits,
    ) -> Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure> {
        let root_capacity = self.map.functions.len().checked_add(1).ok_or_else(|| {
            TargetExecutionAnalysisFailure::internal(
                TargetExecutionAnalysisPhase::Construction,
                "target-cost.root-capacity-overflow",
                "target-cost root inventory exceeded the host index domain",
            )
        })?;
        let mut roots = Vec::with_capacity(root_capacity);
        let function_ids = self
            .program
            .functions()
            .map(|(function, data)| (data.resource(), function))
            .collect::<HashMap<_, _>>();
        for lowered in &self.map.functions {
            let function = function_ids
                .get(lowered.entry_resource())
                .copied()
                .ok_or_else(|| {
                    TargetExecutionAnalysisFailure::internal(
                        TargetExecutionAnalysisPhase::Invariant,
                        "target-cost.lowering-map-root",
                        "verified lowering map references an absent target function",
                    )
                })?;
            roots.push(TargetExecutionRoot::Function(function));
        }
        let load_resource = FunctionTagResourceId::parse("minecraft:load").map_err(|_| {
            TargetExecutionAnalysisFailure::internal(
                TargetExecutionAnalysisPhase::Invariant,
                "target-cost.static-load-resource",
                "the compiler's static vanilla load-tag resource is invalid",
            )
        })?;
        let load_tag = self
            .program
            .function_tags()
            .find_map(|(tag, data)| (data.resource() == &load_resource).then_some(tag))
            .ok_or_else(|| {
                TargetExecutionAnalysisFailure::internal(
                    TargetExecutionAnalysisPhase::Invariant,
                    "target-cost.lowering-load-root",
                    "verified lowering output is missing its generated load tag",
                )
            })?;
        roots.push(TargetExecutionRoot::FunctionTag(load_tag));
        let assumptions = self.map.execution.command_limits().assumptions();
        analyze_verified_target_execution(&self.program, &roots, assumptions, limits)
    }
}

/// Phase that rejected a Core-to-Minecraft lowering request.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LoweringPhase {
    /// Input Core verification.
    CoreVerification,
    /// Target-option validation.
    Options,
    /// Reachable-vocabulary and call-graph legality.
    Legality,
    /// Immutable physical planning and plan verification.
    Planning,
    /// Stage 3 declaration and body construction.
    Construction,
    /// Final Stage 3 structural verification.
    TargetVerification,
}

/// Phase-aware lowering failure with no runnable partial program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweringFailure {
    phase: LoweringPhase,
    diagnostics: Diagnostics,
    report: Option<Box<LoweringDecisionReport>>,
}

impl LoweringFailure {
    fn before_plan(phase: LoweringPhase, diagnostics: Diagnostics) -> Self {
        Self {
            phase,
            diagnostics,
            report: None,
        }
    }

    fn after_plan(
        phase: LoweringPhase,
        diagnostics: Diagnostics,
        report: &LoweringDecisionReport,
    ) -> Self {
        Self {
            phase,
            diagnostics,
            report: Some(Box::new(report.clone())),
        }
    }

    /// Returns the failed pipeline phase.
    #[must_use]
    pub const fn phase(&self) -> LoweringPhase {
        self.phase
    }

    /// Returns deterministic structured diagnostics.
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Returns frozen non-runnable decisions when planning completed before failure.
    #[must_use]
    pub fn report(&self) -> Option<&LoweringDecisionReport> {
        self.report.as_deref()
    }

    /// Dumps frozen non-runnable lowering decisions when planning had completed.
    #[must_use]
    pub fn dump_lowering(&self) -> Option<String> {
        self.report.as_deref().map(LoweringDecisionReport::dump)
    }
}

impl fmt::Display for LoweringFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Minecraft lowering failed during {:?}: {}",
            self.phase, self.diagnostics
        )
    }
}

impl Error for LoweringFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.diagnostics)
    }
}

/// Closed observation points for developer-only lowering measurements.
///
/// These are deliberately private: the compilation contract is expressed by
/// [`LoweringPhase`], while this finer inventory may evolve with the implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoweringMeasurementPhase {
    CoreVerification,
    Options,
    SemanticInventory,
    Legality,
    RuntimeDemand,
    Liveness,
    Assignment,
    EdgeTransfers,
    ControlRecipes,
    Resources,
    PlanFreeze,
    Construction,
    Reconciliation,
    TargetVerification,
}

/// Private observation boundary. Production instantiates the zero-sized no-op sink.
trait LoweringInstrumentation {
    fn before_phase(&mut self, _phase: LoweringMeasurementPhase) {}

    fn after_phase(&mut self, _phase: LoweringMeasurementPhase, _succeeded: bool) {}

    fn skipped(&mut self, _phase: LoweringMeasurementPhase, _reason: &'static str) {}
}

struct NoLoweringInstrumentation;

impl LoweringInstrumentation for NoLoweringInstrumentation {}

fn instrument_lowering_phase<I, T, E>(
    instrumentation: &mut I,
    phase: LoweringMeasurementPhase,
    operation: impl FnOnce() -> Result<T, E>,
) -> Result<T, E>
where
    I: LoweringInstrumentation,
{
    instrumentation.before_phase(phase);
    let result = operation();
    instrumentation.after_phase(phase, result.is_ok());
    result
}

/// Verifies, plans, constructs, and verifies one complete Minecraft target program.
///
/// # Errors
///
/// Returns a phase-aware failure and never exposes a partial runnable target.
pub fn lower_to_minecraft(
    core: &CoreProgram,
    sources: &SourceContext,
    options: &LoweringOptions,
) -> Result<LoweringOutput, LoweringFailure> {
    lower_to_minecraft_with_instrumentation(core, sources, options, &mut NoLoweringInstrumentation)
}

fn lower_to_minecraft_with_instrumentation<I>(
    core: &CoreProgram,
    sources: &SourceContext,
    options: &LoweringOptions,
    instrumentation: &mut I,
) -> Result<LoweringOutput, LoweringFailure>
where
    I: LoweringInstrumentation,
{
    let (inventory, demand, liveness) =
        analyze_lowering_request(core, sources, options, instrumentation)?;
    let (assignment, transfers) = assign_lowering_homes(
        core,
        &inventory,
        &demand,
        liveness.as_ref(),
        options,
        instrumentation,
    )?;
    let (plan, report) = freeze_lowering_plan(
        core,
        &inventory,
        assignment,
        transfers,
        options,
        instrumentation,
    )?;
    let program = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::Construction,
        || {
            construct_program(core, &plan).map_err(|diagnostics| {
                LoweringFailure::after_plan(LoweringPhase::Construction, diagnostics, &report)
            })
        },
    )?;
    instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::Reconciliation,
        || {
            verify_constructed_control_recipes(core, &plan, &program).map_err(|diagnostics| {
                LoweringFailure::after_plan(LoweringPhase::Construction, diagnostics, &report)
            })
        },
    )?;
    instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::TargetVerification,
        || {
            verify_program(&program, sources).map_err(|diagnostics| {
                LoweringFailure::after_plan(LoweringPhase::TargetVerification, diagnostics, &report)
            })
        },
    )?;
    let map = report.map();
    Ok(LoweringOutput {
        program,
        map,
        report,
    })
}

fn analyze_lowering_request<I>(
    core: &CoreProgram,
    sources: &SourceContext,
    options: &LoweringOptions,
    instrumentation: &mut I,
) -> Result<(SemanticInventory, RuntimeDemand, Option<LivenessResult>), LoweringFailure>
where
    I: LoweringInstrumentation,
{
    instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::CoreVerification,
        || {
            crate::ir::core::verify_program(core, sources).map_err(|diagnostics| {
                LoweringFailure::before_plan(LoweringPhase::CoreVerification, diagnostics)
            })
        },
    )?;
    instrument_lowering_phase(instrumentation, LoweringMeasurementPhase::Options, || {
        options.validate().map_err(|error| {
            LoweringFailure::before_plan(LoweringPhase::Options, options_diagnostics(error))
        })
    })?;
    let inventory = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::SemanticInventory,
        || {
            SemanticInventory::new(core).map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Legality, analysis_diagnostics(error))
            })
        },
    )?;
    instrument_lowering_phase(instrumentation, LoweringMeasurementPhase::Legality, || {
        audit_legality(core, &inventory).map_err(|diagnostics| {
            LoweringFailure::before_plan(LoweringPhase::Legality, diagnostics)
        })
    })?;
    let demand = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::RuntimeDemand,
        || {
            RuntimeDemand::for_level(
                core,
                &inventory,
                options.optimization_level(),
                RuntimeDemandLimits::derived(),
            )
            .map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, demand_diagnostics(&error))
            })
        },
    )?;
    let liveness = match options.optimization_level() {
        MinecraftOptimizationLevel::None => {
            instrumentation.skipped(
                LoweringMeasurementPhase::Liveness,
                "physical-optimization-disabled",
            );
            None
        }
        MinecraftOptimizationLevel::Baseline => Some(instrument_lowering_phase(
            instrumentation,
            LoweringMeasurementPhase::Liveness,
            || {
                LivenessResult::for_baseline(core, &inventory, &demand, LivenessLimits::derived())
                    .map_err(|error| {
                        LoweringFailure::before_plan(
                            LoweringPhase::Planning,
                            liveness_diagnostics(&error),
                        )
                    })
            },
        )?),
    };
    Ok((inventory, demand, liveness))
}

fn assign_lowering_homes<I>(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    demand: &RuntimeDemand,
    liveness: Option<&LivenessResult>,
    options: &LoweringOptions,
    instrumentation: &mut I,
) -> Result<(HomeAssignment, EdgeTransferPlan), LoweringFailure>
where
    I: LoweringInstrumentation,
{
    let assignment = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::Assignment,
        || {
            match options.optimization_level() {
                MinecraftOptimizationLevel::None => HomeAssignment::for_none(core, inventory),
                MinecraftOptimizationLevel::Baseline => HomeAssignment::for_baseline(
                    core,
                    inventory,
                    demand,
                    liveness.expect("baseline liveness was constructed in the preceding phase"),
                ),
            }
            .map_err(|error| {
                LoweringFailure::before_plan(
                    LoweringPhase::Planning,
                    assignment_diagnostics(&error),
                )
            })
        },
    )?;
    let transfers = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::EdgeTransfers,
        || {
            match options.optimization_level() {
                MinecraftOptimizationLevel::None => {
                    EdgeTransferPlan::for_none(core, inventory, &assignment)
                }
                MinecraftOptimizationLevel::Baseline => {
                    EdgeTransferPlan::for_baseline(core, inventory, &assignment)
                }
            }
            .map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, transfer_diagnostics(&error))
            })
        },
    )?;
    Ok((assignment, transfers))
}

fn freeze_lowering_plan<I>(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    assignment: HomeAssignment,
    transfers: EdgeTransferPlan,
    options: &LoweringOptions,
    instrumentation: &mut I,
) -> Result<(LoweringPlan, LoweringDecisionReport), LoweringFailure>
where
    I: LoweringInstrumentation,
{
    let control = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::ControlRecipes,
        || {
            ControlRecipePlan::new(
                core,
                inventory,
                &assignment,
                &transfers,
                options.optimization_level(),
            )
            .map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, control_diagnostics(&error))
            })
        },
    )?;
    let resources =
        instrument_lowering_phase(instrumentation, LoweringMeasurementPhase::Resources, || {
            ResourceInventory::for_control_plan(core, inventory, &transfers, &control, options)
                .map_err(|error| {
                    LoweringFailure::before_plan(
                        LoweringPhase::Planning,
                        resource_diagnostics(&error),
                    )
                })
        })?;
    let (plan, report) = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::PlanFreeze,
        || {
            let plan = LoweringPlan::from_selected_parts(
                core, options, assignment, transfers, control, resources,
            )
            .map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, finish_diagnostics(error))
            })?;
            let report = plan.report();
            Ok::<_, LoweringFailure>((plan, report))
        },
    )?;
    Ok((plan, report))
}

fn options_diagnostics(error: LoweringOptionsError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.reserved-pack-namespace",
        error.to_string(),
        OriginId::UNKNOWN,
    ))
}

fn analysis_diagnostics(error: AnalysisError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.analysis-invariant",
        format!("semantic inventory construction failed: {error:?}"),
        OriginId::UNKNOWN,
    ))
}

fn demand_diagnostics(error: &DemandError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.demand-invariant",
        format!("runtime demand computation failed: {error}"),
        OriginId::UNKNOWN,
    ))
}

fn liveness_diagnostics(error: &LivenessError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.liveness-invariant",
        format!("Minecraft liveness computation failed: {error:?}"),
        OriginId::UNKNOWN,
    ))
}

fn assignment_diagnostics(error: &AssignmentError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.assignment-invariant",
        format!("Minecraft home assignment failed: {error:?}"),
        OriginId::UNKNOWN,
    ))
}

fn transfer_diagnostics(error: &EdgeTransferError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.edge-transfer-invariant",
        format!("Minecraft edge-transfer planning failed: {error:?}"),
        OriginId::UNKNOWN,
    ))
}

fn control_diagnostics(error: &ControlRecipePlanError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.control-recipe-invariant",
        format!("Minecraft control-recipe planning failed: {error:?}"),
        OriginId::UNKNOWN,
    ))
}

fn resource_diagnostics(error: &ResourceInventoryError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.resource-invariant",
        format!("Minecraft resource planning failed: {error:?}"),
        OriginId::UNKNOWN,
    ))
}

fn planning_diagnostics(error: PlanBuildError) -> Diagnostics {
    let code = match error {
        PlanBuildError::EntityLimit {
            table: PlanTable::Functions | PlanTable::Homes | PlanTable::TargetFunctions,
        }
        | PlanBuildError::CapacityOverflow { .. }
        | PlanBuildError::CapacityExceeded { .. } => "lower.plan-entity-limit",
        PlanBuildError::MissingDefinition { .. }
        | PlanBuildError::OptimizationLevelMismatch { .. }
        | PlanBuildError::PhaseFunctionCountMismatch { .. }
        | PlanBuildError::InvalidPhaseInput { .. }
        | PlanBuildError::ScaffoldingAlreadyInitialized
        | PlanBuildError::MissingAnalysis { .. }
        | PlanBuildError::InvalidCoreEntity { .. }
        | PlanBuildError::InvalidEdgeShape { .. }
        | PlanBuildError::InvalidParallelCopy
        | PlanBuildError::InvalidRuntimeDemandPolicy
        | PlanBuildError::MissingScaffolding { .. }
        | PlanBuildError::MissingFunctionAbi { .. } => "lower.planning-invariant",
    };
    one_diagnostic(Diagnostic::new(
        code,
        format!("Minecraft lowering plan construction failed: {error:?}"),
        OriginId::UNKNOWN,
    ))
}

fn finish_diagnostics(error: PlanFinishError) -> Diagnostics {
    match error {
        PlanFinishError::Build(error) => planning_diagnostics(error),
        PlanFinishError::Invalid(diagnostics) => diagnostics,
    }
}

fn one_diagnostic(diagnostic: Diagnostic) -> Diagnostics {
    Diagnostics::from_findings(vec![diagnostic])
        .expect("one lowering finding always forms diagnostics")
}

#[cfg(test)]
mod tests {
    use super::{
        LoweringFailure, LoweringInstrumentation, LoweringMeasurementPhase, LoweringPhase,
        lower_to_minecraft, lower_to_minecraft_with_instrumentation, one_diagnostic,
        planning_diagnostics,
    };
    use crate::analysis::minecraft::{
        AnalysisArithmeticCaps, CommandLimitAssumptions, CommandLimitStatus,
        TargetExecutionAnalysisLimits, TargetExecutionRoot, analyze_target_execution,
    };
    use crate::diagnostic::Diagnostic;
    use crate::ir::core::{CoreProgram, CoreType, FunctionBuilder, Terminator, TerminatorKind};
    use crate::ir::minecraft::{
        FunctionTagResourceId, MinecraftDebugDumper, ObjectiveName, PackNamespace, verify_program,
    };
    use crate::lower::minecraft::plan::{PlanBuildError, PlanTable};
    use crate::lower::minecraft::{
        LoweringOptions, LoweringOptionsError, MinecraftOptimizationLevel,
    };
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    #[derive(Debug)]
    enum TimedLoweringOutcome {
        Completed { elapsed_nanoseconds: u128 },
        Skipped { reason: &'static str },
    }

    #[derive(Debug)]
    struct TimedLoweringSample {
        phase: LoweringMeasurementPhase,
        outcome: TimedLoweringOutcome,
    }

    #[derive(Default)]
    struct TimedLoweringRecorder {
        active: Option<(LoweringMeasurementPhase, std::time::Instant)>,
        samples: Vec<TimedLoweringSample>,
    }

    impl LoweringInstrumentation for TimedLoweringRecorder {
        fn before_phase(&mut self, phase: LoweringMeasurementPhase) {
            assert!(
                self.active
                    .replace((phase, std::time::Instant::now()))
                    .is_none(),
                "lowering phase instrumentation must not overlap"
            );
        }

        fn after_phase(&mut self, phase: LoweringMeasurementPhase, succeeded: bool) {
            let (active_phase, started) = self
                .active
                .take()
                .expect("every completed lowering phase has a matching start event");
            assert_eq!(active_phase, phase);
            assert!(succeeded, "the measurement fixture must lower successfully");
            self.samples.push(TimedLoweringSample {
                phase,
                outcome: TimedLoweringOutcome::Completed {
                    elapsed_nanoseconds: started.elapsed().as_nanos(),
                },
            });
        }

        fn skipped(&mut self, phase: LoweringMeasurementPhase, reason: &'static str) {
            assert!(self.active.is_none());
            self.samples.push(TimedLoweringSample {
                phase,
                outcome: TimedLoweringOutcome::Skipped { reason },
            });
        }
    }

    #[test]
    fn option_validation_is_a_real_preplan_failure_phase() {
        let sources = SourceContext::new();
        let core = empty_return_program(&sources, OriginId::UNKNOWN);
        let invalid = LoweringOptions {
            target: JavaEditionTarget::V26_2,
            namespace: PackNamespace::new("minecraft").unwrap(),
            register_objective: ObjectiveName::new("mdl.reg").unwrap(),
            command_limit_assumptions: CommandLimitAssumptions::for_target(
                JavaEditionTarget::V26_2,
            ),
            optimization_level: MinecraftOptimizationLevel::None,
        };

        let failure = lower_to_minecraft(&core, &sources, &invalid).unwrap_err();

        assert_eq!(failure.phase(), LoweringPhase::Options);
        assert!(
            failure
                .diagnostics()
                .contains_code("lower.reserved-pack-namespace")
        );
        assert_eq!(failure.dump_lowering(), None);
        assert!(failure.report().is_none());
        assert_eq!(
            LoweringOptions::new(
                JavaEditionTarget::V26_2,
                PackNamespace::new("minecraft").unwrap(),
                ObjectiveName::new("mdl.reg").unwrap(),
            ),
            Err(LoweringOptionsError::ReservedPackNamespace)
        );
    }

    #[test]
    fn synthetic_checked_failures_preserve_the_report_only_after_plan_freeze() {
        let planning = planning_diagnostics(PlanBuildError::EntityLimit {
            table: PlanTable::Homes,
        });
        let planning = LoweringFailure::before_plan(LoweringPhase::Planning, planning);
        assert_eq!(planning.phase(), LoweringPhase::Planning);
        assert_eq!(planning.dump_lowering(), None);
        assert!(planning.report().is_none());

        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let core = empty_return_program(&sources, origin);
        let output = lower_to_minecraft(&core, &sources, &options()).unwrap();
        let construction = LoweringFailure::after_plan(
            LoweringPhase::Construction,
            one_diagnostic(Diagnostic::new(
                "lower.synthetic-construction",
                "synthetic checked construction failure",
                origin,
            )),
            &output.report,
        );
        assert_eq!(construction.phase(), LoweringPhase::Construction);
        assert!(construction.dump_lowering().is_some());
        assert_eq!(construction.report(), Some(output.report()));

        let diagnostics = verify_program(output.program(), &SourceContext::new()).unwrap_err();
        let target = LoweringFailure::after_plan(
            LoweringPhase::TargetVerification,
            diagnostics,
            &output.report,
        );
        assert_eq!(target.phase(), LoweringPhase::TargetVerification);
        assert!(target.dump_lowering().is_some());
        assert!(
            target
                .diagnostics()
                .contains_code("minecraft.invalid-origin")
        );
    }

    #[test]
    fn lowering_convenience_matches_explicit_read_only_target_analysis() {
        let sources = SourceContext::new();
        let core = empty_return_program(&sources, OriginId::UNKNOWN);
        let assumptions = CommandLimitAssumptions::new(10, 4).unwrap();
        let overridden = options().with_command_limit_assumptions(assumptions);
        let output = lower_to_minecraft(&core, &sources, &overridden).unwrap();
        let limits = TargetExecutionAnalysisLimits::new(
            AnalysisArithmeticCaps::minimum_for(assumptions),
            10_000,
            10_000,
        );

        let convenience = output.analyze_target_execution(limits).unwrap();
        let mut roots = output
            .map
            .functions
            .iter()
            .map(|lowered| {
                let function = output
                    .program
                    .functions()
                    .find_map(|(function, data)| {
                        (data.resource() == lowered.entry_resource()).then_some(function)
                    })
                    .unwrap();
                TargetExecutionRoot::Function(function)
            })
            .collect::<Vec<_>>();
        let load = FunctionTagResourceId::parse("minecraft:load").unwrap();
        let tag = output
            .program
            .function_tags()
            .find_map(|(tag, data)| (data.resource() == &load).then_some(tag))
            .unwrap();
        roots.push(TargetExecutionRoot::FunctionTag(tag));
        let explicit =
            analyze_target_execution(output.program(), &sources, &roots, assumptions, limits)
                .unwrap();

        assert_eq!(convenience, explicit);
        let contract = output.map().execution_contract().command_limits();
        assert_eq!(contract.assumptions(), assumptions);
        assert_eq!(
            contract.target_defaults(),
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2)
        );
        assert!(
            output
                .dump_lowering()
                .contains("CallerBoundedToAssumptions")
        );
        assert_eq!(convenience.assumptions(), assumptions);
        assert_eq!(
            convenience.target_defaults(),
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2)
        );
        assert!(convenience.roots().iter().all(|root| {
            root.sequence_limit_status() == CommandLimitStatus::ProvenWithin
                && root.fork_limit_status() == CommandLimitStatus::ProvenWithin
        }));
        assert_eq!(
            output.analyze_target_execution(limits).unwrap().dump(),
            convenience.dump()
        );
    }

    #[test]
    #[ignore = "environment-specific per-phase measurements; inspect a --release run"]
    fn reports_every_lowering_phase_as_a_raw_completed_or_skipped_sample() {
        let sources = SourceContext::new();
        let core = measurement_program(&sources, 4_096);
        let phases = [
            LoweringMeasurementPhase::CoreVerification,
            LoweringMeasurementPhase::Options,
            LoweringMeasurementPhase::SemanticInventory,
            LoweringMeasurementPhase::Legality,
            LoweringMeasurementPhase::RuntimeDemand,
            LoweringMeasurementPhase::Liveness,
            LoweringMeasurementPhase::Assignment,
            LoweringMeasurementPhase::EdgeTransfers,
            LoweringMeasurementPhase::ControlRecipes,
            LoweringMeasurementPhase::Resources,
            LoweringMeasurementPhase::PlanFreeze,
            LoweringMeasurementPhase::Construction,
            LoweringMeasurementPhase::Reconciliation,
            LoweringMeasurementPhase::TargetVerification,
        ];

        for level in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let options = options().with_optimization_level(level);
            let expected = lower_to_minecraft(&core, &sources, &options).unwrap();
            let mut recorder = TimedLoweringRecorder::default();
            let started = std::time::Instant::now();
            let output =
                lower_to_minecraft_with_instrumentation(&core, &sources, &options, &mut recorder)
                    .unwrap();
            eprintln!(
                "minecraft-lowering level={level:?} elapsed_nanoseconds={}",
                started.elapsed().as_nanos()
            );
            for sample in &recorder.samples {
                match sample.outcome {
                    TimedLoweringOutcome::Completed {
                        elapsed_nanoseconds,
                    } => eprintln!(
                        "lowering-phase level={level:?} phase={:?} status=completed elapsed_nanoseconds={elapsed_nanoseconds}",
                        sample.phase,
                    ),
                    TimedLoweringOutcome::Skipped { reason } => eprintln!(
                        "lowering-phase level={level:?} phase={:?} status=skipped reason={reason}",
                        sample.phase,
                    ),
                }
            }

            assert_eq!(
                recorder
                    .samples
                    .iter()
                    .map(|sample| sample.phase)
                    .collect::<Vec<_>>(),
                phases
            );
            assert_eq!(
                recorder
                    .samples
                    .iter()
                    .filter(|sample| matches!(sample.outcome, TimedLoweringOutcome::Skipped { .. }))
                    .count(),
                usize::from(level == MinecraftOptimizationLevel::None)
            );
            assert_eq!(output.report(), expected.report());
            assert_eq!(
                MinecraftDebugDumper::program(output.program()),
                MinecraftDebugDumper::program(expected.program())
            );
            assert!(!output.report().dump().contains("elapsed"));
        }
    }

    fn empty_return_program(sources: &SourceContext, origin: OriginId) -> CoreProgram {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("empty"), vec![], vec![], origin)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), origin))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        core
    }

    fn measurement_program(sources: &SourceContext, values: usize) -> CoreProgram {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("measurement"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        let entry = builder.entry_block();
        let mut current = builder.body().block(entry).unwrap().parameters()[0].value();
        for _ in 0..values {
            current = builder
                .i32_add_wrapping(current, current, OriginId::UNKNOWN)
                .unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![current]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        core
    }

    fn options() -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
    }
}
