use std::error::Error;
use std::fmt;

use crate::entity::entity_id;
use crate::ir::minecraft::{
    ExternalCallableRef, ExternalTagRequirement, FunctionTagId, McFunctionId,
};
use crate::target::JavaEditionTarget;

entity_id!(
    /// Identity of one deterministic strongly connected execution-cost region.
    pub struct CostRegionId;
);

/// Why understood execution has no proven finite upper bound.
///
/// Declaration order is the deterministic diagnostic precedence used when two
/// conservative reasons meet; it is not a cost ordering.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NoFiniteBoundReason {
    /// A reachable positive-weight cycle has no proven trip bound.
    PositiveCycle,
    /// A selector has no configured finite cardinality bound.
    SelectorCardinality,
}

/// Why target behavior cannot be modeled as a finite or understood unbounded cost.
///
/// Declaration order is the deterministic diagnostic precedence used when two
/// conservative reasons meet; it is not a cost ordering.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum UnknownCostReason {
    /// The command is an explicitly unparsed raw escape hatch.
    RawCommand,
    /// A deployment-provided function may have arbitrary behavior.
    ExternalFunction,
    /// A deployment-provided function tag may have arbitrary entries and behavior.
    ExternalFunctionTag,
    /// An owned tag contains a deployment-provided entry.
    ExternalTagEntry,
    /// A configured deterministic analysis limit was exhausted.
    AnalysisLimit,
}

/// Observable class of one privately constructed count upper bound.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CountUpperKind {
    /// The exact finite upper value.
    Finite(u64),
    /// The result is finite but exact arithmetic stopped above its configured cap.
    AboveAnalysisCap,
    /// Understood behavior has no proven finite bound for the stated reason.
    NoFiniteBoundProven(NoFiniteBoundReason),
    /// Behavior or analysis completion is unknown for the stated reason.
    Unknown(UnknownCostReason),
}

/// A privately constructed upper-bound state.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CountUpper(CountUpperKind);

impl CountUpper {
    /// Returns the closed observable upper-bound class.
    #[must_use]
    pub const fn kind(self) -> CountUpperKind {
        self.0
    }
}

/// A non-inverted lower/upper count range.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CountBound {
    lower: u64,
    upper: CountUpper,
}

impl CountBound {
    /// Constructs a range containing exactly one finite value.
    #[must_use]
    pub const fn exact(value: u64) -> Self {
        Self {
            lower: value,
            upper: CountUpper(CountUpperKind::Finite(value)),
        }
    }

    /// Constructs a finite range after rejecting an inverted pair.
    ///
    /// # Errors
    ///
    /// Returns an error when `lower > upper`.
    pub fn finite(lower: u64, upper: u64) -> Result<Self, CountBoundInvariantError> {
        if lower > upper {
            Err(CountBoundInvariantError::InvertedFiniteRange { lower, upper })
        } else {
            Ok(Self {
                lower,
                upper: CountUpper(CountUpperKind::Finite(upper)),
            })
        }
    }

    pub(super) fn above_analysis_cap(
        lower: u64,
        upper_witness: u64,
        analysis_cap: u64,
    ) -> Result<Self, CountBoundInvariantError> {
        if lower > upper_witness {
            return Err(CountBoundInvariantError::InvertedFiniteRange {
                lower,
                upper: upper_witness,
            });
        }
        if upper_witness <= analysis_cap {
            return Err(CountBoundInvariantError::NotAboveAnalysisCap {
                upper_witness,
                analysis_cap,
            });
        }
        Ok(Self {
            lower,
            upper: CountUpper(CountUpperKind::AboveAnalysisCap),
        })
    }

    /// Constructs a saturated state after checked arithmetic or a merge has already
    /// proved that the exact finite upper lies above the configured cap.
    pub(super) const fn saturated_above_analysis_cap(lower: u64) -> Self {
        Self {
            lower,
            upper: CountUpper(CountUpperKind::AboveAnalysisCap),
        }
    }

    /// Constructs understood behavior with no proven finite upper bound.
    #[must_use]
    pub const fn no_finite_bound(lower: u64, reason: NoFiniteBoundReason) -> Self {
        Self {
            lower,
            upper: CountUpper(CountUpperKind::NoFiniteBoundProven(reason)),
        }
    }

    /// Constructs behavior whose upper cost is unmodeled or incomplete.
    #[must_use]
    pub const fn unknown(lower: u64, reason: UnknownCostReason) -> Self {
        Self {
            lower,
            upper: CountUpper(CountUpperKind::Unknown(reason)),
        }
    }

    /// Returns the proven finite lower bound.
    #[must_use]
    pub const fn lower(self) -> u64 {
        self.lower
    }

