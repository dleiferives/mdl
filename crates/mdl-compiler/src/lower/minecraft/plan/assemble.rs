//! Flattening of immutable planning phase records into the final plan identity space.

use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{
    BlockId, CoreAmbientAnalysis, CoreOp, CoreProgram, CoreType, FunctionId, InstId, ValueId,
};

use super::super::analysis::CoreEdgeKind;
use super::super::assignment::{
    AssignedHome, AssignedHomeId, AssignedHomeRole, AssignedInstructionPlan, AssignedScalarResult,
    FunctionHomeAssignment, HomeAssignment, PHYSICAL_TYPE_ORDER,
};
use super::super::edge_transfer::{
    BlockTransfer, EdgeTemporaryKind, EdgeTransferPlan, FunctionEdgeTransferPlan, TransferLocation,
};
use super::super::physical_preflight::PhysicalPreflight;
use super::super::placement::{ControlRecipePlan, FunctionControlRecipePlan};
#[cfg(test)]
use super::super::realization::PhysicalPlanningLimits;
use super::super::realization::PhysicalRealizationPlan;
use super::super::resources::{FunctionResourceInventory, ResourceInventory};
use super::super::{LoweringOptions, MinecraftOptimizationLevel, TargetPreflight};
use super::{
    BranchTransfer, CallResultDestination, EdgeTransfer, FunctionAbi, FunctionCoalescingPlan,
    FunctionLayout, Home, HomeId, HomeRole, InstructionPlan, LoweringPlan, MoveStep, PackAbi,
    PlanBuildError, PlanFinishError, PlanInputPhase, PlanTable, PlannedFunction, PlannedFunctionId,
    ScalarResultPlacement, verify_plan,
};

const MAX_HOME_COUNT: u64 = u32::MAX as u64 + 1;

struct LocalHomeMap {
    assigned: Box<[HomeId]>,
    edge_temporaries: Box<[(EdgeTemporaryKind, HomeId)]>,
    legacy_edge_temporary: Option<HomeId>,
}

impl LocalHomeMap {
    fn assigned(
        &self,
        function: FunctionId,
        home: AssignedHomeId,
    ) -> Result<HomeId, PlanBuildError> {
        usize::try_from(home.index())
            .ok()
            .and_then(|index| self.assigned.get(index))
            .copied()
            .ok_or_else(|| invalid_assignment(function))
    }

    fn edge_temporary(
        &self,
        function: FunctionId,
        kind: EdgeTemporaryKind,
    ) -> Result<HomeId, PlanBuildError> {
        self.edge_temporaries
            .iter()
            .find_map(|(candidate, home)| (*candidate == kind).then_some(*home))
            .ok_or_else(|| invalid_transfers(function))
    }

    fn edge_home_ids(&self) -> Box<[HomeId]> {
        self.edge_temporaries
            .iter()
            .map(|(_, home)| *home)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }
}

