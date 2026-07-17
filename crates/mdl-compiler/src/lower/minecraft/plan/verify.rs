use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, BlockTarget, CoreOp, CoreProgram, CoreType, FunctionId, InstId, TerminatorKind,
    ValueId,
};
use crate::ir::minecraft::{FakeScoreHolder, FunctionResourceId};
use crate::source::OriginId;

use super::minimum_demand::{FunctionMinimumDemand, MinimumDemandError, MinimumSemanticDemand};
use super::{
    BranchArm, BranchTransfer, EdgeTransfer, FunctionLayout, HomeId, HomeRole, InstructionPlan,
    LoweringPlan, MinecraftOptimizationLevel, PlannedFunctionId, PlannedFunctionRole,
    ScalarResultPlacement,
};
use crate::lower::minecraft::placement::{
    BlockPlacement, BranchArmRecipe, ControlRecipeStatistics, InlineZeroAbiTerminalCall,
    RecipeDecisionReason,
};
use crate::lower::minecraft::recipe::{
    ControlRecipeCost, ControlRecipeKind, RecipePreference, WholeGraphImpact, compare_recipe_costs,
};
use crate::lower::minecraft::{GeneratedNames, LoweringOptions};

const MAX_STRUCTURAL_FINDINGS: usize = 64;

/// Independently verifies one final plan against Core alone.
pub(super) fn verify_plan(core: &CoreProgram, plan: &LoweringPlan) -> Result<(), Diagnostics> {
    if let Err(error) = plan.physical_preflight.verify(&plan.physical) {
        return Err(Diagnostics::from_findings(vec![Diagnostic::new(
            "lower.plan.physical-preflight",
            error.to_string(),
            OriginId::UNKNOWN,
        )])
        .expect("one physical-preflight finding forms diagnostics"));
    }
    let minimum = MinimumSemanticDemand::new(core).map_err(minimum_demand_diagnostics)?;
    let mut verifier = PlanVerifier::new(core, plan, &minimum);
    verifier.verify_program();
    verifier.finish()?;
    super::symbolic::verify_symbolic_home_contents(core, plan, &minimum)
}

struct PlanVerifier<'a> {
    core: &'a CoreProgram,
    plan: &'a LoweringPlan,
    minimum: &'a MinimumSemanticDemand,
    naming_options: Option<LoweringOptions>,
    findings: Vec<Diagnostic>,
    findings_truncated: bool,
    home_referenced: Vec<bool>,
    edge_temporary_used: Vec<bool>,
    function_owners: Vec<u32>,
    token_values: Vec<Option<HomeId>>,
    token_epochs: Vec<u32>,
    token_epoch: u32,
    generated_exact_one_contract_valid: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpectedBranchArmDecision {
    Materialized(RecipeDecisionReason),
    InlineZeroAbiTerminalCall,
}

impl<'a> PlanVerifier<'a> {
    fn new(
        core: &'a CoreProgram,
        plan: &'a LoweringPlan,
        minimum: &'a MinimumSemanticDemand,
    ) -> Self {
        let naming_options = LoweringOptions::new(
            plan.target,
            plan.namespace.clone(),
            plan.pack_abi.register_objective.clone(),
        )
        .ok()
        .map(|options| {
            options
                .with_command_limit_assumptions(
                    plan.preflight()
                        .command_limit_evidence()
                        .configured_assumptions(),
                )
                .with_optimization_level(plan.optimization_level)
        });
        Self {
            core,
            plan,
            minimum,
            naming_options,
            findings: Vec::new(),
            findings_truncated: false,
            home_referenced: vec![false; plan.homes.len()],
            edge_temporary_used: vec![false; plan.homes.len()],
            function_owners: vec![0; plan.target_functions.len()],
            token_values: vec![None; plan.homes.len()],
            token_epochs: vec![0; plan.homes.len()],
            token_epoch: 0,
            generated_exact_one_contract_valid: generated_exact_one_contract_is_structural(
                core, plan,
            ),
        }
    }

    fn verify_program(&mut self) {
        self.verify_program_shape();
        self.verify_preflight_and_ambient();
        self.verify_control_statistics();
        self.verify_home_records();
        self.verify_resource_uniqueness();
        self.verify_scaffolding();
        for (function, declaration) in self.core.functions() {
            let Some(body) = declaration.body() else {
                self.report(
                    "lower.plan.missing-definition",
                    format!("plan input is missing {function:?}"),
                    declaration.origin(),
                );
                continue;
            };
            let Some(minimum) = self.minimum.function(function) else {
                self.report(
                    "lower.plan.missing-minimum-demand",
                    format!("verifier demand has no record for {function:?}"),
                    declaration.origin(),
                );
                continue;
            };
            self.verify_function(function, declaration, body, minimum);
        }
        self.verify_final_ownership();
    }

    fn verify_preflight_and_ambient(&mut self) {
        let inventory = match crate::lower::minecraft::analysis::SemanticInventory::new(self.core) {
            Ok(inventory) => inventory,
            Err(error) => {
                self.report(
                    "lower.plan.preflight",
                    format!(
                        "cannot independently verify target preflight: semantic inventory failed: {error:?}"
                    ),
                    OriginId::UNKNOWN,
                );
                return;
            }
        };
        if let Err(diagnostics) = self.plan.preflight().verify(self.core, &inventory) {
            self.report(
                "lower.plan.preflight",
                format!("retained target preflight failed independent verification: {diagnostics}"),
                OriginId::UNKNOWN,
            );
        }
        if let Err(error) = self.plan.ambient().verify(self.core) {
            self.report(
                "lower.plan.ambient-analysis",
                format!("retained Core ambient analysis failed independent verification: {error}"),
                OriginId::UNKNOWN,
            );
        }
    }

    fn verify_control_statistics(&mut self) {
        let mut branch_arms = 0_u64;
        let mut selected = 0_u64;
        let mut consumed = 0_u64;
        let mut overflowed = false;

        for (_, layout) in self.plan.functions.iter() {
            for recipe in layout.branch_recipes.iter().flatten().copied() {
                match branch_arms.checked_add(2) {
                    Some(next) => branch_arms = next,
                    None => overflowed = true,
                }
                for arm in [BranchArm::Then, BranchArm::Else] {
                    if matches!(
                        recipe.arm(arm),
                        BranchArmRecipe::InlineZeroAbiTerminalCall(_)
                    ) {
                        match selected.checked_add(1) {
                            Some(next) => selected = next,
                            None => overflowed = true,
                        }
                    }
                }
            }
            for placement in layout.block_placements.iter().flatten() {
                if matches!(placement, BlockPlacement::Consumed { .. }) {
                    match consumed.checked_add(1) {
                        Some(next) => consumed = next,
                        None => overflowed = true,
                    }
                }
            }
        }

        if overflowed {
            self.report(
                "lower.plan.control-statistics-overflow",
                "control placement/recipe statistics exceed their exact counter domain",
                OriginId::UNKNOWN,
            );
            return;
        }
        let recounted = ControlRecipeStatistics::from_counts(branch_arms, selected, consumed);
        if self.plan.control_statistics != recounted {
            self.report(
                "lower.plan.control-statistics",
                format!(
                    "recorded control statistics {:?} do not match independently recounted {:?}",
                    self.plan.control_statistics, recounted
                ),
                OriginId::UNKNOWN,
            );
        }
    }

    fn verify_program_shape(&mut self) {
        if self.plan.functions.len() != self.core.len() {
            self.report(
                "lower.plan.function-count",
                format!(
                    "plan has {} function layouts, expected {}",
                    self.plan.functions.len(),
                    self.core.len()
                ),
                OriginId::UNKNOWN,
            );
        }
        if self.minimum.len() != self.core.len() {
            self.report(
                "lower.plan.minimum-demand-count",
                format!(
                    "verifier demand has {} functions, expected {}",
                    self.minimum.len(),
                    self.core.len()
                ),
                OriginId::UNKNOWN,
            );
        }
        let Some(options) = self.naming_options.as_ref() else {
            self.report(
                "lower.plan.naming-options",
                "plan metadata cannot reconstruct compiler-owned naming options",
                OriginId::UNKNOWN,
            );
            return;
        };
        if self.plan.pack_abi.init_sentinel != options.generated_names().init_sentinel() {
            self.report(
                "lower.plan.init-sentinel",
                "plan initialization sentinel disagrees with its pack ABI",
                OriginId::UNKNOWN,
            );
        }
    }

    fn verify_home_records(&mut self) {
        let mut holders = HashMap::<FakeScoreHolder, HomeId>::new();
        if holders.try_reserve(self.plan.homes.len()).is_err() {
            self.report(
                "lower.plan.verifier-capacity",
                "plan verifier cannot allocate the score-holder uniqueness table",
                OriginId::UNKNOWN,
            );
            return;
        }
        for (home, data) in self.plan.homes.iter() {
            if let Some(first) = holders.insert(data.holder.clone(), home) {
                self.report(
                    "lower.plan.duplicate-holder",
                    format!("{first:?} and {home:?} use the same score holder"),
                    OriginId::UNKNOWN,
                );
            }
            let (owner, expected, policy_ok) = match data.role {
                HomeRole::Value {
                    function, value, ..
                } => (
                    function,
                    GeneratedNames::value_holder(function, value),
                    true,
                ),
                HomeRole::Result {
                    function,
                    result_index,
                    ..
                } => (
                    function,
                    GeneratedNames::result_holder(function, result_index),
                    true,
                ),
                HomeRole::EdgeTemporary { function } => (
                    function,
                    GeneratedNames::edge_temporary_holder(function),
                    self.plan.optimization_level == MinecraftOptimizationLevel::None,
                ),
                HomeRole::Register {
                    function, ordinal, ..
                } => (
                    function,
                    GeneratedNames::assigned_home_holder(function, ordinal),
                    self.plan.optimization_level == MinecraftOptimizationLevel::Baseline,
                ),
                HomeRole::RecipeTemporary {
                    function,
                    ordinal,
                    ty,
                } => (
                    function,
                    GeneratedNames::recipe_temporary_holder(function, ty, ordinal),
                    self.plan.optimization_level == MinecraftOptimizationLevel::Baseline,
                ),
                HomeRole::TypedEdgeTemporary {
                    function,
                    ordinal,
                    ty,
                } => (
                    function,
                    GeneratedNames::typed_edge_temporary_holder(function, ty, ordinal),
                    self.plan.optimization_level == MinecraftOptimizationLevel::Baseline,
                ),
            };
            if self.core.function(owner).is_none() {
                self.report(
                    "lower.plan.foreign-home-owner",
                    format!("{home:?} is owned by absent {owner:?}"),
                    OriginId::UNKNOWN,
                );
            }
            if data.holder != expected {
                self.report(
                    "lower.plan.home-name",
                    format!("{home:?} has a holder inconsistent with {:?}", data.role),
                    OriginId::UNKNOWN,
                );
            }
            if !policy_ok {
                self.report(
                    "lower.plan.home-policy",
                    format!(
                        "{home:?} role {:?} is invalid for {:?}",
                        data.role, self.plan.optimization_level
                    ),
                    OriginId::UNKNOWN,
                );
            }
        }
    }

