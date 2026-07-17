use std::error::Error;
use std::fmt;
use std::fmt::Write as _;

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::ir::minecraft::{
    CommandKind, Condition, ExecuteModifierKind, McFunctionId, MinecraftProgram, ReturnCommand,
};
use crate::target::JavaEditionTarget;

use super::solve::MetricSet;
use super::{
    AnalysisArithmeticCaps, CommandLimitAssumptions, CostRegionId, CountBound,
    FunctionLocalSummary, RootExecutionSummary,
};

/// Deterministic work and arithmetic limits for optional cost analysis.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TargetExecutionAnalysisLimits {
    arithmetic: AnalysisArithmeticCaps,
    graph_entities: usize,
    solver_updates: usize,
}

impl TargetExecutionAnalysisLimits {
    /// Creates explicit limits. Zero work limits request immediate typed fallback.
    #[must_use]
    pub const fn new(
        arithmetic: AnalysisArithmeticCaps,
        graph_entities: usize,
        solver_updates: usize,
    ) -> Self {
        Self {
            arithmetic,
            graph_entities,
            solver_updates,
        }
    }

    #[must_use]
    pub const fn arithmetic(self) -> AnalysisArithmeticCaps {
        self.arithmetic
    }

    #[must_use]
    pub const fn graph_entities(self) -> usize {
        self.graph_entities
    }

    #[must_use]
    pub const fn solver_updates(self) -> usize {
        self.solver_updates
    }
}

/// Exact structured-command inventory retained independently of dynamic bounds.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TargetExecutionCensus {
    functions: usize,
    function_tags: usize,
    function_tag_entries: usize,
    top_level_commands: usize,
    command_nodes: usize,
    execute_stages: usize,
    function_calls: usize,
    function_condition_calls: usize,
    score_commands: usize,
    data_commands: usize,
    say_commands: usize,
    teleport_commands: usize,
    return_commands: usize,
    raw_commands: usize,
}

macro_rules! census_accessors {
    ($(($name:ident, $field:ident)),+ $(,)?) => {
        $(
            #[must_use]
            pub const fn $name(self) -> usize { self.$field }
        )+
    };
}

impl TargetExecutionCensus {
    census_accessors!(
        (functions, functions),
        (function_tags, function_tags),
        (function_tag_entries, function_tag_entries),
        (top_level_commands, top_level_commands),
        (command_nodes, command_nodes),
        (execute_stages, execute_stages),
        (function_calls, function_calls),
        (function_condition_calls, function_condition_calls),
        (score_commands, score_commands),
        (data_commands, data_commands),
        (say_commands, say_commands),
        (teleport_commands, teleport_commands),
        (return_commands, return_commands),
        (raw_commands, raw_commands),
    );

    pub(super) fn from_program(program: &MinecraftProgram) -> Option<Self> {
        let mut census = Self {
            functions: program.functions().len(),
            function_tags: program.function_tags().len(),
            ..Self::default()
        };
        for (_, function) in program.functions() {
            census.top_level_commands = census
                .top_level_commands
                .checked_add(function.body().len())?;
            for (_, command) in function.body().commands() {
                census.visit_command(command.kind())?;
            }
        }
        for (_, tag) in program.function_tags() {
            census.function_tag_entries = census
                .function_tag_entries
                .checked_add(tag.entries().len())?;
        }
        Some(census)
    }

