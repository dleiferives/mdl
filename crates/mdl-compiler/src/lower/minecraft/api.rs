use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;

use crate::analysis::minecraft::{
    CommandLimitAssumptions, TargetExecutionAnalysisFailure, TargetExecutionAnalysisLimits,
    TargetExecutionAnalysisPhase, TargetExecutionCostReport, TargetExecutionRoot,
    analyze_verified_target_execution,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::EntityId;
use crate::ir::core::{
    CoreAmbientAnalysis, CoreFunctionLinkage, CoreProgram, CoreType, FunctionId, InstId,
};
use crate::ir::minecraft::{
    CommandId, FakeScoreHolder, FunctionResourceId, FunctionTagResourceId, McFunctionId,
    MinecraftProgram, ObjectiveName, verify_program,
};
use crate::ir::semantic::AmbientContextRequirements;
use crate::source::{OriginId, SourceContext};

use super::analysis::{AnalysisError, SemanticInventory};
use super::assignment::{AssignmentError, HomeAssignment};
use super::audit::audit_legality;
use super::demand::{DemandError, RuntimeDemand, RuntimeDemandLimits};
use super::edge_transfer::{EdgeTransferError, EdgeTransferPlan};
use super::emit::construct_program;
use super::liveness::{LivenessError, LivenessLimits, LivenessResult};
use super::physical_preflight::{PhysicalPreflight, PhysicalPreflightError};
use super::placement::{ControlRecipePlan, ControlRecipePlanError};
use super::plan::{
    LoweringDecisionReport, LoweringPlan, PlanBuildError, PlanFinishError, PlanTable,
    verify_constructed_control_recipes,
};
use super::realization::{PhysicalPlanError, PhysicalPlanningLimits, PhysicalRealizationPlan};
use super::resources::{ResourceInventory, ResourceInventoryError};
use super::{LoweringOptions, LoweringOptionsError, MinecraftOptimizationLevel, TargetPreflight};

/// Required activation discipline for the scalar synchronous ABI.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ActivationContract {
    /// Synchronous invocations may repeat across serial command contexts but may
    /// not recurse or preserve a live activation across ticks.
    SynchronousSerial,
    /// Recursive SCCs use compiler-owned synchronous activation frames. Abnormal
    /// command-sequence termination is not transactional; a reload clears residue.
    SynchronousRecursiveStack,
}

/// Who supplies the bound on live synchronous activation depth.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ActivationDepthContract {
    /// No reachable recursive SCC requires a dynamic activation depth.
    Static,
    /// The compiler preserves arbitrary synchronous recursion until Minecraft's
    /// configured command limit stops the root; it invents no truncation or trap.
    CallerBounded,
}

/// Successful legality inputs and the derived structured-redirect requirement.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommandLimitEvidence {
    configured_assumptions: CommandLimitAssumptions,
    target_defaults: CommandLimitAssumptions,
    minimum_max_command_forks: u64,
}

impl CommandLimitEvidence {
    pub(crate) const fn new(
        configured_assumptions: CommandLimitAssumptions,
        target_defaults: CommandLimitAssumptions,
        minimum_max_command_forks: u64,
    ) -> Self {
        Self {
            configured_assumptions,
            target_defaults,
            minimum_max_command_forks,
        }
    }

    /// Returns the server values configured for this compilation's safety proofs.
    #[must_use]
    pub const fn configured_assumptions(self) -> CommandLimitAssumptions {
        self.configured_assumptions
    }

    /// Returns the selected target's immutable default server values.
    #[must_use]
    pub const fn target_defaults(self) -> CommandLimitAssumptions {
        self.target_defaults
    }

    /// Returns the lowest `minecraft:max_command_forks` proven sufficient for
    /// every reachable compiler-structured ordinary redirect in this generated pack.
    #[must_use]
    pub const fn minimum_max_command_forks(self) -> u64 {
        self.minimum_max_command_forks
    }
}

