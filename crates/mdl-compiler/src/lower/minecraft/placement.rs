//! Closed control-recipe selection and explicit Core-block placement.

use std::error::Error;
use std::fmt;

use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{BlockId, CoreOp, CoreProgram, FunctionId, InstId, TerminatorKind};
use crate::source::OriginId;

use super::MinecraftOptimizationLevel;
use super::analysis::{BranchArm, CoreEdgeKind, SemanticInventory};
use super::assignment::{AssignedInstructionPlan, HomeAssignment};
use super::edge_transfer::{BlockTransfer, EdgeTransferPlan};
use super::recipe::{
    ControlRecipeCost, ControlRecipeKind, RecipeAdvantage, RecipeCostError, RecipePreference,
    RecipeRetentionReason, WholeGraphImpact, compare_recipe_costs,
};

/// Explicit ownership of one reachable Core block in the final target plan.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum BlockPlacement {
    Materialized,
    Consumed {
        source: BlockId,
        arm: BranchArm,
        recipe: ControlRecipeKind,
    },
}

/// Why one branch arm retained its materialized Stage 4 target.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum RecipeDecisionReason {
    OptimizationDisabled,
    DestinationIsEntry,
    IncomingEdgeOccurrenceCount { actual: usize },
    EdgeTransferNotEmpty,
    DestinationHasParameters,
    InstructionCountNotOne { actual: usize },
    InstructionIsNotCall,
    CallHasSemanticArgumentsOrResults,
    CallHasPhysicalArgumentsOrResults,
    CalleeHasParametersOrResults,
    TerminalReturnIsNotEmpty,
    CallerHasResults,
    NormalCompletionNotProven,
    PlacementAlreadyConsumed,
    AccountingUnavailable(RecipeCostError),
    CostRetained(RecipeRetentionReason),
}

/// Exhaustive generated-callee completion contract used by the first recipe.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum GeneratedCompletionContract {
    ExactlyOneOnNormalCompletion,
}

/// Ordered provenance that contributes to one contracted target command.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RecipeOrigins {
    branch: OriginId,
    call: OriginId,
    terminal_return: OriginId,
}

impl RecipeOrigins {
    pub(crate) const fn branch(self) -> OriginId {
        self.branch
    }

    pub(crate) const fn call(self) -> OriginId {
        self.call
    }

    pub(crate) const fn terminal_return(self) -> OriginId {
        self.terminal_return
    }
}

/// Exact logical payload of the first target-control contraction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct InlineZeroAbiTerminalCall {
    consumed_block: BlockId,
    call_instruction: InstId,
    callee: FunctionId,
    terminal_arm: BranchArm,
    advantage: RecipeAdvantage,
    origins: RecipeOrigins,
}

impl InlineZeroAbiTerminalCall {
    pub(crate) const fn consumed_block(self) -> BlockId {
        self.consumed_block
    }

    pub(crate) const fn call_instruction(self) -> InstId {
        self.call_instruction
    }

    pub(crate) const fn callee(self) -> FunctionId {
        self.callee
    }

    pub(crate) const fn terminal_arm(self) -> BranchArm {
        self.terminal_arm
    }

    pub(crate) fn baseline_cost(self) -> Result<ControlRecipeCost, RecipeCostError> {
        ControlRecipeCost::return_dispatcher(self.terminal_arm)
    }

    pub(crate) fn selected_cost(self) -> Result<ControlRecipeCost, RecipeCostError> {
        ControlRecipeCost::inline_zero_abi_terminal_call(
            self.terminal_arm,
            WholeGraphImpact::unique_terminal_arm_contraction(),
        )
    }

    pub(crate) const fn advantage(self) -> RecipeAdvantage {
        self.advantage
    }

    pub(crate) const fn origins(self) -> RecipeOrigins {
        self.origins
    }
}

/// Frozen target choice for one semantic branch arm.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum BranchArmRecipe {
    Materialized { reason: RecipeDecisionReason },
    InlineZeroAbiTerminalCall(InlineZeroAbiTerminalCall),
}

impl BranchArmRecipe {
    pub(crate) const fn kind(self) -> ControlRecipeKind {
        match self {
            Self::Materialized { .. } => ControlRecipeKind::ReturnDispatcher,
            Self::InlineZeroAbiTerminalCall(_) => ControlRecipeKind::InlineZeroAbiTerminalCall,
        }
    }

