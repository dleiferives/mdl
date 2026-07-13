use std::error::Error;
use std::fmt;

use crate::analysis::minecraft::{CommandStepCounts, CountBound, CountUpperKind};

use super::analysis::BranchArm;

/// Closed target-control recipe identity.
///
/// Declaration order is the final deterministic tie-break order. It must not
/// resolve a real cost trade-off.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum ControlRecipeKind {
    ReturnDispatcher,
    InlineZeroAbiTerminalCall,
}

/// Whether the score guard selected the nested command on one concrete path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum GuardOutcome {
    Rejected,
    Selected,
}

/// Closed physical command fragments understood by Stage 5G accounting.
///
/// These are path-sensitive views of target syntax, not a second target IR.
/// The emitter still constructs and verifies ordinary structured Minecraft IR.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ControlRecipeFragment {
    ConditionalReturnRunInternalFunction(GuardOutcome),
    ReturnRunInternalFunction,
    InternalFunctionCall,
    ReturnNonZero,
}

/// Why a local candidate is known not to worsen any root metric or bound class.
///
/// The selector constructs these certificates only after its independent graph
/// and legality checks. Numeric comparison never tries to infer legality.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum WholeGraphProofCertificate {
    BaselineIdentity,
    UniqueTerminalArmContraction,
}

/// Conservative effect of one recipe replacement on whole-graph cost classes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum WholeGraphImpact {
    NonIncreasing(WholeGraphProofCertificate),
    #[allow(
        dead_code,
        reason = "the closed conservative state is exercised by accounting tests and retained for future recipe certificates"
    )]
    Unproven,
}

impl WholeGraphImpact {
    pub(crate) const fn baseline_identity() -> Self {
        Self::NonIncreasing(WholeGraphProofCertificate::BaselineIdentity)
    }

    pub(crate) const fn unique_terminal_arm_contraction() -> Self {
        Self::NonIncreasing(WholeGraphProofCertificate::UniqueTerminalArmContraction)
    }

    #[cfg(test)]
    pub(crate) const fn unproven() -> Self {
        Self::Unproven
    }

    pub(crate) const fn proof(self) -> Option<WholeGraphProofCertificate> {
        match self {
            Self::NonIncreasing(proof) => Some(proof),
            Self::Unproven => None,
        }
    }
}

/// Exact understood runtime work on one semantic branch path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RecipePathCost {
    counts: CommandStepCounts,
    maximum_chain_expansion: CountBound,
}

impl RecipePathCost {
    const ZERO: Self = Self {
        counts: CommandStepCounts::new(0, 0, 0, 0),
        maximum_chain_expansion: CountBound::exact(0),
    };

    fn from_fragments(fragments: &[ControlRecipeFragment]) -> Result<Self, RecipeCostError> {
        fragments
            .iter()
            .copied()
            .try_fold(Self::ZERO, Self::checked_append)
    }

    fn checked_append(self, fragment: ControlRecipeFragment) -> Result<Self, RecipeCostError> {
        self.checked_add(fragment.path_cost())
    }

    fn checked_add(self, other: Self) -> Result<Self, RecipeCostError> {
        let counts =
            self.counts
                .checked_add(other.counts)
                .ok_or(RecipeCostError::ArithmeticOverflow(
                    RecipeCostDimension::PathCommandCounts,
                ))?;
        let left = exact_maximum_chain(self.maximum_chain_expansion)?;
        let right = exact_maximum_chain(other.maximum_chain_expansion)?;
        Ok(Self {
            counts,
            // An individual redirect peak composes by maximum, never by sum.
            maximum_chain_expansion: CountBound::exact(left.max(right)),
        })
    }

    pub(crate) const fn counts(self) -> CommandStepCounts {
        self.counts
    }

    pub(crate) const fn maximum_chain_expansion(self) -> CountBound {
        self.maximum_chain_expansion
    }
}

/// Exact pre-emission structure in the local replacement region.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct StructuredRecipeSize {
    functions: u64,
    helpers: u64,
    top_level_commands: u64,
    command_nodes: u64,
}

impl StructuredRecipeSize {
    const ZERO: Self = Self {
        functions: 0,
        helpers: 0,
        top_level_commands: 0,
        command_nodes: 0,
    };

    fn from_fragments(
        functions: u64,
        helpers: u64,
        fragments: &[ControlRecipeFragment],
    ) -> Result<Self, RecipeCostError> {
        fragments.iter().copied().try_fold(
            Self {
                functions,
                helpers,
                ..Self::ZERO
            },
            |size, fragment| size.checked_add(fragment.structured_size()),
        )
    }