/// Runtime command-budget responsibility retained by the caller.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct CommandLimitContract {
    evidence: CommandLimitEvidence,
}

impl CommandLimitContract {
    pub(crate) const fn new(evidence: CommandLimitEvidence) -> Self {
        Self { evidence }
    }

    /// Returns the complete retained proof input and derived deployment requirement.
    #[must_use]
    pub const fn evidence(self) -> CommandLimitEvidence {
        self.evidence
    }

    /// Returns the configured server values required by retained safety proofs.
    ///
    /// Lower actual gamerule values invalidate `ProvenWithin`; this contract never
    /// configures the server itself.
    #[must_use]
    pub const fn configured_assumptions(self) -> CommandLimitAssumptions {
        self.evidence.configured_assumptions()
    }

    /// Returns the selected target's immutable default server values.
    #[must_use]
    pub const fn target_defaults(self) -> CommandLimitAssumptions {
        self.evidence.target_defaults()
    }

    /// Returns the lowest `minecraft:max_command_forks` proven sufficient for
    /// every reachable compiler-structured ordinary redirect in this generated pack.
    #[must_use]
    pub const fn minimum_max_command_forks(self) -> u64 {
        self.evidence.minimum_max_command_forks()
    }
}

impl fmt::Debug for CommandLimitContract {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandLimitContract")
            .field(
                "configured_max_command_sequence_length",
                &self.configured_assumptions().max_command_sequence_length(),
            )
            .field(
                "configured_max_command_forks",
                &self.configured_assumptions().max_command_forks(),
            )
            .field(
                "target_default_max_command_sequence_length",
                &self.target_defaults().max_command_sequence_length(),
            )
            .field(
                "target_default_max_command_forks",
                &self.target_defaults().max_command_forks(),
            )
            .field(
                "derived_minimum_max_command_forks",
                &self.minimum_max_command_forks(),
            )
            .finish()
    }
}

/// Honest execution restrictions for one generated pack.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionContract {
    activation: ActivationContract,
    activation_depth: ActivationDepthContract,
    command_limits: CommandLimitContract,
}

impl ExecutionContract {
    pub(crate) const fn new(
        command_limit_evidence: CommandLimitEvidence,
        has_recursive_activation: bool,
    ) -> Self {
        Self {
            activation: if has_recursive_activation {
                ActivationContract::SynchronousRecursiveStack
            } else {
                ActivationContract::SynchronousSerial
            },
            activation_depth: if has_recursive_activation {
                ActivationDepthContract::CallerBounded
            } else {
                ActivationDepthContract::Static
            },
            command_limits: CommandLimitContract::new(command_limit_evidence),
        }
    }

    /// Returns the required activation discipline.
    #[must_use]
    pub const fn activation(&self) -> ActivationContract {
        self.activation
    }

    /// Returns the ownership of the synchronous recursion-depth bound.
    #[must_use]
    pub const fn activation_depth(&self) -> ActivationDepthContract {
        self.activation_depth
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
    linkage: CoreFunctionLinkage,
    entry_resource: FunctionResourceId,
    /// Source `tick` modifier (Stage 9B): registered into `#minecraft:tick`.
    tick_handler: bool,
    /// Whether any `schedule` (arm) statement names this function (Stage 9B).
    is_schedule_target: bool,
    generated_entry_requirement: AmbientContextRequirements,
    parameter_homes: Box<[(CoreType, RegisterSlot)]>,
    result_homes: Box<[(CoreType, RegisterSlot)]>,
}

/// Exact target placement of one directly lowered semantic Core occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LoweredCommand {
    function: McFunctionId,
    command: CommandId,
}

impl LoweredCommand {
    pub(crate) const fn new(function: McFunctionId, command: CommandId) -> Self {
        Self { function, command }
    }

    /// Returns the generated target function containing the command.
    #[must_use]
    pub const fn function(self) -> McFunctionId {
        self.function
    }

    /// Returns the command's exact function-local target identity.
    #[must_use]
    pub const fn command(self) -> CommandId {
        self.command
    }
}