    fn verify_resource_uniqueness(&mut self) {
        let mut resources = HashMap::<FunctionResourceId, PlannedFunctionId>::new();
        if resources
            .try_reserve(self.plan.target_functions.len())
            .is_err()
        {
            self.report(
                "lower.plan.verifier-capacity",
                "plan verifier cannot allocate the resource uniqueness table",
                OriginId::UNKNOWN,
            );
            return;
        }
        for (planned, function) in self.plan.target_functions.iter() {
            if let Some(first) = resources.insert(function.resource.clone(), planned) {
                self.report(
                    "lower.plan.duplicate-resource",
                    format!("{first:?} and {planned:?} use the same function resource"),
                    function.origin,
                );
            }
        }
    }

    fn verify_scaffolding(&mut self) {
        let Some((load_resource, init_resource)) = self.naming_options.as_ref().map(|options| {
            let names = options.generated_names();
            (names.load_function(), names.init_try_create_function())
        }) else {
            return;
        };
        self.reference_function(
            self.plan.load,
            PlannedFunctionRole::Load,
            OriginId::UNKNOWN,
            &load_resource,
        );
        self.reference_function(
            self.plan.init_try_create,
            PlannedFunctionRole::InitTryCreate,
            OriginId::UNKNOWN,
            &init_resource,
        );
    }

    fn verify_function(
        &mut self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        minimum: &FunctionMinimumDemand,
    ) {
        let Some(layout) = self.plan.functions.get(function) else {
            self.report(
                "lower.plan.missing-function-layout",
                format!("plan has no layout for {function:?}"),
                declaration.origin(),
            );
            return;
        };
        if layout.diagnostic_name_hint.as_deref() != declaration.name_hint()
            || &*layout.parameter_types != declaration.parameters()
            || &*layout.result_types != declaration.results()
        {
            self.report(
                "lower.plan.metadata-mismatch",
                format!("copied metadata for {function:?} disagrees with Core"),
                declaration.origin(),
            );
        }
        self.verify_function_table_lengths(function, body, layout, minimum);
        self.verify_abi(function, declaration, body, layout);
        self.verify_values(function, body, layout, minimum);
        self.verify_instructions(function, body, layout, minimum);
        self.verify_edge_temporary_declarations(function, layout);
        self.verify_blocks(function, declaration, body, layout, minimum);
    }

    fn verify_function_table_lengths(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        layout: &FunctionLayout,
        minimum: &FunctionMinimumDemand,
    ) {
        let checks = [
            (
                "block-placements",
                layout.block_placements.len(),
                body.block_counts().allocated,
            ),
            (
                "branch-recipes",
                layout.branch_recipes.len(),
                body.block_counts().allocated,
            ),
            (
                "block-functions",
                layout.block_functions.len(),
                body.block_counts().allocated,
            ),
            (
                "edge-transfers",
                layout.edge_transfers.len(),
                body.block_counts().allocated,
            ),
            (
                "value-homes",
                layout.value_homes.len(),
                body.value_counts().allocated,
            ),
            (
                "instruction-plans",
                layout.instruction_plans.len(),
                body.instruction_counts().allocated,
            ),
        ];
        for (table, actual, expected) in checks {
            if actual != expected {
                self.report(
                    "lower.plan.table-length",
                    format!("{function:?} {table} has length {actual}, expected {expected}"),
                    OriginId::UNKNOWN,
                );
            }
        }
        let expected_assigned = self.plan.physical.function_assigned_storage_count(function);
        if self.plan.has_recursive_activation() && layout.assigned_homes.len() != expected_assigned
        {
            self.report(
                "lower.plan.assigned-home-map",
                format!(
                    "{function:?} assigned-home map has length {}, expected {expected_assigned}",
                    layout.assigned_homes.len()
                ),
                OriginId::UNKNOWN,
            );
        }
        if minimum.block_slots() != body.block_counts().allocated
            || minimum.value_slots() != body.value_counts().allocated
            || minimum.instruction_slots() != body.instruction_counts().allocated
        {
            self.report(
                "lower.plan.minimum-demand-shape",
                format!("verifier demand tables for {function:?} disagree with Core"),
                OriginId::UNKNOWN,
            );
        }
    }

    fn verify_abi(
        &mut self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        layout: &FunctionLayout,
    ) {
        if layout.abi.entry_block != body.entry() {
            self.report(
                "lower.plan.abi-entry",
                format!("ABI entry for {function:?} is not the Core entry"),
                declaration.origin(),
            );
        }
        let parameters = body
            .block(body.entry())
            .map_or(&[][..], crate::ir::core::BlockData::parameters);
        if parameters.len() != layout.abi.parameters.len()
            || declaration.results().len() != layout.abi.results.len()
        {
            self.report(
                "lower.plan.abi-arity",
                format!("ABI arity for {function:?} disagrees with Core"),
                declaration.origin(),
            );
        }
        for (parameter_index, (parameter, home)) in
            parameters.iter().zip(&layout.abi.parameters).enumerate()
        {
            let mapped = slot(&layout.value_homes, parameter.value());
            if mapped != Some(*home) {
                self.report(
                    "lower.plan.abi-parameter",
                    format!("ABI parameter {parameter_index} for {function:?} has the wrong home"),
                    parameter.origin(),
                );
            }
            let ty = body
                .value(parameter.value())
                .map(crate::ir::core::ValueData::ty);
            if let Some(ty) = ty {
                self.expect_home_role(
                    *home,
                    HomeRole::Value {
                        function,
                        value: parameter.value(),
                        ty,
                    },
                    parameter.origin(),
                );
            }
        }
        for (result_index, (ty, home)) in declaration
            .results()
            .iter()
            .zip(&layout.abi.results)
            .enumerate()
        {
            self.expect_home_role(
                *home,
                HomeRole::Result {
                    function,
                    result_index,
                    ty: *ty,
                },
                declaration.origin(),
            );
        }
    }

    fn verify_values(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        layout: &FunctionLayout,
        minimum: &FunctionMinimumDemand,
    ) {
        for index in 0..body.value_counts().allocated {
            let Some(value) = dense_id::<ValueId>(index) else {
                self.report(
                    "lower.plan.value-index",
                    format!("value table for {function:?} exceeds the Core ID domain"),
                    OriginId::UNKNOWN,
                );
                break;
            };
            let reachable = minimum.is_value_reachable(value).unwrap_or(false);
            let required = minimum.requires_value(value).unwrap_or(false);
            let mapped = slot(&layout.value_homes, value);
            if !reachable && mapped.is_some() {
                self.report(
                    "lower.plan.value-coverage",
                    format!("unreachable {function:?} {value:?} has a home"),
                    OriginId::UNKNOWN,
                );
            }
            if required && mapped.is_none() {
                self.report(
                    "lower.plan.omitted-required-value",
                    format!("required {function:?} {value:?} has no home"),
                    OriginId::UNKNOWN,
                );
            }
            if self.plan.optimization_level == MinecraftOptimizationLevel::None
                && reachable
                && mapped.is_none()
            {
                self.report(
                    "lower.plan.none-value-coverage",
                    format!("None policy omitted reachable {function:?} {value:?}"),
                    OriginId::UNKNOWN,
                );
            }
            let Some(home) = mapped else { continue };
            let Some(value_data) = body.value(value) else {
                self.report(
                    "lower.plan.invalid-value",
                    format!("mapped {function:?} {value:?} is absent from Core"),
                    OriginId::UNKNOWN,
                );
                continue;
            };
            let is_entry_parameter = matches!(
                value_data.definition(),
                crate::ir::core::ValueDef::BlockParam { block, .. } if block == body.entry()
            );
            let valid = match self.plan.homes.get(home).map(|home| home.role) {
                Some(HomeRole::Value {
                    function: owner,
                    value: correlated,
                    ty,
                }) => {
                    owner == function
                        && correlated == value
                        && ty == value_data.ty()
                        && (self.plan.optimization_level == MinecraftOptimizationLevel::None
                            || is_entry_parameter)
                }
                Some(HomeRole::Register {
                    function: owner,
                    ty,
                    ..
                }) => {
                    owner == function
                        && ty == value_data.ty()
                        && self.plan.optimization_level == MinecraftOptimizationLevel::Baseline
                        && !is_entry_parameter
                }
                _ => false,
            };
            let origin = value_origin(body, value_data);
            if !valid {
                self.report(
                    "lower.plan.value-home",
                    format!("home for {function:?} {value:?} has the wrong role, owner, or type"),
                    origin,
                );
            }
            self.reference_home(home, origin);
        }
    }

    fn verify_instructions(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        layout: &FunctionLayout,
        minimum: &FunctionMinimumDemand,
    ) {
        for index in 0..body.instruction_counts().allocated {
            let Some(instruction) = dense_id::<InstId>(index) else {
                self.report(
                    "lower.plan.instruction-index",
                    format!("instruction table for {function:?} exceeds the Core ID domain"),
                    OriginId::UNKNOWN,
                );
                break;
            };
            let reachable = minimum
                .is_instruction_reachable(instruction)
                .unwrap_or(false);
            let required = minimum.requires_instruction(instruction).unwrap_or(false);
            let planned = slot_ref(&layout.instruction_plans, instruction);
            if reachable != planned.is_some() {
                self.report(
                    "lower.plan.instruction-coverage",
                    format!("instruction-plan coverage for {function:?} {instruction:?} is wrong"),
                    OriginId::UNKNOWN,
                );
            }
            let Some(planned) = planned else { continue };
            let origin = body
                .instruction(instruction)
                .map_or(OriginId::UNKNOWN, crate::ir::core::InstData::origin);
            if required && matches!(planned, InstructionPlan::OmittedPure) {
                self.report(
                    "lower.plan.omitted-required-instruction",
                    format!("required {function:?} {instruction:?} is omitted"),
                    origin,
                );
            }
            if self.plan.optimization_level == MinecraftOptimizationLevel::None
                && matches!(planned, InstructionPlan::OmittedPure)
            {
                self.report(
                    "lower.plan.none-omission",
                    format!("None policy omitted {function:?} {instruction:?}"),
                    origin,
                );
            }
            let Some(data) = body.instruction(instruction) else {
                self.report(
                    "lower.plan.invalid-instruction",
                    format!("planned {function:?} {instruction:?} is absent from Core"),
                    origin,
                );
                continue;
            };
            self.verify_instruction(function, instruction, data, layout, planned);
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the verifier exhaustively audits every closed physical instruction plan in one dispatch"
    )]
    fn verify_instruction(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &crate::ir::core::InstData,
        layout: &FunctionLayout,
        planned: &InstructionPlan,
    ) {
        match planned {
            InstructionPlan::OmittedPure => {
                let has_mapped_result = data
                    .results()
                    .iter()
                    .copied()
                    .any(|value| slot(&layout.value_homes, value).is_some());
                if !data.op().is_trivially_discardable() || has_mapped_result {
                    self.report(
                        "lower.plan.invalid-omission",
                        format!("{function:?} {instruction:?} cannot be omitted"),
                        data.origin(),
                    );
                }
            }
            InstructionPlan::External { helper } => {
                if let CoreOp::External(external) = data.op() {
                    if self.plan.preflight().selected_recipe(*external).is_some() {
                        self.report(
                            "lower.plan.typed-external-placement",
                            format!(
                                "typed Minecraft operation {function:?} {instruction:?} must use its direct selected recipe"
                            ),
                            data.origin(),
                        );
                    }
                }
                if !matches!(data.op(), CoreOp::External(_))
                    || !data.operands().is_empty()
                    || !data.results().is_empty()
                {
                    self.report(
                        "lower.plan.instruction-kind",
                        format!("{function:?} {instruction:?} has an invalid external plan shape"),
                        data.origin(),
                    );
                }
                if let Some(resource) = self.naming_options.as_ref().map(|options| {
                    options
                        .generated_names()
                        .external_helper_function(function, instruction)
                }) {
                    self.reference_function(
                        *helper,
                        PlannedFunctionRole::ExternalHelper {
                            function,
                            instruction,
                        },
                        data.origin(),
                        &resource,
                    );
                }
            }
            InstructionPlan::Minecraft { external, recipe } => {
                let CoreOp::External(actual) = data.op() else {
                    self.report(
                        "lower.plan.instruction-kind",
                        format!(
                            "{function:?} {instruction:?} has a Minecraft command plan for a non-external operation"
                        ),
                        data.origin(),
                    );
                    return;
                };
                if actual != external || !data.operands().is_empty() || !data.results().is_empty() {
                    self.report(
                        "lower.plan.minecraft-shape",
                        format!(
                            "{function:?} {instruction:?} has an invalid Minecraft command plan shape"
                        ),
                        data.origin(),
                    );
                }
                match self.plan.preflight().selected_recipe(*external) {
                    Some(selected) if selected.recipe_id() == *recipe => {}
                    selected => self.report(
                        "lower.plan.minecraft-recipe",
                        format!(
                            "{function:?} {instruction:?} retains recipe {recipe:?}, but preflight selected {selected:?}"
                        ),
                        data.origin(),
                    ),
                }
            }
            InstructionPlan::Scalar { operands, results } => {
                if matches!(data.op(), CoreOp::Call(_)) {
                    self.report(
                        "lower.plan.instruction-kind",
                        format!("call {function:?} {instruction:?} has a scalar plan"),
                        data.origin(),
                    );
                    return;
                }
                self.verify_operands(function, data, layout, operands);
                self.verify_scalar_results(function, instruction, data, layout, results);
            }
            InstructionPlan::Call {
                arguments,
                result_destinations,
            } => {
                if !matches!(data.op(), CoreOp::Call(_)) {
                    self.report(
                        "lower.plan.instruction-kind",
                        format!("scalar {function:?} {instruction:?} has a call plan"),
                        data.origin(),
                    );
                    return;
                }
                self.verify_operands(function, data, layout, arguments);
                self.verify_call_results(function, instruction, data, layout, result_destinations);
            }
        }
    }

