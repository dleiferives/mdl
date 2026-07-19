use crate::entity::{EntityId, EntityLimitError, EntityVec, entity_id};
use crate::ir::core::{BlockId, CoreOp, CoreProgram, CoreType, FunctionId, InstId, ValueId};

use super::MinecraftOptimizationLevel;
use super::analysis::SemanticInventory;
use super::coalescing::{
    CoalescingError, CoalescingFallbackReason, CoalescingOutcome, CoalescingStatistics,
    coalesce_function,
};
use super::demand::{DemandError, RuntimeDemand};
use super::liveness::FunctionLivenessStatistics;
use super::liveness::LivenessResult;
use super::scalar::scalar_output_contract;

entity_id!(
    /// Function-local identity of one score home selected before final plan flattening.
    pub(crate) struct AssignedHomeId;
);

/// Immutable per-program home and instruction decisions.
#[derive(Clone, Debug)]
pub(crate) struct HomeAssignment {
    level: MinecraftOptimizationLevel,
    functions: EntityVec<FunctionId, FunctionHomeAssignment>,
}

/// Complete function-local assignment result.
#[derive(Clone, Debug)]
pub(crate) struct FunctionHomeAssignment {
    homes: EntityVec<AssignedHomeId, AssignedHome>,
    abi: AssignedFunctionAbi,
    value_assignments: Box<[Option<ValueAssignment>]>,
    instruction_plans: Box<[Option<AssignedInstructionPlan>]>,
    #[allow(
        dead_code,
        reason = "the assignment test oracle inspects the exact scratch inventory independently of final flattening"
    )]
    recipe_temporaries: Box<[AssignedHomeId]>,
    coalescing: Option<FunctionCoalescingDecision>,
}

/// Non-authoritative optimization decision retained for deterministic reporting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FunctionCoalescingDecision {
    fallback_reason: Option<CoalescingFallbackReason>,
    statistics: CoalescingStatistics,
    liveness_statistics: FunctionLivenessStatistics,
}

/// One typed function-local physical home and its allocation role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AssignedHome {
    ty: CoreType,
    role: AssignedHomeRole,
}

/// Why one function-local home exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AssignedHomeRole {
    /// Stage 4-compatible storage for a non-entry semantic value.
    LegacyValue { value: ValueId },
    /// Public function-entry storage, still correlated with its Core value.
    PinnedParameter {
        parameter_index: usize,
        value: ValueId,
    },
    /// Fixed function-result ABI storage.
    PinnedResult { result_index: usize },
    /// An optimized physical register, introduced by the Baseline policy.
    Register { ordinal: usize },
    /// Typed fixed-recipe scratch, introduced by the Baseline policy.
    RecipeTemporary { ordinal: usize },
}

/// Pinned function ABI expressed in function-local homes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AssignedFunctionAbi {
    entry_block: BlockId,
    parameters: Box<[AssignedHomeId]>,
    results: Box<[AssignedHomeId]>,
}

/// Exact semantic-value correlation for one assigned home.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ValueAssignment {
    value: ValueId,
    home: AssignedHomeId,
    ty: CoreType,
}

/// Complete physical requirements for one reachable Core instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AssignedInstructionPlan {
    /// A reachable discardable instruction deliberately produces no commands.
    OmittedPure,
    /// One fixed scalar recipe with exact-arity physical operands and results.
    Scalar {
        operands: Box<[AssignedHomeId]>,
        results: Box<[AssignedScalarResult]>,
    },
    /// One retained call with exact-arity arguments and indexed optional result copies.
    Call {
        arguments: Box<[AssignedHomeId]>,
        result_destinations: Box<[Option<AssignedCallResultDestination>]>,
    },
    /// One retained declaration-backed operation outside the scalar vocabulary.
    External {
        results: Box<[AssignedScalarResult]>,
    },
}

/// One result position required by a fixed scalar recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AssignedScalarResult {
    /// The recipe writes a demanded semantic value.
    Semantic {
        result_index: usize,
        value: ValueId,
        home: AssignedHomeId,
    },
    /// The recipe needs a physical output without a semantic assignment.
    RecipeTemporary {
        result_index: usize,
        home: AssignedHomeId,
    },
}

/// One indexed caller-side result copy retained after a call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AssignedCallResultDestination {
    result_index: usize,
    value: ValueId,
    home: AssignedHomeId,
}

/// Fixed target-physical ordering for per-type pools and grouped work.
pub(super) const PHYSICAL_TYPE_ORDER: [CoreType; 4] = [
    CoreType::Bool,
    CoreType::I32,
    CoreType::ListI32,
    CoreType::String,
];

#[derive(Clone, Debug)]
enum DraftInstructionPlan {
    OmittedPure,
    External {
        results: Box<[AssignedScalarResult]>,
    },
    Scalar {
        operands: Box<[AssignedHomeId]>,
        results: Box<[DraftScalarResult]>,
    },
    Call {
        arguments: Box<[AssignedHomeId]>,
        result_destinations: Box<[Option<AssignedCallResultDestination>]>,
    },
}

