#[cfg(test)]
use crate::entity::EntityLimitError;
use crate::entity::{EntityId, EntityVec, entity_id};
#[cfg(test)]
use crate::ir::core::CoreProgram;
use crate::ir::core::{BlockId, CoreType, FunctionId, InstId, ValueId};
use crate::ir::minecraft::{
    FakeScoreHolder, FunctionResourceId, ObjectiveName, PackNamespace, ScoreRef, StoragePath,
};
use crate::source::OriginId;
use crate::target::JavaEditionTarget;

#[cfg(test)]
use super::LoweringOptions;
pub(crate) use super::analysis::BranchArm;
use super::coalescing::CoalescingFallbackReason;
#[cfg(test)]
use super::placement::RecipeDecisionReason;
use super::placement::{BlockPlacement, BranchArmRecipe, BranchRecipe, ControlRecipeStatistics};
use super::{CommandLimitAssumptions, GeneratedNames, MinecraftOptimizationLevel};

mod assemble;
#[cfg(test)]
mod dump;
mod minimum_demand;
mod reconcile;
mod report;
mod symbolic;
mod verify;

pub(crate) use reconcile::verify_constructed_control_recipes;
pub(crate) use report::LoweringReport;
use verify::verify_plan;

entity_id!(
    /// Identity of one physical score home within a lowering plan.
    pub(crate) struct HomeId;
);
entity_id!(
    /// Identity of one target function within a lowering plan.
    pub(crate) struct PlannedFunctionId;
);

/// A conditional edge whose arm remains distinct from its destination.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct BranchEdge {
    source: BlockId,
    arm: BranchArm,
}

impl BranchEdge {
    pub(crate) const fn new(source: BlockId, arm: BranchArm) -> Self {
        Self { source, arm }
    }

    pub(crate) const fn source(self) -> BlockId {
        self.source
    }