    pub(crate) const fn inline_zero_abi_terminal_call(self) -> Option<InlineZeroAbiTerminalCall> {
        match self {
            Self::InlineZeroAbiTerminalCall(recipe) => Some(recipe),
            Self::Materialized { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn decision_reason(self) -> Option<RecipeDecisionReason> {
        match self {
            Self::Materialized { reason } => Some(reason),
            Self::InlineZeroAbiTerminalCall(_) => None,
        }
    }
}

/// Complete pair of choices for one reachable Core branch.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BranchRecipe {
    then_arm: BranchArmRecipe,
    else_arm: BranchArmRecipe,
}

impl BranchRecipe {
    #[cfg(test)]
    pub(crate) const fn all_materialized(reason: RecipeDecisionReason) -> Self {
        Self {
            then_arm: BranchArmRecipe::Materialized { reason },
            else_arm: BranchArmRecipe::Materialized { reason },
        }
    }

    pub(crate) const fn arm(self, arm: BranchArm) -> BranchArmRecipe {
        match arm {
            BranchArm::Then => self.then_arm,
            BranchArm::Else => self.else_arm,
        }
    }
}

/// Deterministic bounded work counters for one complete selection run.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct ControlRecipeStatistics {
    branch_arms_visited: u64,
    candidates_selected: u64,
    blocks_consumed: u64,
}

impl ControlRecipeStatistics {
    pub(crate) const fn from_counts(
        branch_arms_visited: u64,
        candidates_selected: u64,
        blocks_consumed: u64,
    ) -> Self {
        Self {
            branch_arms_visited,
            candidates_selected,
            blocks_consumed,
        }
    }

    pub(crate) const fn branch_arms_visited(self) -> u64 {
        self.branch_arms_visited
    }

    pub(crate) const fn candidates_selected(self) -> u64 {
        self.candidates_selected
    }

    pub(crate) const fn blocks_consumed(self) -> u64 {
        self.blocks_consumed
    }
}

/// Per-function dense placement and branch-recipe tables.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionControlRecipePlan {
    placements: Box<[Option<BlockPlacement>]>,
    branch_recipes: Box<[Option<BranchRecipe>]>,
}

impl FunctionControlRecipePlan {
    pub(crate) fn placement(&self, block: BlockId) -> Option<BlockPlacement> {
        indexed_slot(&self.placements, block).flatten()
    }

    pub(crate) fn placement_slots(&self) -> &[Option<BlockPlacement>] {
        &self.placements
    }

    #[cfg(test)]
    pub(crate) fn branch_recipe(&self, block: BlockId) -> Option<BranchRecipe> {
        indexed_slot(&self.branch_recipes, block).flatten()
    }

    pub(crate) fn branch_recipe_slots(&self) -> &[Option<BranchRecipe>] {
        &self.branch_recipes
    }
}

/// Immutable result of closed recipe selection, aligned with Core functions.
#[derive(Clone, Debug)]
pub(crate) struct ControlRecipePlan {
    level: MinecraftOptimizationLevel,
    functions: EntityVec<FunctionId, FunctionControlRecipePlan>,
    statistics: ControlRecipeStatistics,
}

impl ControlRecipePlan {
    #[allow(
        clippy::too_many_lines,
        reason = "selection freezes one complete dense function/branch placement table in deterministic Core order"
    )]
    pub(crate) fn new(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        assignment: &HomeAssignment,
        transfers: &EdgeTransferPlan,
        level: MinecraftOptimizationLevel,
    ) -> Result<Self, ControlRecipePlanError> {
        if !assignment.matches_level(level) {
            return Err(ControlRecipePlanError::OptimizationLevelMismatch {
                prerequisite: ControlRecipePrerequisite::Assignment,
                expected: level,
                actual: assignment.level(),
            });
        }
        if !transfers.matches_level(level) {
            return Err(ControlRecipePlanError::OptimizationLevelMismatch {
                prerequisite: ControlRecipePrerequisite::Transfers,
                expected: level,
                actual: transfers.level(),
            });
        }
        for (prerequisite, actual) in [
            (ControlRecipePrerequisite::Inventory, inventory.len()),
            (ControlRecipePrerequisite::Assignment, assignment.len()),
            (ControlRecipePrerequisite::Transfers, transfers.len()),
        ] {
            if actual != core.len() {
                return Err(ControlRecipePlanError::FunctionCountMismatch {
                    prerequisite,
                    expected: core.len(),
                    actual,
                });
            }
        }

        let mut functions = EntityVec::new();
        let mut statistics = ControlRecipeStatistics::default();
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(ControlRecipePlanError::MissingDefinition { function })?;
            let semantic = inventory
                .function(function)
                .ok_or(ControlRecipePlanError::MissingInventory { function })?;
            let assigned = assignment
                .function(function)
                .ok_or(ControlRecipePlanError::MissingAssignment { function })?;
            let transfer = transfers
                .function(function)
                .ok_or(ControlRecipePlanError::MissingTransfers { function })?;

            let mut placements = vec![None; body.block_counts().allocated];
            for block in semantic.reachable_blocks().iter().copied() {
                *indexed_slot_mut(&mut placements, block)
                    .ok_or(ControlRecipePlanError::InvalidBlock { function, block })? =
                    Some(BlockPlacement::Materialized);
            }
            let mut branch_recipes = vec![None; body.block_counts().allocated];

            for source in semantic.reachable_blocks().iter().copied() {
                let data = body
                    .block(source)
                    .ok_or(ControlRecipePlanError::InvalidBlock {
                        function,
                        block: source,
                    })?;
                let Some(terminator) = data.terminator() else {
                    return Err(ControlRecipePlanError::MissingTerminator {
                        function,
                        block: source,
                    });
                };
                let TerminatorKind::Branch {
                    then_target,
                    else_target,
                    ..
                } = terminator.kind()
                else {
                    continue;
                };
                let Some(BlockTransfer::Branch {
                    then_edge_index,
                    else_edge_index,
                }) = transfer.block_transfer(source)
                else {
                    return Err(ControlRecipePlanError::TransferShapeMismatch {
                        function,
                        block: source,
                    });
                };

                let then_arm = select_arm(&mut ArmSelectionContext {
                    core,
                    function,
                    body,
                    declaration,
                    semantic,
                    assigned,
                    transfer,
                    source,
                    arm: BranchArm::Then,
                    destination: then_target.block(),
                    edge_index: *then_edge_index,
                    branch_origin: terminator.origin(),
                    level,
                    placements: &mut placements,
                    statistics: &mut statistics,
                })?;
                let else_arm = select_arm(&mut ArmSelectionContext {
                    core,
                    function,
                    body,
                    declaration,
                    semantic,
                    assigned,
                    transfer,
                    source,
                    arm: BranchArm::Else,
                    destination: else_target.block(),
                    edge_index: *else_edge_index,
                    branch_origin: terminator.origin(),
                    level,
                    placements: &mut placements,
                    statistics: &mut statistics,
                })?;
                *indexed_slot_mut(&mut branch_recipes, source).ok_or(
                    ControlRecipePlanError::InvalidBlock {
                        function,
                        block: source,
                    },
                )? = Some(BranchRecipe { then_arm, else_arm });
            }

            let planned = functions
                .push(FunctionControlRecipePlan {
                    placements: placements.into_boxed_slice(),
                    branch_recipes: branch_recipes.into_boxed_slice(),
                })
                .map_err(|EntityLimitError| ControlRecipePlanError::EntityLimit)?;
            if planned != function {
                return Err(ControlRecipePlanError::FunctionAlignment { function });
            }
        }

        Ok(Self {
            level,
            functions,
            statistics,
        })
    }

    pub(crate) const fn level(&self) -> MinecraftOptimizationLevel {
        self.level
    }

    pub(crate) fn matches_level(&self, level: MinecraftOptimizationLevel) -> bool {
        self.level == level
    }

    pub(crate) fn len(&self) -> usize {
        self.functions.len()
    }

    pub(crate) fn function(&self, function: FunctionId) -> Option<&FunctionControlRecipePlan> {
        self.functions.get(function)
    }

    pub(crate) const fn statistics(&self) -> ControlRecipeStatistics {
        self.statistics
    }
}

