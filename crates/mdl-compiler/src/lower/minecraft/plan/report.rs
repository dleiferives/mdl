use std::fmt::Write;

use crate::analysis::minecraft::classify_constructed_command;
use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, CoreFunctionLinkage, CoreProgram, CoreType, FunctionId, InstId,
    MinecraftOperationOrigins, ValueId,
};
use crate::ir::minecraft::{
    FakeScoreHolder, FunctionResourceId, ObjectiveName, PackNamespace, StoragePath,
};
use crate::ir::semantic::{AmbientContextRequirements, minecraft_descriptor};
use crate::source::OriginId;
use crate::target::JavaEditionTarget;

use crate::lower::minecraft::emit::ConstructionMap;
use crate::lower::minecraft::{
    ExecutionContract, LoweredCommand, LoweredFunction, LoweringMap, MinecraftOptimizationLevel,
    RegisterSlot,
};

use super::{
    BranchArm, EdgeTransfer, FunctionLayout, HomeId, HomeRole, InstructionPlan, LoweringPlan,
    PlannedFunctionId, PlannedFunctionRole, ScalarResultPlacement,
};
use crate::lower::minecraft::placement::{
    BlockPlacement, BranchArmRecipe, BranchRecipe, GeneratedCompletionContract,
};

/// Frozen, complete lowering decisions retained after the mutable planner is dropped.
///
/// This deliberately owns typed records rather than a pre-rendered dump. Failures can
/// therefore retain an immutable audit record without keeping the planner alive, while
/// diagnostics and exact-output tests render the same decisions on demand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweringDecisionReport {
    optimization_level: MinecraftOptimizationLevel,
    target: JavaEditionTarget,
    namespace: PackNamespace,
    register_objective: ObjectiveName,
    init_sentinel: StoragePath,
    load: PlannedFunctionId,
    init_try_create: PlannedFunctionId,
    execution: ExecutionContract,
    physical: crate::lower::minecraft::realization::PhysicalRealizationPlan,
    homes: Box<[HomeReport]>,
    target_functions: Box<[TargetFunctionReport]>,
    functions: Box<[FunctionReport]>,
    statistics: LoweringDecisionStatistics,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HomeReport {
    home: HomeId,
    holder: FakeScoreHolder,
    role: HomeRole,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TargetFunctionReport {
    function: PlannedFunctionId,
    resource: FunctionResourceId,
    origin: OriginId,
    role: PlannedFunctionRole,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FunctionReport {
    function: FunctionId,
    linkage: CoreFunctionLinkage,
    diagnostic_name_hint: Option<Box<str>>,
    generated_entry_requirement: AmbientContextRequirements,
    parameters: Box<[(CoreType, HomeId, FakeScoreHolder)]>,
    results: Box<[(CoreType, HomeId, FakeScoreHolder)]>,
    entry_block: BlockId,
    entry_resource: FunctionResourceId,
    blocks: Box<[(BlockId, PlannedFunctionId, FunctionResourceId)]>,
    placements: Box<[(BlockId, BlockPlacement)]>,
    branch_recipes: Box<[(BlockId, BranchRecipe)]>,
    values: Box<[(ValueId, HomeId)]>,
    temporary: Option<(HomeId, FakeScoreHolder)>,
    edge_temporaries: Box<[EdgeTemporaryReport]>,
    instructions: Box<[InstructionReport]>,
    edges: Box<[(BlockId, EdgeTransfer)]>,
    coalescing: Option<super::FunctionCoalescingPlan>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LoweringDecisionStatistics {
    homes: usize,
    physical_storages: usize,
    realizations: usize,
    use_requirements: usize,
    materializations: usize,
    call_occurrences: usize,
    recursive_call_occurrences: usize,
    recursive_spill_bridges: usize,
    physical_recipe_sequence_operations: usize,
    physical_recipe_forks: usize,
    target_functions: usize,
    core_functions: usize,
    branch_arms: u64,
    selected_recipes: u64,
    consumed_blocks: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EdgeTemporaryReport {
    home: HomeId,
    holder: FakeScoreHolder,
    ty: Option<CoreType>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct InstructionReport {
    instruction: InstId,
    plan: InstructionPlan,
    selected_recipe: Option<crate::lower::minecraft::SelectedSemanticRecipe>,
    semantic_provenance: Option<SemanticProvenanceReport>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SemanticProvenanceReport {
    instruction_origin: OriginId,
    operation_origins: MinecraftOperationOrigins,
    message_origin: OriginId,
}

impl LoweringDecisionReport {
    pub(crate) fn from_plan(core: &CoreProgram, plan: &LoweringPlan) -> Self {
        let homes = plan
            .homes
            .iter()
            .map(|(home, data)| HomeReport {
                home,
                holder: data.holder.clone(),
                role: data.role,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let target_functions = plan
            .target_functions
            .iter()
            .map(|(function, data)| TargetFunctionReport {
                function,
                resource: data.resource.clone(),
                origin: data.origin,
                role: data.role,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let functions = plan
            .functions
            .iter()
            .map(|(function, layout)| FunctionReport::from_layout(core, plan, function, layout))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let statistics = plan.control_statistics;
        let physical_preflight = plan.physical_preflight();
        let statistics = LoweringDecisionStatistics {
            homes: homes.len(),
            physical_storages: plan.physical.storage_count(),
            realizations: plan.physical.realization_count(),
            use_requirements: plan.physical.requirement_count(),
            materializations: plan.physical.materialization_count(),
            call_occurrences: plan.physical.call_count(),
            recursive_call_occurrences: physical_preflight.recursive_edges(),
            recursive_spill_bridges: physical_preflight.spill_bridges(),
            physical_recipe_sequence_operations: physical_preflight.local_sequence_operations(),
            physical_recipe_forks: physical_preflight.local_forks(),
            target_functions: target_functions.len(),
            core_functions: functions.len(),
            branch_arms: statistics.branch_arms_visited(),
            selected_recipes: statistics.candidates_selected(),
            consumed_blocks: statistics.blocks_consumed(),
        };
        Self {
            optimization_level: plan.optimization_level,
            target: plan.target,
            namespace: plan.namespace.clone(),
            register_objective: plan.pack_abi.register_objective.clone(),
            init_sentinel: plan.pack_abi.init_sentinel.clone(),
            load: plan.load,
            init_try_create: plan.init_try_create,
            execution: ExecutionContract::new(
                plan.preflight().command_limit_evidence(),
                plan.has_recursive_activation(),
            ),
            physical: plan.physical.clone(),
            homes,
            target_functions,
            functions,
            statistics,
        }
    }

    /// Returns the Minecraft physical-planning policy that produced these decisions.
    #[must_use]
    pub const fn optimization_level(&self) -> MinecraftOptimizationLevel {
        self.optimization_level
    }

    /// Returns the target version used to validate and construct the plan.
    #[must_use]
    pub const fn target(&self) -> JavaEditionTarget {
        self.target
    }

    /// Returns the compiler-owned datapack namespace.
    #[must_use]
    pub const fn namespace(&self) -> &PackNamespace {
        &self.namespace
    }

    /// Returns the compiler-owned scoreboard objective.
    #[must_use]
    pub const fn register_objective(&self) -> &ObjectiveName {
        &self.register_objective
    }

    /// Returns the runtime restrictions attached to the frozen plan.
    #[must_use]
    pub const fn execution_contract(&self) -> &ExecutionContract {
        &self.execution
    }

    /// Returns compact aggregate counts without exposing private plan identities.
    #[must_use]
    pub const fn statistics(&self) -> LoweringDecisionStatistics {
        self.statistics
    }

    /// Renders the complete lowering decisions in deterministic entity order.
    #[must_use]
    pub fn dump(&self) -> String {
        let mut output = String::new();
        writeln!(
            output,
            "lowering-report {:?} {} {} homes={} target-functions={}",
            self.target,
            self.namespace,
            self.register_objective,
            self.homes.len(),
            self.target_functions.len()
        )
        .unwrap();
        dump_optimization_level(&mut output, self.optimization_level);
        if self.optimization_level != MinecraftOptimizationLevel::None {
            writeln!(
                output,
                "control branch-arms={} selected={} consumed={}",
                self.statistics.branch_arms,
                self.statistics.selected_recipes,
                self.statistics.consumed_blocks
            )
            .unwrap();
        }
        writeln!(
            output,
            "execution activation={:?} depth={:?} command-limits={:?}",
            self.execution.activation(),
            self.execution.activation_depth(),
            self.execution.command_limits()
        )
        .unwrap();
        writeln!(
            output,
            "pack-abi load={} init-try-create={} sentinel={:?}",
            self.load.index(),
            self.init_try_create.index(),
            self.init_sentinel
        )
        .unwrap();
        output.push_str(&self.physical.dump());
        writeln!(
            output,
            "physical-preflight local-sequence-operations={} local-forks={}",
            self.statistics.physical_recipe_sequence_operations,
            self.statistics.physical_recipe_forks
        )
        .unwrap();
        for report in &self.homes {
            writeln!(
                output,
                "home {} {} {:?}",
                report.home.index(),
                report.holder,
                report.role
            )
            .unwrap();
        }
        for report in &self.target_functions {
            writeln!(
                output,
                "target-function {} {} {:?} {:?}",
                report.function.index(),
                report.resource,
                report.role,
                report.origin
            )
            .unwrap();
        }
        for report in &self.functions {
            dump_function_report(&mut output, report, self.optimization_level);
        }
        output
    }

    pub(crate) fn map(&self, construction: &ConstructionMap) -> LoweringMap {
        let functions = self
            .functions
            .iter()
            .map(|report| {
                let slots = |entries: &[(CoreType, HomeId, FakeScoreHolder)]| {
                    entries
                        .iter()
                        .map(|(ty, _, holder)| {
                            (
                                *ty,
                                RegisterSlot::new(holder.clone(), self.register_objective.clone()),
                            )
                        })
                        .collect()
                };
                LoweredFunction::new(
                    report.linkage,
                    report.entry_resource.clone(),
                    report.generated_entry_requirement,
                    slots(&report.parameters),
                    slots(&report.results),
                )
            })
            .collect();
        let semantic_commands = self
            .functions
            .iter()
            .map(|report| {
                construction
                    .function_slots(report.function)
                    .expect("verified construction map has every reported Core function")
                    .iter()
                    .map(|location| {
                        location.map(|location| {
                            LoweredCommand::new(location.function(), location.command())
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            })
            .collect();
        let run_modifiers = construction
            .run_scopes()
            .map(|(_, slots)| {
                slots
                    .iter()
                    .map(|slot| {
                        slot.map(|location| {
                            crate::lower::minecraft::LoweredRunModifier::new(
                                location.function,
                                location.command,
                                location.modifier_index,
                                location.recipe,
                            )
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice()
            })
            .collect();
        LoweringMap::new(self.execution, functions, semantic_commands, run_modifiers)
    }
}

impl LoweringDecisionStatistics {
    /// Returns the number of physical score homes retained by the plan.
    #[must_use]
    pub const fn homes(self) -> usize {
        self.homes
    }

    /// Returns the number of logical physical storage declarations.
    #[must_use]
    pub const fn physical_storages(self) -> usize {
        self.physical_storages
    }

    /// Returns the number of semantic value realization occurrences.
    #[must_use]
    pub const fn realizations(self) -> usize {
        self.realizations
    }

    /// Returns the number of exact physical use requirements.
    #[must_use]
    pub const fn use_requirements(self) -> usize {
        self.use_requirements
    }

    /// Returns the number of explicit materialization occurrences.
    #[must_use]
    pub const fn materializations(self) -> usize {
        self.materializations
    }

    /// Returns the number of retained physical call occurrences.
    #[must_use]
    pub const fn call_occurrences(self) -> usize {
        self.call_occurrences
    }

    /// Returns the number of call occurrences using recursive stack activation.
    #[must_use]
    pub const fn recursive_call_occurrences(self) -> usize {
        self.recursive_call_occurrences
    }

    /// Returns the number of planned score/frame spill pairs across recursive calls.
    #[must_use]
    pub const fn recursive_spill_bridges(self) -> usize {
        self.recursive_spill_bridges
    }

    /// Returns exact local sequence operations selected by physical-only recipes.
    #[must_use]
    pub const fn physical_recipe_sequence_operations(self) -> usize {
        self.physical_recipe_sequence_operations
    }

    /// Returns local command forks selected by physical-only recipes.
    #[must_use]
    pub const fn physical_recipe_forks(self) -> usize {
        self.physical_recipe_forks
    }

    /// Returns the number of generated target functions owned by the plan.
    #[must_use]
    pub const fn target_functions(self) -> usize {
        self.target_functions
    }

    /// Returns the number of Core functions represented in the plan.
    #[must_use]
    pub const fn core_functions(self) -> usize {
        self.core_functions
    }

    /// Returns the number of reachable branch arms considered by the selector.
    #[must_use]
    pub const fn branch_arms(self) -> u64 {
        self.branch_arms
    }

    /// Returns the number of control recipes selected over the reference form.
    #[must_use]
    pub const fn selected_recipes(self) -> u64 {
        self.selected_recipes
    }

    /// Returns the number of Core blocks consumed by selected recipes.
    #[must_use]
    pub const fn consumed_blocks(self) -> u64 {
        self.consumed_blocks
    }
}

fn dump_function_report(
    output: &mut String,
    report: &FunctionReport,
    optimization_level: MinecraftOptimizationLevel,
) {
    writeln!(
        output,
        "function {} name={:?} linkage={} params={:?} results={:?}",
        report.function.index(),
        report.diagnostic_name_hint,
        report.linkage,
        report.parameters,
        report.results
    )
    .unwrap();
    writeln!(
        output,
        "  generated-entry-requirement {:?}",
        report.generated_entry_requirement
    )
    .unwrap();
    writeln!(
        output,
        "  abi entry={} resource={} temp={:?}",
        report.entry_block.index(),
        report.entry_resource,
        report.temporary
    )
    .unwrap();
    if let Some(coalescing) = report.coalescing {
        let fallback = coalescing
            .fallback_reason
            .map_or("none", |reason| reason.code());
        writeln!(
            output,
            "  coalescing tracked-values={} liveness-events={} segments={} candidates={} merges={} work={} fallback={}",
            coalescing.liveness_tracked_values,
            coalescing.liveness_events,
            coalescing.liveness_segments,
            coalescing.candidates_considered,
            coalescing.merges_accepted,
            coalescing.work_used,
            fallback
        )
        .unwrap();
    }
    for (block, planned, resource) in &report.blocks {
        writeln!(
            output,
            "  block {} -> {} {resource}",
            block.index(),
            planned.index()
        )
        .unwrap();
    }
    for (value, home) in &report.values {
        writeln!(output, "  value {} -> {}", value.index(), home.index()).unwrap();
    }
    if optimization_level == MinecraftOptimizationLevel::None {
        for instruction in &report.instructions {
            if matches!(instruction.plan, InstructionPlan::Minecraft { .. }) {
                dump_instruction(output, instruction);
            }
        }
    } else {
        for (block, placement) in &report.placements {
            writeln!(output, "  placement block={} {placement:?}", block.index()).unwrap();
        }
        for (block, recipe) in &report.branch_recipes {
            for arm in [BranchArm::Then, BranchArm::Else] {
                dump_branch_recipe(output, *block, arm, recipe.arm(arm));
            }
        }
        for temporary in &report.edge_temporaries {
            dump_edge_temporary(output, temporary);
        }
        for instruction in &report.instructions {
            dump_instruction(output, instruction);
        }
    }
    for (block, transfer) in &report.edges {
        dump_transfer(output, *block, transfer);
    }
}

fn dump_branch_recipe(
    output: &mut String,
    block: BlockId,
    arm: BranchArm,
    recipe: BranchArmRecipe,
) {
    match recipe {
        BranchArmRecipe::Materialized { reason } => {
            writeln!(
                output,
                "  control block={} arm={arm:?} recipe=ReturnDispatcher reason={reason}",
                block.index()
            )
            .unwrap();
        }
        BranchArmRecipe::InlineZeroAbiTerminalCall(recipe) => {
            let origins = recipe.origins();
            writeln!(
                output,
                "  control block={} arm={arm:?} recipe=InlineZeroAbiTerminalCall consumed={} call={} callee={} advantage={} completion={:?} origins=[{:?},{:?},{:?}]",
                block.index(),
                recipe.consumed_block().index(),
                recipe.call_instruction().index(),
                recipe.callee().index(),
                recipe.advantage().code(),
                GeneratedCompletionContract::ExactlyOneOnNormalCompletion,
                origins.branch(),
                origins.call(),
                origins.terminal_return(),
            )
            .unwrap();
            dump_recipe_cost(
                output,
                "baseline",
                recipe
                    .baseline_cost()
                    .expect("verified closed baseline recipe cost remains representable"),
            );
            dump_recipe_cost(
                output,
                "selected",
                recipe
                    .selected_cost()
                    .expect("verified closed selected recipe cost remains representable"),
            );
        }
    }
}

fn dump_recipe_cost(
    output: &mut String,
    label: &str,
    cost: crate::lower::minecraft::recipe::ControlRecipeCost,
) {
    let then_path = cost.path(BranchArm::Then);
    let else_path = cost.path(BranchArm::Else);
    let structure = cost.structured_size();
    writeln!(
        output,
        "    cost {label} kind={:?} arm={:?} impact={:?} then={:?}/chain={:?} else={:?}/chain={:?} structure=[functions={},helpers={},commands={},nodes={}]",
        cost.kind(),
        cost.terminal_arm(),
        cost.whole_graph_impact(),
        then_path.counts(),
        then_path.maximum_chain_expansion(),
        else_path.counts(),
        else_path.maximum_chain_expansion(),
        structure.functions(),
        structure.helpers(),
        structure.top_level_commands(),
        structure.command_nodes(),
    )
    .unwrap();
}

fn dump_optimization_level(output: &mut String, level: MinecraftOptimizationLevel) {
    if level != MinecraftOptimizationLevel::None {
        writeln!(output, "optimization-level {level:?}").unwrap();
    }
}

fn dump_edge_temporary(output: &mut String, temporary: &EdgeTemporaryReport) {
    if let Some(ty) = temporary.ty {
        writeln!(
            output,
            "  edge-temporary home={} holder={} type={ty:?}",
            temporary.home.index(),
            temporary.holder
        )
        .unwrap();
    } else {
        writeln!(
            output,
            "  edge-temporary home={} holder={} type=untyped",
            temporary.home.index(),
            temporary.holder
        )
        .unwrap();
    }
}

impl FunctionReport {
    #[allow(
        clippy::too_many_lines,
        reason = "the report projection copies one dense verified layout into a single immutable audit record"
    )]
    fn from_layout(
        core: &CoreProgram,
        plan: &LoweringPlan,
        function: FunctionId,
        layout: &FunctionLayout,
    ) -> Self {
        let entry_function =
            layout.block_functions[usize::try_from(layout.abi.entry_block.index())
                .expect("Core block identity fits the host index")]
            .expect("verified report input has an entry function");
        let body = core
            .function(function)
            .and_then(crate::ir::core::Function::body)
            .expect("verified report input has every Core function body");
        Self {
            function,
            linkage: layout.linkage,
            diagnostic_name_hint: layout.diagnostic_name_hint.clone(),
            generated_entry_requirement: plan
                .ambient()
                .requirement(function)
                .expect("verified retained ambient analysis has every Core function"),
            parameters: layout
                .parameter_types
                .iter()
                .copied()
                .zip(layout.abi.parameters.iter().copied())
                .map(|(ty, home)| (ty, home, holder(plan, home)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            results: layout
                .result_types
                .iter()
                .copied()
                .zip(layout.abi.results.iter().copied())
                .map(|(ty, home)| (ty, home, holder(plan, home)))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            entry_block: layout.abi.entry_block,
            entry_resource: resource(plan, entry_function),
            blocks: layout
                .block_functions
                .iter()
                .enumerate()
                .filter_map(|(block, planned)| {
                    let block = BlockId::from_index(u32::try_from(block).ok()?);
                    let planned = (*planned)?;
                    Some((block, planned, resource(plan, planned)))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            placements: layout
                .block_placements
                .iter()
                .enumerate()
                .filter_map(|(block, placement)| {
                    Some((
                        BlockId::from_index(u32::try_from(block).ok()?),
                        (*placement)?,
                    ))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            branch_recipes: layout
                .branch_recipes
                .iter()
                .enumerate()
                .filter_map(|(block, recipe)| {
                    Some((BlockId::from_index(u32::try_from(block).ok()?), (*recipe)?))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            values: layout
                .value_homes
                .iter()
                .enumerate()
                .filter_map(|(value, home)| {
                    Some((ValueId::from_index(u32::try_from(value).ok()?), (*home)?))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            temporary: layout
                .parallel_copy_temp
                .map(|home| (home, holder(plan, home))),
            edge_temporaries: layout
                .edge_temporaries
                .iter()
                .copied()
                .map(|home| EdgeTemporaryReport {
                    home,
                    holder: holder(plan, home),
                    ty: plan.home_type(home),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            instructions: layout
                .instruction_plans
                .iter()
                .enumerate()
                .filter_map(|(instruction, instruction_plan)| {
                    let instruction = InstId::from_index(u32::try_from(instruction).ok()?);
                    let physical_plan = instruction_plan.clone()?;
                    let selected_recipe = match &physical_plan {
                        InstructionPlan::Minecraft { external, .. } => {
                            Some(plan_preflight_recipe(plan, *external)?.clone())
                        }
                        _ => None,
                    };
                    let semantic_provenance = selected_recipe.as_ref().map(|selected| {
                        let declaration = core
                            .minecraft_operation(selected.operation())
                            .expect("verified selected recipe names a Core semantic operation");
                        let instruction_origin = body
                            .instruction(instruction)
                            .expect("verified report instruction is attached to its Core body")
                            .origin();
                        SemanticProvenanceReport {
                            instruction_origin,
                            operation_origins: declaration.origins(),
                            message_origin: declaration.attributes().message_origin(),
                        }
                    });
                    Some(InstructionReport {
                        instruction,
                        plan: physical_plan,
                        selected_recipe,
                        semantic_provenance,
                    })
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            edges: layout
                .edge_transfers
                .iter()
                .enumerate()
                .filter_map(|(block, transfer)| {
                    Some((
                        BlockId::from_index(u32::try_from(block).ok()?),
                        transfer.clone()?,
                    ))
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            coalescing: layout.coalescing,
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive match keeps every closed instruction-plan variant's dump rendering together"
)]
fn dump_instruction(output: &mut String, report: &InstructionReport) {
    let instruction = report.instruction;
    match &report.plan {
        InstructionPlan::OmittedPure => {
            writeln!(output, "  instruction {} omitted-pure", instruction.index()).unwrap();
        }
        InstructionPlan::External { helper } => {
            writeln!(
                output,
                "  instruction {} external helper={}",
                instruction.index(),
                helper.index()
            )
            .unwrap();
        }
        InstructionPlan::Minecraft {
            external, recipe, ..
        } => {
            dump_minecraft_instruction(output, report, *external, *recipe);
        }
        InstructionPlan::EntityNbtRead { external, results } => {
            writeln!(
                output,
                "  instruction {} entity-nbt-read external={}",
                instruction.index(),
                external.index()
            )
            .unwrap();
            for result in results.iter().copied() {
                match result {
                    ScalarResultPlacement::Semantic {
                        result_index,
                        value,
                        home,
                    } => {
                        writeln!(
                            output,
                            "    result {result_index} semantic value={} home={}",
                            value.index(),
                            home.index()
                        )
                        .unwrap();
                    }
                    ScalarResultPlacement::RecipeTemporary { result_index, home } => {
                        writeln!(
                            output,
                            "    result {result_index} recipe-temporary home={}",
                            home.index()
                        )
                        .unwrap();
                    }
                }
            }
        }
        InstructionPlan::EntityNbtWrite { external, .. } => {
            writeln!(
                output,
                "  instruction {} entity-nbt-write external={}",
                instruction.index(),
                external.index()
            )
            .unwrap();
        }
        InstructionPlan::Scalar { operands, results } => {
            writeln!(output, "  instruction {} scalar", instruction.index()).unwrap();
            for (operand_index, home) in operands.iter().enumerate() {
                writeln!(output, "    operand {operand_index} home={}", home.index()).unwrap();
            }
            for result in results {
                match *result {
                    ScalarResultPlacement::Semantic {
                        result_index,
                        value,
                        home,
                    } => {
                        writeln!(
                            output,
                            "    result {result_index} semantic value={} home={}",
                            value.index(),
                            home.index()
                        )
                        .unwrap();
                    }
                    ScalarResultPlacement::RecipeTemporary { result_index, home } => {
                        writeln!(
                            output,
                            "    result {result_index} recipe-temporary home={}",
                            home.index()
                        )
                        .unwrap();
                    }
                }
            }
        }
        InstructionPlan::Call {
            arguments,
            result_destinations,
        } => {
            writeln!(output, "  instruction {} call", instruction.index()).unwrap();
            for (argument_index, home) in arguments.iter().enumerate() {
                writeln!(
                    output,
                    "    argument {argument_index} home={}",
                    home.index()
                )
                .unwrap();
            }
            for (slot, destination) in result_destinations.iter().enumerate() {
                if let Some(destination) = destination {
                    writeln!(
                        output,
                        "    result-slot {slot} semantic result={} value={} home={}",
                        destination.result_index(),
                        destination.value().index(),
                        destination.home().index()
                    )
                    .unwrap();
                } else {
                    writeln!(output, "    result-slot {slot} omitted").unwrap();
                }
            }
        }
    }
}

fn dump_minecraft_instruction(
    output: &mut String,
    report: &InstructionReport,
    external: crate::ir::core::ExternalOpId,
    recipe: crate::lower::minecraft::MinecraftRecipeId,
) {
    let selected = report
        .selected_recipe
        .as_ref()
        .expect("verified Minecraft instruction report retains its selected recipe");
    let provenance = report
        .semantic_provenance
        .expect("verified Minecraft instruction report retains exact Core provenance");
    let operation_origins = provenance.operation_origins;
    let descriptor = minecraft_descriptor(selected.semantic_key());
    let command = selected.command_kind();
    let contract = command.contract();
    let cost = classify_constructed_command(&command)
        .expect("every selected semantic recipe has a local target cost");
    writeln!(
        output,
        "  instruction {} minecraft external={} operation={} semantic={:?} recipe={recipe:?} placement=direct message={:?} origins=[instruction={:?},call={:?},member={:?},receiver={:?},attribute={:?}]",
        report.instruction.index(),
        external.index(),
        selected.operation().index(),
        selected.semantic_key(),
        selected.say_message().map(crate::ir::minecraft::SayMessage::as_str),
        provenance.instruction_origin,
        operation_origins.call(),
        operation_origins.member(),
        operation_origins.receiver(),
        provenance.message_origin,
    )
    .unwrap();
    writeln!(
        output,
        "    semantic context={:?} world-effect={:?} observable-effect={:?} fork={:?} work={:?} source-outcome={:?}",
        descriptor.ambient_context(),
        descriptor.world_effect(),
        descriptor.observable_effect(),
        descriptor.fork_behavior(),
        descriptor.work_behavior(),
        descriptor.outcome_behavior(),
    )
    .unwrap();
    writeln!(
        output,
        "    target context={:?} effects={:?} fork={:?} native-outcome={:?} local-counts={:?} max-chain={:?} control-outcomes={:?}",
        contract.context(),
        contract.effects(),
        contract.fork(),
        contract.native_outcome(),
        cost.counts(),
        cost.maximum_chain_expansion(),
        cost.outcomes(),
    )
    .unwrap();
}

fn plan_preflight_recipe(
    plan: &LoweringPlan,
    external: crate::ir::core::ExternalOpId,
) -> Option<&crate::lower::minecraft::SelectedSemanticRecipe> {
    plan.selected_semantic_recipe(external)
}

fn dump_transfer(output: &mut String, block: BlockId, transfer: &EdgeTransfer) {
    match transfer {
        EdgeTransfer::Jump { steps } => {
            writeln!(output, "  edge {} jump", block.index()).unwrap();
            dump_steps(output, steps);
        }
        EdgeTransfer::Branch {
            then_edge,
            else_edge,
        } => {
            writeln!(
                output,
                "  edge {} then helper={:?}",
                block.index(),
                then_edge.helper
            )
            .unwrap();
            dump_steps(output, &then_edge.steps);
            writeln!(
                output,
                "  edge {} else helper={:?}",
                block.index(),
                else_edge.helper
            )
            .unwrap();
            dump_steps(output, &else_edge.steps);
        }
    }
}

fn dump_steps(output: &mut String, steps: &[super::MoveStep]) {
    for step in steps {
        writeln!(
            output,
            "    move {} <- {}",
            step.destination.index(),
            step.source.index()
        )
        .unwrap();
    }
}

fn holder(plan: &LoweringPlan, home: HomeId) -> FakeScoreHolder {
    plan.homes
        .get(home)
        .expect("verified report input has every home")
        .holder
        .clone()
}

fn resource(plan: &LoweringPlan, function: PlannedFunctionId) -> FunctionResourceId {
    plan.target_functions
        .get(function)
        .expect("verified report input has every function")
        .resource
        .clone()
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityId;
    use crate::ir::core::{CoreType, InstId, ValueId};
    use crate::ir::minecraft::FakeScoreHolder;
    use crate::lower::minecraft::MinecraftOptimizationLevel;
    use crate::lower::minecraft::plan::{
        CallResultDestination, HomeId, InstructionPlan, ScalarResultPlacement,
    };

    use super::{
        EdgeTemporaryReport, InstructionReport, dump_edge_temporary, dump_instruction,
        dump_optimization_level,
    };

    #[test]
    fn optimization_level_line_is_absent_for_none_and_explicit_for_baseline() {
        let mut output = String::new();
        dump_optimization_level(&mut output, MinecraftOptimizationLevel::None);
        assert_eq!(output, "");

        dump_optimization_level(&mut output, MinecraftOptimizationLevel::Baseline);
        assert_eq!(output, "optimization-level Baseline\n");
    }

    #[test]
    fn baseline_instruction_records_render_every_indexed_decision() {
        let home = HomeId::from_index;
        let mut output = String::new();
        dump_instruction(
            &mut output,
            &InstructionReport {
                instruction: InstId::from_index(0),
                plan: InstructionPlan::OmittedPure,
                selected_recipe: None,
                semantic_provenance: None,
            },
        );
        dump_instruction(
            &mut output,
            &InstructionReport {
                instruction: InstId::from_index(1),
                plan: InstructionPlan::Scalar {
                    operands: vec![home(1), home(2)].into_boxed_slice(),
                    results: vec![
                        ScalarResultPlacement::Semantic {
                            result_index: 0,
                            value: ValueId::from_index(3),
                            home: home(4),
                        },
                        ScalarResultPlacement::RecipeTemporary {
                            result_index: 1,
                            home: home(5),
                        },
                    ]
                    .into_boxed_slice(),
                },
                selected_recipe: None,
                semantic_provenance: None,
            },
        );
        dump_instruction(
            &mut output,
            &InstructionReport {
                instruction: InstId::from_index(2),
                plan: InstructionPlan::Call {
                    arguments: vec![home(6)].into_boxed_slice(),
                    result_destinations: vec![
                        None,
                        Some(CallResultDestination {
                            result_index: 1,
                            value: ValueId::from_index(7),
                            home: home(8),
                        }),
                    ]
                    .into_boxed_slice(),
                },
                selected_recipe: None,
                semantic_provenance: None,
            },
        );

        assert_eq!(
            output,
            concat!(
                "  instruction 0 omitted-pure\n",
                "  instruction 1 scalar\n",
                "    operand 0 home=1\n",
                "    operand 1 home=2\n",
                "    result 0 semantic value=3 home=4\n",
                "    result 1 recipe-temporary home=5\n",
                "  instruction 2 call\n",
                "    argument 0 home=6\n",
                "    result-slot 0 omitted\n",
                "    result-slot 1 semantic result=1 value=7 home=8\n",
            )
        );
    }

    #[test]
    fn baseline_edge_temporary_rendering_includes_identity_holder_and_type() {
        let mut output = String::new();
        dump_edge_temporary(
            &mut output,
            &EdgeTemporaryReport {
                home: HomeId::from_index(9),
                holder: FakeScoreHolder::new("#f0ti0").unwrap(),
                ty: Some(CoreType::I32),
            },
        );
        assert_eq!(output, "  edge-temporary home=9 holder=#f0ti0 type=I32\n");
    }
}