    /// Returns the privately validated upper-bound state.
    #[must_use]
    pub const fn upper(self) -> CountUpper {
        self.upper
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountBoundInvariantError {
    /// A finite lower bound exceeded its requested upper bound.
    InvertedFiniteRange { lower: u64, upper: u64 },
    /// A capped state did not prove that it crossed the supplied cap.
    NotAboveAnalysisCap {
        upper_witness: u64,
        analysis_cap: u64,
    },
}

impl fmt::Display for CountBoundInvariantError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvertedFiniteRange { lower, upper } => {
                write!(
                    formatter,
                    "count lower bound {lower} exceeds upper bound {upper}"
                )
            }
            Self::NotAboveAnalysisCap {
                upper_witness,
                analysis_cap,
            } => write!(
                formatter,
                "count upper witness {upper_witness} does not exceed analysis cap {analysis_cap}"
            ),
        }
    }
}

impl Error for CountBoundInvariantError {}

/// Assumed configured server gamerules used only for analysis and recipe safety checks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommandLimitAssumptions {
    max_command_sequence_length: u32,
    max_command_forks: u32,
}

impl CommandLimitAssumptions {
    /// Largest configured integer value accepted by the supported Java target.
    pub const MAX_CONFIGURED_VALUE: u32 = i32::MAX as u32;

    /// Creates explicit configured gamerule assumptions after enforcing the
    /// supported Java target's integer value domain.
    ///
    /// # Errors
    ///
    /// Both values may be zero and must be at most [`Self::MAX_CONFIGURED_VALUE`].
    pub const fn new(
        max_command_sequence_length: u32,
        max_command_forks: u32,
    ) -> Result<Self, CommandLimitAssumptionsError> {
        if max_command_sequence_length > Self::MAX_CONFIGURED_VALUE {
            return Err(
                CommandLimitAssumptionsError::MaxCommandSequenceLengthAboveMaximum {
                    value: max_command_sequence_length,
                    maximum: Self::MAX_CONFIGURED_VALUE,
                },
            );
        }
        if max_command_forks > Self::MAX_CONFIGURED_VALUE {
            return Err(CommandLimitAssumptionsError::MaxCommandForksAboveMaximum {
                value: max_command_forks,
                maximum: Self::MAX_CONFIGURED_VALUE,
            });
        }
        Ok(Self {
            max_command_sequence_length,
            max_command_forks,
        })
    }

    /// Uses the selected target's default gamerule values exactly.
    ///
    /// # Panics
    ///
    /// Panics only when an internal target specification violates the same public
    /// configured-value invariant. Every target specification is covered by tests.
    #[must_use]
    pub fn for_target(target: JavaEditionTarget) -> Self {
        let spec = target.spec();
        Self::new(
            spec.default_max_command_sequence(),
            spec.default_max_command_forks(),
        )
        .expect("target command-limit defaults must satisfy the supported gamerule domain")
    }

    /// Returns the assumed configured command-sequence gamerule value.
    #[must_use]
    pub const fn max_command_sequence_length(self) -> u32 {
        self.max_command_sequence_length
    }

    /// Returns the effective sequence quota used by Java command execution.
    ///
    /// Java 26.2 accepts a configured zero but executes with a minimum quota of one.
    #[must_use]
    pub const fn effective_max_command_sequence_length(self) -> u32 {
        if self.max_command_sequence_length == 0 {
            1
        } else {
            self.max_command_sequence_length
        }
    }

    /// Returns the assumed configured command-forks gamerule value.
    #[must_use]
    pub const fn max_command_forks(self) -> u32 {
        self.max_command_forks
    }
}

/// Invalid assumed values for the command-limit gamerules of supported targets.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CommandLimitAssumptionsError {
    /// The configured sequence value exceeds Java's signed-integer gamerule domain.
    MaxCommandSequenceLengthAboveMaximum { value: u32, maximum: u32 },
    /// The configured fork value exceeds Java's signed-integer gamerule domain.
    MaxCommandForksAboveMaximum { value: u32, maximum: u32 },
}

impl fmt::Display for CommandLimitAssumptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxCommandSequenceLengthAboveMaximum { value, maximum } => write!(
                formatter,
                "max command sequence length {value} exceeds target maximum {maximum}"
            ),
            Self::MaxCommandForksAboveMaximum { value, maximum } => write!(
                formatter,
                "max command forks {value} exceeds target maximum {maximum}"
            ),
        }
    }
}

impl Error for CommandLimitAssumptionsError {}

/// Arithmetic caps proven high enough to preserve both hard-limit comparisons.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AnalysisArithmeticCaps {
    sequence: u64,
    forks: u64,
}

impl AnalysisArithmeticCaps {
    /// Validates explicit arithmetic caps against their corresponding assumptions.
    ///
    /// # Errors
    ///
    /// Each cap must be at least one greater than its corresponding configured
    /// gamerule value.
    pub fn new(
        assumptions: CommandLimitAssumptions,
        sequence: u64,
        forks: u64,
    ) -> Result<Self, AnalysisArithmeticCapsError> {
        let minimum_sequence = u64::from(assumptions.max_command_sequence_length()) + 1;
        let minimum_forks = u64::from(assumptions.max_command_forks()) + 1;
        if sequence < minimum_sequence {
            return Err(AnalysisArithmeticCapsError::SequenceTooSmall {
                cap: sequence,
                minimum: minimum_sequence,
            });
        }
        if forks < minimum_forks {
            return Err(AnalysisArithmeticCapsError::ForksTooSmall {
                cap: forks,
                minimum: minimum_forks,
            });
        }
        Ok(Self { sequence, forks })
    }