impl LoweringPlan {
    /// Flattens reviewed immutable phase results and publishes only a plan accepted
    /// by the independent plan verifier.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "publication consumes one-shot immutable phase results at the final ownership boundary"
    )]
    #[allow(
        clippy::too_many_arguments,
        reason = "publication consumes each independently verified immutable phase product exactly once"
    )]
    pub(crate) fn from_selected_parts(
        core: &CoreProgram,
        options: &LoweringOptions,
        preflight: TargetPreflight,
        ambient: CoreAmbientAnalysis,
        assignment: HomeAssignment,
        physical: PhysicalRealizationPlan,
        physical_preflight: PhysicalPreflight,
        transfers: EdgeTransferPlan,
        control: ControlRecipePlan,
        resources: ResourceInventory,
    ) -> Result<Self, PlanFinishError> {
        let candidate = assemble_selected_candidate(
            core,
            options,
            preflight,
            ambient,
            physical,
            physical_preflight,
            &assignment,
            &transfers,
            &control,
            &resources,
        )?;
        verify_plan(core, &candidate).map_err(PlanFinishError::Invalid)?;
        Ok(candidate)
    }

    #[cfg(test)]
    pub(crate) fn from_parts(
        core: &CoreProgram,
        options: &LoweringOptions,
        assignment: HomeAssignment,
        transfers: EdgeTransferPlan,
        resources: ResourceInventory,
    ) -> Result<Self, PlanFinishError> {
        validate_legacy_phase_headers(core, options, &assignment, &transfers, &resources)?;
        let inventory = super::super::analysis::SemanticInventory::new(core)
            .map_err(|_| invalid_control_without_function())?;
        let command_limit_evidence = super::super::audit::audit_legality(
            core,
            &inventory,
            options.target(),
            options.command_limit_assumptions(),
        )
        .map_err(PlanFinishError::Invalid)?;
        let preflight =
            TargetPreflight::new(core, &inventory, options.target(), command_limit_evidence)
                .map_err(PlanFinishError::Invalid)?;
        let ambient =
            CoreAmbientAnalysis::analyze(core).map_err(|_| invalid_control_without_function())?;
        let control = ControlRecipePlan::new(
            core,
            &inventory,
            &assignment,
            &transfers,
            options.optimization_level(),
        )
        .map_err(|_| invalid_control_without_function())?;
        let physical = PhysicalRealizationPlan::for_score_compatibility(
            core,
            &inventory,
            &assignment,
            None,
            PhysicalPlanningLimits::DEFAULT,
        )
        .map_err(|_| invalid_control_without_function())?;
        let physical_preflight = PhysicalPreflight::new(options.target(), &physical)
            .map_err(|_| invalid_control_without_function())?;
        Self::from_selected_parts(
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
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "candidate assembly keeps all immutable phase products explicit for independent verification"
)]
pub(super) fn assemble_selected_candidate(
    core: &CoreProgram,
    options: &LoweringOptions,
    preflight: TargetPreflight,
    ambient: CoreAmbientAnalysis,
    physical: PhysicalRealizationPlan,
    physical_preflight: PhysicalPreflight,
    assignment: &HomeAssignment,
    transfers: &EdgeTransferPlan,
    control: &ControlRecipePlan,
    resources: &ResourceInventory,
) -> Result<LoweringPlan, PlanBuildError> {
    validate_phase_headers(core, options, assignment, transfers, control, resources)?;
    physical_preflight
        .verify(&physical)
        .map_err(|_| PlanBuildError::InvalidPhaseInput {
            phase: PlanInputPhase::Assignment,
            function: None,
        })?;
    validate_home_capacity(required_home_count(core, assignment, transfers)?)?;

    let target_functions = flatten_resources(resources)?;
    let mut homes = EntityVec::new();
    let mut functions = EntityVec::new();
    for (function, declaration) in core.functions() {
        let body = declaration
            .body()
            .ok_or(PlanBuildError::MissingDefinition { function })?;
        let assigned = assignment
            .function(function)
            .ok_or_else(|| invalid_assignment(function))?;
        let transfer = transfers
            .function(function)
            .ok_or_else(|| invalid_transfers(function))?;
        let function_control = control
            .function(function)
            .ok_or_else(|| invalid_control(function))?;
        let function_resources = resources
            .function(function)
            .ok_or_else(|| invalid_resources(function))?;

        let local_homes = flatten_function_homes(
            options.optimization_level(),
            function,
            assigned,
            transfer,
            &mut homes,
        )?;
        let abi = flatten_abi(function, assigned, &local_homes)?;
        let value_homes = flatten_value_homes(function, body, assigned, &local_homes)?;
        let instruction_plans = flatten_instruction_plans(
            function,
            body,
            assigned,
            function_resources,
            &local_homes,
            &preflight,
        )?;
        let edge_transfers =
            flatten_edge_transfers(function, body, transfer, function_resources, &local_homes)?;
        if function_resources.block_slots().len() != body.block_counts().allocated {
            return Err(invalid_resources(function));
        }
        let edge_temporaries = local_homes.edge_home_ids();
        let coalescing = assigned.coalescing().map(|decision| {
            let statistics = decision.statistics();
            let liveness = decision.liveness_statistics();
            FunctionCoalescingPlan::new(
                decision.fallback_reason(),
                statistics.candidates_considered(),
                statistics.merges_accepted(),
                statistics.work_used(),
                liveness.tracked_values(),
                liveness.propagation_events(),
                liveness.retained_segments(),
            )
        });
        let layout = FunctionLayout {
            linkage: declaration.linkage(),
            diagnostic_name_hint: declaration.name_hint().map(Into::into),
            parameter_types: declaration.parameters().into(),
            result_types: declaration.results().into(),
            abi,
            block_placements: checked_placement_slots(function, body, function_control)?.into(),
            branch_recipes: checked_branch_recipe_slots(function, body, function_control)?.into(),
            block_functions: function_resources.block_slots().into(),
            assigned_homes: local_homes.assigned.clone(),
            value_homes,
            parallel_copy_temp: local_homes.legacy_edge_temporary,
            edge_temporaries,
            instruction_plans,
            edge_transfers,
            coalescing,
        };
        let planned =
            functions
                .push(layout)
                .map_err(|EntityLimitError| PlanBuildError::EntityLimit {
                    table: PlanTable::Functions,
                })?;
        if planned != function {
            return Err(invalid_assignment(function));
        }
    }

    Ok(LoweringPlan {
        optimization_level: options.optimization_level(),
        target: options.target(),
        preflight,
        physical,
        physical_preflight,
        ambient,
        namespace: options.namespace().clone(),
        pack_abi: PackAbi {
            register_objective: options.register_objective().clone(),
            init_sentinel: options.generated_names().init_sentinel(),
        },
        load: resources.load(),
        init_try_create: resources.init_try_create(),
        homes,
        target_functions,
        functions,
        control_statistics: control.statistics(),
    })
}

#[cfg(test)]
pub(super) fn assemble_candidate(
    core: &CoreProgram,
    options: &LoweringOptions,
    assignment: &HomeAssignment,
    transfers: &EdgeTransferPlan,
    resources: &ResourceInventory,
) -> Result<LoweringPlan, PlanBuildError> {
    validate_legacy_phase_headers(core, options, assignment, transfers, resources)?;
    let inventory = super::super::analysis::SemanticInventory::new(core)
        .map_err(|_| invalid_control_without_function())?;
    let command_limit_evidence = super::super::audit::audit_legality(
        core,
        &inventory,
        options.target(),
        options.command_limit_assumptions(),
    )
    .map_err(|_| invalid_control_without_function())?;
    let preflight =
        TargetPreflight::new(core, &inventory, options.target(), command_limit_evidence)
            .map_err(|_| invalid_control_without_function())?;
    let ambient =
        CoreAmbientAnalysis::analyze(core).map_err(|_| invalid_control_without_function())?;
    let control = ControlRecipePlan::new(
        core,
        &inventory,
        assignment,
        transfers,
        options.optimization_level(),
    )
    .map_err(|_| invalid_control_without_function())?;
    let physical = PhysicalRealizationPlan::for_score_compatibility(
        core,
        &inventory,
        assignment,
        None,
        PhysicalPlanningLimits::DEFAULT,
    )
    .map_err(|_| invalid_control_without_function())?;
    let physical_preflight = PhysicalPreflight::new(options.target(), &physical)
        .map_err(|_| invalid_control_without_function())?;
    assemble_selected_candidate(
        core,
        options,
        preflight,
        ambient,
        physical,
        physical_preflight,
        assignment,
        transfers,
        &control,
        resources,
    )
}

#[cfg(test)]
fn validate_legacy_phase_headers(
    core: &CoreProgram,
    options: &LoweringOptions,
    assignment: &HomeAssignment,
    transfers: &EdgeTransferPlan,
    resources: &ResourceInventory,
) -> Result<(), PlanBuildError> {
    let expected_level = options.optimization_level();
    for (phase, actual_level) in [
        (PlanInputPhase::Assignment, assignment.level()),
        (PlanInputPhase::Transfers, transfers.level()),
        (PlanInputPhase::Resources, resources.level()),
    ] {
        if actual_level != expected_level {
            return Err(PlanBuildError::OptimizationLevelMismatch {
                phase,
                expected: expected_level,
                actual: actual_level,
            });
        }
    }
    validate_phase_function_count(PlanInputPhase::Assignment, core.len(), assignment.len())?;
    validate_phase_function_count(PlanInputPhase::Transfers, core.len(), transfers.len())?;
    validate_phase_function_count(PlanInputPhase::Resources, core.len(), resources.len())
}