    fn checked_add(self, other: Self) -> Result<Self, RecipeCostError> {
        Ok(Self {
            functions: checked_size_add(
                self.functions,
                other.functions,
                RecipeCostDimension::Functions,
            )?,
            helpers: checked_size_add(self.helpers, other.helpers, RecipeCostDimension::Helpers)?,
            top_level_commands: checked_size_add(
                self.top_level_commands,
                other.top_level_commands,
                RecipeCostDimension::TopLevelCommands,
            )?,
            command_nodes: checked_size_add(
                self.command_nodes,
                other.command_nodes,
                RecipeCostDimension::CommandNodes,
            )?,
        })
    }

    pub(crate) const fn functions(self) -> u64 {
        self.functions
    }

    pub(crate) const fn helpers(self) -> u64 {
        self.helpers
    }

    pub(crate) const fn top_level_commands(self) -> u64 {
        self.top_level_commands
    }

    pub(crate) const fn command_nodes(self) -> u64 {
        self.command_nodes
    }
}

/// Complete local accounting for one closed control-recipe choice.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ControlRecipeCost {
    kind: ControlRecipeKind,
    terminal_arm: BranchArm,
    whole_graph_impact: WholeGraphImpact,
    then_path: RecipePathCost,
    else_path: RecipePathCost,
    structured_size: StructuredRecipeSize,
}

impl ControlRecipeCost {
    /// Accounts the Stage 4 dispatcher plus the materialized terminal wrapper
    /// that the competing recipe proposes to consume.
    pub(crate) fn return_dispatcher(terminal_arm: BranchArm) -> Result<Self, RecipeCostError> {
        let selected =
            ControlRecipeFragment::ConditionalReturnRunInternalFunction(GuardOutcome::Selected);
        let rejected =
            ControlRecipeFragment::ConditionalReturnRunInternalFunction(GuardOutcome::Rejected);
        let tail = ControlRecipeFragment::ReturnRunInternalFunction;
        let call = ControlRecipeFragment::InternalFunctionCall;
        let returned = ControlRecipeFragment::ReturnNonZero;
        let then_path = match terminal_arm {
            BranchArm::Then => RecipePathCost::from_fragments(&[selected, call, returned])?,
            BranchArm::Else => RecipePathCost::from_fragments(&[selected])?,
        };
        let else_path = match terminal_arm {
            BranchArm::Then => RecipePathCost::from_fragments(&[rejected, tail])?,
            BranchArm::Else => RecipePathCost::from_fragments(&[rejected, tail, call, returned])?,
        };
        let structured_size =
            StructuredRecipeSize::from_fragments(2, 0, &[selected, tail, call, returned])?;
        Ok(Self {
            kind: ControlRecipeKind::ReturnDispatcher,
            terminal_arm,
            whole_graph_impact: WholeGraphImpact::baseline_identity(),
            then_path,
            else_path,
            structured_size,
        })
    }

    /// Accounts the dispatcher after replacing one unique zero-ABI terminal
    /// call wrapper with a direct result-propagating call.
    pub(crate) fn inline_zero_abi_terminal_call(
        terminal_arm: BranchArm,
        whole_graph_impact: WholeGraphImpact,
    ) -> Result<Self, RecipeCostError> {
        if matches!(
            whole_graph_impact,
            WholeGraphImpact::NonIncreasing(WholeGraphProofCertificate::BaselineIdentity)
        ) {
            return Err(RecipeCostError::IncompatibleWholeGraphProof {
                recipe: ControlRecipeKind::InlineZeroAbiTerminalCall,
                proof: WholeGraphProofCertificate::BaselineIdentity,
            });
        }
        let selected =
            ControlRecipeFragment::ConditionalReturnRunInternalFunction(GuardOutcome::Selected);
        let rejected =
            ControlRecipeFragment::ConditionalReturnRunInternalFunction(GuardOutcome::Rejected);
        let tail = ControlRecipeFragment::ReturnRunInternalFunction;
        Ok(Self {
            kind: ControlRecipeKind::InlineZeroAbiTerminalCall,
            terminal_arm,
            whole_graph_impact,
            then_path: RecipePathCost::from_fragments(&[selected])?,
            else_path: RecipePathCost::from_fragments(&[rejected, tail])?,
            structured_size: StructuredRecipeSize::from_fragments(1, 0, &[selected, tail])?,
        })
    }