struct ArmSelectionContext<'a> {
    core: &'a CoreProgram,
    function: FunctionId,
    body: &'a crate::ir::core::FunctionBody,
    declaration: &'a crate::ir::core::Function,
    semantic: &'a super::analysis::FunctionSemanticInventory,
    assigned: &'a super::assignment::FunctionHomeAssignment,
    transfer: &'a super::edge_transfer::FunctionEdgeTransferPlan,
    source: BlockId,
    arm: BranchArm,
    destination: BlockId,
    edge_index: usize,
    branch_origin: OriginId,
    level: MinecraftOptimizationLevel,
    placements: &'a mut [Option<BlockPlacement>],
    statistics: &'a mut ControlRecipeStatistics,
}

fn select_arm(
    context: &mut ArmSelectionContext<'_>,
) -> Result<BranchArmRecipe, ControlRecipePlanError> {
    increment(&mut context.statistics.branch_arms_visited)?;
    if context.level == MinecraftOptimizationLevel::None {
        return Ok(materialized(RecipeDecisionReason::OptimizationDisabled));
    }

    let candidate = match match_inline_zero_abi_terminal_call(context) {
        Ok(candidate) => candidate,
        Err(reason) => return Ok(materialized(reason)),
    };
    let baseline_cost = match ControlRecipeCost::return_dispatcher(context.arm) {
        Ok(cost) => cost,
        Err(error) => {
            return Ok(materialized(RecipeDecisionReason::AccountingUnavailable(
                error,
            )));
        }
    };
    let selected_cost = match ControlRecipeCost::inline_zero_abi_terminal_call(
        context.arm,
        WholeGraphImpact::unique_terminal_arm_contraction(),
    ) {
        Ok(cost) => cost,
        Err(error) => {
            return Ok(materialized(RecipeDecisionReason::AccountingUnavailable(
                error,
            )));
        }
    };
    let advantage = match compare_recipe_costs(baseline_cost, selected_cost) {
        Ok(RecipePreference::SelectCandidate(advantage)) => advantage,
        Ok(RecipePreference::RetainBaseline(reason)) => {
            return Ok(materialized(RecipeDecisionReason::CostRetained(reason)));
        }
        Err(error) => {
            return Ok(materialized(RecipeDecisionReason::AccountingUnavailable(
                error,
            )));
        }
    };

    let placement = indexed_slot_mut(context.placements, context.destination).ok_or(
        ControlRecipePlanError::InvalidBlock {
            function: context.function,
            block: context.destination,
        },
    )?;
    if *placement != Some(BlockPlacement::Materialized) {
        return Ok(materialized(RecipeDecisionReason::PlacementAlreadyConsumed));
    }
    *placement = Some(BlockPlacement::Consumed {
        source: context.source,
        arm: context.arm,
        recipe: ControlRecipeKind::InlineZeroAbiTerminalCall,
    });
    increment(&mut context.statistics.candidates_selected)?;
    increment(&mut context.statistics.blocks_consumed)?;
    Ok(BranchArmRecipe::InlineZeroAbiTerminalCall(
        InlineZeroAbiTerminalCall {
            consumed_block: context.destination,
            call_instruction: candidate.call_instruction,
            callee: candidate.callee,
            terminal_arm: context.arm,
            advantage,
            origins: RecipeOrigins {
                branch: context.branch_origin,
                call: candidate.call_origin,
                terminal_return: candidate.return_origin,
            },
        },
    ))
}