impl DraftInstructionPlan {
    fn freeze(
        self,
        function: FunctionId,
        pools: &RecipePools,
    ) -> Result<AssignedInstructionPlan, AssignmentError> {
        match self {
            Self::OmittedPure => Ok(AssignedInstructionPlan::OmittedPure),
            Self::External { results } => Ok(AssignedInstructionPlan::External { results }),
            Self::Call {
                arguments,
                result_destinations,
            } => Ok(AssignedInstructionPlan::Call {
                arguments,
                result_destinations,
            }),
            Self::Scalar { operands, results } => {
                let results = results
                    .into_vec()
                    .into_iter()
                    .map(|result| match result {
                        DraftScalarResult::Semantic {
                            result_index,
                            value,
                            home,
                        } => Ok(AssignedScalarResult::Semantic {
                            result_index,
                            value,
                            home,
                        }),
                        DraftScalarResult::RecipeTemporary {
                            result_index,
                            ty,
                            ordinal,
                        } => Ok(AssignedScalarResult::RecipeTemporary {
                            result_index,
                            home: pools.get(ty, ordinal).ok_or(
                                AssignmentError::MissingRecipeTemporary {
                                    function,
                                    ty,
                                    ordinal,
                                },
                            )?,
                        }),
                    })
                    .collect::<Result<Vec<_>, AssignmentError>>()?
                    .into_boxed_slice();
                Ok(AssignedInstructionPlan::Scalar { operands, results })
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum DraftScalarResult {
    Semantic {
        result_index: usize,
        value: ValueId,
        home: AssignedHomeId,
    },
    RecipeTemporary {
        result_index: usize,
        ty: CoreType,
        ordinal: usize,
    },
}

#[derive(Default)]
struct RecipeRequirements {
    counts: [usize; PHYSICAL_TYPE_ORDER.len()],
}

struct BaselineInstructionDrafts {
    plans: Box<[Option<DraftInstructionPlan>]>,
    recipe_requirements: RecipeRequirements,
}

type BaselineValueHomes = (
    EntityVec<AssignedHomeId, AssignedHome>,
    Vec<Option<ValueAssignment>>,
);

impl RecipeRequirements {
    fn include(&mut self, counts: [usize; PHYSICAL_TYPE_ORDER.len()]) {
        for (required, count) in self.counts.iter_mut().zip(counts) {
            *required = (*required).max(count);
        }
    }
}

#[derive(Default)]
struct RecipePools {
    homes: [Vec<AssignedHomeId>; PHYSICAL_TYPE_ORDER.len()],
}

impl RecipePools {
    fn get(&self, ty: CoreType, ordinal: usize) -> Option<AssignedHomeId> {
        self.homes[recipe_type_index(ty)].get(ordinal).copied()
    }
}

const fn recipe_type_index(ty: CoreType) -> usize {
    match ty {
        CoreType::Bool => 0,
        CoreType::I32 => 1,
        CoreType::ListI32 => 2,
        CoreType::String => 3,
    }
}

impl HomeAssignment {
    /// Builds the exact Stage 4-compatible one-value/one-home policy.
    pub(crate) fn for_none(
        core: &CoreProgram,
        inventory: &SemanticInventory,
    ) -> Result<Self, AssignmentError> {
        let mut functions = EntityVec::new();
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(AssignmentError::MissingDefinition { function })?;
            let semantic = inventory
                .function(function)
                .ok_or(AssignmentError::MissingInventory { function })?;
            let assignment =
                FunctionHomeAssignment::for_none(core, function, declaration, body, semantic)?;
            let assigned = functions.push(assignment).map_err(|EntityLimitError| {
                AssignmentError::EntityLimit {
                    function: None,
                    table: AssignmentTable::Functions,
                }
            })?;
            debug_assert_eq!(assigned, function);
        }
        Ok(Self {
            level: MinecraftOptimizationLevel::None,
            functions,
        })
    }

    /// Builds the Baseline policy from complete demand and liveness phase records.
    pub(crate) fn for_baseline(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        demand: &RuntimeDemand,
        liveness: &LivenessResult,
    ) -> Result<Self, AssignmentError> {
        if !demand.matches_level(MinecraftOptimizationLevel::Baseline) {
            return Err(AssignmentError::DemandPolicyMismatch);
        }
        if !liveness.matches_level(MinecraftOptimizationLevel::Baseline)
            || liveness.len() != core.len()
        {
            return Err(AssignmentError::LivenessAlignment);
        }
        let mut functions = EntityVec::new();
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(AssignmentError::MissingDefinition { function })?;
            let semantic = inventory
                .function(function)
                .ok_or(AssignmentError::MissingInventory { function })?;
            let assignment = FunctionHomeAssignment::for_baseline(
                core,
                function,
                declaration,
                body,
                semantic,
                inventory,
                demand,
                liveness,
            )?;
            let assigned = functions.push(assignment).map_err(|EntityLimitError| {
                AssignmentError::EntityLimit {
                    function: None,
                    table: AssignmentTable::Functions,
                }
            })?;
            debug_assert_eq!(assigned, function);
        }
        Ok(Self {
            level: MinecraftOptimizationLevel::Baseline,
            functions,
        })
    }

    /// Test convenience preserving phase-local fixture setup.
    #[cfg(test)]
    pub(crate) fn for_baseline_derived_liveness(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        demand: &RuntimeDemand,
    ) -> Result<Self, AssignmentError> {
        let liveness = LivenessResult::for_baseline(
            core,
            inventory,
            demand,
            super::liveness::LivenessLimits::derived(),
        )
        .map_err(AssignmentError::Liveness)?;
        Self::for_baseline(core, inventory, demand, &liveness)
    }

    pub(crate) const fn level(&self) -> MinecraftOptimizationLevel {
        self.level
    }

    pub(crate) fn matches_level(&self, level: MinecraftOptimizationLevel) -> bool {
        self.level == level
    }

    pub(crate) fn function(&self, function: FunctionId) -> Option<&FunctionHomeAssignment> {
        self.functions.get(function)
    }

    pub(crate) fn len(&self) -> usize {
        self.functions.len()
    }
}

impl FunctionHomeAssignment {
    fn for_none(
        core: &CoreProgram,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        inventory: &super::analysis::FunctionSemanticInventory,
    ) -> Result<Self, AssignmentError> {
        let entry = body.entry();
        let entry_data = body
            .block(entry)
            .ok_or(AssignmentError::InvalidCoreEntity { function })?;
        let mut entry_parameter_indices = vec![None; body.value_counts().allocated];
        for (parameter_index, parameter) in entry_data.parameters().iter().enumerate() {
            set_indexed_slot(
                &mut entry_parameter_indices,
                parameter.value(),
                parameter_index,
                function,
            )?;
        }

        let mut homes = EntityVec::new();
        let mut value_assignments = vec![None; body.value_counts().allocated];
        for value in inventory.reachable_values().iter().copied() {
            let ty = body
                .value(value)
                .ok_or(AssignmentError::InvalidCoreEntity { function })?
                .ty();
            let role = indexed_slot(&entry_parameter_indices, value)
                .flatten()
                .map_or(AssignedHomeRole::LegacyValue { value }, |parameter_index| {
                    AssignedHomeRole::PinnedParameter {
                        parameter_index,
                        value,
                    }
                });
            let home = push_home(&mut homes, function, AssignedHome { ty, role })?;
            set_indexed_slot(
                &mut value_assignments,
                value,
                ValueAssignment { value, home, ty },
                function,
            )?;
        }

        let mut results = Vec::with_capacity(declaration.results().len());
        for (result_index, ty) in declaration.results().iter().copied().enumerate() {
            results.push(push_home(
                &mut homes,
                function,
                AssignedHome {
                    ty,
                    role: AssignedHomeRole::PinnedResult { result_index },
                },
            )?);
        }
        let parameters = entry_data
            .parameters()
            .iter()
            .map(|parameter| value_home(&value_assignments, function, parameter.value()))
            .collect::<Result<Vec<_>, _>>()?;

        let instruction_plans =
            plan_none_instructions(core, function, body, inventory, &value_assignments)?;

        Ok(Self {
            homes,
            abi: AssignedFunctionAbi {
                entry_block: entry,
                parameters: parameters.into_boxed_slice(),
                results: results.into_boxed_slice(),
            },
            value_assignments: value_assignments.into_boxed_slice(),
            instruction_plans,
            recipe_temporaries: Box::new([]),
            coalescing: None,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the phase boundary keeps Core, inventory, and demand authorities explicit"
    )]
    fn for_baseline(
        core: &CoreProgram,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        function_inventory: &super::analysis::FunctionSemanticInventory,
        inventory: &SemanticInventory,
        demand: &RuntimeDemand,
        liveness: &LivenessResult,
    ) -> Result<Self, AssignmentError> {
        let function_liveness = liveness
            .function(function)
            .ok_or(AssignmentError::LivenessAlignment)?;
        let entry = body.entry();
        let entry_data = body
            .block(entry)
            .ok_or(AssignmentError::InvalidCoreEntity { function })?;
        let mut entry_parameter_indices = vec![None; body.value_counts().allocated];
        for (parameter_index, parameter) in entry_data.parameters().iter().enumerate() {
            set_indexed_slot(
                &mut entry_parameter_indices,
                parameter.value(),
                parameter_index,
                function,
            )?;
        }

        let coalescing = if let Some(completed) = function_liveness.completed() {
            coalesce_function(function, body, function_inventory, completed)?
        } else {
            CoalescingOutcome::DistinctFallback {
                reason: CoalescingFallbackReason::Liveness(
                    function_liveness
                        .fallback_reason()
                        .ok_or(AssignmentError::LivenessAlignment)?,
                ),
                statistics: CoalescingStatistics::default(),
            }
        };
        let coalescing_decision = FunctionCoalescingDecision {
            fallback_reason: coalescing.fallback_reason(),
            statistics: coalescing.statistics(),
            liveness_statistics: function_liveness.statistics(),
        };
        let (mut homes, value_assignments) = allocate_baseline_value_homes(
            function,
            body,
            function_inventory,
            inventory,
            demand,
            &entry_parameter_indices,
            &coalescing,
        )?;

        let mut abi_results = Vec::with_capacity(declaration.results().len());
        for (result_index, ty) in declaration.results().iter().copied().enumerate() {
            abi_results.push(push_home(
                &mut homes,
                function,
                AssignedHome {
                    ty,
                    role: AssignedHomeRole::PinnedResult { result_index },
                },
            )?);
        }
        let parameters = entry_data
            .parameters()
            .iter()
            .map(|parameter| value_home(&value_assignments, function, parameter.value()))
            .collect::<Result<Vec<_>, _>>()?;

        let BaselineInstructionDrafts {
            plans: draft_plans,
            recipe_requirements,
        } = plan_baseline_instructions(
            core,
            function,
            body,
            function_inventory,
            inventory,
            demand,
            &value_assignments,
        )?;
        let (recipe_pools, recipe_temporaries) =
            allocate_recipe_temporaries(&mut homes, function, &recipe_requirements)?;
        let instruction_plans = freeze_instruction_plans(function, draft_plans, &recipe_pools)?;

        Ok(Self {
            homes,
            abi: AssignedFunctionAbi {
                entry_block: entry,
                parameters: parameters.into_boxed_slice(),
                results: abi_results.into_boxed_slice(),
            },
            value_assignments: value_assignments.into_boxed_slice(),
            instruction_plans,
            recipe_temporaries,
            coalescing: Some(coalescing_decision),
        })
    }

    pub(crate) fn homes(
        &self,
    ) -> impl ExactSizeIterator<Item = (AssignedHomeId, &AssignedHome)> + '_ {
        self.homes.iter()
    }

    pub(crate) fn home(&self, home: AssignedHomeId) -> Option<&AssignedHome> {
        self.homes.get(home)
    }

    pub(crate) const fn abi(&self) -> &AssignedFunctionAbi {
        &self.abi
    }

    pub(crate) fn value_assignment(&self, value: ValueId) -> Option<ValueAssignment> {
        indexed_slot(&self.value_assignments, value).flatten()
    }

    pub(crate) fn value_assignment_slots(&self) -> &[Option<ValueAssignment>] {
        &self.value_assignments
    }

    #[allow(
        dead_code,
        reason = "assignment unit tests inspect one instruction without coupling to final plan assembly"
    )]
    pub(crate) fn instruction_plan(&self, instruction: InstId) -> Option<&AssignedInstructionPlan> {
        usize::try_from(instruction.index())
            .ok()
            .and_then(|index| self.instruction_plans.get(index))
            .and_then(Option::as_ref)
    }