    pub(crate) const fn kind(self) -> ControlRecipeKind {
        self.kind
    }

    pub(crate) const fn terminal_arm(self) -> BranchArm {
        self.terminal_arm
    }

    pub(crate) const fn whole_graph_impact(self) -> WholeGraphImpact {
        self.whole_graph_impact
    }

    pub(crate) const fn path(self, arm: BranchArm) -> RecipePathCost {
        match arm {
            BranchArm::Then => self.then_path,
            BranchArm::Else => self.else_path,
        }
    }

    pub(crate) const fn structured_size(self) -> StructuredRecipeSize {
        self.structured_size
    }
}

impl ControlRecipeFragment {
    const fn path_cost(self) -> RecipePathCost {
        let (counts, maximum_chain_expansion) = match self {
            Self::ConditionalReturnRunInternalFunction(GuardOutcome::Rejected) => {
                (CommandStepCounts::new(1, 1, 0, 0), 0)
            }
            Self::ConditionalReturnRunInternalFunction(GuardOutcome::Selected) => {
                (CommandStepCounts::new(2, 1, 1, 0), 1)
            }
            Self::ReturnRunInternalFunction | Self::InternalFunctionCall => {
                (CommandStepCounts::new(1, 0, 1, 0), 0)
            }
            // Native `return value` does not consume a sequence operation.
            Self::ReturnNonZero => (CommandStepCounts::new(0, 0, 0, 0), 0),
        };
        RecipePathCost {
            counts,
            maximum_chain_expansion: CountBound::exact(maximum_chain_expansion),
        }
    }

    const fn structured_size(self) -> StructuredRecipeSize {
        let command_nodes = match self {
            Self::ConditionalReturnRunInternalFunction(_) => 3,
            Self::ReturnRunInternalFunction => 2,
            Self::InternalFunctionCall | Self::ReturnNonZero => 1,
        };
        StructuredRecipeSize {
            functions: 0,
            helpers: 0,
            top_level_commands: 1,
            command_nodes,
        }
    }
}

/// Why an otherwise legal candidate won the numeric comparison.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RecipeAdvantage {
    RuntimeDominance,
    StructuredSizeDominance,
    StableRecipeOrder,
}

/// Why the comparison conservatively retained the Stage 4 dispatcher.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RecipeRetentionReason {
    WholeGraphImpactUnproven,
    RuntimeRegression,
    RuntimeIncomparable,
    StructuredSizeRegression,
    StructuredSizeIncomparable,
    StableRecipeOrder,
}

/// Deterministic result of comparing one already-legal candidate with Stage 4.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RecipePreference {
    SelectCandidate(RecipeAdvantage),
    RetainBaseline(RecipeRetentionReason),
}