    /// Selects the smallest caps that still preserve hard-limit comparisons.
    #[must_use]
    pub fn minimum_for(assumptions: CommandLimitAssumptions) -> Self {
        Self {
            sequence: u64::from(assumptions.max_command_sequence_length()) + 1,
            forks: u64::from(assumptions.max_command_forks()) + 1,
        }
    }

    /// Returns the sequence arithmetic cap.
    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    /// Returns the ordinary checked-redirect fork arithmetic cap.
    #[must_use]
    pub const fn forks(self) -> u64 {
        self.forks
    }

    /// Constructs a sequence bound proven to have crossed this validated cap.
    ///
    /// # Errors
    ///
    /// Returns an error unless `lower` is greater than the sequence cap.
    pub fn sequence_bound_above_cap(
        self,
        lower: u64,
    ) -> Result<CountBound, CountBoundInvariantError> {
        CountBound::above_analysis_cap(lower, lower, self.sequence)
    }

    /// Constructs a fork bound proven to have crossed this validated cap.
    ///
    /// # Errors
    ///
    /// Returns an error unless `lower` is greater than the fork cap.
    pub fn fork_bound_above_cap(self, lower: u64) -> Result<CountBound, CountBoundInvariantError> {
        CountBound::above_analysis_cap(lower, lower, self.forks)
    }
}

/// An inconsistent requested arithmetic cap.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AnalysisArithmeticCapsError {
    /// Sequence arithmetic could stop before distinguishing a hard-limit crossing.
    SequenceTooSmall { cap: u64, minimum: u64 },
    /// Fork arithmetic could stop before distinguishing a hard-limit crossing.
    ForksTooSmall { cap: u64, minimum: u64 },
}

impl fmt::Display for AnalysisArithmeticCapsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SequenceTooSmall { cap, minimum } => write!(
                formatter,
                "sequence arithmetic cap {cap} is smaller than required minimum {minimum}"
            ),
            Self::ForksTooSmall { cap, minimum } => write!(
                formatter,
                "fork arithmetic cap {cap} is smaller than required minimum {minimum}"
            ),
        }
    }
}

impl Error for AnalysisArithmeticCapsError {}

/// Exact local target operations, kept as independent dimensions.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct CommandStepCounts {
    sequence_operations: u64,
    execute_stages: u64,
    internal_function_invocations: u64,
    score_nbt_command_executions: u64,
}

impl CommandStepCounts {
    pub(crate) const fn new(
        sequence_operations: u64,
        execute_stages: u64,
        internal_function_invocations: u64,
        score_nbt_command_executions: u64,
    ) -> Self {
        Self {
            sequence_operations,
            execute_stages,
            internal_function_invocations,
            score_nbt_command_executions,
        }
    }

    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self::new(
            self.sequence_operations
                .checked_add(other.sequence_operations)?,
            self.execute_stages.checked_add(other.execute_stages)?,
            self.internal_function_invocations
                .checked_add(other.internal_function_invocations)?,
            self.score_nbt_command_executions
                .checked_add(other.score_nbt_command_executions)?,
        ))
    }

    /// Returns local command-sequence operations without recursively expanded callees.
    #[must_use]
    pub const fn sequence_operations(self) -> u64 {
        self.sequence_operations
    }

    /// Returns local execute stages.
    #[must_use]
    pub const fn execute_stages(self) -> u64 {
        self.execute_stages
    }

    /// Returns local owned-function invocations.
    #[must_use]
    pub const fn internal_function_invocations(self) -> u64 {
        self.internal_function_invocations
    }

    /// Returns local scoreboard and command-storage command executions.
    #[must_use]
    pub const fn score_nbt_command_executions(self) -> u64 {
        self.score_nbt_command_executions
    }
}

/// Coarse integer result class needed by function conditions and `return run`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ReturnValueClass {
    Zero,
    NonZero,
    UnknownInteger,
}

/// One semantically distinct command/function completion outcome.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommandOutcome {
    Continue,
    Return(ReturnValueClass),
    NoResult,
    Fail,
}

/// Role of an owned callable reference within one structured command step.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InternalCallRole {
    /// A direct `function` command whose result does not control enclosing syntax.
    Ordinary,
    /// A call nested under result-propagating `return run` syntax.
    ///
    /// This describes source structure and is not a compiler tail-call claim.
    ReturnRun,
    /// A function invoked to evaluate an `execute if|unless function` predicate.
    Condition,
}

/// One exact typed structured owned-call descriptor.
///
/// Each occurrence in `CommandStepCost::internal_call_sites` is one syntactic site;
/// equal descriptor values do not identify or collapse those occurrences. Raw text
/// and external references are barriers rather than fabricated owned sites. This is
/// structural inventory, not a claim about runtime execution frequency.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum InternalCallSite {
    Function {
        target: McFunctionId,
        role: InternalCallRole,
    },
    Tag {
        target: FunctionTagId,
        role: InternalCallRole,
    },
}