    fn visit_command(&mut self, command: &CommandKind) -> Option<()> {
        self.command_nodes = self.command_nodes.checked_add(1)?;
        match command {
            CommandKind::Score(_) => self.score_commands = self.score_commands.checked_add(1)?,
            CommandKind::Data(_) => self.data_commands = self.data_commands.checked_add(1)?,
            CommandKind::Say(_) => self.say_commands = self.say_commands.checked_add(1)?,
            CommandKind::Teleport(_) => {
                self.teleport_commands = self.teleport_commands.checked_add(1)?;
            }
            CommandKind::Function(_) => {
                self.function_calls = self.function_calls.checked_add(1)?;
            }
            CommandKind::Raw(_) => self.raw_commands = self.raw_commands.checked_add(1)?,
            CommandKind::Execute(command) => {
                self.execute_stages = self.execute_stages.checked_add(command.modifiers().len())?;
                for modifier in command.modifiers().as_slice() {
                    if matches!(
                        modifier.kind(),
                        ExecuteModifierKind::If(Condition::Function(_))
                            | ExecuteModifierKind::Unless(Condition::Function(_))
                    ) {
                        self.function_calls = self.function_calls.checked_add(1)?;
                        self.function_condition_calls =
                            self.function_condition_calls.checked_add(1)?;
                    }
                }
                self.visit_command(command.run().kind())?;
            }
            CommandKind::Return(command) => {
                self.return_commands = self.return_commands.checked_add(1)?;
                if let ReturnCommand::Run(command) = command {
                    self.visit_command(command.kind())?;
                }
            }
        }
        Some(())
    }
}

/// One memoized strongly connected region shared by every root summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CostRegionSummary {
    id: CostRegionId,
    functions: Box<[McFunctionId]>,
    cyclic: bool,
    sequence_operations: CountBound,
    execute_stages: CountBound,
    internal_function_invocations: CountBound,
    score_nbt_command_executions: CountBound,
    maximum_chain_expansion: CountBound,
}

impl CostRegionSummary {
    pub(super) fn new(
        id: CostRegionId,
        functions: Vec<McFunctionId>,
        cyclic: bool,
        metrics: MetricSet,
    ) -> Self {
        Self {
            id,
            functions: functions.into_boxed_slice(),
            cyclic,
            sequence_operations: metrics.sequence,
            execute_stages: metrics.execute,
            internal_function_invocations: metrics.calls,
            score_nbt_command_executions: metrics.score_nbt,
            maximum_chain_expansion: metrics.max_chain,
        }
    }

    #[must_use]
    pub const fn id(&self) -> CostRegionId {
        self.id
    }
    #[must_use]
    pub fn functions(&self) -> &[McFunctionId] {
        &self.functions
    }
    /// Returns whether the retained structural call graph contains this region as
    /// an SCC cycle.
    ///
    /// Outcome-feasibility refinement may still prove finite execution; this flag
    /// alone does not claim a feasible positive runtime cycle.
    #[must_use]
    pub const fn is_cyclic(&self) -> bool {
        self.cyclic
    }
    #[must_use]
    pub const fn sequence_operations(&self) -> CountBound {
        self.sequence_operations
    }
    #[must_use]
    pub const fn execute_stages(&self) -> CountBound {
        self.execute_stages
    }
    #[must_use]
    pub const fn internal_function_invocations(&self) -> CountBound {
        self.internal_function_invocations
    }
    #[must_use]
    pub const fn score_nbt_command_executions(&self) -> CountBound {
        self.score_nbt_command_executions
    }
    /// Returns maximum ordinary fork-checked redirect expansion for this region.
    #[must_use]
    pub const fn maximum_chain_expansion(&self) -> CountBound {
        self.maximum_chain_expansion
    }
}

/// Linear owned result of one explicit target-execution analysis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetExecutionCostReport {
    target: JavaEditionTarget,
    assumptions: CommandLimitAssumptions,
    target_defaults: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
    completion: TargetExecutionAnalysisCompletion,
    census: TargetExecutionCensus,
    stats: TargetExecutionAnalysisStats,
    functions: Box<[FunctionLocalSummary]>,
    regions: Box<[CostRegionSummary]>,
    roots: Box<[RootExecutionSummary]>,
}

#[derive(Clone, Copy)]
pub(super) struct TargetExecutionReportContext {
    target: JavaEditionTarget,
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
    completion: TargetExecutionAnalysisCompletion,
}

impl TargetExecutionReportContext {
    pub(super) const fn new(
        target: JavaEditionTarget,
        assumptions: CommandLimitAssumptions,
        limits: TargetExecutionAnalysisLimits,
        completion: TargetExecutionAnalysisCompletion,
    ) -> Self {
        Self {
            target,
            assumptions,
            limits,
            completion,
        }
    }
}

/// Whether every requested analysis region reached a final result.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TargetExecutionAnalysisCompletion {
    Complete,
    Incomplete(TargetExecutionAnalysisLimitKind),
}