    fn verify_operands(
        &mut self,
        function: FunctionId,
        data: &crate::ir::core::InstData,
        layout: &FunctionLayout,
        homes: &[HomeId],
    ) {
        if homes.len() != data.operands().len() {
            self.report(
                "lower.plan.instruction-operand-arity",
                format!("{} has the wrong physical operand arity", data.op().name()),
                data.origin(),
            );
        }
        for (operand, home) in data.operands().iter().zip(homes) {
            let expected = slot(&layout.value_homes, *operand);
            if expected != Some(*home) {
                self.report(
                    "lower.plan.instruction-operand",
                    format!("operand {operand:?} in {function:?} has the wrong home"),
                    data.origin(),
                );
            }
            self.reference_home(*home, data.origin());
        }
    }

    fn verify_scalar_results(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &crate::ir::core::InstData,
        layout: &FunctionLayout,
        results: &[ScalarResultPlacement],
    ) {
        let Some(expected_types) = verifier_scalar_result_types(data.op()) else {
            self.report(
                "lower.plan.scalar-contract",
                format!("{function:?} {instruction:?} has no verifier scalar contract"),
                data.origin(),
            );
            return;
        };
        if results.len() != data.results().len() || results.len() != expected_types.len() {
            self.report(
                "lower.plan.scalar-result-arity",
                format!("{function:?} {instruction:?} has the wrong physical result arity"),
                data.origin(),
            );
        }
        for (result_index, ((value, placement), expected_type)) in data
            .results()
            .iter()
            .copied()
            .zip(results.iter().copied())
            .zip(expected_types.iter().copied())
            .enumerate()
        {
            let actual_type = self
                .core
                .function(function)
                .and_then(crate::ir::core::Function::body)
                .and_then(|body| body.value(value))
                .map(crate::ir::core::ValueData::ty);
            if actual_type != Some(expected_type) || placement.result_index() != result_index {
                self.report(
                    "lower.plan.scalar-result-index",
                    format!("result {result_index} of {function:?} {instruction:?} is malformed"),
                    data.origin(),
                );
            }
            let mapped = slot(&layout.value_homes, value);
            match placement {
                ScalarResultPlacement::Semantic {
                    value: correlated,
                    home,
                    ..
                } => {
                    if correlated != value || mapped != Some(home) {
                        self.report(
                            "lower.plan.scalar-semantic-result",
                            format!("semantic result {result_index} has the wrong value or home"),
                            data.origin(),
                        );
                    }
                    self.reference_home(home, data.origin());
                }
                ScalarResultPlacement::RecipeTemporary { home, .. } => {
                    if mapped.is_some()
                        || self.plan.optimization_level != MinecraftOptimizationLevel::Baseline
                    {
                        self.report(
                            "lower.plan.scalar-recipe-result",
                            format!("recipe result {result_index} conflicts with semantic storage"),
                            data.origin(),
                        );
                    }
                    match self.plan.homes.get(home).map(|home| home.role) {
                        Some(HomeRole::RecipeTemporary {
                            function: owner,
                            ty,
                            ..
                        }) if owner == function && ty == expected_type => {}
                        _ => self.report(
                            "lower.plan.recipe-temporary-type",
                            format!(
                                "recipe result {result_index} has a foreign or wrong-type home"
                            ),
                            data.origin(),
                        ),
                    }
                    self.reference_home(home, data.origin());
                }
            }
        }
    }

    fn verify_call_results(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &crate::ir::core::InstData,
        layout: &FunctionLayout,
        destinations: &[Option<super::CallResultDestination>],
    ) {
        let CoreOp::Call(callee) = data.op() else {
            return;
        };
        let signature = self.core.function(*callee);
        if signature.is_none() {
            self.report(
                "lower.plan.invalid-callee",
                format!("{function:?} {instruction:?} calls absent {callee:?}"),
                data.origin(),
            );
        }
        let signature_results = signature.map_or(0, |callee| callee.results().len());
        if destinations.len() != data.results().len() || destinations.len() != signature_results {
            self.report(
                "lower.plan.call-result-arity",
                format!("{function:?} {instruction:?} has the wrong call result arity"),
                data.origin(),
            );
        }
        for (result_index, (value, destination)) in data
            .results()
            .iter()
            .copied()
            .zip(destinations.iter().copied())
            .enumerate()
        {
            let mapped = slot(&layout.value_homes, value);
            match destination {
                Some(destination) => {
                    if destination.result_index() != result_index
                        || destination.value() != value
                        || mapped != Some(destination.home())
                    {
                        self.report(
                            "lower.plan.call-result-index",
                            format!("call result {result_index} in {function:?} is malformed"),
                            data.origin(),
                        );
                    }
                    self.reference_home(destination.home(), data.origin());
                }
                None if mapped.is_some() => self.report(
                    "lower.plan.call-result-destination",
                    format!("mapped call result {result_index} has no destination"),
                    data.origin(),
                ),
                None => {}
            }
        }
    }