/// Local classification of one physical top-level command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandStepCost {
    counts: CommandStepCounts,
    maximum_chain_expansion: CountBound,
    outcomes: Box<[CommandOutcome]>,
    internal_call_sites: Box<[InternalCallSite]>,
    unknown: Option<UnknownCostReason>,
    outcome_costs: Box<[FunctionOutcomeCost]>,
}

impl CommandStepCost {
    pub(super) fn new(
        counts: CommandStepCounts,
        maximum_chain_expansion: CountBound,
        outcomes: Vec<CommandOutcome>,
        internal_call_sites: Vec<InternalCallSite>,
        unknown: Option<UnknownCostReason>,
    ) -> Self {
        let mut outcomes = outcomes;
        outcomes.sort_unstable();
        outcomes.dedup();
        Self {
            counts,
            maximum_chain_expansion,
            outcomes: outcomes.into_boxed_slice(),
            internal_call_sites: internal_call_sites.into_boxed_slice(),
            unknown,
            outcome_costs: Box::new([]),
        }
    }

    /// Returns direct single-context syntax weights without recursively expanded
    /// callee bodies. `outcome_costs` supplies the corresponding conservative local
    /// transfer bounds once analysis runs.
    #[must_use]
    pub const fn counts(&self) -> CommandStepCounts {
        self.counts
    }
    /// Returns the maximum expansion at an ordinary fork-checked execute redirect
    /// reachable in this step. Custom function-condition modifiers are excluded.
    #[must_use]
    pub const fn maximum_chain_expansion(&self) -> CountBound {
        self.maximum_chain_expansion
    }
    /// Returns conservative syntax-local completion classes in stable semantic order.
    #[must_use]
    pub fn outcomes(&self) -> &[CommandOutcome] {
        &self.outcomes
    }
    /// Returns typed owned-call sites in structured command order.
    ///
    /// The number of returned sites is not a runtime invocation count.
    #[must_use]
    pub fn internal_call_sites(&self) -> &[InternalCallSite] {
        &self.internal_call_sites
    }
    /// Returns the stable syntax-intrinsic reason this step cannot be modeled.
    ///
    /// Analysis-limit fallback is reported on authoritative outcome metrics and does
    /// not rewrite this structural classification.
    #[must_use]
    pub const fn intrinsic_unknown_reason(&self) -> Option<UnknownCostReason> {
        self.unknown
    }

    /// Returns outcome-partitioned local execution ranges for this step after
    /// context multiplication, excluding recursively expanded callee bodies.
    /// Unknown callees retain every locally possible result/no-result class rather
    /// than borrowing whole-program solved outcomes.
    #[must_use]
    pub fn outcome_costs(&self) -> &[FunctionOutcomeCost] {
        &self.outcome_costs
    }

    pub(super) fn replace_outcome_costs(&mut self, outcomes: Vec<FunctionOutcomeCost>) {
        self.outcome_costs = outcomes.into_boxed_slice();
    }
}

/// Outcome-partitioned command-step or function-exit cost.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionOutcomeCost {
    outcome: CommandOutcome,
    sequence_operations: CountBound,
    execute_stages: CountBound,
    internal_function_invocations: CountBound,
    score_nbt_command_executions: CountBound,
    maximum_chain_expansion: CountBound,
    unknown: Option<UnknownCostReason>,
}

#[derive(Clone, Copy)]
pub(super) struct FunctionOutcomeMetrics {
    pub(super) sequence_operations: CountBound,
    pub(super) execute_stages: CountBound,
    pub(super) internal_function_invocations: CountBound,
    pub(super) score_nbt_command_executions: CountBound,
    pub(super) maximum_chain_expansion: CountBound,
}

impl FunctionOutcomeCost {
    pub(super) fn new(
        outcome: CommandOutcome,
        metrics: FunctionOutcomeMetrics,
        unknown: Option<UnknownCostReason>,
    ) -> Self {
        Self {
            outcome,
            sequence_operations: metrics.sequence_operations,
            execute_stages: metrics.execute_stages,
            internal_function_invocations: metrics.internal_function_invocations,
            score_nbt_command_executions: metrics.score_nbt_command_executions,
            maximum_chain_expansion: metrics.maximum_chain_expansion,
            unknown,
        }
    }

    #[must_use]
    pub const fn outcome(&self) -> CommandOutcome {
        self.outcome
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
    /// Returns the maximum ordinary fork-checked redirect expansion for this outcome.
    #[must_use]
    pub const fn maximum_chain_expansion(&self) -> CountBound {
        self.maximum_chain_expansion
    }
    #[must_use]
    pub const fn unknown_reason(&self) -> Option<UnknownCostReason> {
        self.unknown
    }
}

/// Ordered local command facts for one function, excluding expanded callee bodies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionLocalSummary {
    function: McFunctionId,
    steps: Box<[CommandStepCost]>,
    exits: Box<[FunctionOutcomeCost]>,
}

impl FunctionLocalSummary {
    pub(super) fn new(
        function: McFunctionId,
        steps: Vec<CommandStepCost>,
        exits: Vec<FunctionOutcomeCost>,
    ) -> Self {
        Self {
            function,
            steps: steps.into_boxed_slice(),
            exits: exits.into_boxed_slice(),
        }
    }