fn validate_phase_headers(
    core: &CoreProgram,
    options: &LoweringOptions,
    assignment: &HomeAssignment,
    transfers: &EdgeTransferPlan,
    control: &ControlRecipePlan,
    resources: &ResourceInventory,
) -> Result<(), PlanBuildError> {
    let expected_level = options.optimization_level();
    for (phase, actual_level) in [
        (PlanInputPhase::Assignment, assignment.level()),
        (PlanInputPhase::Transfers, transfers.level()),
        (PlanInputPhase::ControlRecipes, control.level()),
        (PlanInputPhase::Resources, resources.level()),
    ] {
        if actual_level != expected_level {
            return Err(PlanBuildError::OptimizationLevelMismatch {
                phase,
                expected: expected_level,
                actual: actual_level,
            });
        }
    }
    validate_phase_function_count(PlanInputPhase::Assignment, core.len(), assignment.len())?;
    validate_phase_function_count(PlanInputPhase::Transfers, core.len(), transfers.len())?;
    validate_phase_function_count(PlanInputPhase::ControlRecipes, core.len(), control.len())?;
    validate_phase_function_count(PlanInputPhase::Resources, core.len(), resources.len())
}

fn checked_placement_slots<'a>(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    control: &'a FunctionControlRecipePlan,
) -> Result<&'a [Option<super::super::placement::BlockPlacement>], PlanBuildError> {
    if control.placement_slots().len() == body.block_counts().allocated {
        Ok(control.placement_slots())
    } else {
        Err(invalid_control(function))
    }
}

fn checked_branch_recipe_slots<'a>(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    control: &'a FunctionControlRecipePlan,
) -> Result<&'a [Option<super::super::placement::BranchRecipe>], PlanBuildError> {
    if control.branch_recipe_slots().len() == body.block_counts().allocated {
        Ok(control.branch_recipe_slots())
    } else {
        Err(invalid_control(function))
    }
}

fn validate_phase_function_count(
    phase: PlanInputPhase,
    expected: usize,
    actual: usize,
) -> Result<(), PlanBuildError> {
    if expected == actual {
        Ok(())
    } else {
        Err(PlanBuildError::PhaseFunctionCountMismatch {
            phase,
            expected,
            actual,
        })
    }
}

fn required_home_count(
    core: &CoreProgram,
    assignment: &HomeAssignment,
    transfers: &EdgeTransferPlan,
) -> Result<u64, PlanBuildError> {
    let mut required = 0_u64;
    for (function, _) in core.functions() {
        let assigned = assignment
            .function(function)
            .ok_or_else(|| invalid_assignment(function))?;
        let transfer = transfers
            .function(function)
            .ok_or_else(|| invalid_transfers(function))?;
        required = checked_add_home_count(required, assigned.homes().len())?;
        required = checked_add_home_count(required, transfer.temporaries().len())?;
    }
    Ok(required)
}

fn checked_add_home_count(total: u64, added: usize) -> Result<u64, PlanBuildError> {
    let added = u64::try_from(added).map_err(|_| PlanBuildError::CapacityOverflow {
        table: PlanTable::Homes,
    })?;
    total
        .checked_add(added)
        .ok_or(PlanBuildError::CapacityOverflow {
            table: PlanTable::Homes,
        })
}

fn validate_home_capacity(required: u64) -> Result<(), PlanBuildError> {
    if required <= MAX_HOME_COUNT && usize::try_from(required).is_ok() {
        Ok(())
    } else {
        Err(PlanBuildError::CapacityExceeded {
            table: PlanTable::Homes,
            required,
            maximum: MAX_HOME_COUNT,
        })
    }
}

fn flatten_resources(
    resources: &ResourceInventory,
) -> Result<EntityVec<PlannedFunctionId, PlannedFunction>, PlanBuildError> {
    let mut planned_functions = EntityVec::new();
    for (planned, resource) in resources.planned_functions() {
        let flattened = planned_functions
            .push(PlannedFunction {
                resource: resource.resource().clone(),
                origin: resource.origin(),
                role: resource.role(),
            })
            .map_err(|EntityLimitError| PlanBuildError::EntityLimit {
                table: PlanTable::TargetFunctions,
            })?;
        if flattened != planned {
            return Err(PlanBuildError::InvalidPhaseInput {
                phase: PlanInputPhase::Resources,
                function: None,
            });
        }
    }
    Ok(planned_functions)
}

fn flatten_function_homes(
    level: MinecraftOptimizationLevel,
    function: FunctionId,
    assignment: &FunctionHomeAssignment,
    transfers: &FunctionEdgeTransferPlan,
    homes: &mut EntityVec<HomeId, Home>,
) -> Result<LocalHomeMap, PlanBuildError> {
    let mut assigned = Vec::with_capacity(assignment.homes().len());
    for (local, home) in assignment.homes() {
        let expected_local = u32::try_from(assigned.len())
            .ok()
            .map(AssignedHomeId::from_index);
        if expected_local != Some(local) {
            return Err(invalid_assignment(function));
        }
        let home = flatten_assigned_home(level, function, *home)?;
        assigned.push(push_home(homes, home)?);
    }

    let mut edge_temporaries = Vec::with_capacity(transfers.temporaries().len());
    let mut last_typed_index = None;
    let mut legacy_edge_temporary = None;
    for descriptor in transfers.temporaries() {
        let kind = descriptor.kind();
        if edge_temporaries
            .iter()
            .any(|(existing, _)| *existing == kind)
        {
            return Err(invalid_transfers(function));
        }
        let (holder, role) = match (level, kind) {
            (MinecraftOptimizationLevel::None, EdgeTemporaryKind::LegacyUntyped) => {
                let role = HomeRole::EdgeTemporary { function };
                (super::GeneratedNames::edge_temporary_holder(function), role)
            }
            (MinecraftOptimizationLevel::Baseline, EdgeTemporaryKind::Typed(ty)) => {
                let type_index = physical_type_index(ty);
                if last_typed_index.is_some_and(|last| last >= type_index) {
                    return Err(invalid_transfers(function));
                }
                last_typed_index = Some(type_index);
                let ordinal = 0;
                let role = HomeRole::TypedEdgeTemporary {
                    function,
                    ordinal,
                    ty,
                };
                (
                    super::GeneratedNames::typed_edge_temporary_holder(function, ty, ordinal),
                    role,
                )
            }
            (MinecraftOptimizationLevel::None, EdgeTemporaryKind::Typed(_))
            | (MinecraftOptimizationLevel::Baseline, EdgeTemporaryKind::LegacyUntyped) => {
                return Err(invalid_transfers(function));
            }
        };
        let home = push_home(homes, Home { holder, role })?;
        if kind == EdgeTemporaryKind::LegacyUntyped {
            legacy_edge_temporary = Some(home);
        }
        edge_temporaries.push((kind, home));
    }
    Ok(LocalHomeMap {
        assigned: assigned.into_boxed_slice(),
        edge_temporaries: edge_temporaries.into_boxed_slice(),
        legacy_edge_temporary,
    })
}