    fn verify_edge_temporary_declarations(
        &mut self,
        function: FunctionId,
        layout: &FunctionLayout,
    ) {
        let mut seen = HashSet::<HomeId>::new();
        if seen.try_reserve(layout.edge_temporaries.len()).is_err() {
            self.report(
                "lower.plan.verifier-capacity",
                format!("plan verifier cannot audit edge temporaries for {function:?}"),
                OriginId::UNKNOWN,
            );
            return;
        }
        for (ordinal, home) in layout.edge_temporaries.iter().copied().enumerate() {
            if !seen.insert(home) {
                self.report(
                    "lower.plan.duplicate-edge-temporary",
                    format!("{function:?} lists {home:?} more than once"),
                    OriginId::UNKNOWN,
                );
            }
            let valid = match (self.plan.optimization_level, self.plan.homes.get(home)) {
                (
                    MinecraftOptimizationLevel::None,
                    Some(super::Home {
                        role: HomeRole::EdgeTemporary { function: owner },
                        ..
                    }),
                ) => *owner == function && ordinal == 0,
                (
                    MinecraftOptimizationLevel::Baseline,
                    Some(super::Home {
                        role:
                            HomeRole::TypedEdgeTemporary {
                                function: owner,
                                ordinal: typed_ordinal,
                                ..
                            },
                        ..
                    }),
                ) => *owner == function && *typed_ordinal == 0,
                _ => false,
            };
            if !valid {
                self.report(
                    "lower.plan.edge-temporary-role",
                    format!("{function:?} edge temporary {home:?} has the wrong role"),
                    OriginId::UNKNOWN,
                );
            }
            self.reference_home(home, OriginId::UNKNOWN);
        }
        match self.plan.optimization_level {
            MinecraftOptimizationLevel::None => {
                let expected = layout.edge_temporaries.first().copied();
                if layout.edge_temporaries.len() > 1 || layout.parallel_copy_temp != expected {
                    self.report(
                        "lower.plan.legacy-temporary-correspondence",
                        format!("legacy edge temporary for {function:?} is inconsistent"),
                        OriginId::UNKNOWN,
                    );
                }
            }
            MinecraftOptimizationLevel::Baseline if layout.parallel_copy_temp.is_some() => {
                self.report(
                    "lower.plan.typed-temporary-correspondence",
                    format!("Baseline {function:?} retains a legacy edge temporary"),
                    OriginId::UNKNOWN,
                );
            }
            MinecraftOptimizationLevel::Baseline => {}
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one dense block scan keeps resource coverage, terminator shape, and edge verification together"
    )]
    fn verify_blocks(
        &mut self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        layout: &FunctionLayout,
        minimum: &FunctionMinimumDemand,
    ) {
        let incoming_counts = incoming_edge_occurrence_counts(body, minimum);
        let consumer_counts = recipe_consumer_counts(layout);
        for index in 0..body.block_counts().allocated {
            let Some(block) = dense_id::<BlockId>(index) else {
                self.report(
                    "lower.plan.block-index",
                    format!("block table for {function:?} exceeds the Core ID domain"),
                    OriginId::UNKNOWN,
                );
                break;
            };
            let reachable = minimum.is_block_reachable(block).unwrap_or(false);
            let placement = slot(&layout.block_placements, block);
            let mapped = slot(&layout.block_functions, block);
            if reachable != placement.is_some() {
                self.report(
                    "lower.plan.placement-coverage",
                    format!("placement coverage for {function:?} {block:?} is incorrect"),
                    OriginId::UNKNOWN,
                );
            }
            let block_data = body.block(block);
            let origin = block_data.map_or(OriginId::UNKNOWN, crate::ir::core::BlockData::origin);
            let consumers = consumer_counts.get(index).copied().unwrap_or_default();
            match placement {
                Some(BlockPlacement::Materialized) => {
                    if consumers != 0 {
                        self.report(
                            "lower.plan.materialized-consumer",
                            format!(
                                "materialized {function:?} {block:?} has {consumers} recipe consumers"
                            ),
                            origin,
                        );
                    }
                    if let Some(planned) = mapped {
                        if let Some(resource) = self.naming_options.as_ref().map(|options| {
                            options.generated_names().block_function(function, block)
                        }) {
                            self.reference_function(
                                planned,
                                PlannedFunctionRole::Block { function, block },
                                origin,
                                &resource,
                            );
                        }
                    } else {
                        self.report(
                            "lower.plan.materialized-resource",
                            format!("materialized {function:?} {block:?} has no resource"),
                            origin,
                        );
                    }
                }
                Some(BlockPlacement::Consumed {
                    source,
                    arm,
                    recipe,
                }) => {
                    if block == body.entry() {
                        self.report(
                            "lower.plan.consumed-entry",
                            format!("entry {function:?} {block:?} is consumed"),
                            origin,
                        );
                    }
                    if mapped.is_some() {
                        self.report(
                            "lower.plan.consumed-resource",
                            format!("consumed {function:?} {block:?} retains a resource"),
                            origin,
                        );
                    }
                    if consumers != 1 {
                        self.report(
                            "lower.plan.consumed-owner-count",
                            format!(
                                "consumed {function:?} {block:?} has {consumers} recipe consumers"
                            ),
                            origin,
                        );
                    }
                    let owner_matches = slot(&layout.branch_recipes, source)
                        .map(|branch| branch.arm(arm))
                        .is_some_and(|owner| {
                            owner.kind() == recipe
                                && owner
                                    .inline_zero_abi_terminal_call()
                                    .is_some_and(|inline| inline.consumed_block() == block)
                        });
                    if !owner_matches {
                        self.report(
                            "lower.plan.consumed-owner",
                            format!(
                                "consumed {function:?} {block:?} disagrees with owner {source:?} {arm:?}"
                            ),
                            origin,
                        );
                    }
                }
                None if mapped.is_some() => self.report(
                    "lower.plan.unplaced-resource",
                    format!("unplaced {function:?} {block:?} retains a resource"),
                    origin,
                ),
                None => {}
            }
            let transfer = slot_ref(&layout.edge_transfers, block);
            let branch_recipe = slot(&layout.branch_recipes, block);
            if !reachable {
                if transfer.is_some() {
                    self.report(
                        "lower.plan.detached-transfer",
                        format!("unreachable {function:?} {block:?} owns a transfer"),
                        origin,
                    );
                }
                if branch_recipe.is_some() {
                    self.report(
                        "lower.plan.detached-recipe",
                        format!("unreachable {function:?} {block:?} owns a branch recipe"),
                        origin,
                    );
                }
                continue;
            }
            let Some(block_data) = block_data else {
                self.report(
                    "lower.plan.invalid-block",
                    format!("reachable {function:?} {block:?} is absent from Core"),
                    origin,
                );
                continue;
            };
            let terminator = block_data.terminator();
            let terminator_origin = terminator.map_or(origin, crate::ir::core::Terminator::origin);
            match (terminator.map(crate::ir::core::Terminator::kind), transfer) {
                (Some(TerminatorKind::Jump(target)), Some(EdgeTransfer::Jump { steps })) => {
                    if branch_recipe.is_some() {
                        self.report(
                            "lower.plan.recipe-shape",
                            format!("jump {function:?} {block:?} owns a branch recipe"),
                            terminator_origin,
                        );
                    }
                    self.verify_transfer(
                        function,
                        body,
                        block,
                        target,
                        steps,
                        layout,
                        terminator_origin,
                    );
                }
                (
                    Some(TerminatorKind::Branch {
                        condition,
                        then_target,
                        else_target,
                    }),
                    Some(EdgeTransfer::Branch {
                        then_edge,
                        else_edge,
                    }),
                ) => {
                    let Some(branch_recipe) = branch_recipe else {
                        self.report(
                            "lower.plan.recipe-shape",
                            format!("branch {function:?} {block:?} has no recipe"),
                            terminator_origin,
                        );
                        continue;
                    };
                    self.verify_condition_home(function, *condition, layout, origin);
                    self.verify_branch_transfer(
                        function,
                        body,
                        block,
                        BranchArm::Then,
                        then_target,
                        then_edge,
                        layout,
                        terminator_origin,
                    );
                    self.verify_branch_transfer(
                        function,
                        body,
                        block,
                        BranchArm::Else,
                        else_target,
                        else_edge,
                        layout,
                        terminator_origin,
                    );
                    self.verify_branch_recipe(
                        function,
                        declaration,
                        body,
                        block,
                        BranchArm::Then,
                        then_target,
                        then_edge,
                        branch_recipe.arm(BranchArm::Then),
                        layout,
                        &incoming_counts,
                        terminator_origin,
                    );
                    self.verify_branch_recipe(
                        function,
                        declaration,
                        body,
                        block,
                        BranchArm::Else,
                        else_target,
                        else_edge,
                        branch_recipe.arm(BranchArm::Else),
                        layout,
                        &incoming_counts,
                        terminator_origin,
                    );
                }
                (Some(TerminatorKind::Return(values)), None) => {
                    if branch_recipe.is_some() {
                        self.report(
                            "lower.plan.recipe-shape",
                            format!("return {function:?} {block:?} owns a branch recipe"),
                            terminator_origin,
                        );
                    }
                    self.verify_return(
                        function,
                        declaration,
                        body,
                        values,
                        layout,
                        terminator_origin,
                    );
                }
                (Some(TerminatorKind::Unreachable), None) => self.report(
                    "lower.plan.reachable-unreachable",
                    format!("reachable {function:?} {block:?} is `unreachable`"),
                    terminator_origin,
                ),
                _ => self.report(
                    "lower.plan.transfer-shape",
                    format!("transfer shape for {function:?} {block:?} disagrees with Core"),
                    origin,
                ),
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "independent recipe verification keeps Core shape, physical transfer, placement, and predicted cost explicit"
    )]
    fn verify_branch_recipe(
        &mut self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        source: BlockId,
        arm: BranchArm,
        target: &BlockTarget,
        transfer: &BranchTransfer,
        recipe: BranchArmRecipe,
        layout: &FunctionLayout,
        incoming_counts: &[usize],
        branch_origin: OriginId,
    ) {
        let expected = self.expected_branch_arm_decision(
            function,
            declaration,
            body,
            arm,
            target,
            transfer,
            incoming_counts,
        );
        match recipe {
            BranchArmRecipe::Materialized { reason } => {
                if expected != ExpectedBranchArmDecision::Materialized(reason) {
                    self.report(
                        "lower.plan.recipe-decision",
                        format!(
                            "materialized branch {function:?} {source:?} {arm:?} records {reason:?}, independently expected {expected:?}"
                        ),
                        branch_origin,
                    );
                }
                if !matches!(
                    slot(&layout.block_placements, target.block()),
                    Some(BlockPlacement::Materialized)
                ) {
                    self.report(
                        "lower.plan.materialized-arm-target",
                        format!(
                            "materialized branch arm {function:?} {source:?} {arm:?} targets an unmaterialized block"
                        ),
                        branch_origin,
                    );
                }
            }
            BranchArmRecipe::InlineZeroAbiTerminalCall(recipe) => {
                if expected != ExpectedBranchArmDecision::InlineZeroAbiTerminalCall {
                    self.report(
                        "lower.plan.recipe-decision",
                        format!(
                            "selected branch {function:?} {source:?} {arm:?} independently expected {expected:?}"
                        ),
                        branch_origin,
                    );
                }
                self.verify_inline_zero_abi_terminal_call(
                    function,
                    declaration,
                    body,
                    source,
                    arm,
                    target,
                    transfer,
                    recipe,
                    layout,
                    incoming_counts,
                    branch_origin,
                );
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "independent selection rederivation keeps semantic shape, physical transfer, and cost inputs explicit"
    )]
    fn expected_branch_arm_decision(
        &self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        arm: BranchArm,
        target: &BlockTarget,
        transfer: &BranchTransfer,
        incoming_counts: &[usize],
    ) -> ExpectedBranchArmDecision {
        let retain = ExpectedBranchArmDecision::Materialized;
        if self.plan.optimization_level == MinecraftOptimizationLevel::None {
            return retain(RecipeDecisionReason::OptimizationDisabled);
        }
        if target.block() == body.entry() {
            return retain(RecipeDecisionReason::DestinationIsEntry);
        }
        let incoming = usize::try_from(target.block().index())
            .ok()
            .and_then(|index| incoming_counts.get(index))
            .copied()
            .unwrap_or_default();
        if incoming != 1 {
            return retain(RecipeDecisionReason::IncomingEdgeOccurrenceCount { actual: incoming });
        }
        if !transfer.steps().is_empty() || transfer.helper().is_some() {
            return retain(RecipeDecisionReason::EdgeTransferNotEmpty);
        }
        let Some(block) = body.block(target.block()) else {
            return retain(RecipeDecisionReason::InstructionIsNotCall);
        };
        if !block.parameters().is_empty() {
            return retain(RecipeDecisionReason::DestinationHasParameters);
        }
        if block.instructions().len() != 1 {
            return retain(RecipeDecisionReason::InstructionCountNotOne {
                actual: block.instructions().len(),
            });
        }
        let instruction = block.instructions()[0];
        let Some(call) = body.instruction(instruction) else {
            return retain(RecipeDecisionReason::InstructionIsNotCall);
        };
        let CoreOp::Call(callee) = call.op() else {
            return retain(RecipeDecisionReason::InstructionIsNotCall);
        };
        if !call.operands().is_empty() || !call.results().is_empty() {
            return retain(RecipeDecisionReason::CallHasSemanticArgumentsOrResults);
        }
        if !matches!(
            self.plan.instruction_plan(function, instruction),
            Some(InstructionPlan::Call {
                arguments,
                result_destinations,
            }) if arguments.is_empty() && result_destinations.is_empty()
        ) {
            return retain(RecipeDecisionReason::CallHasPhysicalArgumentsOrResults);
        }
        let Some(callee) = self.core.function(*callee) else {
            return retain(RecipeDecisionReason::NormalCompletionNotProven);
        };
        if !callee.parameters().is_empty() || !callee.results().is_empty() {
            return retain(RecipeDecisionReason::CalleeHasParametersOrResults);
        }
        if !declaration.results().is_empty() {
            return retain(RecipeDecisionReason::CallerHasResults);
        }
        if !matches!(
            block
                .terminator()
                .map(crate::ir::core::Terminator::kind),
            Some(TerminatorKind::Return(values)) if values.is_empty()
        ) {
            return retain(RecipeDecisionReason::TerminalReturnIsNotEmpty);
        }
        if callee.body().is_none() {
            return retain(RecipeDecisionReason::NormalCompletionNotProven);
        }

        let baseline = match ControlRecipeCost::return_dispatcher(arm) {
            Ok(cost) => cost,
            Err(error) => return retain(RecipeDecisionReason::AccountingUnavailable(error)),
        };
        let candidate = match ControlRecipeCost::inline_zero_abi_terminal_call(
            arm,
            WholeGraphImpact::unique_terminal_arm_contraction(),
        ) {
            Ok(cost) => cost,
            Err(error) => return retain(RecipeDecisionReason::AccountingUnavailable(error)),
        };
        match compare_recipe_costs(baseline, candidate) {
            Ok(RecipePreference::SelectCandidate(_)) => {
                ExpectedBranchArmDecision::InlineZeroAbiTerminalCall
            }
            Ok(RecipePreference::RetainBaseline(reason)) => {
                retain(RecipeDecisionReason::CostRetained(reason))
            }
            Err(error) => retain(RecipeDecisionReason::AccountingUnavailable(error)),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        reason = "one exhaustive verifier rederives the complete first closed recipe without trusting selector facts"
    )]
    fn verify_inline_zero_abi_terminal_call(
        &mut self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        source: BlockId,
        arm: BranchArm,
        target: &BlockTarget,
        transfer: &BranchTransfer,
        recipe: InlineZeroAbiTerminalCall,
        layout: &FunctionLayout,
        incoming_counts: &[usize],
        branch_origin: OriginId,
    ) {
        let consumed = recipe.consumed_block();
        if self.plan.optimization_level != MinecraftOptimizationLevel::Baseline
            || recipe.terminal_arm() != arm
            || consumed != target.block()
            || consumed == body.entry()
            || !target.arguments().is_empty()
            || !transfer.steps().is_empty()
            || transfer.helper().is_some()
            || slot(&layout.block_placements, consumed)
                != Some(BlockPlacement::Consumed {
                    source,
                    arm,
                    recipe: ControlRecipeKind::InlineZeroAbiTerminalCall,
                })
        {
            self.report(
                "lower.plan.inline-terminal-shape",
                format!(
                    "inline terminal recipe for {function:?} {source:?} {arm:?} has invalid placement or edge shape"
                ),
                branch_origin,
            );
        }
        let incoming = usize::try_from(consumed.index())
            .ok()
            .and_then(|index| incoming_counts.get(index))
            .copied()
            .unwrap_or_default();
        if incoming != 1 {
            self.report(
                "lower.plan.inline-terminal-incoming",
                format!("inline terminal {function:?} {consumed:?} has {incoming} incoming edges"),
                branch_origin,
            );
        }

        let Some(block) = body.block(consumed) else {
            self.report(
                "lower.plan.inline-terminal-block",
                format!("inline terminal {function:?} {consumed:?} is absent"),
                branch_origin,
            );
            return;
        };
        if !block.parameters().is_empty()
            || block.instructions() != [recipe.call_instruction()]
            || !matches!(
                block.terminator().map(crate::ir::core::Terminator::kind),
                Some(TerminatorKind::Return(values)) if values.is_empty()
            )
            || !declaration.results().is_empty()
        {
            self.report(
                "lower.plan.inline-terminal-body",
                format!(
                    "inline terminal {function:?} {consumed:?} is not one call plus empty return"
                ),
                block.origin(),
            );
            return;
        }
        let Some(call) = body.instruction(recipe.call_instruction()) else {
            self.report(
                "lower.plan.inline-terminal-call",
                format!(
                    "inline terminal call {:?} is absent",
                    recipe.call_instruction()
                ),
                block.origin(),
            );
            return;
        };
        let CoreOp::Call(callee) = call.op() else {
            self.report(
                "lower.plan.inline-terminal-call",
                format!(
                    "inline terminal instruction {:?} is not a call",
                    recipe.call_instruction()
                ),
                call.origin(),
            );
            return;
        };
        if *callee != recipe.callee()
            || !call.operands().is_empty()
            || !call.results().is_empty()
            || !matches!(
                self.plan.instruction_plan(function, recipe.call_instruction()),
                Some(InstructionPlan::Call { arguments, result_destinations })
                    if arguments.is_empty() && result_destinations.is_empty()
            )
        {
            self.report(
                "lower.plan.inline-terminal-call",
                format!(
                    "inline terminal call {:?} has invalid semantic or physical ABI",
                    recipe.call_instruction()
                ),
                call.origin(),
            );
        }
        let callee_valid = self.core.function(*callee).is_some_and(|declaration| {
            declaration.parameters().is_empty()
                && declaration.results().is_empty()
                && declaration.body().is_some_and(|callee_body| {
                    matches!(
                        self.plan.block_placement(*callee, callee_body.entry()),
                        Some(BlockPlacement::Materialized)
                    )
                })
        });
        if !callee_valid || !self.generated_exact_one_contract_valid {
            self.report(
                "lower.plan.inline-terminal-completion",
                format!("inline terminal callee {callee:?} lacks the generated exact-one completion contract"),
                call.origin(),
            );
        }
        let origins = recipe.origins();
        let return_origin = block
            .terminator()
            .map_or(OriginId::UNKNOWN, crate::ir::core::Terminator::origin);
        if origins.branch() != branch_origin
            || origins.call() != call.origin()
            || origins.terminal_return() != return_origin
        {
            self.report(
                "lower.plan.inline-terminal-origins",
                format!("inline terminal {function:?} {consumed:?} has stale origins"),
                branch_origin,
            );
        }

        let recounted_baseline = ControlRecipeCost::return_dispatcher(arm);
        let recounted_selected = ControlRecipeCost::inline_zero_abi_terminal_call(
            arm,
            WholeGraphImpact::unique_terminal_arm_contraction(),
        );
        let preference = recounted_baseline.and_then(|baseline| {
            recounted_selected.and_then(|selected| compare_recipe_costs(baseline, selected))
        });
        if recounted_baseline != recipe.baseline_cost()
            || recounted_selected != recipe.selected_cost()
            || preference != Ok(RecipePreference::SelectCandidate(recipe.advantage()))
        {
            self.report(
                "lower.plan.inline-terminal-cost",
                format!("inline terminal {function:?} {consumed:?} has stale predicted cost"),
                branch_origin,
            );
        }
    }

    fn verify_condition_home(
        &mut self,
        function: FunctionId,
        condition: ValueId,
        layout: &FunctionLayout,
        origin: OriginId,
    ) {
        let Some(home) = slot(&layout.value_homes, condition) else {
            self.report(
                "lower.plan.branch-condition-home",
                format!("branch condition {condition:?} in {function:?} has no home"),
                origin,
            );
            return;
        };
        if home_type(self.plan, home, function) != Some(CoreType::Bool) {
            self.report(
                "lower.plan.branch-condition-type",
                format!("branch condition {condition:?} in {function:?} is not Boolean"),
                origin,
            );
        }
        self.reference_home(home, origin);
    }

    fn verify_return(
        &mut self,
        function: FunctionId,
        declaration: &crate::ir::core::Function,
        body: &crate::ir::core::FunctionBody,
        values: &[ValueId],
        layout: &FunctionLayout,
        origin: OriginId,
    ) {
        if values.len() != declaration.results().len() {
            self.report(
                "lower.plan.return-arity",
                format!("return in {function:?} disagrees with its signature"),
                origin,
            );
        }
        for ((value, expected_type), result_home) in values
            .iter()
            .zip(declaration.results())
            .zip(layout.abi.results.iter())
        {
            let value_type = body.value(*value).map(crate::ir::core::ValueData::ty);
            let value_home = slot(&layout.value_homes, *value);
            if value_type != Some(*expected_type) || value_home.is_none() {
                self.report(
                    "lower.plan.return-value",
                    format!("return value {value:?} in {function:?} is not materialized correctly"),
                    origin,
                );
            }
            if let Some(value_home) = value_home {
                self.reference_home(value_home, origin);
            }
            self.reference_home(*result_home, origin);
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "branch verification keeps the Core edge, resource owner, and physical transfer explicit"
    )]
    fn verify_branch_transfer(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        source: BlockId,
        arm: BranchArm,
        target: &BlockTarget,
        transfer: &BranchTransfer,
        layout: &FunctionLayout,
        origin: OriginId,
    ) {
        self.verify_transfer(
            function,
            body,
            source,
            target,
            transfer.steps(),
            layout,
            origin,
        );
        match (transfer.steps().is_empty(), transfer.helper()) {
            (true, None) => {}
            (false, Some(helper)) => {
                if let Some(resource) = self.naming_options.as_ref().map(|options| {
                    options
                        .generated_names()
                        .branch_helper_function(function, source, arm)
                }) {
                    self.reference_function(
                        helper,
                        PlannedFunctionRole::BranchHelper {
                            function,
                            edge: super::BranchEdge::new(source, arm),
                        },
                        origin,
                        &resource,
                    );
                }
            }
            _ => self.report(
                "lower.plan.helper-correspondence",
                format!("branch helper for {function:?} {source:?} {arm:?} is incorrect"),
                origin,
            ),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the checker keeps the complete semantic edge and local physical namespace explicit"
    )]
    fn verify_transfer(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        source_block: BlockId,
        target: &BlockTarget,
        steps: &[super::MoveStep],
        layout: &FunctionLayout,
        origin: OriginId,
    ) {
        if target.block() == body.entry() {
            self.report(
                "lower.plan.edge-targets-entry",
                format!("edge from {source_block:?} targets the ABI entry"),
                origin,
            );
        }
        let Some(destination) = body.block(target.block()) else {
            self.report(
                "lower.plan.edge-destination",
                format!("edge from {source_block:?} targets an absent block"),
                origin,
            );
            return;
        };
        if destination.parameters().len() != target.arguments().len() {
            self.report(
                "lower.plan.edge-arity",
                format!("edge from {source_block:?} has invalid arity"),
                origin,
            );
            return;
        }
        let mut expected = Vec::new();
        for (parameter, source) in destination.parameters().iter().zip(target.arguments()) {
            let Some(destination_home) = slot(&layout.value_homes, parameter.value()) else {
                continue;
            };
            let Some(source_home) = slot(&layout.value_homes, *source) else {
                self.report(
                    "lower.plan.edge-source-home",
                    format!("retained edge copy from {source_block:?} has no source home"),
                    origin,
                );
                continue;
            };
            if destination_home != source_home {
                expected.push((destination_home, source_home));
            }
        }
        self.verify_transfer_tokens(function, &expected, steps, &layout.edge_temporaries, origin);
    }

    fn verify_transfer_tokens(
        &mut self,
        function: FunctionId,
        expected: &[(HomeId, HomeId)],
        steps: &[super::MoveStep],
        temporaries: &[HomeId],
        origin: OriginId,
    ) {
        self.token_epoch = self.token_epoch.wrapping_add(1);
        if self.token_epoch == 0 {
            self.token_epochs.fill(0);
            self.token_epoch = 1;
        }
        for step in steps {
            let destination_valid =
                self.valid_transfer_home(function, step.destination(), temporaries);
            let source_valid = self.valid_transfer_home(function, step.source(), temporaries);
            if !destination_valid || !source_valid {
                self.report(
                    "lower.plan.invalid-move-role",
                    format!("transfer in {function:?} uses a foreign or non-transfer home"),
                    origin,
                );
            }
            self.verify_move_types(function, *step, temporaries, origin);
            self.reference_transfer_home(step.destination(), temporaries, origin);
            self.reference_transfer_home(step.source(), temporaries, origin);
            let source = self.read_token(step.source(), temporaries);
            if source.is_none() {
                self.report(
                    "lower.plan.uninitialized-temporary",
                    format!("transfer in {function:?} reads an uninitialized temporary"),
                    origin,
                );
            }
            if !self.write_token(step.destination(), source) {
                self.report(
                    "lower.plan.invalid-move-home",
                    format!("transfer in {function:?} writes an invalid home"),
                    origin,
                );
            }
        }
        for (destination, source) in expected {
            if self.read_token(*destination, temporaries) != Some(*source) {
                self.report(
                    "lower.plan.incorrect-transfer",
                    format!("transfer in {function:?} does not implement simultaneous copies"),
                    origin,
                );
                break;
            }
            self.verify_assignment_types(function, *destination, *source, origin);
        }
    }

    fn valid_transfer_home(
        &self,
        function: FunctionId,
        home: HomeId,
        temporaries: &[HomeId],
    ) -> bool {
        match self.plan.homes.get(home).map(|data| data.role) {
            Some(
                HomeRole::Value {
                    function: owner, ..
                }
                | HomeRole::Register {
                    function: owner, ..
                },
            ) => owner == function,
            Some(
                HomeRole::EdgeTemporary { function: owner }
                | HomeRole::TypedEdgeTemporary {
                    function: owner, ..
                },
            ) => owner == function && temporaries.contains(&home),
            Some(HomeRole::Result { .. } | HomeRole::RecipeTemporary { .. }) | None => false,
        }
    }

    fn verify_move_types(
        &mut self,
        function: FunctionId,
        step: super::MoveStep,
        temporaries: &[HomeId],
        origin: OriginId,
    ) {
        let destination_type =
            transfer_home_type(self.plan, step.destination(), function, temporaries);
        let source_type = transfer_home_type(self.plan, step.source(), function, temporaries);
        if destination_type.is_some() && source_type.is_some() && destination_type != source_type {
            self.report(
                "lower.plan.move-type",
                format!("transfer step in {function:?} crosses physical types"),
                origin,
            );
        }
    }

    fn reference_transfer_home(&mut self, home: HomeId, temporaries: &[HomeId], origin: OriginId) {
        self.reference_home(home, origin);
        if temporaries.contains(&home) {
            if let Some(used) =
                entity_index(home).and_then(|index| self.edge_temporary_used.get_mut(index))
            {
                *used = true;
            }
        }
    }

    fn read_token(&self, home: HomeId, temporaries: &[HomeId]) -> Option<HomeId> {
        let index = entity_index(home)?;
        let epoch = *self.token_epochs.get(index)?;
        if epoch == self.token_epoch {
            self.token_values[index]
        } else if temporaries.contains(&home) {
            None
        } else {
            Some(home)
        }
    }

    fn write_token(&mut self, home: HomeId, token: Option<HomeId>) -> bool {
        let Some(index) = entity_index(home).filter(|index| *index < self.token_values.len())
        else {
            return false;
        };
        self.token_epochs[index] = self.token_epoch;
        self.token_values[index] = token;
        true
    }

    fn verify_assignment_types(
        &mut self,
        function: FunctionId,
        destination: HomeId,
        source: HomeId,
        origin: OriginId,
    ) {
        let destination_type = home_type(self.plan, destination, function);
        let source_type = home_type(self.plan, source, function);
        if destination_type.is_none() || destination_type != source_type {
            self.report(
                "lower.plan.edge-type",
                format!("edge assignment in {function:?} has incompatible homes"),
                origin,
            );
        }
    }

    fn expect_home_role(&mut self, home: HomeId, expected: HomeRole, origin: OriginId) {
        let Some(actual) = self.plan.homes.get(home).map(|home| home.role) else {
            self.report(
                "lower.plan.invalid-home",
                format!("plan references absent {home:?}"),
                origin,
            );
            return;
        };
        if actual != expected {
            self.report(
                "lower.plan.home-role",
                format!("{home:?} has role {actual:?}, expected {expected:?}"),
                origin,
            );
        }
        self.reference_home(home, origin);
    }

    fn reference_home(&mut self, home: HomeId, origin: OriginId) {
        let Some(index) = entity_index(home).filter(|index| *index < self.home_referenced.len())
        else {
            self.report(
                "lower.plan.invalid-home",
                format!("plan references absent {home:?}"),
                origin,
            );
            return;
        };
        self.home_referenced[index] = true;
    }

    fn reference_function(
        &mut self,
        planned: PlannedFunctionId,
        expected_role: PlannedFunctionRole,
        expected_origin: OriginId,
        expected_resource: &FunctionResourceId,
    ) {
        let Some(actual) = self.plan.target_functions.get(planned) else {
            self.report(
                "lower.plan.invalid-planned-function",
                format!("plan references absent {planned:?}"),
                expected_origin,
            );
            return;
        };
        if actual.role != expected_role {
            self.report(
                "lower.plan.function-role",
                format!(
                    "{planned:?} has role {:?}, expected {expected_role:?}",
                    actual.role
                ),
                expected_origin,
            );
        }
        if actual.origin != expected_origin {
            self.report(
                "lower.plan.function-origin",
                format!("{planned:?} has the wrong source origin"),
                expected_origin,
            );
        }
        if &actual.resource != expected_resource {
            self.report(
                "lower.plan.function-resource",
                format!("{planned:?} has the wrong generated resource"),
                expected_origin,
            );
        }
        increment(&mut self.function_owners, planned);
    }

    fn verify_final_ownership(&mut self) {
        for index in 0..self.plan.homes.len() {
            let Some(home) = dense_id::<HomeId>(index) else {
                break;
            };
            if !self.home_referenced.get(index).copied().unwrap_or(false) {
                self.report(
                    "lower.plan.orphan-home",
                    format!("{home:?} has no forward owner or physical use"),
                    OriginId::UNKNOWN,
                );
            }
            if matches!(
                self.plan.homes.get(home).map(|home| home.role),
                Some(HomeRole::EdgeTemporary { .. } | HomeRole::TypedEdgeTemporary { .. })
            ) && !self
                .edge_temporary_used
                .get(index)
                .copied()
                .unwrap_or(false)
            {
                self.report(
                    "lower.plan.unused-edge-temporary",
                    format!("{home:?} is declared but unused by every transfer"),
                    OriginId::UNKNOWN,
                );
            }
        }
        for index in 0..self.function_owners.len() {
            let owners = self.function_owners[index];
            let Some(planned) = dense_id::<PlannedFunctionId>(index) else {
                break;
            };
            if owners != 1 {
                self.report(
                    "lower.plan.function-ownership",
                    format!("{planned:?} has {owners} forward owners"),
                    OriginId::UNKNOWN,
                );
            }
        }
    }

    fn report(&mut self, code: &'static str, message: impl Into<String>, origin: OriginId) {
        if self.findings.len() < MAX_STRUCTURAL_FINDINGS {
            self.findings.push(Diagnostic::new(code, message, origin));
        } else if !self.findings_truncated {
            self.findings.push(Diagnostic::new(
                "lower.plan.too-many-findings",
                "additional structural plan findings were suppressed",
                OriginId::UNKNOWN,
            ));
            self.findings_truncated = true;
        }
    }

    fn finish(self) -> Result<(), Diagnostics> {
        match Diagnostics::from_findings(self.findings) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(()),
        }
    }
}

/// Rederives the fixed-emitter completion theorem without trusting recipe payloads.
///
/// Materialized block instructions never emit an early native return. A finite block
/// completion therefore ends either in explicit `return 1` for a Core return or in
/// `return run` propagation to another verified generated block/recipe target. Any
/// infinite propagation does not complete, so every normal finite completion has
/// success `1` and result `1`. Reachable `unreachable` blocks invalidate the theorem.
fn generated_exact_one_contract_is_structural(core: &CoreProgram, plan: &LoweringPlan) -> bool {
    core.functions().all(|(function, declaration)| {
        let Some(body) = declaration.body() else {
            return false;
        };
        let Some(layout) = plan.functions.get(function) else {
            return false;
        };
        if !matches!(
            slot(&layout.block_placements, body.entry()),
            Some(BlockPlacement::Materialized)
        ) {
            return false;
        }
        layout
            .block_placements
            .iter()
            .enumerate()
            .filter_map(|(index, placement)| placement.map(|placement| (index, placement)))
            .all(|(index, placement)| {
                if !matches!(placement, BlockPlacement::Materialized) {
                    return true;
                }
                let Ok(index) = u32::try_from(index) else {
                    return false;
                };
                body.block(BlockId::from_index(index))
                    .and_then(crate::ir::core::BlockData::terminator)
                    .is_some_and(|terminator| {
                        !matches!(terminator.kind(), TerminatorKind::Unreachable)
                    })
            })
    })
}

fn verifier_scalar_result_types(operation: &CoreOp) -> Option<&'static [CoreType]> {
    const BOOL: &[CoreType] = &[CoreType::Bool];
    const I32: &[CoreType] = &[CoreType::I32];
    const OVERFLOW: &[CoreType] = &[CoreType::I32, CoreType::Bool];
    match operation {
        CoreOp::BoolConstant(_) | CoreOp::I32Compare(_) | CoreOp::BoolNot => Some(BOOL),
        CoreOp::I32Constant(_) | CoreOp::I32AddWrapping => Some(I32),
        CoreOp::I32AddOverflowing => Some(OVERFLOW),
        CoreOp::Call(_) | CoreOp::External(_) => None,
    }
}