    #[must_use]
    pub const fn function(&self) -> McFunctionId {
        self.function
    }
    #[must_use]
    pub fn steps(&self) -> &[CommandStepCost] {
        &self.steps
    }
    #[must_use]
    pub fn exits(&self) -> &[FunctionOutcomeCost] {
        &self.exits
    }

    pub(super) fn replace_exits(&mut self, exits: Vec<FunctionOutcomeCost>) {
        self.exits = exits.into_boxed_slice();
    }

    pub(super) fn steps_mut(&mut self) -> &mut [CommandStepCost] {
        &mut self.steps
    }

    pub(super) fn mark_analysis_limit(&mut self) {
        for step in &mut self.steps {
            let outcomes = step
                .outcomes
                .iter()
                .copied()
                .map(|outcome| {
                    let unknown = CountBound::unknown(0, UnknownCostReason::AnalysisLimit);
                    FunctionOutcomeCost::new(
                        outcome,
                        FunctionOutcomeMetrics {
                            sequence_operations: unknown,
                            execute_stages: unknown,
                            internal_function_invocations: unknown,
                            score_nbt_command_executions: unknown,
                            maximum_chain_expansion: unknown,
                        },
                        Some(UnknownCostReason::AnalysisLimit),
                    )
                })
                .collect::<Vec<_>>();
            step.replace_outcome_costs(outcomes);
        }
        let mut outcomes = vec![];
        let mut active = true;
        for step in &self.steps {
            if !active {
                break;
            }
            active = false;
            for outcome in &step.outcomes {
                match outcome {
                    CommandOutcome::Continue | CommandOutcome::NoResult => active = true,
                    CommandOutcome::Return(_) | CommandOutcome::Fail => outcomes.push(*outcome),
                }
            }
        }
        if active {
            outcomes.push(CommandOutcome::NoResult);
        }
        outcomes.sort_unstable();
        outcomes.dedup();
        let unknown = CountBound::unknown(0, UnknownCostReason::AnalysisLimit);
        self.exits = outcomes
            .into_iter()
            .map(|outcome| {
                FunctionOutcomeCost::new(
                    outcome,
                    FunctionOutcomeMetrics {
                        sequence_operations: unknown,
                        execute_stages: unknown,
                        internal_function_invocations: unknown,
                        score_nbt_command_executions: unknown,
                        maximum_chain_expansion: unknown,
                    },
                    Some(UnknownCostReason::AnalysisLimit),
                )
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
    }
}

/// One supported external entry into the generated target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TargetExecutionRoot {
    Function(McFunctionId),
    FunctionTag(FunctionTagId),
}

/// One actual root sequence produced after resolving a requested entry.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ResolvedTargetExecutionRoot {
    Function(McFunctionId),
    FunctionTagFunction {
        tag: FunctionTagId,
        entry_index: u32,
        function: McFunctionId,
    },
    UnresolvedExternalTagEntry {
        tag: FunctionTagId,
        entry_index: u32,
        target: ExternalCallableRef,
        requirement: ExternalTagRequirement,
    },
    UnresolvedAnalysisLimitFunctionTag {
        tag: FunctionTagId,
    },
}

/// Internal region identity when one exists for a resolved root sequence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RootEntryRegion {
    Internal(CostRegionId),
    ExternalTagEntry,
    AnalysisLimit,
}

/// Conservative comparison of one root metric with its configured hard limit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CommandLimitStatus {
    /// The proven finite upper satisfies the metric's target-specific limit rule.
    ProvenWithin,
    /// The retained bounds prove neither compliance nor violation. A conservative
    /// upper bound need not describe an actually attainable execution.
    MayExceed,
    /// The proven lower bound reaches the metric's first rejected count.
    ProvenExceeds,
    /// Understood behavior has no proven finite upper bound.
    NoFiniteBoundProven(NoFiniteBoundReason),
    /// Behavior or analysis completion is unknown.
    Unknown(UnknownCostReason),
}

impl CommandLimitStatus {
    fn compare_first_rejected(bound: CountBound, first_rejected: u64) -> Self {
        if bound.lower() >= first_rejected {
            return Self::ProvenExceeds;
        }
        match bound.upper().kind() {
            CountUpperKind::Finite(upper) if upper < first_rejected => Self::ProvenWithin,
            CountUpperKind::Finite(_) | CountUpperKind::AboveAnalysisCap => Self::MayExceed,
            CountUpperKind::NoFiniteBoundProven(reason) => Self::NoFiniteBoundProven(reason),
            CountUpperKind::Unknown(reason) => Self::Unknown(reason),
        }
    }

    fn compare_sequence(bound: CountBound, configured_limit: u32) -> Self {
        let effective_limit = configured_limit.max(1);
        let first_rejected = u64::from(effective_limit) + 1;
        Self::compare_first_rejected(bound, first_rejected)
    }