    pub(crate) fn instruction_slots(&self) -> &[Option<AssignedInstructionPlan>] {
        &self.instruction_plans
    }

    #[allow(
        dead_code,
        reason = "assignment unit tests assert exact recipe-scratch ownership before flattening"
    )]
    pub(crate) fn recipe_temporaries(&self) -> &[AssignedHomeId] {
        &self.recipe_temporaries
    }

    pub(crate) const fn coalescing(&self) -> Option<FunctionCoalescingDecision> {
        self.coalescing
    }
}

impl FunctionCoalescingDecision {
    pub(crate) const fn fallback_reason(self) -> Option<CoalescingFallbackReason> {
        self.fallback_reason
    }

    pub(crate) const fn statistics(self) -> CoalescingStatistics {
        self.statistics
    }

    pub(crate) const fn liveness_statistics(self) -> FunctionLivenessStatistics {
        self.liveness_statistics
    }
}

impl AssignedHome {
    pub(crate) const fn ty(self) -> CoreType {
        self.ty
    }

    pub(crate) const fn role(self) -> AssignedHomeRole {
        self.role
    }
}

impl AssignedFunctionAbi {
    pub(crate) const fn entry_block(&self) -> BlockId {
        self.entry_block
    }

    pub(crate) fn parameters(&self) -> &[AssignedHomeId] {
        &self.parameters
    }

    pub(crate) fn results(&self) -> &[AssignedHomeId] {
        &self.results
    }
}

impl ValueAssignment {
    pub(crate) const fn value(self) -> ValueId {
        self.value
    }

    pub(crate) const fn home(self) -> AssignedHomeId {
        self.home
    }

    pub(crate) const fn ty(self) -> CoreType {
        self.ty
    }
}

impl AssignedCallResultDestination {
    pub(crate) const fn result_index(self) -> usize {
        self.result_index
    }

    pub(crate) const fn value(self) -> ValueId {
        self.value
    }

    pub(crate) const fn home(self) -> AssignedHomeId {
        self.home
    }
}

impl AssignedInstructionPlan {
    pub(crate) fn scalar_operands(&self) -> Option<&[AssignedHomeId]> {
        match self {
            Self::Scalar { operands, .. } => Some(operands),
            Self::OmittedPure | Self::Call { .. } | Self::External { .. } => None,
        }
    }

    pub(crate) fn scalar_results(&self) -> Option<&[AssignedScalarResult]> {
        match self {
            Self::Scalar { results, .. } => Some(results),
            Self::OmittedPure | Self::Call { .. } | Self::External { .. } => None,
        }
    }

    pub(crate) fn call_arguments(&self) -> Option<&[AssignedHomeId]> {
        match self {
            Self::Call { arguments, .. } => Some(arguments),
            Self::OmittedPure | Self::Scalar { .. } | Self::External { .. } => None,
        }
    }

    pub(crate) fn call_result_destinations(
        &self,
    ) -> Option<&[Option<AssignedCallResultDestination>]> {
        match self {
            Self::Call {
                result_destinations,
                ..
            } => Some(result_destinations),
            Self::OmittedPure | Self::Scalar { .. } | Self::External { .. } => None,
        }
    }
}

impl AssignedScalarResult {
    pub(crate) const fn result_index(self) -> usize {
        match self {
            Self::Semantic { result_index, .. } | Self::RecipeTemporary { result_index, .. } => {
                result_index
            }
        }
    }

    pub(crate) const fn semantic_value(self) -> Option<ValueId> {
        match self {
            Self::Semantic { value, .. } => Some(value),
            Self::RecipeTemporary { .. } => None,
        }
    }