fn minimum_demand_diagnostics(error: MinimumDemandError) -> Diagnostics {
    Diagnostics::from_findings(vec![Diagnostic::new(
        "lower.plan.minimum-demand",
        format!("independent minimum-demand derivation failed: {error:?}"),
        OriginId::UNKNOWN,
    )])
    .expect("one minimum-demand finding always forms diagnostics")
}

fn incoming_edge_occurrence_counts(
    body: &crate::ir::core::FunctionBody,
    minimum: &FunctionMinimumDemand,
) -> Vec<usize> {
    let mut counts = vec![0_usize; body.block_counts().allocated];
    for source in body.block_order().iter().copied() {
        if !minimum.is_block_reachable(source).unwrap_or(false) {
            continue;
        }
        let Some(terminator) = body
            .block(source)
            .and_then(crate::ir::core::BlockData::terminator)
        else {
            continue;
        };
        terminator.kind().for_each_successor(|target| {
            if let Some(count) = usize::try_from(target.block().index())
                .ok()
                .and_then(|index| counts.get_mut(index))
            {
                *count = count
                    .checked_add(1)
                    .expect("an in-memory semantic edge inventory fits the host index domain");
            }
        });
    }
    counts
}

fn recipe_consumer_counts(layout: &FunctionLayout) -> Vec<usize> {
    let mut counts = vec![0_usize; layout.block_placements.len()];
    for recipe in layout.branch_recipes.iter().flatten().copied() {
        for arm in [BranchArm::Then, BranchArm::Else] {
            let Some(inline) = recipe.arm(arm).inline_zero_abi_terminal_call() else {
                continue;
            };
            if let Some(count) = usize::try_from(inline.consumed_block().index())
                .ok()
                .and_then(|index| counts.get_mut(index))
            {
                *count = count
                    .checked_add(1)
                    .expect("an in-memory recipe inventory fits the host index domain");
            }
        }
    }
    counts
}