    fn compare_forks(bound: CountBound, execute_stages: CountBound, configured_limit: u32) -> Self {
        // At configured zero, a direct command with no execute stage still runs. A
        // reached execute redirect producing zero contexts can nevertheless hit the
        // fork guard, so expansion=0 alone is insufficient evidence of compliance.
        if configured_limit == 0
            && bound.upper().kind() == CountUpperKind::Finite(0)
            && execute_stages.upper().kind() != CountUpperKind::Finite(0)
        {
            return Self::MayExceed;
        }
        let first_rejected_positive_expansion = u64::from(configured_limit.max(1));
        Self::compare_first_rejected(bound, first_rejected_positive_expansion)
    }
}

/// Conservative whole-invocation cost ranges for one supported root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootExecutionSummary {
    root: ResolvedTargetExecutionRoot,
    entry_region: RootEntryRegion,
    sequence_operations: CountBound,
    execute_stages: CountBound,
    internal_function_invocations: CountBound,
    score_nbt_command_executions: CountBound,
    maximum_chain_expansion: CountBound,
    sequence_limit_status: CommandLimitStatus,
    fork_limit_status: CommandLimitStatus,
}

#[derive(Clone, Copy)]
pub(super) struct RootExecutionBounds {
    pub(super) sequence_operations: CountBound,
    pub(super) execute_stages: CountBound,
    pub(super) internal_function_invocations: CountBound,
    pub(super) score_nbt_command_executions: CountBound,
    pub(super) maximum_chain_expansion: CountBound,
}

impl RootExecutionBounds {
    pub(super) const fn new(
        sequence_operations: CountBound,
        execute_stages: CountBound,
        internal_function_invocations: CountBound,
        score_nbt_command_executions: CountBound,
        maximum_chain_expansion: CountBound,
    ) -> Self {
        Self {
            sequence_operations,
            execute_stages,
            internal_function_invocations,
            score_nbt_command_executions,
            maximum_chain_expansion,
        }
    }

    pub(super) const fn repeated(bound: CountBound) -> Self {
        Self::new(bound, bound, bound, bound, bound)
    }
}

impl RootExecutionSummary {
    pub(super) fn new(
        root: ResolvedTargetExecutionRoot,
        entry_region: RootEntryRegion,
        bounds: RootExecutionBounds,
        assumptions: CommandLimitAssumptions,
    ) -> Self {
        let sequence_limit_status = CommandLimitStatus::compare_sequence(
            bounds.sequence_operations,
            assumptions.max_command_sequence_length(),
        );
        let fork_limit_status = CommandLimitStatus::compare_forks(
            bounds.maximum_chain_expansion,
            bounds.execute_stages,
            assumptions.max_command_forks(),
        );
        Self {
            root,
            entry_region,
            sequence_operations: bounds.sequence_operations,
            execute_stages: bounds.execute_stages,
            internal_function_invocations: bounds.internal_function_invocations,
            score_nbt_command_executions: bounds.score_nbt_command_executions,
            maximum_chain_expansion: bounds.maximum_chain_expansion,
            sequence_limit_status,
            fork_limit_status,
        }
    }