/// Exact selected recipe and target placement of one Core run modifier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LoweredRunModifier {
    function: McFunctionId,
    command: CommandId,
    modifier_index: usize,
    recipe: super::RunModifierRecipeId,
}

impl LoweredRunModifier {
    pub(crate) const fn new(
        function: McFunctionId,
        command: CommandId,
        modifier_index: usize,
        recipe: super::RunModifierRecipeId,
    ) -> Self {
        Self {
            function,
            command,
            modifier_index,
            recipe,
        }
    }

    #[must_use]
    pub const fn function(self) -> McFunctionId {
        self.function
    }
    #[must_use]
    pub const fn command(self) -> CommandId {
        self.command
    }
    #[must_use]
    pub const fn modifier_index(self) -> usize {
        self.modifier_index
    }
    #[must_use]
    pub const fn recipe(self) -> super::RunModifierRecipeId {
        self.recipe
    }
}

impl LoweredFunction {
    #[allow(
        clippy::too_many_arguments,
        reason = "mirrors the growing set of independent source markers threaded through Core's own Function"
    )]
    pub(crate) fn new(
        linkage: CoreFunctionLinkage,
        entry_resource: FunctionResourceId,
        tick_handler: bool,
        is_schedule_target: bool,
        generated_entry_requirement: AmbientContextRequirements,
        parameter_homes: Vec<(CoreType, RegisterSlot)>,
        result_homes: Vec<(CoreType, RegisterSlot)>,
    ) -> Self {
        Self {
            linkage,
            entry_resource,
            tick_handler,
            is_schedule_target,
            generated_entry_requirement,
            parameter_homes: parameter_homes.into_boxed_slice(),
            result_homes: result_homes.into_boxed_slice(),
        }
    }

    /// Returns whether this ABI is internal or a supported datapack entry.
    #[must_use]
    pub const fn linkage(&self) -> CoreFunctionLinkage {
        self.linkage
    }

    /// Returns the generated entry function resource.
    #[must_use]
    pub const fn entry_resource(&self) -> &FunctionResourceId {
        &self.entry_resource
    }

    /// Returns whether this function carries the source `tick` modifier
    /// (Stage 9B).
    #[must_use]
    pub const fn tick_handler(&self) -> bool {
        self.tick_handler
    }

    /// Returns whether any `schedule` (arm) statement names this function
    /// (Stage 9B).
    #[must_use]
    pub const fn is_schedule_target(&self) -> bool {
        self.is_schedule_target
    }

    /// Returns the optimized Core function's required incoming Minecraft context.
    ///
    /// This is a generated-entry requirement, independently recomputed after Core
    /// optimization. It is intentionally distinct from the source HIR behavior
    /// published by the frontend.
    #[must_use]
    pub const fn generated_entry_requirement(&self) -> AmbientContextRequirements {
        self.generated_entry_requirement
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
    semantic_commands: Box<[Box<[Option<LoweredCommand>]>]>,
    run_modifiers: Box<[Box<[Option<LoweredRunModifier>]>]>,
    export_count: usize,
}

impl LoweringMap {
    pub(crate) fn new(
        execution: ExecutionContract,
        functions: Vec<LoweredFunction>,
        semantic_commands: Vec<Box<[Option<LoweredCommand>]>>,
        run_modifiers: Vec<Box<[Option<LoweredRunModifier>]>>,
    ) -> Self {
        debug_assert_eq!(functions.len(), semantic_commands.len());
        let export_count = functions
            .iter()
            .filter(|function| function.linkage == CoreFunctionLinkage::DatapackExport)
            .count();
        Self {
            execution,
            functions: functions.into_boxed_slice(),
            semantic_commands: semantic_commands.into_boxed_slice(),
            run_modifiers: run_modifiers.into_boxed_slice(),
            export_count,
        }
    }