struct MatchedTerminalCall {
    call_instruction: InstId,
    callee: FunctionId,
    call_origin: OriginId,
    return_origin: OriginId,
}

fn match_inline_zero_abi_terminal_call(
    context: &ArmSelectionContext<'_>,
) -> Result<MatchedTerminalCall, RecipeDecisionReason> {
    if context.destination == context.body.entry() {
        return Err(RecipeDecisionReason::DestinationIsEntry);
    }
    let incoming = context
        .semantic
        .incoming_edge_indices(context.destination)
        .unwrap_or_default();
    if incoming != [context.edge_index] {
        return Err(RecipeDecisionReason::IncomingEdgeOccurrenceCount {
            actual: incoming.len(),
        });
    }
    let edge = context
        .transfer
        .edges()
        .get(context.edge_index)
        .ok_or(RecipeDecisionReason::EdgeTransferNotEmpty)?;
    if edge.source() != context.source
        || edge.kind() != CoreEdgeKind::Branch(context.arm)
        || edge.destination() != context.destination
        || !edge.is_empty()
    {
        return Err(RecipeDecisionReason::EdgeTransferNotEmpty);
    }

    let block = context
        .body
        .block(context.destination)
        .ok_or(RecipeDecisionReason::InstructionIsNotCall)?;
    if !block.parameters().is_empty() {
        return Err(RecipeDecisionReason::DestinationHasParameters);
    }
    if block.instructions().len() != 1 {
        return Err(RecipeDecisionReason::InstructionCountNotOne {
            actual: block.instructions().len(),
        });
    }
    let call_instruction = block.instructions()[0];
    let call = context
        .body
        .instruction(call_instruction)
        .ok_or(RecipeDecisionReason::InstructionIsNotCall)?;
    let CoreOp::Call(callee) = call.op() else {
        return Err(RecipeDecisionReason::InstructionIsNotCall);
    };
    if !call.operands().is_empty() || !call.results().is_empty() {
        return Err(RecipeDecisionReason::CallHasSemanticArgumentsOrResults);
    }
    let Some(AssignedInstructionPlan::Call {
        arguments,
        result_destinations,
    }) = context.assigned.instruction_plan(call_instruction)
    else {
        return Err(RecipeDecisionReason::InstructionIsNotCall);
    };
    if !arguments.is_empty() || !result_destinations.is_empty() {
        return Err(RecipeDecisionReason::CallHasPhysicalArgumentsOrResults);
    }
    let callee_declaration = context
        .core
        .function(*callee)
        .ok_or(RecipeDecisionReason::NormalCompletionNotProven)?;
    if !callee_declaration.parameters().is_empty() || !callee_declaration.results().is_empty() {
        return Err(RecipeDecisionReason::CalleeHasParametersOrResults);
    }
    if !context.declaration.results().is_empty() {
        return Err(RecipeDecisionReason::CallerHasResults);
    }
    let terminator = block
        .terminator()
        .ok_or(RecipeDecisionReason::TerminalReturnIsNotEmpty)?;
    if !matches!(terminator.kind(), TerminatorKind::Return(values) if values.is_empty()) {
        return Err(RecipeDecisionReason::TerminalReturnIsNotEmpty);
    }
    if callee_declaration.body().is_none() {
        return Err(RecipeDecisionReason::NormalCompletionNotProven);
    }

    Ok(MatchedTerminalCall {
        call_instruction,
        callee: *callee,
        call_origin: call.origin(),
        return_origin: terminator.origin(),
    })
}

const fn materialized(reason: RecipeDecisionReason) -> BranchArmRecipe {
    BranchArmRecipe::Materialized { reason }
}

fn increment(counter: &mut u64) -> Result<(), ControlRecipePlanError> {
    *counter = counter
        .checked_add(1)
        .ok_or(ControlRecipePlanError::StatisticsOverflow)?;
    Ok(())
}

fn indexed_slot<T: Copy, I: EntityId>(slots: &[T], id: I) -> Option<T> {
    usize::try_from(id.index())
        .ok()
        .and_then(|index| slots.get(index))
        .copied()
}

fn indexed_slot_mut<T, I: EntityId>(slots: &mut [T], id: I) -> Option<&mut T> {
    usize::try_from(id.index())
        .ok()
        .and_then(|index| slots.get_mut(index))
}