/// Compares an already-legality-checked candidate against the Stage 4 dispatcher.
///
/// No frequency, root cost, or whole-target analysis is invented here. An explicit
/// graph-impact certificate is required before exact path-local Pareto comparison.
pub(crate) fn compare_recipe_costs(
    baseline: ControlRecipeCost,
    candidate: ControlRecipeCost,
) -> Result<RecipePreference, RecipeCostError> {
    if baseline.kind != ControlRecipeKind::ReturnDispatcher {
        return Err(RecipeCostError::ExpectedReturnDispatcherBaseline {
            actual: baseline.kind,
        });
    }
    if baseline.terminal_arm != candidate.terminal_arm {
        return Err(RecipeCostError::MismatchedTerminalArm {
            baseline: baseline.terminal_arm,
            candidate: candidate.terminal_arm,
        });
    }
    if candidate.whole_graph_impact.proof().is_none() {
        return Ok(RecipePreference::RetainBaseline(
            RecipeRetentionReason::WholeGraphImpactUnproven,
        ));
    }

    match compare_runtime(baseline, candidate)? {
        ParetoRelation::CandidateDominates => Ok(RecipePreference::SelectCandidate(
            RecipeAdvantage::RuntimeDominance,
        )),
        ParetoRelation::BaselineDominates => Ok(RecipePreference::RetainBaseline(
            RecipeRetentionReason::RuntimeRegression,
        )),
        ParetoRelation::Incomparable => Ok(RecipePreference::RetainBaseline(
            RecipeRetentionReason::RuntimeIncomparable,
        )),
        ParetoRelation::Equal => match compare_structure(baseline, candidate) {
            ParetoRelation::CandidateDominates => Ok(RecipePreference::SelectCandidate(
                RecipeAdvantage::StructuredSizeDominance,
            )),
            ParetoRelation::BaselineDominates => Ok(RecipePreference::RetainBaseline(
                RecipeRetentionReason::StructuredSizeRegression,
            )),
            ParetoRelation::Incomparable => Ok(RecipePreference::RetainBaseline(
                RecipeRetentionReason::StructuredSizeIncomparable,
            )),
            ParetoRelation::Equal if candidate.kind < baseline.kind => Ok(
                RecipePreference::SelectCandidate(RecipeAdvantage::StableRecipeOrder),
            ),
            ParetoRelation::Equal => Ok(RecipePreference::RetainBaseline(
                RecipeRetentionReason::StableRecipeOrder,
            )),
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParetoRelation {
    Equal,
    CandidateDominates,
    BaselineDominates,
    Incomparable,
}

#[derive(Default)]
struct ParetoAccumulator {
    candidate_better: bool,
    baseline_better: bool,
}

impl ParetoAccumulator {
    fn observe(&mut self, baseline: u64, candidate: u64) {
        self.candidate_better |= candidate < baseline;
        self.baseline_better |= baseline < candidate;
    }

    const fn finish(self) -> ParetoRelation {
        match (self.candidate_better, self.baseline_better) {
            (false, false) => ParetoRelation::Equal,
            (true, false) => ParetoRelation::CandidateDominates,
            (false, true) => ParetoRelation::BaselineDominates,
            (true, true) => ParetoRelation::Incomparable,
        }
    }
}

fn compare_runtime(
    baseline: ControlRecipeCost,
    candidate: ControlRecipeCost,
) -> Result<ParetoRelation, RecipeCostError> {
    let mut comparison = ParetoAccumulator::default();
    for arm in [BranchArm::Then, BranchArm::Else] {
        let baseline = baseline.path(arm);
        let candidate = candidate.path(arm);
        compare_step_counts(&mut comparison, baseline.counts, candidate.counts);
        comparison.observe(
            exact_maximum_chain(baseline.maximum_chain_expansion)?,
            exact_maximum_chain(candidate.maximum_chain_expansion)?,
        );
    }
    Ok(comparison.finish())
}

fn compare_step_counts(
    comparison: &mut ParetoAccumulator,
    baseline: CommandStepCounts,
    candidate: CommandStepCounts,
) {
    comparison.observe(
        baseline.sequence_operations(),
        candidate.sequence_operations(),
    );
    comparison.observe(baseline.execute_stages(), candidate.execute_stages());
    comparison.observe(
        baseline.internal_function_invocations(),
        candidate.internal_function_invocations(),
    );
    comparison.observe(
        baseline.score_nbt_command_executions(),
        candidate.score_nbt_command_executions(),
    );
}

fn compare_structure(baseline: ControlRecipeCost, candidate: ControlRecipeCost) -> ParetoRelation {
    let baseline = baseline.structured_size;
    let candidate = candidate.structured_size;
    let mut comparison = ParetoAccumulator::default();
    comparison.observe(baseline.functions, candidate.functions);
    comparison.observe(baseline.helpers, candidate.helpers);
    comparison.observe(baseline.top_level_commands, candidate.top_level_commands);
    comparison.observe(baseline.command_nodes, candidate.command_nodes);
    comparison.finish()
}

fn exact_maximum_chain(bound: CountBound) -> Result<u64, RecipeCostError> {
    match bound.upper().kind() {
        CountUpperKind::Finite(upper) if bound.lower() == upper => Ok(upper),
        _ => Err(RecipeCostError::NonExactLocalMaximumChain { bound }),
    }
}

fn checked_size_add(
    left: u64,
    right: u64,
    dimension: RecipeCostDimension,
) -> Result<u64, RecipeCostError> {
    left.checked_add(right)
        .ok_or(RecipeCostError::ArithmeticOverflow(dimension))
}

/// Checked local-accounting dimension that exceeded the integer domain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RecipeCostDimension {
    PathCommandCounts,
    Functions,
    Helpers,
    TopLevelCommands,
    CommandNodes,
}

/// Invalid or unrepresentable local recipe accounting.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RecipeCostError {
    ArithmeticOverflow(RecipeCostDimension),
    NonExactLocalMaximumChain {
        bound: CountBound,
    },
    IncompatibleWholeGraphProof {
        recipe: ControlRecipeKind,
        proof: WholeGraphProofCertificate,
    },
    ExpectedReturnDispatcherBaseline {
        actual: ControlRecipeKind,
    },
    MismatchedTerminalArm {
        baseline: BranchArm,
        candidate: BranchArm,
    },
}

impl fmt::Display for RecipeCostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ArithmeticOverflow(dimension) => {
                write!(
                    formatter,
                    "recipe cost arithmetic overflowed in {dimension:?}"
                )
            }
            Self::NonExactLocalMaximumChain { bound } => write!(
                formatter,
                "recipe-local maximum chain must be exact, found {bound:?}"
            ),
            Self::IncompatibleWholeGraphProof { recipe, proof } => write!(
                formatter,
                "whole-graph proof {proof:?} is incompatible with recipe {recipe:?}"
            ),
            Self::ExpectedReturnDispatcherBaseline { actual } => write!(
                formatter,
                "recipe comparison expected ReturnDispatcher baseline, found {actual:?}"
            ),
            Self::MismatchedTerminalArm {
                baseline,
                candidate,
            } => write!(
                formatter,
                "recipe comparison used different terminal arms: baseline {baseline:?}, candidate {candidate:?}"
            ),
        }
    }
}