fn flatten_assigned_home(
    level: MinecraftOptimizationLevel,
    function: FunctionId,
    assigned: AssignedHome,
) -> Result<Home, PlanBuildError> {
    let ty = assigned.ty();
    let (holder, role) = match (level, assigned.role()) {
        (
            MinecraftOptimizationLevel::None,
            AssignedHomeRole::LegacyValue { value }
            | AssignedHomeRole::PinnedParameter { value, .. },
        )
        | (MinecraftOptimizationLevel::Baseline, AssignedHomeRole::PinnedParameter { value, .. }) => {
            (
                super::GeneratedNames::value_holder(function, value),
                HomeRole::Value {
                    function,
                    value,
                    ty,
                },
            )
        }
        (
            MinecraftOptimizationLevel::None | MinecraftOptimizationLevel::Baseline,
            AssignedHomeRole::PinnedResult { result_index },
        ) => (
            super::GeneratedNames::result_holder(function, result_index),
            HomeRole::Result {
                function,
                result_index,
                ty,
            },
        ),
        (MinecraftOptimizationLevel::Baseline, AssignedHomeRole::Register { ordinal }) => (
            super::GeneratedNames::assigned_home_holder(function, ordinal),
            HomeRole::Register {
                function,
                ordinal,
                ty,
            },
        ),
        (MinecraftOptimizationLevel::Baseline, AssignedHomeRole::RecipeTemporary { ordinal }) => (
            super::GeneratedNames::recipe_temporary_holder(function, ty, ordinal),
            HomeRole::RecipeTemporary {
                function,
                ordinal,
                ty,
            },
        ),
        (
            MinecraftOptimizationLevel::None,
            AssignedHomeRole::Register { .. } | AssignedHomeRole::RecipeTemporary { .. },
        )
        | (MinecraftOptimizationLevel::Baseline, AssignedHomeRole::LegacyValue { .. }) => {
            return Err(invalid_assignment(function));
        }
    };
    Ok(Home { holder, role })
}

fn push_home(homes: &mut EntityVec<HomeId, Home>, home: Home) -> Result<HomeId, PlanBuildError> {
    homes
        .push(home)
        .map_err(|EntityLimitError| PlanBuildError::EntityLimit {
            table: PlanTable::Homes,
        })
}

fn flatten_abi(
    function: FunctionId,
    assignment: &FunctionHomeAssignment,
    homes: &LocalHomeMap,
) -> Result<FunctionAbi, PlanBuildError> {
    let parameters = assignment
        .abi()
        .parameters()
        .iter()
        .copied()
        .map(|home| homes.assigned(function, home))
        .collect::<Result<Vec<_>, _>>()?;
    let results = assignment
        .abi()
        .results()
        .iter()
        .copied()
        .map(|home| homes.assigned(function, home))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FunctionAbi::new(
        assignment.abi().entry_block(),
        parameters,
        results,
    ))
}

fn flatten_value_homes(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assignment: &FunctionHomeAssignment,
    homes: &LocalHomeMap,
) -> Result<Box<[Option<HomeId>]>, PlanBuildError> {
    if assignment.value_assignment_slots().len() != body.value_counts().allocated {
        return Err(invalid_assignment(function));
    }
    assignment
        .value_assignment_slots()
        .iter()
        .copied()
        .enumerate()
        .map(|(index, value_assignment)| {
            let Some(value_assignment) = value_assignment else {
                return Ok(None);
            };
            let value = indexed_value(index).ok_or_else(|| invalid_assignment(function))?;
            validate_value_assignment(function, body, value, value_assignment, assignment)?;
            Ok(Some(homes.assigned(function, value_assignment.home())?))
        })
        .collect::<Result<Vec<_>, PlanBuildError>>()
        .map(Vec::into_boxed_slice)
}

fn validate_value_assignment(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    expected_value: ValueId,
    value_assignment: super::super::assignment::ValueAssignment,
    assignment: &FunctionHomeAssignment,
) -> Result<(), PlanBuildError> {
    let value_type = body
        .value(expected_value)
        .ok_or_else(|| invalid_assignment(function))?
        .ty();
    let assigned_home = assignment
        .home(value_assignment.home())
        .copied()
        .ok_or_else(|| invalid_assignment(function))?;
    if value_assignment.value() != expected_value
        || value_assignment.ty() != value_type
        || assigned_home.ty() != value_type
    {
        return Err(invalid_assignment(function));
    }
    Ok(())
}