/// Prerequisite whose provenance or shape disagreed with recipe selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlRecipePrerequisite {
    Inventory,
    Assignment,
    Transfers,
}

/// Typed failure while constructing immutable placement and recipe decisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ControlRecipePlanError {
    OptimizationLevelMismatch {
        prerequisite: ControlRecipePrerequisite,
        expected: MinecraftOptimizationLevel,
        actual: MinecraftOptimizationLevel,
    },
    FunctionCountMismatch {
        prerequisite: ControlRecipePrerequisite,
        expected: usize,
        actual: usize,
    },
    MissingDefinition {
        function: FunctionId,
    },
    MissingInventory {
        function: FunctionId,
    },
    MissingAssignment {
        function: FunctionId,
    },
    MissingTransfers {
        function: FunctionId,
    },
    FunctionAlignment {
        function: FunctionId,
    },
    InvalidBlock {
        function: FunctionId,
        block: BlockId,
    },
    MissingTerminator {
        function: FunctionId,
        block: BlockId,
    },
    TransferShapeMismatch {
        function: FunctionId,
        block: BlockId,
    },
    StatisticsOverflow,
    EntityLimit,
}

impl fmt::Display for ControlRecipePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ControlRecipePlanError {}

#[cfg(test)]
#[allow(
    clippy::similar_names,
    reason = "caller and callee are the precise semantic roles exercised by these recipe fixtures"
)]
mod tests {
    use super::*;
    use crate::ir::core::{
        BlockTarget, CoreType, FunctionBuilder, Terminator, ValueId, verify_program,
    };
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::source::{Origin, SourceContext};

    #[derive(Clone, Copy)]
    struct TestOrigins {
        branch: OriginId,
        then_call: OriginId,
        then_return: OriginId,
        else_call: OriginId,
        else_return: OriginId,
    }

    struct SingleArmFixture {
        sources: SourceContext,
        core: CoreProgram,
        caller: FunctionId,
        callee: FunctionId,
        entry: BlockId,
        selected_block: BlockId,
        other_block: BlockId,
        call_instruction: InstId,
        origins: RecipeOrigins,
    }

    struct BothArmsFixture {
        sources: SourceContext,
        core: CoreProgram,
        caller: FunctionId,
        callee: FunctionId,
        entry: BlockId,
        then_block: BlockId,
        else_block: BlockId,
        then_call: InstId,
        else_call: InstId,
        origins: TestOrigins,
    }

    struct RejectedFixture {
        sources: SourceContext,
        core: CoreProgram,
        caller: FunctionId,
        entry: BlockId,
        candidate_block: BlockId,
    }

    #[test]
    fn none_materializes_every_reachable_block_and_disables_both_arms() {
        let fixture = single_eligible_arm(BranchArm::Then);
        let (inventory, plan) = control_plan(
            &fixture.core,
            &fixture.sources,
            MinecraftOptimizationLevel::None,
        );

        for (function, _) in fixture.core.functions() {
            let semantic = inventory.function(function).unwrap();
            let planned = plan.function(function).unwrap();
            for block in semantic.reachable_blocks().iter().copied() {
                assert_eq!(planned.placement(block), Some(BlockPlacement::Materialized));
            }
        }
        let recipe = plan
            .function(fixture.caller)
            .unwrap()
            .branch_recipe(fixture.entry)
            .unwrap();
        for arm in [BranchArm::Then, BranchArm::Else] {
            assert_eq!(
                recipe.arm(arm),
                BranchArmRecipe::Materialized {
                    reason: RecipeDecisionReason::OptimizationDisabled,
                }
            );
        }
        assert_eq!(
            plan.statistics(),
            ControlRecipeStatistics {
                branch_arms_visited: 2,
                candidates_selected: 0,
                blocks_consumed: 0,
            }
        );
    }

    #[test]
    fn baseline_selects_then_arm_and_retains_exact_cost_and_provenance() {
        assert_single_arm_selection(BranchArm::Then);
    }

    #[test]
    fn baseline_selects_else_arm_and_retains_exact_cost_and_provenance() {
        assert_single_arm_selection(BranchArm::Else);
    }

    #[test]
    fn baseline_selects_two_independently_eligible_arms() {
        let fixture = both_eligible_arms();
        let (_, plan) = control_plan(
            &fixture.core,
            &fixture.sources,
            MinecraftOptimizationLevel::Baseline,
        );
        let function = plan.function(fixture.caller).unwrap();
        let branch = function.branch_recipe(fixture.entry).unwrap();

        for (arm, block, instruction, call_origin, return_origin) in [
            (
                BranchArm::Then,
                fixture.then_block,
                fixture.then_call,
                fixture.origins.then_call,
                fixture.origins.then_return,
            ),
            (
                BranchArm::Else,
                fixture.else_block,
                fixture.else_call,
                fixture.origins.else_call,
                fixture.origins.else_return,
            ),
        ] {
            assert_eq!(
                function.placement(block),
                Some(BlockPlacement::Consumed {
                    source: fixture.entry,
                    arm,
                    recipe: ControlRecipeKind::InlineZeroAbiTerminalCall,
                })
            );
            let selected = branch.arm(arm).inline_zero_abi_terminal_call().unwrap();
            assert_eq!(selected.consumed_block(), block);
            assert_eq!(selected.call_instruction(), instruction);
            assert_eq!(selected.callee(), fixture.callee);
            assert_eq!(selected.origins().branch(), fixture.origins.branch);
            assert_eq!(selected.origins().call(), call_origin);
            assert_eq!(selected.origins().terminal_return(), return_origin);
        }
        assert_eq!(
            plan.statistics(),
            ControlRecipeStatistics {
                branch_arms_visited: 2,
                candidates_selected: 2,
                blocks_consumed: 2,
            }
        );
    }