    pub(crate) const fn home(self) -> AssignedHomeId {
        match self {
            Self::Semantic { home, .. } | Self::RecipeTemporary { home, .. } => home,
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "physical allocation consumes the immutable semantic, demand, and coalescing authorities"
)]
fn allocate_baseline_value_homes(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    function_inventory: &super::analysis::FunctionSemanticInventory,
    inventory: &SemanticInventory,
    demand: &RuntimeDemand,
    entry_parameter_indices: &[Option<usize>],
    coalescing: &CoalescingOutcome,
) -> Result<BaselineValueHomes, AssignmentError> {
    let groups = coalescing.groups();
    let mut group_homes = groups.map(|groups| vec![None; groups.len()]);
    let mut homes = EntityVec::new();
    let mut value_assignments = vec![None; body.value_counts().allocated];
    let mut register_ordinal = 0_usize;
    for value in function_inventory.reachable_values().iter().copied() {
        let ty = body
            .value(value)
            .ok_or(AssignmentError::InvalidCoreEntity { function })?
            .ty();
        let parameter_index = indexed_slot(entry_parameter_indices, value).flatten();
        let role = if let Some(parameter_index) = parameter_index {
            AssignedHomeRole::PinnedParameter {
                parameter_index,
                value,
            }
        } else {
            if !demand.requires_value(function, value, inventory)? {
                continue;
            }
            if let (Some(groups), Some(group_homes)) = (groups, group_homes.as_mut()) {
                let group = groups
                    .group(value)
                    .ok_or(AssignmentError::MissingCoalescingGroup { function, value })?;
                if let Some(home) = group_homes.get(group).copied().flatten() {
                    set_indexed_slot(
                        &mut value_assignments,
                        value,
                        ValueAssignment { value, home, ty },
                        function,
                    )?;
                    continue;
                }
            }
            let ordinal = register_ordinal;
            register_ordinal = register_ordinal
                .checked_add(1)
                .ok_or(AssignmentError::OrdinalOverflow { function })?;
            AssignedHomeRole::Register { ordinal }
        };
        let home = push_home(&mut homes, function, AssignedHome { ty, role })?;
        set_indexed_slot(
            &mut value_assignments,
            value,
            ValueAssignment { value, home, ty },
            function,
        )?;
        if parameter_index.is_none() {
            if let (Some(groups), Some(group_homes)) = (groups, group_homes.as_mut()) {
                let group = groups
                    .group(value)
                    .ok_or(AssignmentError::MissingCoalescingGroup { function, value })?;
                let slot = group_homes
                    .get_mut(group)
                    .ok_or(AssignmentError::MissingCoalescingGroup { function, value })?;
                *slot = Some(home);
            }
        }
    }
    Ok((homes, value_assignments))
}

fn plan_none_instructions(
    core: &CoreProgram,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    inventory: &super::analysis::FunctionSemanticInventory,
    value_assignments: &[Option<ValueAssignment>],
) -> Result<Box<[Option<AssignedInstructionPlan>]>, AssignmentError> {
    let mut plans = vec![None; body.instruction_counts().allocated];
    for instruction in inventory.reachable_instructions().iter().copied() {
        let data = body
            .instruction(instruction)
            .ok_or(AssignmentError::InvalidCoreEntity { function })?;
        validate_instruction_shape(core, function, instruction, data)?;
        let operands = assigned_operands(function, data, value_assignments)?;
        let plan = match data.op() {
            CoreOp::Call(_) => {
                let result_destinations = data
                    .results()
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(result_index, value)| {
                        Ok(Some(AssignedCallResultDestination {
                            result_index,
                            value,
                            home: value_home(value_assignments, function, value)?,
                        }))
                    })
                    .collect::<Result<Vec<_>, AssignmentError>>()?
                    .into_boxed_slice();
                AssignedInstructionPlan::Call {
                    arguments: operands,
                    result_destinations,
                }
            }
            CoreOp::BoolConstant(_)
            | CoreOp::I32Constant(_)
            | CoreOp::I32AddWrapping
            | CoreOp::I32SubWrapping
            | CoreOp::I32AddOverflowing
            | CoreOp::I32Compare(_)
            | CoreOp::BoolNot
            | CoreOp::ListI32Empty
            | CoreOp::ListI32Length
            | CoreOp::ListI32Push
            | CoreOp::ListI32LastOrZero
            | CoreOp::ListI32WithoutLast
            | CoreOp::StringConstant(_)
            | CoreOp::StringLength
            | CoreOp::StringEndsWithAscii(_)
            | CoreOp::StringWithoutLastUnit => {
                let results = data
                    .results()
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(result_index, value)| {
                        Ok(AssignedScalarResult::Semantic {
                            result_index,
                            value,
                            home: value_home(value_assignments, function, value)?,
                        })
                    })
                    .collect::<Result<Vec<_>, AssignmentError>>()?
                    .into_boxed_slice();
                AssignedInstructionPlan::Scalar { operands, results }
            }
            CoreOp::External(_) => AssignedInstructionPlan::External {
                results: data
                    .results()
                    .iter()
                    .copied()
                    .enumerate()
                    .map(|(result_index, value)| {
                        Ok(AssignedScalarResult::Semantic {
                            result_index,
                            value,
                            home: value_home(value_assignments, function, value)?,
                        })
                    })
                    .collect::<Result<Vec<_>, AssignmentError>>()?
                    .into_boxed_slice(),
            },
        };
        set_indexed_slot(&mut plans, instruction, plan, function)?;
    }
    Ok(plans.into_boxed_slice())
}

fn assigned_operands(
    function: FunctionId,
    data: &crate::ir::core::InstData,
    assignments: &[Option<ValueAssignment>],
) -> Result<Box<[AssignedHomeId]>, AssignmentError> {
    data.operands()
        .iter()
        .copied()
        .map(|value| value_home(assignments, function, value))
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

#[allow(
    clippy::too_many_arguments,
    reason = "the planner validates each immutable phase authority at its boundary"
)]
fn plan_baseline_instructions(
    core: &CoreProgram,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    inventory: &super::analysis::FunctionSemanticInventory,
    program_inventory: &SemanticInventory,
    demand: &RuntimeDemand,
    assignments: &[Option<ValueAssignment>],
) -> Result<BaselineInstructionDrafts, AssignmentError> {
    let mut plans = vec![None; body.instruction_counts().allocated];
    let mut recipe_requirements = RecipeRequirements::default();
    for instruction in inventory.reachable_instructions().iter().copied() {
        let data = body
            .instruction(instruction)
            .ok_or(AssignmentError::InvalidCoreEntity { function })?;
        validate_instruction_shape(core, function, instruction, data)?;
        let plan = if demand.requires_instruction(function, instruction, program_inventory)? {
            draft_retained_instruction(
                function,
                body,
                instruction,
                data,
                assignments,
                &mut recipe_requirements,
            )?
        } else {
            validate_baseline_omission(function, instruction, data, assignments)?;
            DraftInstructionPlan::OmittedPure
        };
        set_indexed_slot(&mut plans, instruction, plan, function)?;
    }
    Ok(BaselineInstructionDrafts {
        plans: plans.into_boxed_slice(),
        recipe_requirements,
    })
}

fn validate_baseline_omission(
    function: FunctionId,
    instruction: InstId,
    data: &crate::ir::core::InstData,
    assignments: &[Option<ValueAssignment>],
) -> Result<(), AssignmentError> {
    let has_assigned_result = data
        .results()
        .iter()
        .copied()
        .any(|value| indexed_slot(assignments, value).flatten().is_some());
    if data.op().is_trivially_discardable() && !has_assigned_result {
        Ok(())
    } else {
        Err(AssignmentError::InvalidOmission {
            function,
            instruction,
        })
    }
}