/// Deterministic work budget that caused conservative analysis fallback.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TargetExecutionAnalysisLimitKind {
    GraphEntities,
    SolverUpdates,
}

/// Deterministic construction and retained-record counts for scale auditing.
///
/// `graph_edges` counts unique caller-to-owned-function connectivity after tag
/// expansion, not typed site occurrences or runtime invocations. On a complete
/// report, `graph_entities` is exact charged construction work; on a graph-limit
/// report it is the deterministic preflight/exhaustion witness and can exceed the
/// configured limit even though no partial graph edges are retained. `solver_updates`
/// includes completed function transfers, exit-bit discoveries, and final weighted
/// evaluations consumed before fallback. `command_transfer_visits` counts every
/// structured command transfer evaluated across local-outcome construction,
/// feasibility, and weighted solving, including recursively nested commands.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct TargetExecutionAnalysisStats {
    graph_entities: usize,
    graph_edges: usize,
    tag_expansion_entries: usize,
    solver_updates: usize,
    command_transfer_visits: usize,
    retained_functions: usize,
    retained_regions: usize,
    retained_roots: usize,
}

#[derive(Clone, Copy)]
pub(super) struct TargetExecutionAnalysisStatsInput {
    pub(super) graph_entities: usize,
    pub(super) graph_edges: usize,
    pub(super) tag_expansion_entries: usize,
    pub(super) solver_updates: usize,
    pub(super) command_transfer_visits: usize,
    pub(super) retained_functions: usize,
    pub(super) retained_regions: usize,
    pub(super) retained_roots: usize,
}

impl TargetExecutionAnalysisStats {
    pub(super) const fn new(input: TargetExecutionAnalysisStatsInput) -> Self {
        Self {
            graph_entities: input.graph_entities,
            graph_edges: input.graph_edges,
            tag_expansion_entries: input.tag_expansion_entries,
            solver_updates: input.solver_updates,
            command_transfer_visits: input.command_transfer_visits,
            retained_functions: input.retained_functions,
            retained_regions: input.retained_regions,
            retained_roots: input.retained_roots,
        }
    }

    census_accessors!(
        (graph_entities, graph_entities),
        (graph_edges, graph_edges),
        (tag_expansion_entries, tag_expansion_entries),
        (solver_updates, solver_updates),
        (command_transfer_visits, command_transfer_visits),
        (retained_functions, retained_functions),
        (retained_regions, retained_regions),
        (retained_roots, retained_roots),
    );
}

impl TargetExecutionCostReport {
    pub(super) fn new(
        context: TargetExecutionReportContext,
        census: TargetExecutionCensus,
        stats: TargetExecutionAnalysisStats,
        functions: Vec<FunctionLocalSummary>,
        regions: Vec<CostRegionSummary>,
        roots: Vec<RootExecutionSummary>,
    ) -> Self {
        Self {
            target: context.target,
            assumptions: context.assumptions,
            target_defaults: CommandLimitAssumptions::for_target(context.target),
            limits: context.limits,
            completion: context.completion,
            census,
            stats,
            functions: functions.into_boxed_slice(),
            regions: regions.into_boxed_slice(),
            roots: roots.into_boxed_slice(),
        }
    }

    #[must_use]
    pub const fn target(&self) -> JavaEditionTarget {
        self.target
    }
    /// Returns configured gamerule assumptions used by every retained root status.
    /// The actual deployment must provide values no lower than these assumptions for
    /// callers to rely on `ProvenWithin` results.
    #[must_use]
    pub const fn assumptions(&self) -> CommandLimitAssumptions {
        self.assumptions
    }
    /// Returns the immutable command-limit defaults of the selected target.
    #[must_use]
    pub const fn target_defaults(&self) -> CommandLimitAssumptions {
        self.target_defaults
    }
    #[must_use]
    pub const fn limits(&self) -> TargetExecutionAnalysisLimits {
        self.limits
    }
    #[must_use]
    pub const fn completion(&self) -> TargetExecutionAnalysisCompletion {
        self.completion
    }
    #[must_use]
    pub const fn census(&self) -> TargetExecutionCensus {
        self.census
    }
    #[must_use]
    pub const fn stats(&self) -> TargetExecutionAnalysisStats {
        self.stats
    }
    #[must_use]
    pub fn functions(&self) -> &[FunctionLocalSummary] {
        &self.functions
    }
    #[must_use]
    pub fn regions(&self) -> &[CostRegionSummary] {
        &self.regions
    }
    #[must_use]
    pub fn roots(&self) -> &[RootExecutionSummary] {
        &self.roots
    }