fn slot<I: EntityId, T: Copy>(slots: &[Option<T>], id: I) -> Option<T> {
    slot_ref(slots, id).copied()
}

fn slot_ref<I: EntityId, T>(slots: &[Option<T>], id: I) -> Option<&T> {
    entity_index(id)
        .and_then(|index| slots.get(index))
        .and_then(Option::as_ref)
}

fn increment<I: EntityId>(counts: &mut [u32], id: I) {
    if let Some(count) = entity_index(id).and_then(|index| counts.get_mut(index)) {
        *count = count.saturating_add(1);
    }
}

fn entity_index(id: impl EntityId) -> Option<usize> {
    usize::try_from(id.index()).ok()
}

fn dense_id<I: EntityId>(index: usize) -> Option<I> {
    u32::try_from(index).ok().map(I::from_index)
}

fn value_origin(
    body: &crate::ir::core::FunctionBody,
    value: crate::ir::core::ValueData,
) -> OriginId {
    match value.definition() {
        crate::ir::core::ValueDef::BlockParam {
            block,
            parameter_index,
        } => body
            .block(block)
            .and_then(|block| {
                usize::try_from(parameter_index)
                    .ok()
                    .and_then(|index| block.parameters().get(index))
            })
            .map_or(OriginId::UNKNOWN, crate::ir::core::BlockParam::origin),
        crate::ir::core::ValueDef::InstResult { instruction, .. } => body
            .instruction(instruction)
            .map_or(OriginId::UNKNOWN, crate::ir::core::InstData::origin),
    }
}