    /// Returns one source/Core modifier's selected recipe and exact physical placement.
    #[must_use]
    pub fn run_modifier(
        &self,
        scope: crate::ir::core::RunScopeId,
        modifier_index: usize,
    ) -> Option<LoweredRunModifier> {
        self.run_modifiers
            .get(usize::try_from(scope.index()).ok()?)?
            .get(modifier_index)
            .copied()
            .flatten()
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

    /// Looks up the primary correlated command for one directly lowered semantic
    /// Core occurrence. Recipe-required adjacent setup remains part of the verified
    /// target fragment but is not assigned a second source identity. Nonsemantic,
    /// omitted, helper-based, and invalid identities return `None`.
    #[must_use]
    pub fn semantic_command(
        &self,
        function: FunctionId,
        instruction: InstId,
    ) -> Option<LoweredCommand> {
        let function = usize::try_from(function.index()).ok()?;
        let instruction = usize::try_from(instruction.index()).ok()?;
        self.semantic_commands
            .get(function)?
            .get(instruction)
            .copied()
            .flatten()
    }

    /// Iterates every direct semantic occurrence and its primary correlated command
    /// in stable Core function/instruction order.
    pub fn semantic_commands(
        &self,
    ) -> impl Iterator<Item = (FunctionId, InstId, LoweredCommand)> + '_ {
        self.semantic_commands
            .iter()
            .enumerate()
            .flat_map(|(function, instructions)| {
                instructions
                    .iter()
                    .enumerate()
                    .filter_map(move |(instruction, location)| {
                        Some((
                            FunctionId::from_index(u32::try_from(function).ok()?),
                            InstId::from_index(u32::try_from(instruction).ok()?),
                            (*location)?,
                        ))
                    })
            })
    }

    /// Renders deterministic post-construction semantic-command correlations.
    #[must_use]
    pub fn dump_semantic_commands(&self) -> String {
        let mut output = String::new();
        for (function, instruction, location) in self.semantic_commands() {
            writeln!(
                output,
                "constructed-semantic-command core-function={} instruction={} target-function={} command={}",
                function.index(),
                instruction.index(),
                location.function().index(),
                location.command().index(),
            )
            .expect("writing to a String cannot fail");
        }
        output
    }