    #[test]
    fn duplicate_same_destination_edge_occurrences_are_both_rejected() {
        let fixture = duplicate_destination_edges();
        let (_, plan) = control_plan(
            &fixture.core,
            &fixture.sources,
            MinecraftOptimizationLevel::Baseline,
        );
        let function = plan.function(fixture.caller).unwrap();
        let branch = function.branch_recipe(fixture.entry).unwrap();

        for arm in [BranchArm::Then, BranchArm::Else] {
            assert_eq!(
                branch.arm(arm).decision_reason(),
                Some(RecipeDecisionReason::IncomingEdgeOccurrenceCount { actual: 2 })
            );
        }
        assert_eq!(
            function.placement(fixture.candidate_block),
            Some(BlockPlacement::Materialized)
        );
        assert_eq!(
            plan.statistics(),
            ControlRecipeStatistics {
                branch_arms_visited: 2,
                candidates_selected: 0,
                blocks_consumed: 0,
            }
        );
    }

    #[test]
    fn a_nonzero_callee_abi_is_rejected_from_verified_core() {
        let fixture = nonzero_result_callee();
        let (_, plan) = control_plan(
            &fixture.core,
            &fixture.sources,
            MinecraftOptimizationLevel::Baseline,
        );
        let function = plan.function(fixture.caller).unwrap();
        let branch = function.branch_recipe(fixture.entry).unwrap();

        // In verified Core a nonzero callee ABI necessarily appears on the call
        // operation too, so semantic call arity is the first exact failed check.
        assert_eq!(
            branch.arm(BranchArm::Then).decision_reason(),
            Some(RecipeDecisionReason::CallHasSemanticArgumentsOrResults)
        );
        assert_eq!(
            function.placement(fixture.candidate_block),
            Some(BlockPlacement::Materialized)
        );
        assert_eq!(
            plan.statistics(),
            ControlRecipeStatistics {
                branch_arms_visited: 2,
                candidates_selected: 0,
                blocks_consumed: 0,
            }
        );
    }

    #[test]
    fn a_nonempty_physical_edge_transfer_is_rejected_before_contraction() {
        let fixture = nonempty_edge_transfer();
        let (_, plan) = control_plan(
            &fixture.core,
            &fixture.sources,
            MinecraftOptimizationLevel::Baseline,
        );
        let function = plan.function(fixture.caller).unwrap();
        let branch = function.branch_recipe(fixture.entry).unwrap();

        assert_eq!(
            branch.arm(BranchArm::Then).decision_reason(),
            Some(RecipeDecisionReason::EdgeTransferNotEmpty)
        );
        assert_eq!(
            function.placement(fixture.candidate_block),
            Some(BlockPlacement::Materialized)
        );
        assert_eq!(
            plan.statistics(),
            ControlRecipeStatistics {
                branch_arms_visited: 2,
                candidates_selected: 0,
                blocks_consumed: 0,
            }
        );
    }

    fn assert_single_arm_selection(arm: BranchArm) {
        let fixture = single_eligible_arm(arm);
        let (_, plan) = control_plan(
            &fixture.core,
            &fixture.sources,
            MinecraftOptimizationLevel::Baseline,
        );
        let function = plan.function(fixture.caller).unwrap();
        let branch = function.branch_recipe(fixture.entry).unwrap();
        let selected = branch.arm(arm).inline_zero_abi_terminal_call().unwrap();

        assert_eq!(
            function.placement(fixture.selected_block),
            Some(BlockPlacement::Consumed {
                source: fixture.entry,
                arm,
                recipe: ControlRecipeKind::InlineZeroAbiTerminalCall,
            })
        );
        assert_eq!(
            function.placement(fixture.other_block),
            Some(BlockPlacement::Materialized)
        );
        assert_eq!(selected.consumed_block(), fixture.selected_block);
        assert_eq!(selected.call_instruction(), fixture.call_instruction);
        assert_eq!(selected.callee(), fixture.callee);
        assert_eq!(
            selected.baseline_cost().unwrap(),
            ControlRecipeCost::return_dispatcher(arm).unwrap()
        );
        assert_eq!(
            selected.selected_cost().unwrap(),
            ControlRecipeCost::inline_zero_abi_terminal_call(
                arm,
                WholeGraphImpact::unique_terminal_arm_contraction(),
            )
            .unwrap()
        );
        assert_eq!(selected.advantage(), RecipeAdvantage::RuntimeDominance);
        assert_eq!(selected.origins(), fixture.origins);
        assert_eq!(
            branch.arm(opposite(arm)).decision_reason(),
            Some(RecipeDecisionReason::InstructionCountNotOne { actual: 0 })
        );
        assert_eq!(
            plan.statistics(),
            ControlRecipeStatistics {
                branch_arms_visited: 2,
                candidates_selected: 1,
                blocks_consumed: 1,
            }
        );
    }