    pub(crate) const fn arm(self) -> BranchArm {
        self.arm
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Home {
    holder: FakeScoreHolder,
    role: HomeRole,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HomeRole {
    Value {
        function: FunctionId,
        value: ValueId,
        ty: CoreType,
    },
    Result {
        function: FunctionId,
        result_index: usize,
        ty: CoreType,
    },
    EdgeTemporary {
        function: FunctionId,
    },
    Register {
        function: FunctionId,
        ordinal: usize,
        ty: CoreType,
    },
    RecipeTemporary {
        function: FunctionId,
        ordinal: usize,
        ty: CoreType,
    },
    TypedEdgeTemporary {
        function: FunctionId,
        ordinal: usize,
        ty: CoreType,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PlannedFunction {
    resource: FunctionResourceId,
    origin: OriginId,
    role: PlannedFunctionRole,
}

impl PlannedFunction {
    pub(crate) const fn resource(&self) -> &FunctionResourceId {
        &self.resource
    }

    pub(crate) const fn origin(&self) -> OriginId {
        self.origin
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlannedFunctionRole {
    Load,
    InitTryCreate,
    Block {
        function: FunctionId,
        block: BlockId,
    },
    BranchHelper {
        function: FunctionId,
        edge: BranchEdge,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionAbi {
    entry_block: BlockId,
    parameters: Box<[HomeId]>,
    results: Box<[HomeId]>,
}

impl FunctionAbi {
    pub(crate) fn new(entry_block: BlockId, parameters: Vec<HomeId>, results: Vec<HomeId>) -> Self {
        Self {
            entry_block,
            parameters: parameters.into_boxed_slice(),
            results: results.into_boxed_slice(),
        }
    }
}

impl CallResultDestination {
    pub(crate) const fn result_index(self) -> usize {
        self.result_index
    }

    pub(crate) const fn value(self) -> ValueId {
        self.value
    }

    pub(crate) const fn home(self) -> HomeId {
        self.home
    }
}

/// Final exact physical requirements for one reachable Core instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InstructionPlan {
    OmittedPure,
    Scalar {
        operands: Box<[HomeId]>,
        results: Box<[ScalarResultPlacement]>,
    },
    Call {
        arguments: Box<[HomeId]>,
        result_destinations: Box<[Option<CallResultDestination>]>,
    },
}

/// One indexed physical output of a fixed scalar recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScalarResultPlacement {
    Semantic {
        result_index: usize,
        value: ValueId,
        home: HomeId,
    },
    RecipeTemporary {
        result_index: usize,
        home: HomeId,
    },
}

impl ScalarResultPlacement {
    pub(crate) const fn result_index(self) -> usize {
        match self {
            Self::Semantic { result_index, .. } | Self::RecipeTemporary { result_index, .. } => {
                result_index
            }
        }
    }

    pub(crate) const fn home(self) -> HomeId {
        match self {
            Self::Semantic { home, .. } | Self::RecipeTemporary { home, .. } => home,
        }
    }

    #[allow(
        dead_code,
        reason = "plan assembly tests inspect optional semantic correlation through one convenience accessor"
    )]
    pub(crate) const fn value(self) -> Option<ValueId> {
        match self {
            Self::Semantic { value, .. } => Some(value),
            Self::RecipeTemporary { .. } => None,
        }
    }
}

/// One indexed caller-side result copy retained after a call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CallResultDestination {
    result_index: usize,
    value: ValueId,
    home: HomeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MoveStep {
    destination: HomeId,
    source: HomeId,
}

impl MoveStep {
    pub(super) const fn new(destination: HomeId, source: HomeId) -> Self {
        Self {
            destination,
            source,
        }
    }

    pub(crate) const fn destination(self) -> HomeId {
        self.destination
    }

    pub(crate) const fn source(self) -> HomeId {
        self.source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BranchTransfer {
    steps: Box<[MoveStep]>,
    helper: Option<PlannedFunctionId>,
}

impl BranchTransfer {
    pub(super) fn new(steps: Vec<MoveStep>, helper: Option<PlannedFunctionId>) -> Self {
        Self {
            steps: steps.into_boxed_slice(),
            helper,
        }
    }

    pub(crate) fn steps(&self) -> &[MoveStep] {
        &self.steps
    }

    pub(crate) const fn helper(&self) -> Option<PlannedFunctionId> {
        self.helper
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum EdgeTransfer {
    Jump {
        steps: Box<[MoveStep]>,
    },
    Branch {
        then_edge: BranchTransfer,
        else_edge: BranchTransfer,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct FunctionLayout {
    diagnostic_name_hint: Option<Box<str>>,
    parameter_types: Box<[CoreType]>,
    result_types: Box<[CoreType]>,
    abi: FunctionAbi,
    block_placements: Box<[Option<BlockPlacement>]>,
    branch_recipes: Box<[Option<BranchRecipe>]>,
    block_functions: Box<[Option<PlannedFunctionId>]>,
    value_homes: Box<[Option<HomeId>]>,
    parallel_copy_temp: Option<HomeId>,
    edge_temporaries: Box<[HomeId]>,
    instruction_plans: Box<[Option<InstructionPlan>]>,
    edge_transfers: Box<[Option<EdgeTransfer>]>,
    coalescing: Option<FunctionCoalescingPlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FunctionCoalescingPlan {
    fallback_reason: Option<CoalescingFallbackReason>,
    candidates_considered: u64,
    merges_accepted: u64,
    work_used: u64,
    liveness_tracked_values: u64,
    liveness_events: u64,
    liveness_segments: u64,
}

impl FunctionCoalescingPlan {
    pub(crate) const fn new(
        fallback_reason: Option<CoalescingFallbackReason>,
        candidates_considered: u64,
        merges_accepted: u64,
        work_used: u64,
        liveness_tracked_values: u64,
        liveness_events: u64,
        liveness_segments: u64,
    ) -> Self {
        Self {
            fallback_reason,
            candidates_considered,
            merges_accepted,
            work_used,
            liveness_tracked_values,
            liveness_events,
            liveness_segments,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) struct FunctionLayoutBuilder {
    pub(super) diagnostic_name_hint: Option<Box<str>>,
    pub(super) parameter_types: Box<[CoreType]>,
    pub(super) result_types: Box<[CoreType]>,
    pub(super) abi: Option<FunctionAbi>,
    pub(super) block_functions: Vec<Option<PlannedFunctionId>>,
    pub(super) value_homes: Vec<Option<HomeId>>,
    pub(super) parallel_copy_temp: Option<HomeId>,
    pub(super) instruction_plans: Vec<Option<InstructionPlan>>,
    pub(super) edge_transfers: Vec<Option<EdgeTransfer>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackAbi {
    register_objective: ObjectiveName,
    init_sentinel: StoragePath,
}

/// Frozen lowering decisions. Its fields remain private to the planning pipeline.
#[derive(Clone, Debug)]
pub(crate) struct LoweringPlan {
    optimization_level: MinecraftOptimizationLevel,
    target: JavaEditionTarget,
    command_limit_assumptions: CommandLimitAssumptions,
    namespace: PackNamespace,
    pack_abi: PackAbi,
    load: PlannedFunctionId,
    init_try_create: PlannedFunctionId,
    homes: EntityVec<HomeId, Home>,
    target_functions: EntityVec<PlannedFunctionId, PlannedFunction>,
    functions: EntityVec<FunctionId, FunctionLayout>,
    control_statistics: ControlRecipeStatistics,
}

impl LoweringPlan {
    pub(crate) fn report(&self) -> LoweringReport {
        LoweringReport::from_plan(self)
    }

    pub(crate) const fn target(&self) -> JavaEditionTarget {
        self.target
    }

    #[allow(
        dead_code,
        reason = "direct assembly tests expose policy provenance without widening the production plan API"
    )]
    pub(crate) const fn optimization_level(&self) -> MinecraftOptimizationLevel {
        self.optimization_level
    }

    pub(crate) const fn load(&self) -> PlannedFunctionId {
        self.load
    }

    pub(crate) const fn init_try_create(&self) -> PlannedFunctionId {
        self.init_try_create
    }

    pub(crate) const fn register_objective(&self) -> &ObjectiveName {
        &self.pack_abi.register_objective
    }

    pub(crate) const fn init_sentinel(&self) -> &StoragePath {
        &self.pack_abi.init_sentinel
    }

    pub(crate) fn planned_functions(
        &self,
    ) -> impl ExactSizeIterator<Item = (PlannedFunctionId, &PlannedFunction)> + '_ {
        self.target_functions.iter()
    }

    pub(crate) fn score(&self, home: HomeId) -> Option<ScoreRef> {
        let holder = self.homes.get(home)?.holder.clone();
        Some(ScoreRef::new(
            holder.into(),
            self.pack_abi.register_objective.clone(),
        ))
    }

    pub(crate) fn home_type(&self, home: HomeId) -> Option<CoreType> {
        match self.homes.get(home)?.role {
            HomeRole::Value { ty, .. }
            | HomeRole::Result { ty, .. }
            | HomeRole::Register { ty, .. }
            | HomeRole::RecipeTemporary { ty, .. }
            | HomeRole::TypedEdgeTemporary { ty, .. } => Some(ty),
            HomeRole::EdgeTemporary { .. } => None,
        }
    }

    pub(crate) fn value_home(&self, function: FunctionId, value: ValueId) -> Option<HomeId> {
        let layout = self.functions.get(function)?;
        usize::try_from(value.index())
            .ok()
            .and_then(|index| layout.value_homes.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn block_function(
        &self,
        function: FunctionId,
        block: BlockId,
    ) -> Option<PlannedFunctionId> {
        let layout = self.functions.get(function)?;
        usize::try_from(block.index())
            .ok()
            .and_then(|index| layout.block_functions.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn block_placement(
        &self,
        function: FunctionId,
        block: BlockId,
    ) -> Option<BlockPlacement> {
        let layout = self.functions.get(function)?;
        usize::try_from(block.index())
            .ok()
            .and_then(|index| layout.block_placements.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn branch_recipe(
        &self,
        function: FunctionId,
        block: BlockId,
    ) -> Option<BranchRecipe> {
        let layout = self.functions.get(function)?;
        usize::try_from(block.index())
            .ok()
            .and_then(|index| layout.branch_recipes.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn branch_arm_recipe(
        &self,
        function: FunctionId,
        block: BlockId,
        arm: BranchArm,
    ) -> Option<BranchArmRecipe> {
        Some(self.branch_recipe(function, block)?.arm(arm))
    }

    pub(crate) fn function_parameters(&self, function: FunctionId) -> Option<&[HomeId]> {
        Some(&self.functions.get(function)?.abi.parameters)
    }

    pub(crate) fn function_results(&self, function: FunctionId) -> Option<&[HomeId]> {
        Some(&self.functions.get(function)?.abi.results)
    }

    pub(crate) fn function_entry(&self, function: FunctionId) -> Option<PlannedFunctionId> {
        let layout = self.functions.get(function)?;
        self.block_function(function, layout.abi.entry_block)
    }

    pub(crate) fn edge_transfer(
        &self,
        function: FunctionId,
        block: BlockId,
    ) -> Option<&EdgeTransfer> {
        let layout = self.functions.get(function)?;
        usize::try_from(block.index())
            .ok()
            .and_then(|index| layout.edge_transfers.get(index))
            .and_then(Option::as_ref)
    }

    pub(crate) fn instruction_plan(
        &self,
        function: FunctionId,
        instruction: InstId,
    ) -> Option<&InstructionPlan> {
        let layout = self.functions.get(function)?;
        usize::try_from(instruction.index())
            .ok()
            .and_then(|index| layout.instruction_plans.get(index))
            .and_then(Option::as_ref)
    }
}

/// The only mutable authority allowed to assemble a lowering plan.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct PlanBuilder {
    pub(super) options: LoweringOptions,
    pub(super) load: Option<PlannedFunctionId>,
    pub(super) init_try_create: Option<PlannedFunctionId>,
    pub(super) homes: EntityVec<HomeId, Home>,
    pub(super) target_functions: EntityVec<PlannedFunctionId, PlannedFunction>,
    pub(super) functions: EntityVec<FunctionId, FunctionLayoutBuilder>,
}

#[cfg(test)]
impl PlanBuilder {
    pub(crate) fn new(
        core: &CoreProgram,
        options: LoweringOptions,
    ) -> Result<Self, PlanBuildError> {
        let mut functions = EntityVec::new();
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(PlanBuildError::MissingDefinition { function })?;
            let planned_function = functions
                .push(FunctionLayoutBuilder {
                    diagnostic_name_hint: declaration.name_hint().map(Into::into),
                    parameter_types: declaration.parameters().into(),
                    result_types: declaration.results().into(),
                    abi: None,
                    block_functions: vec![None; body.blocks.len()],
                    value_homes: vec![None; body.values.len()],
                    parallel_copy_temp: None,
                    instruction_plans: vec![None; body.instructions.len()],
                    edge_transfers: vec![None; body.blocks.len()],
                })
                .map_err(|EntityLimitError| PlanBuildError::EntityLimit {
                    table: PlanTable::Functions,
                })?;
            debug_assert_eq!(planned_function, function);
        }
        Ok(Self {
            options,
            load: None,
            init_try_create: None,
            homes: EntityVec::new(),
            target_functions: EntityVec::new(),
            functions,
        })
    }

    pub(crate) fn initialize_scaffolding(&mut self) -> Result<(), PlanBuildError> {
        if self.load.is_some() || self.init_try_create.is_some() {
            return Err(PlanBuildError::ScaffoldingAlreadyInitialized);
        }
        let load = self.allocate_planned_function(PlannedFunctionRole::Load, OriginId::UNKNOWN)?;
        let init =
            self.allocate_planned_function(PlannedFunctionRole::InitTryCreate, OriginId::UNKNOWN)?;
        self.load = Some(load);
        self.init_try_create = Some(init);
        Ok(())
    }

    pub(crate) fn allocate_home(&mut self, role: HomeRole) -> Result<HomeId, PlanBuildError> {
        let holder = match role {
            HomeRole::Value {
                function, value, ..
            } => GeneratedNames::value_holder(function, value),
            HomeRole::Result {
                function,
                result_index,
                ..
            } => GeneratedNames::result_holder(function, result_index),
            HomeRole::EdgeTemporary { function } => GeneratedNames::edge_temporary_holder(function),
            HomeRole::Register {
                function, ordinal, ..
            } => GeneratedNames::assigned_home_holder(function, ordinal),
            HomeRole::RecipeTemporary {
                function,
                ordinal,
                ty,
            } => GeneratedNames::recipe_temporary_holder(function, ty, ordinal),
            HomeRole::TypedEdgeTemporary {
                function,
                ordinal,
                ty,
            } => GeneratedNames::typed_edge_temporary_holder(function, ty, ordinal),
        };
        self.homes
            .push(Home { holder, role })
            .map_err(|EntityLimitError| PlanBuildError::EntityLimit {
                table: PlanTable::Homes,
            })
    }

    pub(crate) fn allocate_planned_function(
        &mut self,
        role: PlannedFunctionRole,
        origin: OriginId,
    ) -> Result<PlannedFunctionId, PlanBuildError> {
        let names = self.options.generated_names();
        let resource = match role {
            PlannedFunctionRole::Load => names.load_function(),
            PlannedFunctionRole::InitTryCreate => names.init_try_create_function(),
            PlannedFunctionRole::Block { function, block } => names.block_function(function, block),
            PlannedFunctionRole::BranchHelper { function, edge } => {
                names.branch_helper_function(function, edge.source(), edge.arm())
            }
        };
        self.target_functions
            .push(PlannedFunction {
                resource,
                origin,
                role,
            })
            .map_err(|EntityLimitError| PlanBuildError::EntityLimit {
                table: PlanTable::TargetFunctions,
            })
    }

    pub(crate) fn finish(
        mut self,
        core: &CoreProgram,
        analyses: &super::analysis::SemanticInventory,
    ) -> Result<LoweringPlan, PlanFinishError> {
        self.populate_legacy_instruction_plans(core, analyses)?;
        let load = self.load.ok_or(PlanBuildError::MissingScaffolding {
            role: ScaffoldingRole::Load,
        })?;
        let init_try_create = self
            .init_try_create
            .ok_or(PlanBuildError::MissingScaffolding {
                role: ScaffoldingRole::InitTryCreate,
            })?;
        let mut functions = EntityVec::new();
        let mut branch_arms_visited = 0_u64;
        for (function, layout) in self.functions.into_iter() {
            let abi = layout
                .abi
                .ok_or(PlanBuildError::MissingFunctionAbi { function })?;
            let edge_temporaries = layout
                .parallel_copy_temp
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let body = core
                .function(function)
                .and_then(crate::ir::core::Function::body)
                .ok_or(PlanBuildError::MissingDefinition { function })?;
            let function_analysis = analyses
                .function(function)
                .ok_or(PlanBuildError::MissingAnalysis { function })?;
            let mut block_placements = vec![None; body.block_counts().allocated];
            let mut branch_recipes = vec![None; body.block_counts().allocated];
            for block in function_analysis.reachable_blocks().iter().copied() {
                let index = usize::try_from(block.index())
                    .map_err(|_| PlanBuildError::InvalidCoreEntity { function })?;
                *block_placements
                    .get_mut(index)
                    .ok_or(PlanBuildError::InvalidCoreEntity { function })? =
                    Some(BlockPlacement::Materialized);
                if matches!(
                    body.block(block)
                        .and_then(crate::ir::core::BlockData::terminator)
                        .map(crate::ir::core::Terminator::kind),
                    Some(crate::ir::core::TerminatorKind::Branch { .. })
                ) {
                    branch_arms_visited = branch_arms_visited.checked_add(2).ok_or(
                        PlanBuildError::CapacityOverflow {
                            table: PlanTable::Functions,
                        },
                    )?;
                    *branch_recipes
                        .get_mut(index)
                        .ok_or(PlanBuildError::InvalidCoreEntity { function })? = Some(
                        BranchRecipe::all_materialized(RecipeDecisionReason::OptimizationDisabled),
                    );
                }
            }
            let frozen_function = functions
                .push(FunctionLayout {
                    diagnostic_name_hint: layout.diagnostic_name_hint,
                    parameter_types: layout.parameter_types,
                    result_types: layout.result_types,
                    abi,
                    block_placements: block_placements.into_boxed_slice(),
                    branch_recipes: branch_recipes.into_boxed_slice(),
                    block_functions: layout.block_functions.into_boxed_slice(),
                    value_homes: layout.value_homes.into_boxed_slice(),
                    parallel_copy_temp: layout.parallel_copy_temp,
                    edge_temporaries,
                    instruction_plans: layout.instruction_plans.into_boxed_slice(),
                    edge_transfers: layout.edge_transfers.into_boxed_slice(),
                    coalescing: None,
                })
                .map_err(|EntityLimitError| PlanBuildError::EntityLimit {
                    table: PlanTable::Functions,
                })?;
            debug_assert_eq!(frozen_function, function);
        }
        let pack_abi = PackAbi {
            register_objective: self.options.register_objective().clone(),
            init_sentinel: self.options.generated_names().init_sentinel(),
        };
        let plan = LoweringPlan {
            optimization_level: self.options.optimization_level(),
            target: self.options.target(),
            command_limit_assumptions: self.options.command_limit_assumptions(),
            namespace: self.options.namespace().clone(),
            pack_abi,
            load,
            init_try_create,
            homes: self.homes,
            target_functions: self.target_functions,
            functions,
            control_statistics: ControlRecipeStatistics::from_counts(branch_arms_visited, 0, 0),
        };
        verify_plan(core, &plan).map_err(PlanFinishError::Invalid)?;
        Ok(plan)
    }

    /// Keeps the retired mutable-builder tests honest while production lowering
    /// publishes immutable assignment records through `LoweringPlan::from_parts`.
    fn populate_legacy_instruction_plans(
        &mut self,
        core: &CoreProgram,
        analyses: &super::analysis::SemanticInventory,
    ) -> Result<(), PlanBuildError> {
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(PlanBuildError::MissingDefinition { function })?;
            let inventory = analyses
                .function(function)
                .ok_or(PlanBuildError::MissingAnalysis { function })?;
            let layout = self
                .functions
                .get_mut(function)
                .ok_or(PlanBuildError::InvalidCoreEntity { function })?;
            for instruction in inventory.reachable_instructions().iter().copied() {
                let data = body
                    .instruction(instruction)
                    .ok_or(PlanBuildError::InvalidCoreEntity { function })?;
                let operands = data
                    .operands()
                    .iter()
                    .copied()
                    .map(|value| legacy_value_home(layout, function, value))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_boxed_slice();
                let plan = if matches!(data.op(), crate::ir::core::CoreOp::Call(_)) {
                    let result_destinations = data
                        .results()
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(result_index, value)| {
                            Ok(Some(CallResultDestination {
                                result_index,
                                value,
                                home: legacy_value_home(layout, function, value)?,
                            }))
                        })
                        .collect::<Result<Vec<_>, PlanBuildError>>()?
                        .into_boxed_slice();
                    InstructionPlan::Call {
                        arguments: operands,
                        result_destinations,
                    }
                } else {
                    let results = data
                        .results()
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(result_index, value)| {
                            Ok(ScalarResultPlacement::Semantic {
                                result_index,
                                value,
                                home: legacy_value_home(layout, function, value)?,
                            })
                        })
                        .collect::<Result<Vec<_>, PlanBuildError>>()?
                        .into_boxed_slice();
                    InstructionPlan::Scalar { operands, results }
                };
                let index = usize::try_from(instruction.index())
                    .map_err(|_| PlanBuildError::InvalidCoreEntity { function })?;
                let slot = layout
                    .instruction_plans
                    .get_mut(index)
                    .ok_or(PlanBuildError::InvalidCoreEntity { function })?;
                *slot = Some(plan);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
fn legacy_value_home(
    layout: &FunctionLayoutBuilder,
    function: FunctionId,
    value: ValueId,
) -> Result<HomeId, PlanBuildError> {
    usize::try_from(value.index())
        .ok()
        .and_then(|index| layout.value_homes.get(index))
        .copied()
        .flatten()
        .ok_or(PlanBuildError::InvalidCoreEntity { function })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PlanFinishError {
    Build(PlanBuildError),
    Invalid(crate::diagnostic::Diagnostics),
}

impl From<PlanBuildError> for PlanFinishError {
    fn from(error: PlanBuildError) -> Self {
        Self::Build(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanTable {
    Functions,
    Homes,
    TargetFunctions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanInputPhase {
    Assignment,
    Transfers,
    ControlRecipes,
    Resources,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    dead_code,
    reason = "constructed only by the cfg(test) Stage 4 mutable-planner oracle"
)]
pub(crate) enum ScaffoldingRole {
    Load,
    InitTryCreate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlanBuildError {
    MissingDefinition {
        function: FunctionId,
    },
    EntityLimit {
        table: PlanTable,
    },
    CapacityOverflow {
        table: PlanTable,
    },
    CapacityExceeded {
        table: PlanTable,
        required: u64,
        maximum: u64,
    },
    OptimizationLevelMismatch {
        phase: PlanInputPhase,
        expected: MinecraftOptimizationLevel,
        actual: MinecraftOptimizationLevel,
    },
    PhaseFunctionCountMismatch {
        phase: PlanInputPhase,
        expected: usize,
        actual: usize,
    },
    InvalidPhaseInput {
        phase: PlanInputPhase,
        function: Option<FunctionId>,
    },
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    ScaffoldingAlreadyInitialized,
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    MissingAnalysis {
        function: FunctionId,
    },
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    InvalidCoreEntity {
        function: FunctionId,
    },
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    InvalidEdgeShape {
        function: FunctionId,
        block: BlockId,
    },
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    InvalidParallelCopy,
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    InvalidRuntimeDemandPolicy,
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    MissingScaffolding {
        role: ScaffoldingRole,
    },
    #[allow(
        dead_code,
        reason = "the cfg(test) Stage 4 mutable-plan oracle retains its original typed failure surface"
    )]
    MissingFunctionAbi {
        function: FunctionId,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        BranchArm, BranchEdge, FunctionAbi, HomeRole, PlanBuildError, PlanBuilder,
        PlannedFunctionRole, ScaffoldingRole,
    };
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator, TerminatorKind,
        ValueId,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::lower::minecraft::LoweringOptions;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    fn options() -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
    }

    fn one_function() -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("diagnostic only"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut body = FunctionBuilder::new(&program, &sources, function).unwrap();
        body.terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
        program
            .define_function(function, body.finish().unwrap())
            .unwrap();
        (program, function)
    }

    #[test]
    fn builder_records_every_defined_function_without_early_physicalization() {
        let (program, function) = one_function();
        let builder = PlanBuilder::new(&program, options()).unwrap();
        let layout = builder.functions.get(function).unwrap();

        assert_eq!(
            layout.diagnostic_name_hint.as_deref(),
            Some("diagnostic only")
        );
        assert!(layout.block_functions.iter().all(Option::is_none));
        assert!(layout.value_homes.iter().all(Option::is_none));
        assert!(layout.edge_transfers.iter().all(Option::is_none));
        assert!(builder.homes.is_empty());
        assert!(builder.target_functions.is_empty());
    }

    #[test]
    fn roles_are_single_owners_of_generated_names() {
        let (program, function) = one_function();
        let mut builder = PlanBuilder::new(&program, options()).unwrap();
        let value = ValueId::from_index(3);
        let home = builder
            .allocate_home(HomeRole::Value {
                function,
                value,
                ty: CoreType::I32,
            })
            .unwrap();
        let edge = BranchEdge::new(BlockId::from_index(0), BranchArm::Else);
        let helper = builder
            .allocate_planned_function(
                PlannedFunctionRole::BranchHelper { function, edge },
                OriginId::UNKNOWN,
            )
            .unwrap();

        assert_eq!(builder.homes.get(home).unwrap().holder.to_string(), "#f0v3");
        assert_eq!(
            builder
                .target_functions
                .get(helper)
                .unwrap()
                .resource
                .to_string(),
            "mdl:__mdl/f0/e0_1"
        );
    }

    #[test]
    fn finish_is_the_only_immutable_plan_boundary() {
        let (program, function) = one_function();
        let mut builder = PlanBuilder::new(&program, options()).unwrap();
        builder.initialize_scaffolding().unwrap();
        let entry = BlockId::from_index(0);
        let block_function = builder
            .allocate_planned_function(
                PlannedFunctionRole::Block {
                    function,
                    block: entry,
                },
                OriginId::UNKNOWN,
            )
            .unwrap();
        let layout = builder.functions.get_mut(function).unwrap();
        layout.block_functions[0] = Some(block_function);
        layout.abi = Some(FunctionAbi::new(entry, vec![], vec![]));

        let analyses = SemanticInventory::new(&program).unwrap();
        let plan = builder.finish(&program, &analyses).unwrap();
        assert_eq!(plan.target, JavaEditionTarget::V26_2);
        assert_eq!(plan.namespace.as_str(), "mdl");
        assert_eq!(plan.pack_abi.register_objective.as_str(), "mdl.reg");
        assert_eq!(
            plan.target_functions.get(plan.load).unwrap().role,
            PlannedFunctionRole::Load
        );
        assert_eq!(
            plan.target_functions
                .get(plan.init_try_create)
                .unwrap()
                .role,
            PlannedFunctionRole::InitTryCreate
        );
        assert_eq!(
            plan.functions.get(function).unwrap().block_functions[0],
            Some(block_function)
        );
    }

    #[test]
    fn finish_rejects_incomplete_builder_state() {
        let empty = CoreProgram::new();
        let analyses = SemanticInventory::new(&empty).unwrap();
        assert!(matches!(
            PlanBuilder::new(&empty, options())
                .unwrap()
                .finish(&empty, &analyses),
            Err(super::PlanFinishError::Build(
                PlanBuildError::MissingScaffolding {
                    role: ScaffoldingRole::Load
                }
            ))
        ));
    }

    #[test]
    fn scaffolding_can_only_be_initialized_once() {
        let empty = CoreProgram::new();
        let mut builder = PlanBuilder::new(&empty, options()).unwrap();
        builder.initialize_scaffolding().unwrap();

        assert_eq!(
            builder.initialize_scaffolding(),
            Err(PlanBuildError::ScaffoldingAlreadyInitialized)
        );
    }
}