    /// Iterates supported datapack entries in stable Core function order.
    pub fn exported_functions(&self) -> impl Iterator<Item = (FunctionId, &LoweredFunction)> + '_ {
        self.functions
            .iter()
            .enumerate()
            .filter_map(|(index, lowered)| {
                if lowered.linkage != CoreFunctionLinkage::DatapackExport {
                    return None;
                }
                let function = FunctionId::from_index(u32::try_from(index).ok()?);
                Some((function, lowered))
            })
    }

    /// Returns the number of mapped Core functions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Returns the number of supported datapack entries.
    #[must_use]
    pub const fn export_count(&self) -> usize {
        self.export_count
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
        let mut output = self.report.dump();
        output.push_str(&self.map.dump_semantic_commands());
        output
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
        let root_capacity = self.map.export_count.checked_add(1).ok_or_else(|| {
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
        for lowered in self
            .map
            .functions
            .iter()
            .filter(|function| function.linkage == CoreFunctionLinkage::DatapackExport)
        {
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
        // Stage 9B: each schedule-target function becomes its own independent
        // root — when the scheduler fires it, that is a wholly separate
        // synchronous run from its own entry point, exactly like an export
        // root's invocation. Independent of `DatapackExport` linkage.
        for lowered in self
            .map
            .functions
            .iter()
            .filter(|function| function.is_schedule_target())
        {
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
        // Stage 9B: `#minecraft:tick` becomes one aggregate `FunctionTag` root
        // covering all handlers together, matching vanilla's real per-tick
        // combined command budget — constructed only if at least one tick
        // handler exists, so a program with zero `tick` usage adds no root.
        if self
            .map
            .functions
            .iter()
            .any(|function| function.tick_handler())
        {
            let tick_resource = FunctionTagResourceId::parse("minecraft:tick").map_err(|_| {
                TargetExecutionAnalysisFailure::internal(
                    TargetExecutionAnalysisPhase::Invariant,
                    "target-cost.static-tick-resource",
                    "the compiler's static vanilla tick-tag resource is invalid",
                )
            })?;
            let tick_tag = self
                .program
                .function_tags()
                .find_map(|(tag, data)| (data.resource() == &tick_resource).then_some(tag))
                .ok_or_else(|| {
                    TargetExecutionAnalysisFailure::internal(
                        TargetExecutionAnalysisPhase::Invariant,
                        "target-cost.lowering-tick-root",
                        "verified lowering output is missing its generated tick tag",
                    )
                })?;
            roots.push(TargetExecutionRoot::FunctionTag(tick_tag));
        }
        let assumptions = self.map.execution.command_limits().configured_assumptions();
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
    AmbientAnalysis,
    TargetPreflight,
    RuntimeDemand,
    Liveness,
    Assignment,
    PhysicalRealizations,
    PhysicalPreflight,
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
    let (inventory, preflight, ambient, demand, liveness) =
        analyze_lowering_request(core, sources, options, instrumentation)?;
    let (assignment, physical, physical_preflight, transfers) = assign_lowering_homes(
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
        preflight,
        ambient,
        assignment,
        physical,
        physical_preflight,
        transfers,
        options,
        instrumentation,
    )?;
    let constructed = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::Construction,
        || {
            construct_program(core, &plan).map_err(|diagnostics| {
                LoweringFailure::after_plan(LoweringPhase::Construction, diagnostics, &report)
            })
        },
    )?;
    let (program, command_map) = constructed.into_parts();
    instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::Reconciliation,
        || {
            verify_constructed_control_recipes(core, &plan, &program, &command_map).map_err(
                |diagnostics| {
                    LoweringFailure::after_plan(LoweringPhase::Construction, diagnostics, &report)
                },
            )
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
    let map = report.map(&command_map);
    Ok(LoweringOutput {
        program,
        map,
        report,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the lowering analysis boundary instruments each dependency-ordered phase explicitly"
)]
fn analyze_lowering_request<I>(
    core: &CoreProgram,
    sources: &SourceContext,
    options: &LoweringOptions,
    instrumentation: &mut I,
) -> Result<
    (
        SemanticInventory,
        TargetPreflight,
        CoreAmbientAnalysis,
        RuntimeDemand,
        Option<LivenessResult>,
    ),
    LoweringFailure,
>
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
    let command_limits =
        instrument_lowering_phase(instrumentation, LoweringMeasurementPhase::Legality, || {
            audit_legality(
                core,
                &inventory,
                options.target(),
                options.command_limit_assumptions(),
            )
            .map_err(|diagnostics| {
                LoweringFailure::before_plan(LoweringPhase::Legality, diagnostics)
            })
        })?;
    let ambient = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::AmbientAnalysis,
        || {
            let ambient = CoreAmbientAnalysis::analyze(core).map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, ambient_diagnostics(&error))
            })?;
            ambient.verify(core).map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, ambient_diagnostics(&error))
            })?;
            Ok(ambient)
        },
    )?;
    let preflight = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::TargetPreflight,
        || {
            let preflight =
                TargetPreflight::new(core, &inventory, options.target(), command_limits).map_err(
                    |diagnostics| {
                        LoweringFailure::before_plan(LoweringPhase::Legality, diagnostics)
                    },
                )?;
            preflight.verify(core, &inventory).map_err(|diagnostics| {
                LoweringFailure::before_plan(LoweringPhase::Legality, diagnostics)
            })?;
            Ok(preflight)
        },
    )?;
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
    let liveness = Some(instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::Liveness,
        || {
            LivenessResult::for_level(
                core,
                &inventory,
                &demand,
                options.optimization_level(),
                LivenessLimits::derived(),
            )
            .map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, liveness_diagnostics(&error))
            })
        },
    )?);
    Ok((inventory, preflight, ambient, demand, liveness))
}