    /// Renders a deterministic compact report without solver scratch state.
    #[must_use]
    pub fn dump(&self) -> String {
        let mut output = String::new();
        writeln!(
            output,
            "target-execution-cost {:?} completion={:?} assumptions=sequence:{}/effective-sequence:{}/forks:{} defaults=sequence:{}/forks:{} caps=sequence:{}/forks:{} work=graph:{}/solver:{} functions={} tags={} tag-entries={} top-level-commands={} command-nodes={} execute-stages={} calls={} condition-calls={} score={} data={} say={} returns={} raw={} regions={} roots={} stats=graph-entities:{}/graph-edges:{}/tag-entries:{}/solver-updates:{}/command-transfer-visits:{}/retained-functions:{}/retained-regions:{}/retained-roots:{}",
            self.target,
            self.completion,
            self.assumptions.max_command_sequence_length(),
            self.assumptions
                .effective_max_command_sequence_length(),
            self.assumptions.max_command_forks(),
            self.target_defaults.max_command_sequence_length(),
            self.target_defaults.max_command_forks(),
            self.limits.arithmetic.sequence(),
            self.limits.arithmetic.forks(),
            self.limits.graph_entities,
            self.limits.solver_updates,
            self.census.functions,
            self.census.function_tags,
            self.census.function_tag_entries,
            self.census.top_level_commands,
            self.census.command_nodes,
            self.census.execute_stages,
            self.census.function_calls,
            self.census.function_condition_calls,
            self.census.score_commands,
            self.census.data_commands,
            self.census.say_commands,
            self.census.return_commands,
            self.census.raw_commands,
            self.regions.len(),
            self.roots.len(),
            self.stats.graph_entities,
            self.stats.graph_edges,
            self.stats.tag_expansion_entries,
            self.stats.solver_updates,
            self.stats.command_transfer_visits,
            self.stats.retained_functions,
            self.stats.retained_regions,
            self.stats.retained_roots,
        )
        .unwrap();
        dump_functions(&mut output, &self.functions);
        dump_regions(&mut output, &self.regions);
        for root in &self.roots {
            writeln!(output, "root {root:?}").unwrap();
        }
        output
    }
}

fn dump_functions(output: &mut String, functions: &[FunctionLocalSummary]) {
    for function in functions {
        writeln!(
            output,
            "function {:?} steps={} exits={}",
            function.function(),
            function.steps().len(),
            function.exits().len(),
        )
        .unwrap();
        for (index, step) in function.steps().iter().enumerate() {
            let counts = step.counts();
            writeln!(
                    output,
                    "  step {index} direct=sequence:{}/execute:{}/calls:{}/score-nbt:{} max-chain={:?} outcomes={:?} call-sites={:?} unknown={:?}",
                    counts.sequence_operations(),
                    counts.execute_stages(),
                    counts.internal_function_invocations(),
                    counts.score_nbt_command_executions(),
                    step.maximum_chain_expansion(),
                    step.outcomes(),
                    step.internal_call_sites(),
                    step.intrinsic_unknown_reason(),
                )
                .unwrap();
            for outcome in step.outcome_costs() {
                writeln!(
                        output,
                        "    outcome {:?} sequence={:?} execute={:?} calls={:?} score-nbt={:?} max-chain={:?} unknown={:?}",
                        outcome.outcome(),
                        outcome.sequence_operations(),
                        outcome.execute_stages(),
                        outcome.internal_function_invocations(),
                        outcome.score_nbt_command_executions(),
                        outcome.maximum_chain_expansion(),
                        outcome.unknown_reason(),
                    )
                    .unwrap();
            }
        }
        for exit in function.exits() {
            writeln!(
                    output,
                    "  exit {:?} sequence={:?} execute={:?} calls={:?} score-nbt={:?} max-chain={:?} unknown={:?}",
                    exit.outcome(),
                    exit.sequence_operations(),
                    exit.execute_stages(),
                    exit.internal_function_invocations(),
                    exit.score_nbt_command_executions(),
                    exit.maximum_chain_expansion(),
                    exit.unknown_reason(),
                )
                .unwrap();
        }
    }
}