    #[must_use]
    pub const fn root(&self) -> &ResolvedTargetExecutionRoot {
        &self.root
    }
    #[must_use]
    pub const fn entry_region(&self) -> RootEntryRegion {
        self.entry_region
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
    /// Returns the maximum ordinary fork-checked redirect expansion for this root.
    #[must_use]
    pub const fn maximum_chain_expansion(&self) -> CountBound {
        self.maximum_chain_expansion
    }
    /// Compares total root command-sequence work with Java's effective sequence quota.
    /// A configured zero has an effective quota of one.
    #[must_use]
    pub const fn sequence_limit_status(&self) -> CommandLimitStatus {
        self.sequence_limit_status
    }
    /// Compares maximum ordinary fork-checked redirect expansion with the configured
    /// fork rule. This never sums forks across commands or loop iterations. At
    /// configured zero, exact zero is within only when execute-stage cost also proves
    /// that no execute stage is reached; a possible checked zero-output redirect is
    /// not proven within.
    #[must_use]
    pub const fn fork_limit_status(&self) -> CommandLimitStatus {
        self.fork_limit_status
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityId;

    use super::*;

    #[test]
    fn count_bounds_reject_inverted_and_false_capped_states() {
        assert_eq!(
            CountBound::exact(4).upper().kind(),
            CountUpperKind::Finite(4)
        );
        assert!(matches!(
            CountBound::finite(5, 4),
            Err(CountBoundInvariantError::InvertedFiniteRange { lower: 5, upper: 4 })
        ));
        assert!(matches!(
            AnalysisArithmeticCaps::new(CommandLimitAssumptions::new(1, 1).unwrap(), 10, 10)
                .unwrap()
                .sequence_bound_above_cap(10),
            Err(CountBoundInvariantError::NotAboveAnalysisCap { .. })
        ));
        assert_eq!(
            AnalysisArithmeticCaps::new(CommandLimitAssumptions::new(1, 1).unwrap(), 10, 10)
                .unwrap()
                .sequence_bound_above_cap(11)
                .unwrap()
                .upper()
                .kind(),
            CountUpperKind::AboveAnalysisCap
        );
        assert_eq!(
            CountBound::no_finite_bound(2, NoFiniteBoundReason::PositiveCycle)
                .upper()
                .kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
        );
        assert_eq!(
            CountBound::unknown(0, UnknownCostReason::RawCommand)
                .upper()
                .kind(),
            CountUpperKind::Unknown(UnknownCostReason::RawCommand)
        );
    }

    #[test]
    fn arithmetic_caps_must_preserve_limit_comparisons() {
        let assumptions = CommandLimitAssumptions::new(65_536, 4).unwrap();
        assert_eq!(
            AnalysisArithmeticCaps::minimum_for(assumptions).sequence(),
            65_537
        );
        assert!(matches!(
            AnalysisArithmeticCaps::new(assumptions, 65_536, 5),
            Err(AnalysisArithmeticCapsError::SequenceTooSmall { .. })
        ));
        assert!(matches!(
            AnalysisArithmeticCaps::new(assumptions, 65_537, 4),
            Err(AnalysisArithmeticCapsError::ForksTooSmall { .. })
        ));
        assert!(AnalysisArithmeticCaps::new(assumptions, 65_537, 5).is_ok());
    }

    #[test]
    fn command_limit_assumptions_match_the_java_26_2_gamerule_domain() {
        let maximum = CommandLimitAssumptions::MAX_CONFIGURED_VALUE;
        assert_eq!(
            CommandLimitAssumptions::new(0, 0).unwrap(),
            CommandLimitAssumptions {
                max_command_sequence_length: 0,
                max_command_forks: 0,
            }
        );
        assert_eq!(
            CommandLimitAssumptions::new(maximum, maximum).unwrap(),
            CommandLimitAssumptions {
                max_command_sequence_length: maximum,
                max_command_forks: maximum,
            }
        );
        assert_eq!(
            CommandLimitAssumptions::new(maximum + 1, 0),
            Err(
                CommandLimitAssumptionsError::MaxCommandSequenceLengthAboveMaximum {
                    value: maximum + 1,
                    maximum,
                }
            )
        );
        assert_eq!(
            CommandLimitAssumptions::new(0, maximum + 1),
            Err(CommandLimitAssumptionsError::MaxCommandForksAboveMaximum {
                value: maximum + 1,
                maximum,
            })
        );
        assert_eq!(
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2),
            CommandLimitAssumptions::new(65_536, 65_536).unwrap()
        );
        assert_eq!(
            CommandLimitAssumptions::new(0, 0)
                .unwrap()
                .effective_max_command_sequence_length(),
            1
        );
        assert_eq!(
            CommandLimitAssumptions::new(1, 0)
                .unwrap()
                .effective_max_command_sequence_length(),
            1
        );
        assert_eq!(
            CommandLimitAssumptions::new(2, 0)
                .unwrap()
                .effective_max_command_sequence_length(),
            2
        );
    }

    #[test]
    fn arithmetic_caps_cover_zero_and_maximum_configured_limits() {
        let zero = CommandLimitAssumptions::new(0, 0).unwrap();
        assert_eq!(AnalysisArithmeticCaps::minimum_for(zero).sequence(), 1);
        assert_eq!(AnalysisArithmeticCaps::minimum_for(zero).forks(), 1);
        assert!(matches!(
            AnalysisArithmeticCaps::new(zero, 0, 1),
            Err(AnalysisArithmeticCapsError::SequenceTooSmall { minimum: 1, .. })
        ));
        assert!(matches!(
            AnalysisArithmeticCaps::new(zero, 1, 0),
            Err(AnalysisArithmeticCapsError::ForksTooSmall { minimum: 1, .. })
        ));
        assert!(AnalysisArithmeticCaps::new(zero, 1, 1).is_ok());

        let maximum = CommandLimitAssumptions::new(
            CommandLimitAssumptions::MAX_CONFIGURED_VALUE,
            CommandLimitAssumptions::MAX_CONFIGURED_VALUE,
        )
        .unwrap();
        let first_unconfigured = u64::from(CommandLimitAssumptions::MAX_CONFIGURED_VALUE) + 1;
        assert_eq!(
            AnalysisArithmeticCaps::minimum_for(maximum).sequence(),
            first_unconfigured
        );
        assert_eq!(
            AnalysisArithmeticCaps::minimum_for(maximum).forks(),
            first_unconfigured
        );
    }

    #[test]
    fn root_limit_status_uses_target_specific_boundaries() {
        let assumptions = CommandLimitAssumptions::new(10, 4).unwrap();
        let summary = |sequence, forks| {
            RootExecutionSummary::new(
                ResolvedTargetExecutionRoot::Function(McFunctionId::from_index(0)),
                RootEntryRegion::AnalysisLimit,
                RootExecutionBounds::new(
                    sequence,
                    CountBound::exact(0),
                    CountBound::exact(0),
                    CountBound::exact(0),
                    forks,
                ),
                assumptions,
            )
        };

        let within = summary(CountBound::exact(10), CountBound::exact(3));
        assert_eq!(
            within.sequence_limit_status(),
            CommandLimitStatus::ProvenWithin
        );
        assert_eq!(within.fork_limit_status(), CommandLimitStatus::ProvenWithin);

        let maybe = summary(
            CountBound::finite(9, 11).unwrap(),
            CountBound::finite(3, 4).unwrap(),
        );
        assert_eq!(maybe.sequence_limit_status(), CommandLimitStatus::MayExceed);
        assert_eq!(maybe.fork_limit_status(), CommandLimitStatus::MayExceed);

        let exceeds = summary(CountBound::exact(11), CountBound::exact(4));
        assert_eq!(
            exceeds.sequence_limit_status(),
            CommandLimitStatus::ProvenExceeds
        );
        assert_eq!(
            exceeds.fork_limit_status(),
            CommandLimitStatus::ProvenExceeds
        );

        let unbounded = summary(
            CountBound::no_finite_bound(0, NoFiniteBoundReason::PositiveCycle),
            CountBound::no_finite_bound(0, NoFiniteBoundReason::SelectorCardinality),
        );
        assert_eq!(
            unbounded.sequence_limit_status(),
            CommandLimitStatus::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
        );
        assert_eq!(
            unbounded.fork_limit_status(),
            CommandLimitStatus::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality)
        );

        let unknown = summary(
            CountBound::unknown(0, UnknownCostReason::RawCommand),
            CountBound::unknown(0, UnknownCostReason::AnalysisLimit),
        );
        assert_eq!(
            unknown.sequence_limit_status(),
            CommandLimitStatus::Unknown(UnknownCostReason::RawCommand)
        );
        assert_eq!(
            unknown.fork_limit_status(),
            CommandLimitStatus::Unknown(UnknownCostReason::AnalysisLimit)
        );

        assert_eq!(
            CommandLimitStatus::compare_sequence(CountBound::exact(4), 4),
            CommandLimitStatus::ProvenWithin
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(CountBound::exact(4), CountBound::exact(1), 4,),
            CommandLimitStatus::ProvenExceeds
        );
    }