fn assign_lowering_homes<I>(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    demand: &RuntimeDemand,
    liveness: Option<&LivenessResult>,
    options: &LoweringOptions,
    instrumentation: &mut I,
) -> Result<
    (
        HomeAssignment,
        PhysicalRealizationPlan,
        PhysicalPreflight,
        EdgeTransferPlan,
    ),
    LoweringFailure,
>
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
                    liveness.expect("physical liveness was constructed in the preceding phase"),
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
    let physical = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::PhysicalRealizations,
        || {
            PhysicalRealizationPlan::for_score_compatibility(
                core,
                inventory,
                &assignment,
                liveness,
                PhysicalPlanningLimits::DEFAULT,
            )
            .map_err(|error| {
                LoweringFailure::before_plan(
                    LoweringPhase::Planning,
                    physical_plan_diagnostics(&error),
                )
            })
        },
    )?;
    let physical_preflight = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::PhysicalPreflight,
        || {
            PhysicalPreflight::new(options.target(), &physical).map_err(|error| {
                LoweringFailure::before_plan(
                    LoweringPhase::Planning,
                    physical_preflight_diagnostics(error),
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
    Ok((assignment, physical, physical_preflight, transfers))
}

#[allow(
    clippy::too_many_arguments,
    reason = "freezing consumes each independently verified immutable phase product exactly once"
)]
fn freeze_lowering_plan<I>(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    preflight: TargetPreflight,
    ambient: CoreAmbientAnalysis,
    assignment: HomeAssignment,
    physical: PhysicalRealizationPlan,
    physical_preflight: PhysicalPreflight,
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
            ResourceInventory::for_control_plan(
                core, inventory, &transfers, &control, &preflight, options,
            )
            .map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, resource_diagnostics(&error))
            })
        })?;
    let (plan, report) = instrument_lowering_phase(
        instrumentation,
        LoweringMeasurementPhase::PlanFreeze,
        || {
            let plan = LoweringPlan::from_selected_parts(
                core,
                options,
                preflight,
                ambient,
                assignment,
                physical,
                physical_preflight,
                transfers,
                control,
                resources,
            )
            .map_err(|error| {
                LoweringFailure::before_plan(LoweringPhase::Planning, finish_diagnostics(error))
            })?;
            let report = plan.report(core);
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

fn physical_preflight_diagnostics(error: PhysicalPreflightError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.physical-preflight-invariant",
        format!("Minecraft physical preflight failed: {error}"),
        OriginId::UNKNOWN,
    ))
}

fn physical_plan_diagnostics(error: &PhysicalPlanError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.physical-realization-invariant",
        format!("Minecraft physical realization planning failed: {error}"),
        OriginId::UNKNOWN,
    ))
}

fn ambient_diagnostics(error: &crate::ir::core::CoreAmbientAnalysisError) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.ambient-invariant",
        format!("Core ambient-context analysis failed: {error}"),
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
            .exported_functions()
            .map(|(_, lowered)| {
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
        assert_eq!(contract.configured_assumptions(), assumptions);
        assert_eq!(
            contract.target_defaults(),
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2)
        );
        assert_eq!(contract.minimum_max_command_forks(), 0);
        let dump = output.dump_lowering();
        assert!(dump.contains("configured_max_command_sequence_length: 10"));
        assert!(dump.contains("configured_max_command_forks: 4"));
        assert!(dump.contains("target_default_max_command_forks: 65536"));
        assert!(dump.contains("derived_minimum_max_command_forks: 0"));
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
            LoweringMeasurementPhase::AmbientAnalysis,
            LoweringMeasurementPhase::TargetPreflight,
            LoweringMeasurementPhase::RuntimeDemand,
            LoweringMeasurementPhase::Liveness,
            LoweringMeasurementPhase::Assignment,
            LoweringMeasurementPhase::PhysicalRealizations,
            LoweringMeasurementPhase::PhysicalPreflight,
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