fn dump_regions(output: &mut String, regions: &[CostRegionSummary]) {
    for region in regions {
        writeln!(
                output,
                "region {:?} functions={:?} cyclic={} sequence={:?} execute={:?} calls={:?} score-nbt={:?} max-chain={:?}",
                region.id,
                region.functions,
                region.cyclic,
                region.sequence_operations,
                region.execute_stages,
                region.internal_function_invocations,
                region.score_nbt_command_executions,
                region.maximum_chain_expansion,
            )
            .unwrap();
    }
}

/// Phase in which checked target analysis could not produce any report.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TargetExecutionAnalysisPhase {
    Configuration,
    Verification,
    Construction,
    Invariant,
}

/// Bounded failure from invalid configuration, required verification, or checked
/// internal analysis construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetExecutionAnalysisFailure {
    phase: TargetExecutionAnalysisPhase,
    diagnostics: Diagnostics,
}

impl TargetExecutionAnalysisFailure {
    pub(super) const fn verification(diagnostics: Diagnostics) -> Self {
        Self {
            phase: TargetExecutionAnalysisPhase::Verification,
            diagnostics,
        }
    }

    pub(crate) fn internal(
        phase: TargetExecutionAnalysisPhase,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        let diagnostics = Diagnostics::from_findings(vec![Diagnostic::new(
            code,
            message,
            crate::source::OriginId::UNKNOWN,
        )])
        .expect("one analysis failure diagnostic is nonempty");
        Self { phase, diagnostics }
    }

    #[must_use]
    pub const fn phase(&self) -> TargetExecutionAnalysisPhase {
        self.phase
    }
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }
}

impl fmt::Display for TargetExecutionAnalysisFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "target execution analysis failed during {:?}: {}",
            self.phase, self.diagnostics
        )
    }
}

impl Error for TargetExecutionAnalysisFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.diagnostics)
    }
}

#[cfg(test)]
mod tests {
    use crate::ir::minecraft::{
        AtMostOneSelector, CommandKind, CommandNode, ExecuteCommand, ExecuteModifier,
        ExecuteModifierKind, ExecuteModifiers, FunctionResourceId, MinecraftProgramBuilder,
        ReturnCommand, SayCommand, SayMessage, UnsafeRawCommand,
    };
    use crate::source::OriginId;
    use crate::target::JavaEditionTarget;

    use super::TargetExecutionCensus;

    #[test]
    fn census_counts_top_level_and_recursive_command_nodes_separately() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:census").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let raw = || {
            CommandNode::new(
                CommandKind::Raw(UnsafeRawCommand::new("say x").unwrap()),
                OriginId::UNKNOWN,
            )
            .unwrap()
        };
        let execute = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(
                        ExecuteModifierKind::As(AtMostOneSelector::SelfExecutor.into()),
                        OriginId::UNKNOWN,
                    ),
                    vec![ExecuteModifier::new(
                        ExecuteModifierKind::At(AtMostOneSelector::NearestPlayer.into()),
                        OriginId::UNKNOWN,
                    )],
                ),
                raw(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let returned = CommandNode::new(
            CommandKind::Return(ReturnCommand::run(raw())),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(execute).unwrap();
        body.push(returned).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Say(SayCommand::new(SayMessage::new("hello").unwrap())),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let census = TargetExecutionCensus::from_program(&program).unwrap();
        assert_eq!(census.functions(), 1);
        assert_eq!(census.top_level_commands(), 3);
        assert_eq!(census.command_nodes(), 5);
        assert_eq!(census.execute_stages(), 2);
        assert_eq!(census.say_commands(), 1);
        assert_eq!(census.return_commands(), 1);
        assert_eq!(census.raw_commands(), 2);
    }
}