fn home_type(plan: &LoweringPlan, home: HomeId, function: FunctionId) -> Option<CoreType> {
    match plan.homes.get(home)?.role {
        HomeRole::Value {
            function: owner,
            ty,
            ..
        }
        | HomeRole::Result {
            function: owner,
            ty,
            ..
        }
        | HomeRole::Register {
            function: owner,
            ty,
            ..
        }
        | HomeRole::RecipeTemporary {
            function: owner,
            ty,
            ..
        }
        | HomeRole::TypedEdgeTemporary {
            function: owner,
            ty,
            ..
        } if owner == function => Some(ty),
        HomeRole::EdgeTemporary { function: owner } if owner == function => None,
        _ => None,
    }
}

fn transfer_home_type(
    plan: &LoweringPlan,
    home: HomeId,
    function: FunctionId,
    temporaries: &[HomeId],
) -> Option<CoreType> {
    match plan.homes.get(home)?.role {
        HomeRole::EdgeTemporary { function: owner }
            if owner == function && temporaries.contains(&home) =>
        {
            None
        }
        HomeRole::Value { .. } | HomeRole::Register { .. } => home_type(plan, home, function),
        HomeRole::TypedEdgeTemporary { .. } if temporaries.contains(&home) => {
            home_type(plan, home, function)
        }
        HomeRole::Result { .. }
        | HomeRole::RecipeTemporary { .. }
        | HomeRole::EdgeTemporary { .. }
        | HomeRole::TypedEdgeTemporary { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::verify_plan;
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, EntityQueryDecl, ExternalSemanticBinding,
        FunctionBuilder, FunctionId, InstId, MinecraftOperationAttributes,
        MinecraftOperationOrigins, RunModifierInstance, TargetFragment, Terminator, TerminatorKind,
        ValueDef, ValueId,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::ir::semantic::{
        EntityKind, MessageLiteral, MinecraftSemanticKey, StaticEntityQuery,
    };
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::assignment::HomeAssignment;
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::lower::minecraft::edge_transfer::EdgeTransferPlan;
    use crate::lower::minecraft::placement::{
        BlockPlacement, BranchRecipe, ControlRecipePlan, ControlRecipeStatistics,
        RecipeDecisionReason,
    };
    use crate::lower::minecraft::plan::assemble::{
        assemble_candidate, assemble_selected_candidate,
    };
    use crate::lower::minecraft::plan::{
        EdgeTransfer, HomeRole, InstructionPlan, LoweringPlan, PlannedFunctionId,
        PlannedFunctionRole,
    };
    use crate::lower::minecraft::recipe::ControlRecipeKind;
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

    fn candidate(core: &CoreProgram, level: MinecraftOptimizationLevel) -> LoweringPlan {
        let options = options(level);
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
        let resources = ResourceInventory::new(core, &inventory, &transfers, &options).unwrap();
        let plan = assemble_candidate(core, &options, &assignment, &transfers, &resources).unwrap();
        verify_plan(core, &plan).unwrap();
        plan
    }

    fn selected_baseline_candidate(core: &CoreProgram) -> LoweringPlan {
        let level = MinecraftOptimizationLevel::Baseline;
        let options = options(level);
        let inventory = SemanticInventory::new(core).unwrap();
        let command_limit_evidence = crate::lower::minecraft::audit::audit_legality(
            core,
            &inventory,
            options.target(),
            options.command_limit_assumptions(),
        )
        .unwrap();
        let preflight = crate::lower::minecraft::TargetPreflight::new(
            core,
            &inventory,
            options.target(),
            command_limit_evidence,
        )
        .unwrap();
        let ambient = crate::ir::core::CoreAmbientAnalysis::analyze(core).unwrap();
        let demand =
            RuntimeDemand::for_level(core, &inventory, level, RuntimeDemandLimits::derived())
                .unwrap();
        let assignment =
            HomeAssignment::for_baseline_derived_liveness(core, &inventory, &demand).unwrap();
        let physical =
            crate::lower::minecraft::realization::PhysicalRealizationPlan::for_score_compatibility(
                core,
                &inventory,
                &assignment,
                None,
                crate::lower::minecraft::realization::PhysicalPlanningLimits::DEFAULT,
            )
            .unwrap();
        let physical_preflight =
            crate::lower::minecraft::physical_preflight::PhysicalPreflight::new(
                options.target(),
                &physical,
            )
            .unwrap();
        let transfers = EdgeTransferPlan::for_baseline(core, &inventory, &assignment).unwrap();
        let control =
            ControlRecipePlan::new(core, &inventory, &assignment, &transfers, level).unwrap();
        let resources = ResourceInventory::for_control_plan(
            core, &inventory, &transfers, &control, &preflight, &options,
        )
        .unwrap();
        let plan = assemble_selected_candidate(
            core,
            &options,
            preflight,
            ambient,
            physical,
            physical_preflight,
            &assignment,
            &transfers,
            &control,
            &resources,
        )
        .unwrap();
        verify_plan(core, &plan).unwrap();
        plan
    }

    fn assert_invalid(program: &CoreProgram, plan: &LoweringPlan, expected_code: &str) {
        let diagnostics = verify_plan(program, plan).expect_err("corrupted plan must be rejected");
        assert!(
            diagnostics.contains_code(expected_code),
            "expected {expected_code}, got {diagnostics}"
        );
    }

    #[test]
    fn rejects_a_corrupted_physical_preflight_inventory() {
        let (core, _, _) = returned_constant_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::None);
        plan.physical_preflight.corrupt_spill_bridges();
        assert_invalid(&core, &plan, "lower.plan.physical-preflight");
    }

    #[test]
    fn rejects_an_omitted_required_instruction() {
        let (core, function, value) = returned_constant_program();
        let instruction = defining_instruction(&core, function, value);
        let mut plan = candidate(&core, MinecraftOptimizationLevel::Baseline);
        let layout = plan.functions.get_mut(function).unwrap();
        layout.instruction_plans[index(instruction)] = Some(InstructionPlan::OmittedPure);

        assert_invalid(&core, &plan, "lower.plan.omitted-required-instruction");
    }

    #[test]
    fn external_plan_owns_one_dedicated_isolation_helper() {
        let (core, function, instruction) = external_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::None);
        let load = plan.load;
        let Some(InstructionPlan::External { helper }) = plan
            .functions
            .get(function)
            .and_then(|layout| layout.instruction_plans.get(index(instruction)))
            .and_then(Option::as_ref)
        else {
            panic!("external instruction must own an isolation helper")
        };
        let helper = *helper;
        let helper_data = plan.target_functions.get(helper).unwrap();
        assert_eq!(
            helper_data.role,
            PlannedFunctionRole::ExternalHelper {
                function,
                instruction
            }
        );
        assert!(helper_data.resource.to_string().ends_with("/f0/x0"));

        let Some(InstructionPlan::External { helper }) = plan
            .functions
            .get_mut(function)
            .and_then(|layout| layout.instruction_plans.get_mut(index(instruction)))
            .and_then(Option::as_mut)
        else {
            unreachable!()
        };
        *helper = load;
        assert_invalid(&core, &plan, "lower.plan.function-role");
    }

    #[test]
    fn rejects_corrupted_derived_command_fork_evidence() {
        let core = at_most_one_run_scope_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::None);
        let retained = plan.preflight.command_limit_evidence();
        assert_eq!(retained.minimum_max_command_forks(), 2);
        plan.preflight.replace_command_limit_evidence_for_test(
            crate::lower::minecraft::CommandLimitEvidence::new(
                retained.configured_assumptions(),
                retained.target_defaults(),
                0,
            ),
        );

        assert_invalid(&core, &plan, "lower.plan.preflight");
    }

    #[test]
    fn rejects_a_typed_recipe_corrupted_into_an_external_helper() {
        let (core, function, instruction) = typed_say_program();
        let mut plan = selected_baseline_candidate(&core);
        let helper = plan.load;
        plan.functions.get_mut(function).unwrap().instruction_plans[index(instruction)] =
            Some(InstructionPlan::External { helper });

        assert_invalid(&core, &plan, "lower.plan.typed-external-placement");
    }

    #[test]
    fn rejects_a_wrong_index_on_an_optional_call_destination() {
        let (core, caller, call) = later_call_result_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::Baseline);
        let Some(InstructionPlan::Call {
            result_destinations,
            ..
        }) = plan
            .functions
            .get_mut(caller)
            .unwrap()
            .instruction_plans
            .get_mut(index(call))
            .and_then(Option::as_mut)
        else {
            panic!("fixture must retain a call plan")
        };
        let destination = result_destinations[1]
            .as_mut()
            .expect("later demanded result must have a destination");
        destination.result_index = 0;

        assert_invalid(&core, &plan, "lower.plan.call-result-index");
    }

    #[test]
    fn rejects_a_value_assigned_to_a_same_typed_foreign_function_home() {
        let (core, consumer, call) = later_call_result_program();
        let identity = FunctionId::from_index(0);
        assert_ne!(identity, consumer);
        let identity_definition = core.function(identity).unwrap().body().unwrap();
        let identity_value = identity_definition
            .block(identity_definition.entry())
            .unwrap()
            .parameters()[1]
            .value();
        let consumer_definition = core.function(consumer).unwrap().body().unwrap();
        let consumer_value = consumer_definition.instruction(call).unwrap().results()[1];
        assert_eq!(
            identity_definition.value(identity_value).unwrap().ty(),
            consumer_definition.value(consumer_value).unwrap().ty(),
            "the corruption must isolate function ownership rather than physical type"
        );

        let mut plan = candidate(&core, MinecraftOptimizationLevel::Baseline);
        let foreign_home = plan
            .value_home(identity, identity_value)
            .expect("callee ABI parameter must have a home");
        plan.functions.get_mut(consumer).unwrap().value_homes[index(consumer_value)] =
            Some(foreign_home);

        assert_invalid(&core, &plan, "lower.plan.value-home");
    }

    #[test]
    fn rejects_a_recipe_temporary_with_the_wrong_physical_type() {
        let (core, function) = overflow_flag_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::Baseline);
        let (home, role) = plan
            .homes
            .iter()
            .find_map(|(home, data)| match data.role {
                HomeRole::RecipeTemporary {
                    function: owner,
                    ordinal,
                    ty: CoreType::I32,
                } if owner == function => Some((home, (owner, ordinal))),
                _ => None,
            })
            .expect("overflow recipe must own integer scratch");
        plan.homes.get_mut(home).unwrap().role = HomeRole::RecipeTemporary {
            function: role.0,
            ordinal: role.1,
            ty: CoreType::Bool,
        };

        assert_invalid(&core, &plan, "lower.plan.recipe-temporary-type");
    }

    #[test]
    fn rejects_a_missing_helper_for_a_nonempty_branch_transfer() {
        let (core, function, entry) = moved_branch_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::None);
        let Some(EdgeTransfer::Branch {
            then_edge,
            else_edge,
        }) = plan
            .functions
            .get_mut(function)
            .unwrap()
            .edge_transfers
            .get_mut(index(entry))
            .and_then(Option::as_mut)
        else {
            panic!("fixture must retain a branch transfer")
        };
        let moved = if then_edge.steps.is_empty() {
            else_edge
        } else {
            then_edge
        };
        assert!(!moved.steps.is_empty());
        assert!(moved.helper.take().is_some());

        assert_invalid(&core, &plan, "lower.plan.helper-correspondence");
    }

    #[test]
    fn rejects_a_corrupted_baseline_recipe_rejection_reason() {
        let (core, function, entry) = moved_branch_program();
        let mut plan = selected_baseline_candidate(&core);
        plan.functions.get_mut(function).unwrap().branch_recipes[index(entry)] = Some(
            BranchRecipe::all_materialized(RecipeDecisionReason::OptimizationDisabled),
        );

        assert_invalid(&core, &plan, "lower.plan.recipe-decision");
    }

    #[test]
    fn rejects_corrupted_control_recipe_statistics() {
        let (core, _, _) = moved_branch_program();
        let mut plan = selected_baseline_candidate(&core);
        plan.control_statistics = ControlRecipeStatistics::default();

        assert_invalid(&core, &plan, "lower.plan.control-statistics");
    }

    #[test]
    fn rejects_a_corrupted_selected_recipe_owner() {
        let (core, function, source, consumed) = selected_terminal_call_program();
        let mut plan = selected_baseline_candidate(&core);
        plan.functions.get_mut(function).unwrap().block_placements[index(consumed)] =
            Some(BlockPlacement::Consumed {
                source,
                arm: super::BranchArm::Else,
                recipe: ControlRecipeKind::InlineZeroAbiTerminalCall,
            });

        assert_invalid(&core, &plan, "lower.plan.inline-terminal-shape");
    }

    #[test]
    fn rejects_a_dangling_scaffolding_resource_reference() {
        let (core, _, _) = returned_constant_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::None);
        plan.load = PlannedFunctionId::from_index(u32::MAX);

        assert_invalid(&core, &plan, "lower.plan.invalid-planned-function");
    }

    #[test]
    fn accepts_a_conservative_reachable_value_with_nonoverlapping_register_reuse() {
        let (core, function, dead_value, live_value) = dead_then_live_program();
        let dead_instruction = defining_instruction(&core, function, dead_value);
        let mut plan = candidate(&core, MinecraftOptimizationLevel::Baseline);
        let shared_home = plan
            .value_home(function, live_value)
            .expect("returned value must have a register");
        let layout = plan.functions.get_mut(function).unwrap();
        assert!(layout.value_homes[index(dead_value)].is_none());
        layout.value_homes[index(dead_value)] = Some(shared_home);
        layout.instruction_plans[index(dead_instruction)] = Some(InstructionPlan::Scalar {
            operands: Box::new([]),
            results: Box::new([super::ScalarResultPlacement::Semantic {
                result_index: 0,
                value: dead_value,
                home: shared_home,
            }]),
        });

        verify_plan(&core, &plan).expect(
            "a reachable conservative superset may reuse a register after its old value dies",
        );
    }

    #[test]
    fn rejects_a_corrupted_cycle_schedule_in_the_structural_token_checker() {
        let (core, function, loop_block) = swap_loop_program();
        let mut plan = candidate(&core, MinecraftOptimizationLevel::None);
        let Some(EdgeTransfer::Jump { steps }) = plan
            .functions
            .get_mut(function)
            .unwrap()
            .edge_transfers
            .get_mut(index(loop_block))
            .and_then(Option::as_mut)
        else {
            panic!("fixture backedge must retain a jump transfer")
        };
        assert_eq!(steps.len(), 3);
        steps.swap(0, 1);

        assert_invalid(&core, &plan, "lower.plan.incorrect-transfer");
    }

    fn returned_constant_program() -> (CoreProgram, FunctionId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("returned_constant"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let value = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, value)
    }

    fn overflow_flag_program() -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("overflow_flag"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (_, overflowed) = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![overflowed]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    fn dead_then_live_program() -> (CoreProgram, FunctionId, ValueId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("dead_then_live"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let dead = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let live = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![live]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, dead, live)
    }

    fn swap_loop_program() -> (CoreProgram, FunctionId, BlockId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("swap_loop"),
                vec![CoreType::I32, CoreType::I32],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry_values = block_values(&builder, builder.entry_block());
        let loop_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first = builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let second = builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
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
                TerminatorKind::Jump(BlockTarget::new(loop_block, vec![second, first])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, loop_block)
    }

    fn later_call_result_program() -> (CoreProgram, FunctionId, InstId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let identity = core
            .declare_function(
                Some("callee"),
                vec![CoreType::I32, CoreType::Bool],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let consumer = core
            .declare_function(
                Some("caller"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut identity_builder = FunctionBuilder::new(&core, &sources, identity).unwrap();
        let parameters = block_values(&identity_builder, identity_builder.entry_block());
        identity_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(parameters),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(identity, identity_builder.finish().unwrap())
            .unwrap();

        let mut consumer_builder = FunctionBuilder::new(&core, &sources, consumer).unwrap();
        let integer = consumer_builder.i32_constant(3, OriginId::UNKNOWN).unwrap();
        let boolean = consumer_builder
            .bool_constant(true, OriginId::UNKNOWN)
            .unwrap();
        let results = consumer_builder
            .call(identity, vec![integer, boolean], OriginId::UNKNOWN)
            .unwrap();
        let call = defining_instruction_in_body(consumer_builder.body(), results[0]);
        consumer_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![results[1]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(consumer, consumer_builder.finish().unwrap())
            .unwrap();
        (core, consumer, call)
    }

    fn moved_branch_program() -> (CoreProgram, FunctionId, BlockId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("moved_branch"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let moved = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .append_block_parameter(moved, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let direct = builder.create_block(OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let value = builder.i32_constant(9, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(moved, vec![value]),
                    else_target: BlockTarget::new(direct, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for block in [moved, direct] {
            builder.switch_to_block(block).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, entry)
    }

    fn selected_terminal_call_program() -> (CoreProgram, FunctionId, BlockId, BlockId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let callee = core
            .declare_function(Some("leaf"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let function = core
            .declare_function(
                Some("dispatcher"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut callee_builder = FunctionBuilder::new(&core, &sources, callee).unwrap();
        let condition = callee_builder
            .bool_constant(true, OriginId::UNKNOWN)
            .unwrap();
        let callee_then = callee_builder.create_block(OriginId::UNKNOWN).unwrap();
        let callee_else = callee_builder.create_block(OriginId::UNKNOWN).unwrap();
        callee_builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(callee_then, vec![]),
                    else_target: BlockTarget::new(callee_else, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for block in [callee_then, callee_else] {
            callee_builder.switch_to_block(block).unwrap();
            callee_builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        core.define_function(callee, callee_builder.finish().unwrap())
            .unwrap();

        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let source = builder.entry_block();
        let condition = block_values(&builder, source)[0];
        let consumed = builder.create_block(OriginId::UNKNOWN).unwrap();
        let other = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(consumed, vec![]),
                    else_target: BlockTarget::new(other, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(consumed).unwrap();
        builder.call(callee, vec![], OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(other).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, source, consumed)
    }

    fn defining_instruction(core: &CoreProgram, function: FunctionId, value: ValueId) -> InstId {
        defining_instruction_in_body(core.function(function).unwrap().body().unwrap(), value)
    }

    fn external_program() -> (CoreProgram, FunctionId, InstId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let fragment = core
            .declare_target_fragment(TargetFragment::unsafe_minecraft_command("return 1").unwrap())
            .unwrap();
        let external = core
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = core
            .declare_function(Some("raw"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder
            .external(external, vec![], OriginId::UNKNOWN)
            .unwrap();
        let instruction = builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .instructions()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, instruction)
    }

    fn typed_say_program() -> (CoreProgram, FunctionId, InstId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let operation = core
            .declare_minecraft_operation(
                MinecraftSemanticKey::Say,
                EntityKind::ArmorStand,
                MinecraftOperationAttributes::Say {
                    message: MessageLiteral::new("typed placement").unwrap(),
                    message_origin: OriginId::UNKNOWN,
                },
                MinecraftOperationOrigins::new(
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                ),
            )
            .unwrap();
        let external = core
            .declare_external_op(
                ExternalSemanticBinding::MinecraftOperation(operation),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = core
            .declare_function(Some("typed"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder
            .external(external, vec![], OriginId::UNKNOWN)
            .unwrap();
        let instruction = builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .instructions()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, instruction)
    }

    fn at_most_one_run_scope_program() -> CoreProgram {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let body = core
            .declare_function(Some("body"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut body_builder = FunctionBuilder::new(&core, &sources, body).unwrap();
        body_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(body, body_builder.finish().unwrap())
            .unwrap();

        let query = core
            .declare_entity_query(EntityQueryDecl::from_semantic(
                StaticEntityQuery::entities(EntityKind::ArmorStand)
                    .limit(1)
                    .unwrap(),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let scope = core
            .declare_run_scope(
                vec![RunModifierInstance::AsEntityQuery {
                    query,
                    origin: OriginId::UNKNOWN,
                }],
                body,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let external = core
            .declare_external_op(
                ExternalSemanticBinding::MinecraftRunScope(scope),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let entry = core
            .declare_function(Some("entry"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut entry_builder = FunctionBuilder::new(&core, &sources, entry).unwrap();
        entry_builder
            .external(external, vec![], OriginId::UNKNOWN)
            .unwrap();
        entry_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(entry, entry_builder.finish().unwrap())
            .unwrap();
        core
    }

    fn defining_instruction_in_body(
        body: &crate::ir::core::FunctionBody,
        value: ValueId,
    ) -> InstId {
        let ValueDef::InstResult { instruction, .. } = body.value(value).unwrap().definition()
        else {
            panic!("fixture value must be an instruction result")
        };
        instruction
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

    fn index(id: impl EntityId) -> usize {
        usize::try_from(id.index()).unwrap()
    }
}