    fn control_plan(
        core: &CoreProgram,
        sources: &SourceContext,
        level: MinecraftOptimizationLevel,
    ) -> (SemanticInventory, ControlRecipePlan) {
        verify_program(core, sources).unwrap();
        let inventory = SemanticInventory::new(core).unwrap();
        let assignment = match level {
            MinecraftOptimizationLevel::None => HomeAssignment::for_none(core, &inventory).unwrap(),
            MinecraftOptimizationLevel::Baseline => {
                let demand = RuntimeDemand::for_level(
                    core,
                    &inventory,
                    level,
                    RuntimeDemandLimits::derived(),
                )
                .unwrap();
                HomeAssignment::for_baseline_derived_liveness(core, &inventory, &demand).unwrap()
            }
        };
        let transfers = match level {
            MinecraftOptimizationLevel::None => {
                EdgeTransferPlan::for_none(core, &inventory, &assignment).unwrap()
            }
            MinecraftOptimizationLevel::Baseline => {
                EdgeTransferPlan::for_baseline(core, &inventory, &assignment).unwrap()
            }
        };
        let plan =
            ControlRecipePlan::new(core, &inventory, &assignment, &transfers, level).unwrap();
        (inventory, plan)
    }

    fn single_eligible_arm(arm: BranchArm) -> SingleArmFixture {
        let (sources, origins) = test_origins();
        let mut core = CoreProgram::new();
        let callee = declare(&mut core, "leaf", vec![], vec![]);
        let caller = declare(&mut core, "caller", vec![CoreType::Bool], vec![]);
        define_empty_function(&mut core, &sources, callee);

        let mut builder = FunctionBuilder::new(&core, &sources, caller).unwrap();
        let entry = builder.entry_block();
        let condition = block_values(&builder, entry)[0];
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                origins.branch,
            ))
            .unwrap();

        let (selected_block, other_block, call_origin, return_origin) = match arm {
            BranchArm::Then => (
                then_block,
                else_block,
                origins.then_call,
                origins.then_return,
            ),
            BranchArm::Else => (
                else_block,
                then_block,
                origins.else_call,
                origins.else_return,
            ),
        };
        let call_instruction = add_zero_abi_terminal_call(
            &mut builder,
            selected_block,
            callee,
            call_origin,
            return_origin,
        );
        add_empty_return(&mut builder, other_block, OriginId::UNKNOWN);
        core.define_function(caller, builder.finish().unwrap())
            .unwrap();

        SingleArmFixture {
            sources,
            core,
            caller,
            callee,
            entry,
            selected_block,
            other_block,
            call_instruction,
            origins: RecipeOrigins {
                branch: origins.branch,
                call: call_origin,
                terminal_return: return_origin,
            },
        }
    }

    fn both_eligible_arms() -> BothArmsFixture {
        let (sources, origins) = test_origins();
        let mut core = CoreProgram::new();
        let callee = declare(&mut core, "leaf", vec![], vec![]);
        let caller = declare(&mut core, "caller", vec![CoreType::Bool], vec![]);
        define_empty_function(&mut core, &sources, callee);

        let mut builder = FunctionBuilder::new(&core, &sources, caller).unwrap();
        let entry = builder.entry_block();
        let condition = block_values(&builder, entry)[0];
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                origins.branch,
            ))
            .unwrap();
        let then_call = add_zero_abi_terminal_call(
            &mut builder,
            then_block,
            callee,
            origins.then_call,
            origins.then_return,
        );
        let else_call = add_zero_abi_terminal_call(
            &mut builder,
            else_block,
            callee,
            origins.else_call,
            origins.else_return,
        );
        core.define_function(caller, builder.finish().unwrap())
            .unwrap();

        BothArmsFixture {
            sources,
            core,
            caller,
            callee,
            entry,
            then_block,
            else_block,
            then_call,
            else_call,
            origins,
        }
    }

    fn duplicate_destination_edges() -> RejectedFixture {
        let (sources, origins) = test_origins();
        let mut core = CoreProgram::new();
        let callee = declare(&mut core, "leaf", vec![], vec![]);
        let caller = declare(&mut core, "caller", vec![CoreType::Bool], vec![]);
        define_empty_function(&mut core, &sources, callee);

        let mut builder = FunctionBuilder::new(&core, &sources, caller).unwrap();
        let entry = builder.entry_block();
        let condition = block_values(&builder, entry)[0];
        let candidate_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(candidate_block, vec![]),
                    else_target: BlockTarget::new(candidate_block, vec![]),
                },
                origins.branch,
            ))
            .unwrap();
        add_zero_abi_terminal_call(
            &mut builder,
            candidate_block,
            callee,
            origins.then_call,
            origins.then_return,
        );
        core.define_function(caller, builder.finish().unwrap())
            .unwrap();

        RejectedFixture {
            sources,
            core,
            caller,
            entry,
            candidate_block,
        }
    }

    fn nonzero_result_callee() -> RejectedFixture {
        let (sources, origins) = test_origins();
        let mut core = CoreProgram::new();
        let callee = declare(&mut core, "result_leaf", vec![], vec![CoreType::I32]);
        let caller = declare(&mut core, "caller", vec![CoreType::Bool], vec![]);

        let mut callee_builder = FunctionBuilder::new(&core, &sources, callee).unwrap();
        let result = callee_builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        callee_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(callee, callee_builder.finish().unwrap())
            .unwrap();

        let mut builder = FunctionBuilder::new(&core, &sources, caller).unwrap();
        let entry = builder.entry_block();
        let condition = block_values(&builder, entry)[0];
        let candidate_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let other_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(candidate_block, vec![]),
                    else_target: BlockTarget::new(other_block, vec![]),
                },
                origins.branch,
            ))
            .unwrap();
        builder.switch_to_block(candidate_block).unwrap();
        builder.call(callee, vec![], origins.then_call).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                origins.then_return,
            ))
            .unwrap();
        add_empty_return(&mut builder, other_block, OriginId::UNKNOWN);
        core.define_function(caller, builder.finish().unwrap())
            .unwrap();

        RejectedFixture {
            sources,
            core,
            caller,
            entry,
            candidate_block,
        }
    }

    fn nonempty_edge_transfer() -> RejectedFixture {
        let (sources, origins) = test_origins();
        let mut core = CoreProgram::new();
        let callee = declare(&mut core, "sink", vec![CoreType::I32], vec![]);
        let caller = declare(
            &mut core,
            "caller",
            vec![CoreType::Bool, CoreType::I32],
            vec![],
        );

        let mut callee_builder = FunctionBuilder::new(&core, &sources, callee).unwrap();
        callee_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(callee, callee_builder.finish().unwrap())
            .unwrap();

        let mut builder = FunctionBuilder::new(&core, &sources, caller).unwrap();
        let entry = builder.entry_block();
        let parameters = block_values(&builder, entry);
        let candidate_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let destination_value = builder
            .append_block_parameter(candidate_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let other_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: parameters[0],
                    then_target: BlockTarget::new(candidate_block, vec![parameters[1]]),
                    else_target: BlockTarget::new(other_block, vec![]),
                },
                origins.branch,
            ))
            .unwrap();
        builder.switch_to_block(candidate_block).unwrap();
        builder
            .call(callee, vec![destination_value], origins.then_call)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                origins.then_return,
            ))
            .unwrap();
        add_empty_return(&mut builder, other_block, OriginId::UNKNOWN);
        core.define_function(caller, builder.finish().unwrap())
            .unwrap();

        RejectedFixture {
            sources,
            core,
            caller,
            entry,
            candidate_block,
        }
    }

    fn declare(
        core: &mut CoreProgram,
        name: &str,
        parameters: Vec<CoreType>,
        results: Vec<CoreType>,
    ) -> FunctionId {
        core.declare_function(Some(name), parameters, results, OriginId::UNKNOWN)
            .unwrap()
    }

    fn define_empty_function(
        core: &mut CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
    ) {
        let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
    }

    fn add_zero_abi_terminal_call(
        builder: &mut FunctionBuilder<'_>,
        block: BlockId,
        callee: FunctionId,
        call_origin: OriginId,
        return_origin: OriginId,
    ) -> InstId {
        builder.switch_to_block(block).unwrap();
        builder.call(callee, vec![], call_origin).unwrap();
        let instruction = builder.body().block(block).unwrap().instructions()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                return_origin,
            ))
            .unwrap();
        instruction
    }

    fn add_empty_return(builder: &mut FunctionBuilder<'_>, block: BlockId, origin: OriginId) {
        builder.switch_to_block(block).unwrap();
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), origin))
            .unwrap();
    }

    fn block_values(builder: &FunctionBuilder<'_>, block: BlockId) -> Vec<ValueId> {
        builder
            .body()
            .block(block)
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect()
    }

    fn test_origins() -> (SourceContext, TestOrigins) {
        let mut sources = SourceContext::new();
        let file = sources.add_file("placement.mdl", "bcdef").unwrap();
        let mut source_origin = |offset| {
            let span = sources.span(file, offset, offset + 1).unwrap();
            sources.add_origin(Origin::Source(span)).unwrap()
        };
        let origins = TestOrigins {
            branch: source_origin(0),
            then_call: source_origin(1),
            then_return: source_origin(2),
            else_call: source_origin(3),
            else_return: source_origin(4),
        };
        (sources, origins)
    }

    const fn opposite(arm: BranchArm) -> BranchArm {
        match arm {
            BranchArm::Then => BranchArm::Else,
            BranchArm::Else => BranchArm::Then,
        }
    }
}