fn flatten_instruction_plans(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    assignment: &FunctionHomeAssignment,
    resources: &FunctionResourceInventory,
    homes: &LocalHomeMap,
    preflight: &TargetPreflight,
) -> Result<Box<[Option<InstructionPlan>]>, PlanBuildError> {
    if assignment.instruction_slots().len() != body.instruction_counts().allocated {
        return Err(invalid_assignment(function));
    }
    assignment
        .instruction_slots()
        .iter()
        .enumerate()
        .map(|(index, plan)| {
            let instruction =
                indexed_instruction(index).ok_or_else(|| invalid_assignment(function))?;
            plan.as_ref()
                .map(|plan| {
                    flatten_instruction_plan(
                        function,
                        instruction,
                        body,
                        plan,
                        resources,
                        homes,
                        preflight,
                    )
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, PlanBuildError>>()
        .map(Vec::into_boxed_slice)
}

#[allow(
    clippy::too_many_lines,
    reason = "instruction-plan flattening keeps one arm per InstructionPlan variant in a single pass"
)]
fn flatten_instruction_plan(
    function: FunctionId,
    instruction: InstId,
    body: &crate::ir::core::FunctionBody,
    plan: &AssignedInstructionPlan,
    resources: &FunctionResourceInventory,
    homes: &LocalHomeMap,
    preflight: &TargetPreflight,
) -> Result<InstructionPlan, PlanBuildError> {
    match plan {
        AssignedInstructionPlan::OmittedPure => Ok(InstructionPlan::OmittedPure),
        AssignedInstructionPlan::External { results } => {
            let data = body
                .instruction(instruction)
                .ok_or_else(|| invalid_assignment(function))?;
            let CoreOp::External(external) = data.op() else {
                return Err(invalid_assignment(function));
            };
            if let Some(recipe) = preflight.selected_recipe(*external) {
                if recipe.is_unusable_inline() {
                    return Ok(InstructionPlan::External {
                        helper: resources
                            .external_helper(instruction)
                            .ok_or_else(|| invalid_resources(function))?,
                    });
                }
                if resources.external_helper(instruction).is_some() {
                    return Err(invalid_resources(function));
                }
                Ok(InstructionPlan::Minecraft {
                    external: *external,
                    recipe: recipe.recipe_id(),
                    results: results
                        .iter()
                        .copied()
                        .map(|result| match result {
                            AssignedScalarResult::Semantic {
                                result_index,
                                value,
                                home,
                            } => Ok(ScalarResultPlacement::Semantic {
                                result_index,
                                value,
                                home: homes.assigned(function, home)?,
                            }),
                            AssignedScalarResult::RecipeTemporary { result_index, home } => {
                                Ok(ScalarResultPlacement::RecipeTemporary {
                                    result_index,
                                    home: homes.assigned(function, home)?,
                                })
                            }
                        })
                        .collect::<Result<Vec<_>, PlanBuildError>>()?
                        .into_boxed_slice(),
                })
            } else {
                Ok(InstructionPlan::External {
                    helper: resources
                        .external_helper(instruction)
                        .ok_or_else(|| invalid_resources(function))?,
                })
            }
        }
        AssignedInstructionPlan::Scalar { operands, results } => {
            let operands = flatten_assigned_ids(function, operands, homes)?;
            let results = results
                .iter()
                .copied()
                .map(|result| match result {
                    AssignedScalarResult::Semantic {
                        result_index,
                        value,
                        home,
                    } => Ok(ScalarResultPlacement::Semantic {
                        result_index,
                        value,
                        home: homes.assigned(function, home)?,
                    }),
                    AssignedScalarResult::RecipeTemporary { result_index, home } => {
                        Ok(ScalarResultPlacement::RecipeTemporary {
                            result_index,
                            home: homes.assigned(function, home)?,
                        })
                    }
                })
                .collect::<Result<Vec<_>, PlanBuildError>>()?
                .into_boxed_slice();
            Ok(InstructionPlan::Scalar { operands, results })
        }
        AssignedInstructionPlan::Call {
            arguments,
            result_destinations,
        } => {
            let arguments = flatten_assigned_ids(function, arguments, homes)?;
            let result_destinations = result_destinations
                .iter()
                .copied()
                .map(|destination| {
                    destination
                        .map(|destination| {
                            Ok(CallResultDestination {
                                result_index: destination.result_index(),
                                value: destination.value(),
                                home: homes.assigned(function, destination.home())?,
                            })
                        })
                        .transpose()
                })
                .collect::<Result<Vec<_>, PlanBuildError>>()?
                .into_boxed_slice();
            Ok(InstructionPlan::Call {
                arguments,
                result_destinations,
            })
        }
    }
}

fn flatten_assigned_ids(
    function: FunctionId,
    assigned: &[AssignedHomeId],
    homes: &LocalHomeMap,
) -> Result<Box<[HomeId]>, PlanBuildError> {
    assigned
        .iter()
        .copied()
        .map(|home| homes.assigned(function, home))
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

fn flatten_edge_transfers(
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    transfers: &FunctionEdgeTransferPlan,
    resources: &FunctionResourceInventory,
    homes: &LocalHomeMap,
) -> Result<Box<[Option<EdgeTransfer>]>, PlanBuildError> {
    if transfers.block_slots().len() != body.block_counts().allocated
        || resources.branch_helper_slots().len() != transfers.edges().len()
    {
        return Err(invalid_transfers(function));
    }
    let mut owners = vec![0_u8; transfers.edges().len()];
    let result = transfers
        .block_slots()
        .iter()
        .copied()
        .enumerate()
        .map(|(index, transfer)| {
            let block = indexed_block(index).ok_or_else(|| invalid_transfers(function))?;
            transfer
                .map(|transfer| {
                    flatten_block_transfer(
                        function,
                        block,
                        transfer,
                        transfers,
                        resources,
                        homes,
                        &mut owners,
                    )
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, PlanBuildError>>()?;
    if owners.into_iter().any(|owners| owners != 1) {
        return Err(invalid_transfers(function));
    }
    Ok(result.into_boxed_slice())
}

#[allow(
    clippy::too_many_arguments,
    reason = "one block-transfer conversion keeps each immutable owner and local identity map explicit"
)]
fn flatten_block_transfer(
    function: FunctionId,
    block: BlockId,
    transfer: BlockTransfer,
    transfers: &FunctionEdgeTransferPlan,
    resources: &FunctionResourceInventory,
    homes: &LocalHomeMap,
    owners: &mut [u8],
) -> Result<EdgeTransfer, PlanBuildError> {
    match transfer {
        BlockTransfer::Jump { edge_index } => {
            let edge = checked_edge(
                function,
                block,
                edge_index,
                CoreEdgeKind::Jump,
                transfers,
                owners,
            )?;
            if resources.branch_helper(edge_index).is_some() {
                return Err(invalid_resources(function));
            }
            Ok(EdgeTransfer::Jump {
                steps: flatten_steps(function, edge.steps(), homes)?,
            })
        }
        BlockTransfer::Branch {
            then_edge_index,
            else_edge_index,
        } => {
            let then_edge = checked_edge(
                function,
                block,
                then_edge_index,
                CoreEdgeKind::Branch(super::BranchArm::Then),
                transfers,
                owners,
            )?;
            let else_edge = checked_edge(
                function,
                block,
                else_edge_index,
                CoreEdgeKind::Branch(super::BranchArm::Else),
                transfers,
                owners,
            )?;
            let then_helper =
                checked_branch_helper(function, then_edge_index, then_edge.is_empty(), resources)?;
            let else_helper =
                checked_branch_helper(function, else_edge_index, else_edge.is_empty(), resources)?;
            Ok(EdgeTransfer::Branch {
                then_edge: BranchTransfer::new(
                    flatten_steps(function, then_edge.steps(), homes)?.into_vec(),
                    then_helper,
                ),
                else_edge: BranchTransfer::new(
                    flatten_steps(function, else_edge.steps(), homes)?.into_vec(),
                    else_helper,
                ),
            })
        }
    }
}

fn checked_branch_helper(
    function: FunctionId,
    edge_index: usize,
    transfer_is_empty: bool,
    resources: &FunctionResourceInventory,
) -> Result<Option<PlannedFunctionId>, PlanBuildError> {
    let helper = resources.branch_helper(edge_index);
    if transfer_is_empty == helper.is_none() {
        Ok(helper)
    } else {
        Err(invalid_resources(function))
    }
}

fn checked_edge<'a>(
    function: FunctionId,
    block: BlockId,
    edge_index: usize,
    kind: CoreEdgeKind,
    transfers: &'a FunctionEdgeTransferPlan,
    owners: &mut [u8],
) -> Result<&'a super::super::edge_transfer::PlannedEdgeTransfer, PlanBuildError> {
    let edge = transfers
        .edges()
        .get(edge_index)
        .ok_or_else(|| invalid_transfers(function))?;
    if edge.source() != block || edge.kind() != kind {
        return Err(invalid_transfers(function));
    }
    let owner = owners
        .get_mut(edge_index)
        .ok_or_else(|| invalid_transfers(function))?;
    *owner = owner
        .checked_add(1)
        .ok_or_else(|| invalid_transfers(function))?;
    Ok(edge)
}

fn flatten_steps(
    function: FunctionId,
    steps: &[super::super::edge_transfer::TransferStep],
    homes: &LocalHomeMap,
) -> Result<Box<[MoveStep]>, PlanBuildError> {
    steps
        .iter()
        .copied()
        .map(|step| {
            Ok(MoveStep::new(
                flatten_transfer_location(function, step.destination(), homes)?,
                flatten_transfer_location(function, step.source(), homes)?,
            ))
        })
        .collect::<Result<Vec<_>, PlanBuildError>>()
        .map(Vec::into_boxed_slice)
}

fn flatten_transfer_location(
    function: FunctionId,
    location: TransferLocation,
    homes: &LocalHomeMap,
) -> Result<HomeId, PlanBuildError> {
    match location {
        TransferLocation::Home(home) => homes.assigned(function, home),
        TransferLocation::Temporary(kind) => homes.edge_temporary(function, kind),
    }
}

fn physical_type_index(ty: CoreType) -> usize {
    PHYSICAL_TYPE_ORDER
        .iter()
        .position(|candidate| *candidate == ty)
        .expect("the physical type order covers every Core type")
}

fn indexed_value(index: usize) -> Option<ValueId> {
    u32::try_from(index).ok().map(ValueId::from_index)
}

fn indexed_instruction(index: usize) -> Option<InstId> {
    u32::try_from(index).ok().map(InstId::from_index)
}

fn indexed_block(index: usize) -> Option<BlockId> {
    u32::try_from(index).ok().map(BlockId::from_index)
}

const fn invalid_assignment(function: FunctionId) -> PlanBuildError {
    PlanBuildError::InvalidPhaseInput {
        phase: PlanInputPhase::Assignment,
        function: Some(function),
    }
}

const fn invalid_transfers(function: FunctionId) -> PlanBuildError {
    PlanBuildError::InvalidPhaseInput {
        phase: PlanInputPhase::Transfers,
        function: Some(function),
    }
}

const fn invalid_control(function: FunctionId) -> PlanBuildError {
    PlanBuildError::InvalidPhaseInput {
        phase: PlanInputPhase::ControlRecipes,
        function: Some(function),
    }
}

#[cfg(test)]
const fn invalid_control_without_function() -> PlanBuildError {
    PlanBuildError::InvalidPhaseInput {
        phase: PlanInputPhase::ControlRecipes,
        function: None,
    }
}

const fn invalid_resources(function: FunctionId) -> PlanBuildError {
    PlanBuildError::InvalidPhaseInput {
        phase: PlanInputPhase::Resources,
        function: Some(function),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_HOME_COUNT, assemble_candidate, checked_add_home_count, validate_home_capacity,
        validate_phase_function_count,
    };
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind, ValueDef, ValueId,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::assignment::HomeAssignment;
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::lower::minecraft::edge_transfer::EdgeTransferPlan;
    use crate::lower::minecraft::liveness::{LivenessEventLimit, LivenessLimits, LivenessResult};
    use crate::lower::minecraft::plan::{
        HomeRole, InstructionPlan, LoweringPlan, PlanBuildError, PlanInputPhase, PlanTable,
        ScalarResultPlacement,
    };
    use crate::lower::minecraft::resources::ResourceInventory;
    use crate::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    fn options(level: MinecraftOptimizationLevel) -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
        .with_optimization_level(level)
    }

    fn none_parts(
        core: &CoreProgram,
        options: &LoweringOptions,
    ) -> (HomeAssignment, EdgeTransferPlan, ResourceInventory) {
        let inventory = SemanticInventory::new(core).unwrap();
        let assignment = HomeAssignment::for_none(core, &inventory).unwrap();
        let transfers = EdgeTransferPlan::for_none(core, &inventory, &assignment).unwrap();
        let resources = ResourceInventory::new(core, &inventory, &transfers, options).unwrap();
        (assignment, transfers, resources)
    }

    fn baseline_parts(
        core: &CoreProgram,
        options: &LoweringOptions,
    ) -> (HomeAssignment, EdgeTransferPlan, ResourceInventory) {
        baseline_parts_with_liveness_limits(core, options, LivenessLimits::derived())
    }

    fn baseline_parts_with_liveness_limits(
        core: &CoreProgram,
        options: &LoweringOptions,
        limits: LivenessLimits,
    ) -> (HomeAssignment, EdgeTransferPlan, ResourceInventory) {
        let inventory = SemanticInventory::new(core).unwrap();
        let demand = RuntimeDemand::for_level(
            core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let liveness = LivenessResult::for_baseline(core, &inventory, &demand, limits).unwrap();
        let assignment =
            HomeAssignment::for_baseline(core, &inventory, &demand, &liveness).unwrap();
        let transfers = EdgeTransferPlan::for_baseline(core, &inventory, &assignment).unwrap();
        let resources = ResourceInventory::new(core, &inventory, &transfers, options).unwrap();
        (assignment, transfers, resources)
    }

    #[test]
    fn none_publishes_exact_legacy_home_resource_and_instruction_order() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("legacy"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let parameter = block_values(&builder, builder.entry_block())[0];
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(parameter, one, OriginId::UNKNOWN)
            .unwrap();
        let sum_instruction = defining_instruction(builder.body(), sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let options = options(MinecraftOptimizationLevel::None);
        let (assignment, transfers, resources) = none_parts(&core, &options);
        let plan =
            LoweringPlan::from_parts(&core, &options, assignment, transfers, resources).unwrap();

        assert_eq!(plan.optimization_level(), MinecraftOptimizationLevel::None);
        assert_eq!(
            plan.homes
                .iter()
                .map(|(_, home)| home.holder.to_string())
                .collect::<Vec<_>>(),
            ["#f0v0", "#f0v1", "#f0v2", "#f0r0"]
        );
        assert!(matches!(
            plan.homes.get(crate::lower::minecraft::plan::HomeId::from_index(0)).unwrap().role,
            HomeRole::Value { value, .. } if value == parameter
        ));
        assert_eq!(
            plan.target_functions
                .iter()
                .map(|(_, function)| function.resource.to_string())
                .collect::<Vec<_>>(),
            [
                "mdl:__mdl/load",
                "mdl:__mdl/init/try_create",
                "mdl:__mdl/f0/b0"
            ]
        );
        let InstructionPlan::Scalar { operands, results } =
            plan.instruction_plan(function, sum_instruction).unwrap()
        else {
            panic!("wrapping add must retain a scalar plan")
        };
        assert_eq!(operands.len(), 2);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result_index(), 0);
        assert_eq!(results[0].value(), Some(sum));
        assert_eq!(results[0].home(), plan.value_home(function, sum).unwrap());
    }

    #[test]
    fn baseline_candidate_names_recipe_scratch_without_semantic_correlation() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("overflow"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (sum, overflowed) = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap();
        let instruction = defining_instruction(builder.body(), sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![overflowed]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let options = options(MinecraftOptimizationLevel::Baseline);
        let (assignment, transfers, resources) = baseline_parts(&core, &options);
        let plan =
            assemble_candidate(&core, &options, &assignment, &transfers, &resources).unwrap();

        assert_eq!(
            plan.homes
                .iter()
                .map(|(_, home)| home.holder.to_string())
                .collect::<Vec<_>>(),
            ["#f0h0", "#f0h1", "#f0h2", "#f0r0", "#f0qi0"]
        );
        assert!(plan.value_home(function, sum).is_none());
        assert!(matches!(
            plan.homes.iter().last().unwrap().1.role,
            HomeRole::RecipeTemporary {
                ordinal: 0,
                ty: CoreType::I32,
                ..
            }
        ));
        let InstructionPlan::Scalar { results, .. } =
            plan.instruction_plan(function, instruction).unwrap()
        else {
            panic!("overflowing add must retain a scalar plan")
        };
        assert!(matches!(
            results[0],
            ScalarResultPlacement::RecipeTemporary {
                result_index: 0,
                ..
            }
        ));
        assert_eq!(results[0].value(), None);
        assert_eq!(results[1].value(), Some(overflowed));

        let published = LoweringPlan::from_parts(&core, &options, assignment, transfers, resources)
            .expect("partial semantic results with recipe scratch form a valid final plan");
        assert!(published.value_home(function, sum).is_none());
    }

    #[test]
    fn baseline_candidate_flattens_typed_cycle_scratch_after_assigned_homes() {
        let (core, function) = typed_cycle_program();
        let options = options(MinecraftOptimizationLevel::Baseline);
        let (assignment, transfers, resources) = baseline_parts(&core, &options);
        assert_eq!(resources.branch_helper(function, 0), None);
        assert!(resources.branch_helper(function, 1).is_some());
        assert_eq!(resources.branch_helper(function, 2), None);
        assert!(!transfers.function(function).unwrap().edges()[1].is_empty());
        assert!(transfers.function(function).unwrap().edges()[2].is_empty());
        let plan =
            assemble_candidate(&core, &options, &assignment, &transfers, &resources).unwrap();
        let layout = plan.functions.get(function).unwrap();

        assert_eq!(
            plan.homes
                .iter()
                .take(4)
                .map(|(_, home)| home.holder.to_string())
                .collect::<Vec<_>>(),
            ["#f0v0", "#f0v1", "#f0v2", "#f0h0"]
        );
        assert!(layout.parallel_copy_temp.is_none());
        assert_eq!(layout.edge_temporaries.len(), 1);
        let temporary = layout.edge_temporaries[0];
        let temporary_home = plan.homes.get(temporary).unwrap();
        assert_eq!(temporary_home.holder.to_string(), "#f0ti0");
        assert_eq!(
            temporary_home.role,
            HomeRole::TypedEdgeTemporary {
                function,
                ordinal: 0,
                ty: CoreType::I32,
            }
        );
        assert!(layout.edge_transfers.iter().flatten().any(|transfer| {
            let steps: Vec<_> = match transfer {
                super::EdgeTransfer::Jump { steps } => steps.iter().collect(),
                super::EdgeTransfer::Branch {
                    then_edge,
                    else_edge,
                } => then_edge.steps().iter().chain(else_edge.steps()).collect(),
            };
            steps
                .iter()
                .any(|step| step.destination() == temporary || step.source() == temporary)
        }));

        let (assignment, transfers, resources) = baseline_parts(&core, &options);
        let repeated =
            assemble_candidate(&core, &options, &assignment, &transfers, &resources).unwrap();
        assert_eq!(format!("{plan:#?}"), format!("{repeated:#?}"));

        let (assignment, transfers, resources) = baseline_parts(&core, &options);
        let published = LoweringPlan::from_parts(&core, &options, assignment, transfers, resources)
            .expect("the critical-edge helper must survive final independent verification");
        let published_layout = published.functions.get(function).unwrap();
        let (then_edge, else_edge) = published_layout
            .edge_transfers
            .iter()
            .flatten()
            .find_map(|transfer| match transfer {
                super::EdgeTransfer::Branch {
                    then_edge,
                    else_edge,
                } => Some((then_edge, else_edge)),
                super::EdgeTransfer::Jump { .. } => None,
            })
            .expect("the published typed cycle retains its branch");
        assert!(!then_edge.steps().is_empty());
        assert!(then_edge.helper().is_some());
        assert!(else_edge.steps().is_empty());
        assert!(else_edge.helper().is_none());
    }

    #[test]
    fn baseline_publishes_deterministic_all_distinct_liveness_fallback_reporting() {
        let (core, _) = identity_program();
        let options = options(MinecraftOptimizationLevel::Baseline);
        let limits = LivenessLimits::derived().with_event_limit(LivenessEventLimit::new(0));
        let build = || {
            let (assignment, transfers, resources) =
                baseline_parts_with_liveness_limits(&core, &options, limits);
            LoweringPlan::from_parts(&core, &options, assignment, transfers, resources)
                .unwrap()
                .report(&core)
                .dump()
        };
        let first = build();
        let second = build();
        assert_eq!(first, second);
        assert!(first.contains(
            "coalescing tracked-values=1 liveness-events=0 segments=0 candidates=0 merges=0 work=0 fallback=liveness-propagation-events"
        ));
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the complete two-function fixture makes indexed caller and callee results explicit"
    )]
    #[allow(
        clippy::similar_names,
        reason = "caller and callee identities make the two sides of the call boundary explicit"
    )]
    fn baseline_call_keeps_only_the_later_indexed_destination() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let callee = core
            .declare_function(
                Some("callee"),
                vec![CoreType::I32, CoreType::Bool],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let caller = core
            .declare_function(
                Some("caller"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut callee_builder = FunctionBuilder::new(&core, &sources, callee).unwrap();
        let callee_parameters = block_values(&callee_builder, callee_builder.entry_block());
        callee_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(callee_parameters),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(callee, callee_builder.finish().unwrap())
            .unwrap();

        let mut caller_builder = FunctionBuilder::new(&core, &sources, caller).unwrap();
        let integer = caller_builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let boolean = caller_builder
            .bool_constant(true, OriginId::UNKNOWN)
            .unwrap();
        let results = caller_builder
            .call(callee, vec![integer, boolean], OriginId::UNKNOWN)
            .unwrap();
        let call = defining_instruction(caller_builder.body(), results[0]);
        caller_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![results[1]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(caller, caller_builder.finish().unwrap())
            .unwrap();

        let options = options(MinecraftOptimizationLevel::Baseline);
        let (assignment, transfers, resources) = baseline_parts(&core, &options);
        let plan =
            assemble_candidate(&core, &options, &assignment, &transfers, &resources).unwrap();
        let InstructionPlan::Call {
            arguments,
            result_destinations,
        } = plan.instruction_plan(caller, call).unwrap()
        else {
            panic!("call must retain its indexed result plan")
        };
        assert_eq!(arguments.len(), 2);
        assert_eq!(result_destinations.len(), 2);
        assert!(result_destinations[0].is_none());
        let destination = result_destinations[1].unwrap();
        assert_eq!(destination.result_index(), 1);
        assert_eq!(destination.value(), results[1]);
        assert_eq!(
            destination.home(),
            plan.value_home(caller, results[1]).unwrap()
        );
    }

    #[test]
    fn empty_phase_records_reject_cross_policy_before_incidental_shape_can_match() {
        let (core, _) = empty_program();
        let none_options = options(MinecraftOptimizationLevel::None);
        let baseline_options = options(MinecraftOptimizationLevel::Baseline);
        let (assignment, transfers, resources) = none_parts(&core, &none_options);

        assert_eq!(
            assemble_candidate(
                &core,
                &baseline_options,
                &assignment,
                &transfers,
                &resources,
            )
            .unwrap_err(),
            PlanBuildError::OptimizationLevelMismatch {
                phase: PlanInputPhase::Assignment,
                expected: MinecraftOptimizationLevel::Baseline,
                actual: MinecraftOptimizationLevel::None,
            }
        );

        let (core, _) = identity_program();
        let (assignment, transfers, resources) = none_parts(&core, &none_options);
        assert!(matches!(
            assemble_candidate(
                &core,
                &baseline_options,
                &assignment,
                &transfers,
                &resources,
            ),
            Err(PlanBuildError::OptimizationLevelMismatch {
                phase: PlanInputPhase::Assignment,
                ..
            })
        ));
    }

    #[test]
    fn exact_phase_counts_reject_an_extra_function_and_home_capacity_is_checked() {
        assert_eq!(
            validate_phase_function_count(PlanInputPhase::Transfers, 3, 4),
            Err(PlanBuildError::PhaseFunctionCountMismatch {
                phase: PlanInputPhase::Transfers,
                expected: 3,
                actual: 4,
            })
        );
        assert_eq!(validate_home_capacity(MAX_HOME_COUNT), Ok(()));
        assert_eq!(
            validate_home_capacity(MAX_HOME_COUNT + 1),
            Err(PlanBuildError::CapacityExceeded {
                table: PlanTable::Homes,
                required: MAX_HOME_COUNT + 1,
                maximum: MAX_HOME_COUNT,
            })
        );
        assert_eq!(
            checked_add_home_count(u64::MAX, 1),
            Err(PlanBuildError::CapacityOverflow {
                table: PlanTable::Homes,
            })
        );
    }

    fn empty_program() -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("empty"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
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

    fn identity_program() -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("identity"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let parameter = block_values(&builder, builder.entry_block())[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the typed cycle fixture keeps its three control-flow blocks and simultaneous copy visible"
    )]
    fn typed_cycle_program() -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("typed-cycle"),
                vec![CoreType::I32, CoreType::I32, CoreType::Bool],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let entry_values = block_values(&builder, entry);
        let loop_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first = builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let second = builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let condition = builder
            .append_block_parameter(loop_block, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let output = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(loop_block, entry_values)),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(loop_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(loop_block, vec![second, first, condition]),
                    else_target: BlockTarget::new(exit, vec![first]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![output]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
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
        let ValueDef::InstResult { instruction, .. } = body.value(value).unwrap().definition()
        else {
            panic!("fixture value must be an instruction result")
        };
        instruction
    }
}