impl Error for RecipeCostError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_counts(
        cost: RecipePathCost,
        sequence: u64,
        execute: u64,
        calls: u64,
        score_nbt: u64,
        maximum_chain: u64,
    ) {
        let counts = cost.counts();
        assert_eq!(counts.sequence_operations(), sequence);
        assert_eq!(counts.execute_stages(), execute);
        assert_eq!(counts.internal_function_invocations(), calls);
        assert_eq!(counts.score_nbt_command_executions(), score_nbt);
        assert_eq!(
            cost.maximum_chain_expansion(),
            CountBound::exact(maximum_chain)
        );
    }

    #[test]
    fn closed_fragments_account_the_terminal_then_arm_exactly() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Then).unwrap();
        let candidate = ControlRecipeCost::inline_zero_abi_terminal_call(
            BranchArm::Then,
            WholeGraphImpact::unique_terminal_arm_contraction(),
        )
        .unwrap();

        assert_counts(baseline.path(BranchArm::Then), 3, 1, 2, 0, 1);
        assert_counts(baseline.path(BranchArm::Else), 2, 1, 1, 0, 0);
        assert_counts(candidate.path(BranchArm::Then), 2, 1, 1, 0, 1);
        assert_counts(candidate.path(BranchArm::Else), 2, 1, 1, 0, 0);
        assert_eq!(
            baseline.structured_size(),
            StructuredRecipeSize {
                functions: 2,
                helpers: 0,
                top_level_commands: 4,
                command_nodes: 7,
            }
        );
        assert_eq!(
            candidate.structured_size(),
            StructuredRecipeSize {
                functions: 1,
                helpers: 0,
                top_level_commands: 2,
                command_nodes: 5,
            }
        );
    }

    #[test]
    fn closed_fragments_account_the_terminal_else_arm_exactly() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Else).unwrap();
        let candidate = ControlRecipeCost::inline_zero_abi_terminal_call(
            BranchArm::Else,
            WholeGraphImpact::unique_terminal_arm_contraction(),
        )
        .unwrap();

        assert_counts(baseline.path(BranchArm::Then), 2, 1, 1, 0, 1);
        assert_counts(baseline.path(BranchArm::Else), 3, 1, 2, 0, 0);
        assert_counts(candidate.path(BranchArm::Then), 2, 1, 1, 0, 1);
        assert_counts(candidate.path(BranchArm::Else), 2, 1, 1, 0, 0);
    }

    #[test]
    fn certified_terminal_contraction_wins_by_strict_runtime_dominance() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Then).unwrap();
        let candidate = ControlRecipeCost::inline_zero_abi_terminal_call(
            BranchArm::Then,
            WholeGraphImpact::unique_terminal_arm_contraction(),
        )
        .unwrap();

        assert_eq!(
            compare_recipe_costs(baseline, candidate),
            Ok(RecipePreference::SelectCandidate(
                RecipeAdvantage::RuntimeDominance
            ))
        );
    }

    #[test]
    fn unproven_graph_impact_retains_the_dispatcher_despite_local_savings() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Then).unwrap();
        let candidate = ControlRecipeCost::inline_zero_abi_terminal_call(
            BranchArm::Then,
            WholeGraphImpact::unproven(),
        )
        .unwrap();

        assert_eq!(
            compare_recipe_costs(baseline, candidate),
            Ok(RecipePreference::RetainBaseline(
                RecipeRetentionReason::WholeGraphImpactUnproven
            ))
        );
    }

    #[test]
    fn a_cross_path_runtime_tradeoff_is_incomparable() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Then).unwrap();
        let mut candidate = baseline;
        candidate.kind = ControlRecipeKind::InlineZeroAbiTerminalCall;
        candidate.whole_graph_impact = WholeGraphImpact::unique_terminal_arm_contraction();
        candidate.then_path.counts = CommandStepCounts::new(2, 1, 2, 0);
        candidate.else_path.counts = CommandStepCounts::new(2, 1, 2, 0);

        assert_eq!(
            compare_recipe_costs(baseline, candidate),
            Ok(RecipePreference::RetainBaseline(
                RecipeRetentionReason::RuntimeIncomparable
            ))
        );
    }

    #[test]
    fn equal_runtime_uses_structure_then_stable_recipe_order() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Then).unwrap();
        let mut smaller = baseline;
        smaller.kind = ControlRecipeKind::InlineZeroAbiTerminalCall;
        smaller.whole_graph_impact = WholeGraphImpact::unique_terminal_arm_contraction();
        smaller.structured_size.functions -= 1;
        assert_eq!(
            compare_recipe_costs(baseline, smaller),
            Ok(RecipePreference::SelectCandidate(
                RecipeAdvantage::StructuredSizeDominance
            ))
        );

        assert_eq!(
            compare_recipe_costs(baseline, baseline),
            Ok(RecipePreference::RetainBaseline(
                RecipeRetentionReason::StableRecipeOrder
            ))
        );
    }

    #[test]
    fn structure_never_purchases_a_runtime_regression() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Then).unwrap();
        let mut candidate = baseline;
        candidate.kind = ControlRecipeKind::InlineZeroAbiTerminalCall;
        candidate.whole_graph_impact = WholeGraphImpact::unique_terminal_arm_contraction();
        candidate.then_path.counts = CommandStepCounts::new(4, 1, 2, 0);
        candidate.structured_size = StructuredRecipeSize::ZERO;

        assert_eq!(
            compare_recipe_costs(baseline, candidate),
            Ok(RecipePreference::RetainBaseline(
                RecipeRetentionReason::RuntimeRegression
            ))
        );
    }

    #[test]
    fn local_arithmetic_is_checked_and_maximum_chain_is_not_summed() {
        let one = RecipePathCost {
            counts: CommandStepCounts::new(1, 0, 0, 0),
            maximum_chain_expansion: CountBound::exact(3),
        };
        let two = RecipePathCost {
            counts: CommandStepCounts::new(2, 0, 0, 0),
            maximum_chain_expansion: CountBound::exact(5),
        };
        let sum = one.checked_add(two).unwrap();
        assert_counts(sum, 3, 0, 0, 0, 5);

        let overflow = RecipePathCost {
            counts: CommandStepCounts::new(u64::MAX, 0, 0, 0),
            maximum_chain_expansion: CountBound::exact(0),
        }
        .checked_add(one);
        assert_eq!(
            overflow,
            Err(RecipeCostError::ArithmeticOverflow(
                RecipeCostDimension::PathCommandCounts
            ))
        );
    }

    #[test]
    fn comparison_rejects_mismatched_regions_and_invalid_proofs() {
        let baseline = ControlRecipeCost::return_dispatcher(BranchArm::Then).unwrap();
        let other_arm = ControlRecipeCost::inline_zero_abi_terminal_call(
            BranchArm::Else,
            WholeGraphImpact::unique_terminal_arm_contraction(),
        )
        .unwrap();
        assert_eq!(
            compare_recipe_costs(baseline, other_arm),
            Err(RecipeCostError::MismatchedTerminalArm {
                baseline: BranchArm::Then,
                candidate: BranchArm::Else,
            })
        );
        assert_eq!(
            ControlRecipeCost::inline_zero_abi_terminal_call(
                BranchArm::Then,
                WholeGraphImpact::baseline_identity(),
            ),
            Err(RecipeCostError::IncompatibleWholeGraphProof {
                recipe: ControlRecipeKind::InlineZeroAbiTerminalCall,
                proof: WholeGraphProofCertificate::BaselineIdentity,
            })
        );
    }
}