fn draft_retained_instruction(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    instruction: InstId,
    data: &crate::ir::core::InstData,
    assignments: &[Option<ValueAssignment>],
    recipe_requirements: &mut RecipeRequirements,
) -> Result<DraftInstructionPlan, AssignmentError> {
    if matches!(data.op(), CoreOp::External(_)) {
        let results = data
            .results()
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(result_index, value)| {
                indexed_slot(assignments, value)
                    .flatten()
                    .map(|assignment| AssignedScalarResult::Semantic {
                        result_index,
                        value,
                        home: assignment.home(),
                    })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        return Ok(DraftInstructionPlan::External { results });
    }
    let operands = assigned_operands(function, data, assignments)?;
    if matches!(data.op(), CoreOp::Call(_)) {
        let result_destinations = data
            .results()
            .iter()
            .copied()
            .enumerate()
            .map(|(result_index, value)| {
                indexed_slot(assignments, value)
                    .flatten()
                    .map(|assignment| AssignedCallResultDestination {
                        result_index,
                        value,
                        home: assignment.home(),
                    })
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        return Ok(DraftInstructionPlan::Call {
            arguments: operands,
            result_destinations,
        });
    }

    let (results, local_recipe_counts) =
        draft_scalar_results(function, body, instruction, data, assignments)?;
    recipe_requirements.include(local_recipe_counts);
    Ok(DraftInstructionPlan::Scalar { operands, results })
}

fn draft_scalar_results(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    instruction: InstId,
    data: &crate::ir::core::InstData,
    assignments: &[Option<ValueAssignment>],
) -> Result<(Box<[DraftScalarResult]>, [usize; PHYSICAL_TYPE_ORDER.len()]), AssignmentError> {
    let contract =
        scalar_output_contract(data.op()).ok_or(AssignmentError::InvalidInstructionShape {
            function,
            instruction,
        })?;
    if contract.result_types().len() != data.results().len() {
        return Err(AssignmentError::InvalidInstructionShape {
            function,
            instruction,
        });
    }

    let mut local_recipe_counts = [0_usize; PHYSICAL_TYPE_ORDER.len()];
    let mut results = Vec::with_capacity(data.results().len());
    for (result_index, (value, expected_type)) in data
        .results()
        .iter()
        .copied()
        .zip(contract.result_types().iter().copied())
        .enumerate()
    {
        let actual_type = body
            .value(value)
            .ok_or(AssignmentError::InvalidCoreEntity { function })?
            .ty();
        if actual_type != expected_type {
            return Err(AssignmentError::InvalidInstructionShape {
                function,
                instruction,
            });
        }
        if let Some(assignment) = indexed_slot(assignments, value).flatten() {
            results.push(DraftScalarResult::Semantic {
                result_index,
                value,
                home: assignment.home(),
            });
        } else {
            let type_index = recipe_type_index(expected_type);
            let ordinal = local_recipe_counts[type_index];
            local_recipe_counts[type_index] = local_recipe_counts[type_index]
                .checked_add(1)
                .ok_or(AssignmentError::OrdinalOverflow { function })?;
            results.push(DraftScalarResult::RecipeTemporary {
                result_index,
                ty: expected_type,
                ordinal,
            });
        }
    }
    if results
        .iter()
        .all(|result| matches!(result, DraftScalarResult::RecipeTemporary { .. }))
    {
        return Err(AssignmentError::InvalidRetainedScalar {
            function,
            instruction,
        });
    }
    Ok((results.into_boxed_slice(), local_recipe_counts))
}

fn allocate_recipe_temporaries(
    homes: &mut EntityVec<AssignedHomeId, AssignedHome>,
    function: FunctionId,
    requirements: &RecipeRequirements,
) -> Result<(RecipePools, Box<[AssignedHomeId]>), AssignmentError> {
    let mut pools = RecipePools::default();
    let mut temporaries = Vec::new();
    for (type_index, ty) in PHYSICAL_TYPE_ORDER.iter().copied().enumerate() {
        for ordinal in 0..requirements.counts[type_index] {
            let home = push_home(
                homes,
                function,
                AssignedHome {
                    ty,
                    role: AssignedHomeRole::RecipeTemporary { ordinal },
                },
            )?;
            pools.homes[type_index].push(home);
            temporaries.push(home);
        }
    }
    Ok((pools, temporaries.into_boxed_slice()))
}

fn freeze_instruction_plans(
    function: FunctionId,
    drafts: Box<[Option<DraftInstructionPlan>]>,
    pools: &RecipePools,
) -> Result<Box<[Option<AssignedInstructionPlan>]>, AssignmentError> {
    drafts
        .into_vec()
        .into_iter()
        .map(|plan| plan.map(|plan| plan.freeze(function, pools)).transpose())
        .collect::<Result<Vec<_>, AssignmentError>>()
        .map(Vec::into_boxed_slice)
}

fn push_home(
    homes: &mut EntityVec<AssignedHomeId, AssignedHome>,
    function: FunctionId,
    home: AssignedHome,
) -> Result<AssignedHomeId, AssignmentError> {
    homes
        .push(home)
        .map_err(|EntityLimitError| AssignmentError::EntityLimit {
            function: Some(function),
            table: AssignmentTable::Homes,
        })
}

fn validate_instruction_shape(
    core: &CoreProgram,
    function: FunctionId,
    instruction: InstId,
    data: &crate::ir::core::InstData,
) -> Result<(), AssignmentError> {
    let signature = data
        .op()
        .signature(core)
        .ok_or(AssignmentError::InvalidCoreEntity { function })?;
    if signature.operands.len() != data.operands().len()
        || signature.results.len() != data.results().len()
    {
        Err(AssignmentError::InvalidInstructionShape {
            function,
            instruction,
        })
    } else {
        Ok(())
    }
}

fn value_home(
    assignments: &[Option<ValueAssignment>],
    function: FunctionId,
    value: ValueId,
) -> Result<AssignedHomeId, AssignmentError> {
    indexed_slot(assignments, value)
        .flatten()
        .map(ValueAssignment::home)
        .ok_or(AssignmentError::MissingValueAssignment { function, value })
}

fn set_indexed_slot<I: EntityId, T>(
    slots: &mut [Option<T>],
    id: I,
    value: T,
    function: FunctionId,
) -> Result<(), AssignmentError> {
    let slot = usize::try_from(id.index())
        .ok()
        .and_then(|index| slots.get_mut(index))
        .ok_or(AssignmentError::InvalidCoreEntity { function })?;
    if slot.is_some() {
        return Err(AssignmentError::DuplicateAssignment { function });
    }
    *slot = Some(value);
    Ok(())
}

fn indexed_slot<I: EntityId, T: Copy>(slots: &[T], id: I) -> Option<T> {
    usize::try_from(id.index())
        .ok()
        .and_then(|index| slots.get(index))
        .copied()
}

/// Checked failure while producing a function-local assignment result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AssignmentError {
    MissingDefinition {
        function: FunctionId,
    },
    MissingInventory {
        function: FunctionId,
    },
    InvalidCoreEntity {
        function: FunctionId,
    },
    MissingValueAssignment {
        function: FunctionId,
        value: ValueId,
    },
    MissingCoalescingGroup {
        function: FunctionId,
        value: ValueId,
    },
    DuplicateAssignment {
        function: FunctionId,
    },
    InvalidInstructionShape {
        function: FunctionId,
        instruction: InstId,
    },
    InvalidOmission {
        function: FunctionId,
        instruction: InstId,
    },
    InvalidRetainedScalar {
        function: FunctionId,
        instruction: InstId,
    },
    MissingRecipeTemporary {
        function: FunctionId,
        ty: CoreType,
        ordinal: usize,
    },
    OrdinalOverflow {
        function: FunctionId,
    },
    DemandPolicyMismatch,
    LivenessAlignment,
    Demand(DemandError),
    Liveness(super::liveness::LivenessError),
    Coalescing(CoalescingError),
    EntityLimit {
        function: Option<FunctionId>,
        table: AssignmentTable,
    },
}

impl From<DemandError> for AssignmentError {
    fn from(error: DemandError) -> Self {
        Self::Demand(error)
    }
}

impl From<super::liveness::LivenessError> for AssignmentError {
    fn from(error: super::liveness::LivenessError) -> Self {
        Self::Liveness(error)
    }
}

impl From<CoalescingError> for AssignmentError {
    fn from(error: CoalescingError) -> Self {
        Self::Coalescing(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AssignmentTable {
    Functions,
    Homes,
}

#[cfg(test)]
mod tests {
    use super::{AssignedHomeRole, AssignedInstructionPlan, AssignedScalarResult, HomeAssignment};
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionEditor, FunctionId,
        Terminator, TerminatorKind, ValueDef, ValueId,
    };
    use crate::lower::minecraft::MinecraftOptimizationLevel;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::demand::{
        RuntimeDemand, RuntimeDemandCompletion, RuntimeDemandFallbackReason, RuntimeDemandLimits,
        RuntimeDemandTableLimit,
    };
    use crate::source::{OriginId, SourceContext};

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one expected-order fixture keeps the complete ABI and home order visible"
    )]
    fn none_preserves_inventory_value_order_types_and_pinned_abi() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("ordered"),
                vec![CoreType::I32, CoreType::Bool],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let parameters = block_values(&builder, entry);
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(parameters[0], one, OriginId::UNKNOWN)
            .unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![sum])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![joined]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let function_assignment = assignment.function(function).unwrap();
        let ordered = function_assignment
            .homes()
            .map(|(home, data)| (home.index(), data.ty(), data.role()))
            .collect::<Vec<_>>();

        assert_eq!(
            ordered,
            vec![
                (
                    0,
                    CoreType::I32,
                    AssignedHomeRole::PinnedParameter {
                        parameter_index: 0,
                        value: parameters[0],
                    },
                ),
                (
                    1,
                    CoreType::Bool,
                    AssignedHomeRole::PinnedParameter {
                        parameter_index: 1,
                        value: parameters[1],
                    },
                ),
                (
                    2,
                    CoreType::I32,
                    AssignedHomeRole::LegacyValue { value: one }
                ),
                (
                    3,
                    CoreType::I32,
                    AssignedHomeRole::LegacyValue { value: sum }
                ),
                (
                    4,
                    CoreType::I32,
                    AssignedHomeRole::LegacyValue { value: joined },
                ),
                (
                    5,
                    CoreType::I32,
                    AssignedHomeRole::PinnedResult { result_index: 0 },
                ),
            ]
        );
        assert_eq!(function_assignment.abi().entry_block(), entry);
        assert_eq!(
            function_assignment
                .abi()
                .parameters()
                .iter()
                .map(|home| home.index())
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(function_assignment.abi().results()[0].index(), 5);
        assert!(function_assignment.recipe_temporaries().is_empty());
        for value in [parameters[0], parameters[1], one, sum, joined] {
            let mapped = function_assignment.value_assignment(value).unwrap();
            assert_eq!(mapped.value(), value);
            assert_eq!(
                function_assignment
                    .homes()
                    .find(|(home, _)| *home == mapped.home())
                    .unwrap()
                    .1
                    .ty(),
                mapped.ty()
            );
        }
    }

    #[test]
    fn none_records_both_overflow_results_as_indexed_semantic_destinations() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("overflow"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let parameters = block_values(&builder, builder.entry_block());
        let results = builder
            .i32_add_overflowing(parameters[0], parameters[1], OriginId::UNKNOWN)
            .unwrap();
        let instruction = defining_instruction(builder.body(), results.0);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![results.0, results.1]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let function_assignment = assignment.function(function).unwrap();
        let AssignedInstructionPlan::Scalar {
            operands,
            results: planned,
        } = function_assignment.instruction_plan(instruction).unwrap()
        else {
            panic!("overflowing add must use a scalar plan")
        };
        assert_eq!(operands.len(), 2);
        assert_eq!(planned.len(), 2);
        for (result_index, (planned, value)) in
            planned.iter().zip([results.0, results.1]).enumerate()
        {
            assert_eq!(
                *planned,
                AssignedScalarResult::Semantic {
                    result_index,
                    value,
                    home: function_assignment.value_assignment(value).unwrap().home(),
                }
            );
        }
        assert!(
            planned
                .iter()
                .all(|result| !matches!(result, AssignedScalarResult::RecipeTemporary { .. }))
        );
    }

    #[test]
    fn none_call_plan_preserves_every_multi_result_index() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let entry_function = core
            .declare_function(Some("caller"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let target_function = core
            .declare_function(
                Some("callee"),
                vec![CoreType::I32, CoreType::Bool],
                vec![CoreType::Bool, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut entry_builder = FunctionBuilder::new(&core, &sources, entry_function).unwrap();
        let integer = entry_builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let boolean = entry_builder
            .bool_constant(true, OriginId::UNKNOWN)
            .unwrap();
        let call_results = entry_builder
            .call(target_function, vec![integer, boolean], OriginId::UNKNOWN)
            .unwrap();
        let call_instruction = defining_instruction(entry_builder.body(), call_results[0]);
        entry_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(entry_function, entry_builder.finish().unwrap())
            .unwrap();

        let mut target_builder = FunctionBuilder::new(&core, &sources, target_function).unwrap();
        let target_parameters = block_values(&target_builder, target_builder.entry_block());
        target_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![target_parameters[1], target_parameters[0]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(target_function, target_builder.finish().unwrap())
            .unwrap();

        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let entry_assignment = assignment.function(entry_function).unwrap();
        let AssignedInstructionPlan::Call {
            arguments,
            result_destinations,
        } = entry_assignment.instruction_plan(call_instruction).unwrap()
        else {
            panic!("call must use a call plan")
        };
        assert_eq!(arguments.len(), 2);
        assert_eq!(result_destinations.len(), 2);
        for (result_index, (destination, value)) in
            result_destinations.iter().zip(call_results).enumerate()
        {
            let destination = destination.unwrap();
            assert_eq!(destination.result_index, result_index);
            assert_eq!(destination.value, value);
            assert_eq!(
                destination.home,
                entry_assignment.value_assignment(value).unwrap().home()
            );
        }
    }

    #[test]
    fn unreachable_and_detached_instructions_keep_distinct_empty_dense_slots() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("history"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let reachable = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let detached = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let reachable_instruction = defining_instruction(builder.body(), reachable);
        let detached_instruction = defining_instruction(builder.body(), detached);
        let unreachable_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(unreachable_block).unwrap();
        let unreachable = builder.i32_constant(3, OriginId::UNKNOWN).unwrap();
        let unreachable_instruction = defining_instruction(builder.body(), unreachable);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        FunctionEditor::new(&core, &sources, function, &mut body)
            .unwrap()
            .erase_pure_inst(detached_instruction)
            .unwrap();
        core.define_function(function, body).unwrap();

        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let function_assignment = assignment.function(function).unwrap();

        assert!(matches!(
            function_assignment.instruction_plan(reachable_instruction),
            Some(AssignedInstructionPlan::Scalar { .. })
        ));
        assert!(
            function_assignment
                .instruction_plan(detached_instruction)
                .is_none()
        );
        assert!(
            function_assignment
                .instruction_plan(unreachable_instruction)
                .is_none()
        );
        assert_eq!(function_assignment.instruction_slots().len(), 3);
        assert!(function_assignment.value_assignment(reachable).is_some());
        assert!(function_assignment.value_assignment(detached).is_none());
        assert!(function_assignment.value_assignment(unreachable).is_none());
    }

    #[test]
    fn baseline_overflowing_add_covers_all_four_result_masks() {
        for (demand_sum, demand_overflow) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (core, function, [left, right, sum, overflow], instruction) =
                overflow_mask_program(demand_sum, demand_overflow);
            let inventory = SemanticInventory::new(&core).unwrap();
            let demand = RuntimeDemand::for_level(
                &core,
                &inventory,
                MinecraftOptimizationLevel::Baseline,
                RuntimeDemandLimits::derived(),
            )
            .unwrap();
            let assignment =
                HomeAssignment::for_baseline_derived_liveness(&core, &inventory, &demand).unwrap();
            let function_assignment = assignment.function(function).unwrap();

            assert_eq!(
                function_assignment.value_assignment(sum).is_some(),
                demand_sum
            );
            assert_eq!(
                function_assignment.value_assignment(overflow).is_some(),
                demand_overflow
            );
            let retained = demand_sum || demand_overflow;
            assert_eq!(
                function_assignment.value_assignment(left).is_some(),
                retained
            );
            assert_eq!(
                function_assignment.value_assignment(right).is_some(),
                retained
            );

            if !retained {
                assert!(matches!(
                    function_assignment.instruction_plan(instruction),
                    Some(AssignedInstructionPlan::OmittedPure)
                ));
                assert!(function_assignment.recipe_temporaries().is_empty());
                continue;
            }

            let AssignedInstructionPlan::Scalar { results, .. } =
                function_assignment.instruction_plan(instruction).unwrap()
            else {
                panic!("retained overflowing add must have a scalar plan")
            };
            assert_eq!(results.len(), 2);
            assert_scalar_result(
                function_assignment,
                results[0],
                0,
                sum,
                CoreType::I32,
                demand_sum,
            );
            assert_scalar_result(
                function_assignment,
                results[1],
                1,
                overflow,
                CoreType::Bool,
                demand_overflow,
            );
            assert_eq!(
                function_assignment.recipe_temporaries().len(),
                usize::from(demand_sum ^ demand_overflow)
            );
        }
    }

    #[test]
    fn baseline_calls_keep_effects_and_only_indexed_demanded_result_copies() {
        let fixture = call_mask_program();
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let demand = RuntimeDemand::for_level(
            &fixture.core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let assignment =
            HomeAssignment::for_baseline_derived_liveness(&fixture.core, &inventory, &demand)
                .unwrap();

        let unused = assignment.function(fixture.unused_function).unwrap();
        let AssignedInstructionPlan::Call {
            arguments,
            result_destinations,
        } = unused.instruction_plan(fixture.unused_call).unwrap()
        else {
            panic!("effectful unused call must remain materialized")
        };
        assert_eq!(arguments.len(), 2);
        assert_eq!(&**result_destinations, &[None, None]);
        assert!(
            fixture
                .unused_results
                .iter()
                .all(|value| unused.value_assignment(*value).is_none())
        );

        let selected = assignment.function(fixture.selected_function).unwrap();
        let AssignedInstructionPlan::Call {
            arguments,
            result_destinations,
        } = selected.instruction_plan(fixture.selected_call).unwrap()
        else {
            panic!("selected-result call must remain materialized")
        };
        assert_eq!(arguments.len(), 2);
        assert!(result_destinations[0].is_none());
        let later = result_destinations[1].unwrap();
        assert_eq!(later.result_index(), 1);
        assert_eq!(later.value(), fixture.selected_results[1]);
        assert_eq!(
            later.home(),
            selected
                .value_assignment(fixture.selected_results[1])
                .unwrap()
                .home()
        );
        assert!(
            selected
                .value_assignment(fixture.selected_results[0])
                .is_none()
        );

        let target = assignment.function(fixture.target_function).unwrap();
        assert_eq!(target.abi().parameters().len(), 2);
        assert_eq!(target.abi().results().len(), 2);
        assert!(matches!(
            target.home(target.abi().parameters()[0]).unwrap().role(),
            AssignedHomeRole::PinnedParameter {
                parameter_index: 0,
                ..
            }
        ));
        assert!(matches!(
            target.home(target.abi().results()[1]).unwrap().role(),
            AssignedHomeRole::PinnedResult { result_index: 1 }
        ));
    }

    #[test]
    fn baseline_conservative_fallback_materializes_the_whole_reachable_shape() {
        let (core, function) = constant_program(2);
        let inventory = SemanticInventory::new(&core).unwrap();
        let demand = RuntimeDemand::for_level(
            &core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived().with_table_limit(RuntimeDemandTableLimit::new(0)),
        )
        .unwrap();
        assert_eq!(
            demand.completion(),
            RuntimeDemandCompletion::ConservativeFallback(RuntimeDemandFallbackReason::DenseTables)
        );

        let assignment =
            HomeAssignment::for_baseline_derived_liveness(&core, &inventory, &demand).unwrap();
        let function_assignment = assignment.function(function).unwrap();
        assert_eq!(function_assignment.homes().len(), 2);
        assert!(
            function_assignment
                .instruction_slots()
                .iter()
                .all(|plan| { matches!(plan, Some(AssignedInstructionPlan::Scalar { .. })) })
        );
        for (ordinal, (_, home)) in function_assignment.homes().enumerate() {
            assert_eq!(home.role(), AssignedHomeRole::Register { ordinal });
        }
    }

    #[test]
    fn baseline_assignment_is_deterministic() {
        let fixture = call_mask_program();
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let demand = RuntimeDemand::for_level(
            &fixture.core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let first =
            HomeAssignment::for_baseline_derived_liveness(&fixture.core, &inventory, &demand)
                .unwrap();
        let second =
            HomeAssignment::for_baseline_derived_liveness(&fixture.core, &inventory, &demand)
                .unwrap();

        assert_eq!(format!("{first:#?}"), format!("{second:#?}"));
    }

    #[test]
    fn none_assignment_is_deterministic() {
        let (core, _) = constant_program(32);
        let inventory = SemanticInventory::new(&core).unwrap();
        let first = HomeAssignment::for_none(&core, &inventory).unwrap();
        let second = HomeAssignment::for_none(&core, &inventory).unwrap();

        assert_eq!(format!("{first:#?}"), format!("{second:#?}"));
    }

    #[test]
    fn assignment_records_policy_even_when_no_home_can_reveal_it() {
        let (core, _) = constant_program(0);
        let inventory = SemanticInventory::new(&core).unwrap();
        let none = HomeAssignment::for_none(&core, &inventory).unwrap();
        let demand = RuntimeDemand::for_level(
            &core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let baseline =
            HomeAssignment::for_baseline_derived_liveness(&core, &inventory, &demand).unwrap();

        assert_eq!(none.level(), MinecraftOptimizationLevel::None);
        assert!(none.matches_level(MinecraftOptimizationLevel::None));
        assert!(!none.matches_level(MinecraftOptimizationLevel::Baseline));
        assert_eq!(baseline.level(), MinecraftOptimizationLevel::Baseline);
        assert!(baseline.matches_level(MinecraftOptimizationLevel::Baseline));
        assert!(!baseline.matches_level(MinecraftOptimizationLevel::None));
    }

    #[test]
    fn twenty_thousand_instruction_shape_remains_dense_and_linear() {
        let (core, function) = constant_program(20_000);
        let inventory = SemanticInventory::new(&core).unwrap();
        let assignment = HomeAssignment::for_none(&core, &inventory).unwrap();
        let function_assignment = assignment.function(function).unwrap();

        assert_eq!(function_assignment.homes().len(), 20_000);
        assert_eq!(function_assignment.instruction_slots().len(), 20_000);
        assert!(
            function_assignment
                .instruction_slots()
                .iter()
                .all(|plan| matches!(plan, Some(AssignedInstructionPlan::Scalar { .. })))
        );
        assert_eq!(
            function_assignment
                .value_assignment(ValueId::from_index(19_999))
                .unwrap()
                .home()
                .index(),
            19_999
        );
    }

    fn constant_program(count: usize) -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("constants"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        for value in 0..count {
            builder
                .i32_constant(i32::try_from(value).unwrap(), OriginId::UNKNOWN)
                .unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    fn overflow_mask_program(
        demand_sum: bool,
        demand_overflow: bool,
    ) -> (
        CoreProgram,
        FunctionId,
        [ValueId; 4],
        crate::ir::core::InstId,
    ) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let result_types = match (demand_sum, demand_overflow) {
            (false, false) => vec![],
            (true, false) => vec![CoreType::I32],
            (false, true) => vec![CoreType::Bool],
            (true, true) => vec![CoreType::I32, CoreType::Bool],
        };
        let function = core
            .declare_function(
                Some("overflow-mask"),
                vec![],
                result_types,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (sum, overflow) = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap();
        let returned = match (demand_sum, demand_overflow) {
            (false, false) => vec![],
            (true, false) => vec![sum],
            (false, true) => vec![overflow],
            (true, true) => vec![sum, overflow],
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(returned),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let instruction = defining_instruction(builder.body(), sum);
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, [left, right, sum, overflow], instruction)
    }

    fn assert_scalar_result(
        assignment: &super::FunctionHomeAssignment,
        result: AssignedScalarResult,
        expected_index: usize,
        expected_value: ValueId,
        expected_type: CoreType,
        semantic: bool,
    ) {
        match (semantic, result) {
            (
                true,
                AssignedScalarResult::Semantic {
                    result_index,
                    value,
                    home,
                },
            ) => {
                assert_eq!(result_index, expected_index);
                assert_eq!(value, expected_value);
                assert_eq!(
                    home,
                    assignment.value_assignment(expected_value).unwrap().home()
                );
                assert_eq!(assignment.home(home).unwrap().ty(), expected_type);
            }
            (false, AssignedScalarResult::RecipeTemporary { result_index, home }) => {
                assert_eq!(result_index, expected_index);
                assert_eq!(assignment.home(home).unwrap().ty(), expected_type);
                assert!(assignment.recipe_temporaries().contains(&home));
            }
            _ => panic!("scalar result placement disagrees with the demand mask"),
        }
    }

    struct CallMaskFixture {
        core: CoreProgram,
        target_function: FunctionId,
        unused_function: FunctionId,
        selected_function: FunctionId,
        unused_results: [ValueId; 2],
        selected_results: [ValueId; 2],
        unused_call: crate::ir::core::InstId,
        selected_call: crate::ir::core::InstId,
    }

    #[allow(
        clippy::similar_names,
        reason = "the fixture deliberately contrasts unused and selected call-result paths"
    )]
    fn call_mask_program() -> CallMaskFixture {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let target_function = core
            .declare_function(
                Some("target"),
                vec![CoreType::I32, CoreType::Bool],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let unused_function = core
            .declare_function(Some("unused"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let selected_function = core
            .declare_function(
                Some("selected"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut target_builder = FunctionBuilder::new(&core, &sources, target_function).unwrap();
        let target_parameters = block_values(&target_builder, target_builder.entry_block());
        target_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(target_parameters),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(target_function, target_builder.finish().unwrap())
            .unwrap();

        let mut unused_builder = FunctionBuilder::new(&core, &sources, unused_function).unwrap();
        let unused_integer = unused_builder.i32_constant(11, OriginId::UNKNOWN).unwrap();
        let unused_boolean = unused_builder
            .bool_constant(false, OriginId::UNKNOWN)
            .unwrap();
        let unused_values = unused_builder
            .call(
                target_function,
                vec![unused_integer, unused_boolean],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let unused_results = [unused_values[0], unused_values[1]];
        let unused_call = defining_instruction(unused_builder.body(), unused_values[0]);
        unused_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(unused_function, unused_builder.finish().unwrap())
            .unwrap();

        let mut selected_builder =
            FunctionBuilder::new(&core, &sources, selected_function).unwrap();
        let selected_integer = selected_builder
            .i32_constant(13, OriginId::UNKNOWN)
            .unwrap();
        let selected_boolean = selected_builder
            .bool_constant(true, OriginId::UNKNOWN)
            .unwrap();
        let selected_values = selected_builder
            .call(
                target_function,
                vec![selected_integer, selected_boolean],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let selected_results = [selected_values[0], selected_values[1]];
        let selected_call = defining_instruction(selected_builder.body(), selected_values[0]);
        selected_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![selected_values[1]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(selected_function, selected_builder.finish().unwrap())
            .unwrap();

        CallMaskFixture {
            core,
            target_function,
            unused_function,
            selected_function,
            unused_results,
            selected_results,
            unused_call,
            selected_call,
        }
    }

    fn block_values(
        builder: &FunctionBuilder<'_>,
        block: crate::ir::core::BlockId,
    ) -> Vec<ValueId> {
        builder
            .body()
            .block(block)
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect()
    }

    fn defining_instruction(
        body: &crate::ir::core::FunctionBody,
        value: ValueId,
    ) -> crate::ir::core::InstId {
        match body.value(value).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => panic!("expected instruction result"),
        }
    }
}