    #[test]
    fn root_limit_status_handles_zero_and_largest_configured_values() {
        assert_eq!(
            CommandLimitStatus::compare_sequence(CountBound::exact(1), 0),
            CommandLimitStatus::ProvenWithin
        );
        assert_eq!(
            CommandLimitStatus::compare_sequence(CountBound::finite(1, 2).unwrap(), 0),
            CommandLimitStatus::MayExceed
        );
        assert_eq!(
            CommandLimitStatus::compare_sequence(CountBound::exact(2), 0),
            CommandLimitStatus::ProvenExceeds
        );

        assert_eq!(
            CommandLimitStatus::compare_forks(CountBound::exact(0), CountBound::exact(0), 0,),
            CommandLimitStatus::ProvenWithin
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(
                CountBound::finite(0, 1).unwrap(),
                CountBound::exact(1),
                0,
            ),
            CommandLimitStatus::MayExceed
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(CountBound::exact(1), CountBound::exact(1), 0,),
            CommandLimitStatus::ProvenExceeds
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(CountBound::exact(0), CountBound::exact(1), 0,),
            CommandLimitStatus::MayExceed
        );

        let maximum_configured = CommandLimitAssumptions::MAX_CONFIGURED_VALUE;
        let maximum = u64::from(maximum_configured);
        assert_eq!(
            CommandLimitStatus::compare_sequence(CountBound::exact(maximum), maximum_configured),
            CommandLimitStatus::ProvenWithin
        );
        assert_eq!(
            CommandLimitStatus::compare_sequence(
                CountBound::exact(maximum + 1),
                maximum_configured,
            ),
            CommandLimitStatus::ProvenExceeds
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(
                CountBound::exact(maximum - 1),
                CountBound::exact(1),
                maximum_configured,
            ),
            CommandLimitStatus::ProvenWithin
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(
                CountBound::exact(maximum),
                CountBound::exact(1),
                maximum_configured,
            ),
            CommandLimitStatus::ProvenExceeds
        );
    }

    #[test]
    fn crossing_lower_bounds_take_precedence_over_incomplete_uppers() {
        let capped = CountBound::above_analysis_cap(0, 12, 11).unwrap();
        assert_eq!(
            CommandLimitStatus::compare_sequence(capped, 10),
            CommandLimitStatus::MayExceed
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(capped, CountBound::exact(1), 4),
            CommandLimitStatus::MayExceed
        );

        assert_eq!(
            CommandLimitStatus::compare_sequence(
                CountBound::no_finite_bound(11, NoFiniteBoundReason::PositiveCycle),
                10,
            ),
            CommandLimitStatus::ProvenExceeds
        );
        assert_eq!(
            CommandLimitStatus::compare_sequence(
                CountBound::unknown(11, UnknownCostReason::RawCommand),
                10,
            ),
            CommandLimitStatus::ProvenExceeds
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(
                CountBound::no_finite_bound(4, NoFiniteBoundReason::SelectorCardinality),
                CountBound::exact(1),
                4,
            ),
            CommandLimitStatus::ProvenExceeds
        );
        assert_eq!(
            CommandLimitStatus::compare_forks(
                CountBound::unknown(4, UnknownCostReason::AnalysisLimit),
                CountBound::exact(1),
                4,
            ),
            CommandLimitStatus::ProvenExceeds
        );
    }
}
