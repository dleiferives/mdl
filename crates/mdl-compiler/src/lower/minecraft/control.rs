use crate::diagnostic::Diagnostics;
use crate::ir::core::{BlockId, CoreType, FunctionId};
use crate::ir::minecraft::{
    CommandKind, Condition, ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers,
    FunctionCall, InternalCallableRef, McFunctionId, ReturnCommand, ScoreRange,
};
use crate::source::OriginId;

use super::construct::{FunctionLoweringCx, command, invariant_diagnostics};
use super::placement::BranchArmRecipe;
use super::plan::{BranchArm, BranchTransfer, EdgeTransfer, HomeId};
use super::scalar::copy_home;

/// Lowers one verified unconditional Core edge as physical copies and a tail transfer.
pub(crate) fn lower_jump(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    source: BlockId,
    destination: BlockId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let steps = match context.plan().edge_transfer(function, source) {
        Some(EdgeTransfer::Jump { steps }) => steps,
        Some(EdgeTransfer::Branch { .. }) | None => {
            return Err(invariant_diagnostics(
                "verified jump has no planned unconditional edge transfer",
                origin,
            ));
        }
    };
    for step in steps.iter().copied() {
        copy_home(context, step.destination(), step.source(), origin)?;
    }
    context.push(tail_call_to_block(context, function, destination, origin)?)
}

/// Copies semantic return values into fixed result slots and returns success.
pub(crate) fn lower_return(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    values: &[HomeId],
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let results = context
        .plan()
        .function_results(function)
        .ok_or_else(|| invariant_diagnostics("return has no planned function ABI", origin))?;
    if results.len() != values.len() {
        return Err(invariant_diagnostics(
            "verified return disagrees with the planned function ABI",
            origin,
        ));
    }
    for (result, value) in results.iter().copied().zip(values.iter().copied()) {
        context.require_same_type(result, value)?;
        copy_home(context, result, value, origin)?;
    }
    context.push(command(
        CommandKind::Return(ReturnCommand::Value(1)),
        origin,
    )?)
}

/// Emits the two-command single-evaluation dispatcher for one verified branch.
pub(crate) fn lower_branch(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    source: BlockId,
    condition: HomeId,
    then_destination: BlockId,
    else_destination: BlockId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    context.require_type(condition, CoreType::Bool)?;
    let (then_edge, else_edge) = branch_transfers(context, function, source, origin)?;
    let (then_target, then_origin) = branch_target(
        context,
        function,
        source,
        BranchArm::Then,
        then_destination,
        then_edge,
        origin,
    )?;
    let (else_target, else_origin) = branch_target(
        context,
        function,
        source,
        BranchArm::Else,
        else_destination,
        else_edge,
        origin,
    )?;
    let then_tail = tail_call_to_target(then_target, then_origin)?;
    let conditional = command(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(
                ExecuteModifier::new(
                    ExecuteModifierKind::If(Condition::ScoreMatches(
                        context.score(condition)?,
                        ScoreRange::exact(1),
                    )),
                    origin,
                ),
                vec![],
            ),
            then_tail,
        )),
        origin,
    )?;
    context.push(conditional)?;
    context.push(tail_call_to_target(else_target, else_origin)?)
}

/// Defines one preplanned branch-edge helper as moves followed by a tail transfer.
pub(crate) fn lower_branch_helper(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    source: BlockId,
    arm: BranchArm,
    destination: BlockId,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let edge = match (context.plan().edge_transfer(function, source), arm) {
        (Some(EdgeTransfer::Branch { then_edge, .. }), BranchArm::Then) => then_edge,
        (Some(EdgeTransfer::Branch { else_edge, .. }), BranchArm::Else) => else_edge,
        (Some(EdgeTransfer::Jump { .. }) | None, _) => {
            return Err(invariant_diagnostics(
                "branch helper has no planned conditional edge",
                origin,
            ));
        }
    };
    if edge.helper() != Some(context.planned_function()) || edge.steps().is_empty() {
        return Err(invariant_diagnostics(
            "branch helper context disagrees with its planned edge",
            origin,
        ));
    }
    for step in edge.steps().iter().copied() {
        copy_home(context, step.destination(), step.source(), origin)?;
    }
    context.push(tail_call_to_block(context, function, destination, origin)?)
}

fn branch_transfers<'a>(
    context: &FunctionLoweringCx<'a, '_>,
    function: FunctionId,
    source: BlockId,
    origin: OriginId,
) -> Result<(&'a BranchTransfer, &'a BranchTransfer), Diagnostics> {
    match context.plan().edge_transfer(function, source) {
        Some(EdgeTransfer::Branch {
            then_edge,
            else_edge,
        }) => Ok((then_edge, else_edge)),
        Some(EdgeTransfer::Jump { .. }) | None => Err(invariant_diagnostics(
            "verified branch has no planned conditional edge transfer",
            origin,
        )),
    }
}

fn edge_target(
    context: &FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    destination: BlockId,
    edge: &BranchTransfer,
    origin: OriginId,
) -> Result<McFunctionId, Diagnostics> {
    let planned = match edge.helper() {
        Some(helper) => helper,
        None => context
            .plan()
            .block_function(function, destination)
            .ok_or_else(|| {
                invariant_diagnostics("branch destination has no planned function", origin)
            })?,
    };
    context.function(planned)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one branch-arm target keeps semantic ownership, transfer, selected recipe, and provenance explicit"
)]
fn branch_target(
    context: &FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    source: BlockId,
    arm: BranchArm,
    destination: BlockId,
    edge: &BranchTransfer,
    branch_origin: OriginId,
) -> Result<(McFunctionId, OriginId), Diagnostics> {
    let recipe = context
        .plan()
        .branch_arm_recipe(function, source, arm)
        .ok_or_else(|| invariant_diagnostics("branch arm has no planned recipe", branch_origin))?;
    match recipe {
        BranchArmRecipe::Materialized { .. } => Ok((
            edge_target(context, function, destination, edge, branch_origin)?,
            branch_origin,
        )),
        BranchArmRecipe::InlineZeroAbiTerminalCall(recipe) => {
            if recipe.consumed_block() != destination
                || !edge.steps().is_empty()
                || edge.helper().is_some()
            {
                return Err(invariant_diagnostics(
                    "inline terminal branch target disagrees with its verified edge",
                    branch_origin,
                ));
            }
            let entry = context
                .plan()
                .function_entry(recipe.callee())
                .ok_or_else(|| {
                    invariant_diagnostics(
                        "inline terminal callee has no planned entry",
                        recipe.origins().call(),
                    )
                })?;
            Ok((context.function(entry)?, recipe.origins().call()))
        }
    }
}

fn tail_call_to_block(
    context: &FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    destination: BlockId,
    origin: OriginId,
) -> Result<crate::ir::minecraft::CommandNode, Diagnostics> {
    let planned = context
        .plan()
        .block_function(function, destination)
        .ok_or_else(|| invariant_diagnostics("jump destination has no planned function", origin))?;
    let target = context.function(planned)?;
    tail_call_to_target(target, origin)
}

fn tail_call_to_target(
    target: McFunctionId,
    origin: OriginId,
) -> Result<crate::ir::minecraft::CommandNode, Diagnostics> {
    let call = command(
        CommandKind::Function(FunctionCall::new(
            InternalCallableRef::Function(target).into(),
        )),
        origin,
    )?;
    command(CommandKind::Return(ReturnCommand::run(call)), origin)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{lower_branch, lower_branch_helper, lower_jump, lower_return};
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace, render_function};
    use crate::lower::minecraft::LoweringOptions;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::construct::TargetConstruction;
    use crate::lower::minecraft::plan::{BranchArm, EdgeTransfer, LoweringPlan, PlanBuilder};
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    #[test]
    fn empty_edge_is_one_tail_transfer() {
        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("empty_edge"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let destination = builder.create_block(origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(destination, vec![])),
                origin,
            ))
            .unwrap();
        builder.switch_to_block(destination).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let plan = build_plan(&core);
        let (rendered, origins) = render_jump(&core, &plan, function, entry, destination, origin);
        assert_eq!(rendered, "return run function mdl:__mdl/f0/b1\n");
        assert_eq!(origins, vec![origin]);
    }

    #[test]
    fn value_producing_join_copies_before_tail_transfer() {
        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("join"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let join = builder.create_block(origin).unwrap();
        let parameter = builder
            .append_block_parameter(join, CoreType::I32, origin)
            .unwrap();
        let value = builder.i32_constant(42, origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![value])),
                origin,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let plan = build_plan(&core);
        assert_ne!(
            plan.value_home(function, parameter),
            plan.value_home(function, value)
        );
        let (rendered, origins) = render_jump(&core, &plan, function, entry, join, origin);
        assert_eq!(
            rendered,
            concat!(
                "scoreboard players operation #f0v0 mdl.reg = #f0v1 mdl.reg\n",
                "return run function mdl:__mdl/f0/b1\n"
            )
        );
        assert_eq!(origins, vec![origin, origin]);
    }

    #[test]
    fn self_loop_swap_consumes_the_planned_temporary_schedule() {
        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("swap"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let loop_block = builder.create_block(origin).unwrap();
        let left = builder
            .append_block_parameter(loop_block, CoreType::I32, origin)
            .unwrap();
        let right = builder
            .append_block_parameter(loop_block, CoreType::I32, origin)
            .unwrap();
        let zero = builder.i32_constant(0, origin).unwrap();
        let one = builder.i32_constant(1, origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(loop_block, vec![zero, one])),
                origin,
            ))
            .unwrap();
        builder.switch_to_block(loop_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(loop_block, vec![right, left])),
                origin,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let plan = build_plan(&core);
        let (rendered, origins) =
            render_jump(&core, &plan, function, loop_block, loop_block, origin);
        assert_eq!(
            rendered,
            concat!(
                "scoreboard players operation #f0t0 mdl.reg = #f0v0 mdl.reg\n",
                "scoreboard players operation #f0v0 mdl.reg = #f0v1 mdl.reg\n",
                "scoreboard players operation #f0v1 mdl.reg = #f0t0 mdl.reg\n",
                "return run function mdl:__mdl/f0/b1\n"
            )
        );
        assert_eq!(origins, vec![origin; 4]);
    }

    #[test]
    fn zero_result_return_is_one_success_command() {
        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("unit"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), origin))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let plan = build_plan(&core);
        let (rendered, origins) = render_return(&core, &plan, function, &[], origin);
        assert_eq!(rendered, "return 1\n");
        assert_eq!(origins, vec![origin]);
    }

    #[test]
    fn multiple_results_copy_in_signature_order_before_success() {
        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("pair"),
                vec![],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let integer = builder.i32_constant(9, origin).unwrap();
        let boolean = builder.bool_constant(true, origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![integer, boolean]),
                origin,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let plan = build_plan(&core);
        let values = [
            plan.value_home(function, integer).unwrap(),
            plan.value_home(function, boolean).unwrap(),
        ];
        let (rendered, origins) = render_return(&core, &plan, function, &values, origin);
        assert_eq!(
            rendered,
            concat!(
                "scoreboard players operation #f0r0 mdl.reg = #f0v0 mdl.reg\n",
                "scoreboard players operation #f0r1 mdl.reg = #f0v1 mdl.reg\n",
                "return 1\n"
            )
        );
        assert_eq!(origins, vec![origin; 3]);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the nested dispatcher golden keeps both structural branch blocks visible"
    )]
    fn nested_direct_branches_each_use_the_two_command_dispatcher() {
        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("nested"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let nested = builder.create_block(origin).unwrap();
        let first_exit = builder.create_block(origin).unwrap();
        let second_exit = builder.create_block(origin).unwrap();
        let outer_condition = builder.bool_constant(true, origin).unwrap();
        let inner_condition = builder.bool_constant(false, origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: outer_condition,
                    then_target: BlockTarget::new(nested, vec![]),
                    else_target: BlockTarget::new(first_exit, vec![]),
                },
                origin,
            ))
            .unwrap();
        builder.switch_to_block(nested).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: inner_condition,
                    then_target: BlockTarget::new(first_exit, vec![]),
                    else_target: BlockTarget::new(second_exit, vec![]),
                },
                origin,
            ))
            .unwrap();
        for block in [first_exit, second_exit] {
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

        let plan = build_plan(&core);
        let rendered = render_branches(
            &core,
            &plan,
            function,
            &[
                BranchSpec {
                    source: entry,
                    condition: plan.value_home(function, outer_condition).unwrap(),
                    then_destination: nested,
                    else_destination: first_exit,
                    origin,
                },
                BranchSpec {
                    source: nested,
                    condition: plan.value_home(function, inner_condition).unwrap(),
                    then_destination: first_exit,
                    else_destination: second_exit,
                    origin,
                },
            ],
        );
        assert_eq!(
            rendered.blocks[&entry].0,
            concat!(
                "execute if score #f0v0 mdl.reg matches 1 run return run function mdl:__mdl/f0/b1\n",
                "return run function mdl:__mdl/f0/b2\n"
            )
        );
        assert_eq!(
            rendered.blocks[&nested].0,
            concat!(
                "execute if score #f0v1 mdl.reg matches 1 run return run function mdl:__mdl/f0/b2\n",
                "return run function mdl:__mdl/f0/b3\n"
            )
        );
        assert_eq!(rendered.blocks[&entry].1, vec![origin; 2]);
        assert_eq!(rendered.blocks[&nested].1, vec![origin; 2]);
        assert!(rendered.helpers.is_empty());
    }

    #[test]
    fn moved_then_and_moved_else_edges_select_only_their_own_helper() {
        for moves_on_then in [true, false] {
            let mut sources = SourceContext::new();
            let origin = sources.add_origin(Origin::Unknown).unwrap();
            let mut core = CoreProgram::new();
            let function = core
                .declare_function(Some("mixed"), vec![], vec![], OriginId::UNKNOWN)
                .unwrap();
            let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
            let entry = builder.entry_block();
            let direct = builder.create_block(origin).unwrap();
            let moved = builder.create_block(origin).unwrap();
            builder
                .append_block_parameter(moved, CoreType::I32, origin)
                .unwrap();
            let condition = builder.bool_constant(true, origin).unwrap();
            let value = builder.i32_constant(17, origin).unwrap();
            let moved_target = BlockTarget::new(moved, vec![value]);
            let direct_target = BlockTarget::new(direct, vec![]);
            let (then_target, else_target) = if moves_on_then {
                (moved_target, direct_target)
            } else {
                (direct_target, moved_target)
            };
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Branch {
                        condition,
                        then_target,
                        else_target,
                    },
                    origin,
                ))
                .unwrap();
            for block in [direct, moved] {
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

            let plan = build_plan(&core);
            let spec = BranchSpec {
                source: entry,
                condition: plan.value_home(function, condition).unwrap(),
                then_destination: if moves_on_then { moved } else { direct },
                else_destination: if moves_on_then { direct } else { moved },
                origin,
            };
            let rendered = render_branches(&core, &plan, function, &[spec]);
            let expected = if moves_on_then {
                concat!(
                    "execute if score #f0v1 mdl.reg matches 1 run return run function mdl:__mdl/f0/e0_0\n",
                    "return run function mdl:__mdl/f0/b1\n"
                )
            } else {
                concat!(
                    "execute if score #f0v1 mdl.reg matches 1 run return run function mdl:__mdl/f0/b1\n",
                    "return run function mdl:__mdl/f0/e0_1\n"
                )
            };
            assert_eq!(rendered.blocks[&entry].0, expected);
            let arm = if moves_on_then {
                BranchArm::Then
            } else {
                BranchArm::Else
            };
            assert_eq!(rendered.helpers.len(), 1);
            assert_eq!(
                rendered.helpers[&(entry, arm)].0,
                concat!(
                    "scoreboard players operation #f0v0 mdl.reg = #f0v2 mdl.reg\n",
                    "return run function mdl:__mdl/f0/b2\n"
                )
            );
        }
    }

    #[test]
    fn same_destination_edges_keep_distinct_argument_helpers() {
        let mut sources = SourceContext::new();
        let origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("same_join"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let join = builder.create_block(origin).unwrap();
        builder
            .append_block_parameter(join, CoreType::I32, origin)
            .unwrap();
        let condition = builder.bool_constant(true, origin).unwrap();
        let then_value = builder.i32_constant(4, origin).unwrap();
        let else_value = builder.i32_constant(5, origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(join, vec![then_value]),
                    else_target: BlockTarget::new(join, vec![else_value]),
                },
                origin,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let plan = build_plan(&core);
        let rendered = render_branches(
            &core,
            &plan,
            function,
            &[BranchSpec {
                source: entry,
                condition: plan.value_home(function, condition).unwrap(),
                then_destination: join,
                else_destination: join,
                origin,
            }],
        );
        assert_eq!(
            rendered.blocks[&entry].0,
            concat!(
                "execute if score #f0v1 mdl.reg matches 1 run return run function mdl:__mdl/f0/e0_0\n",
                "return run function mdl:__mdl/f0/e0_1\n"
            )
        );
        assert_eq!(
            rendered.helpers[&(entry, BranchArm::Then)].0,
            concat!(
                "scoreboard players operation #f0v0 mdl.reg = #f0v2 mdl.reg\n",
                "return run function mdl:__mdl/f0/b1\n"
            )
        );
        assert_eq!(
            rendered.helpers[&(entry, BranchArm::Else)].0,
            concat!(
                "scoreboard players operation #f0v0 mdl.reg = #f0v3 mdl.reg\n",
                "return run function mdl:__mdl/f0/b1\n"
            )
        );
    }

    fn build_plan(core: &CoreProgram) -> LoweringPlan {
        let analyses = SemanticInventory::new(core).unwrap();
        let mut builder = PlanBuilder::new(core, options()).unwrap();
        builder
            .physicalize(
                core,
                &analyses,
                &crate::lower::minecraft::demand::RuntimeDemand::optimization_disabled(),
            )
            .unwrap();
        builder.finish(core, &analyses).unwrap()
    }

    #[derive(Clone, Copy)]
    struct BranchSpec {
        source: BlockId,
        condition: crate::lower::minecraft::plan::HomeId,
        then_destination: BlockId,
        else_destination: BlockId,
        origin: OriginId,
    }

    struct RenderedBranches {
        blocks: BTreeMap<BlockId, (String, Vec<OriginId>)>,
        helpers: BTreeMap<(BlockId, BranchArm), (String, Vec<OriginId>)>,
    }

    fn render_branches(
        core: &CoreProgram,
        plan: &LoweringPlan,
        function: FunctionId,
        specs: &[BranchSpec],
    ) -> RenderedBranches {
        let mut construction = TargetConstruction::declare(plan).unwrap();
        construction.define_initialization(plan).unwrap();
        let body = core.function(function).unwrap().body().unwrap();
        let mut block_targets = BTreeMap::new();
        for block in body.block_order().iter().copied() {
            let planned = plan.block_function(function, block).unwrap();
            let target = construction.declarations().function(planned).unwrap();
            block_targets.insert(block, target);
            let mut context = construction.begin_function(body, plan, planned).unwrap();
            if let Some(spec) = specs.iter().find(|spec| spec.source == block) {
                lower_branch(
                    &mut context,
                    function,
                    spec.source,
                    spec.condition,
                    spec.then_destination,
                    spec.else_destination,
                    spec.origin,
                )
                .unwrap();
            }
            context.finish();
        }
        let mut helper_targets = BTreeMap::new();
        for spec in specs {
            let EdgeTransfer::Branch {
                then_edge,
                else_edge,
            } = plan.edge_transfer(function, spec.source).unwrap()
            else {
                panic!("branch fixture must have branch transfers");
            };
            for (arm, edge, destination) in [
                (BranchArm::Then, then_edge, spec.then_destination),
                (BranchArm::Else, else_edge, spec.else_destination),
            ] {
                let Some(helper) = edge.helper() else {
                    continue;
                };
                let target = construction.declarations().function(helper).unwrap();
                helper_targets.insert((spec.source, arm), target);
                let mut context = construction.begin_function(body, plan, helper).unwrap();
                lower_branch_helper(
                    &mut context,
                    function,
                    spec.source,
                    arm,
                    destination,
                    spec.origin,
                )
                .unwrap();
                context.finish();
            }
        }
        let program = construction.finish().unwrap();
        let blocks = block_targets
            .into_iter()
            .map(|(block, target)| (block, render_target(&program, target)))
            .collect();
        let helpers = helper_targets
            .into_iter()
            .map(|(edge, target)| (edge, render_target(&program, target)))
            .collect();
        RenderedBranches { blocks, helpers }
    }

    fn render_target(
        program: &crate::ir::minecraft::MinecraftProgram,
        target: crate::ir::minecraft::McFunctionId,
    ) -> (String, Vec<OriginId>) {
        let function = program.function(target).unwrap();
        let rendered = String::from_utf8(render_function(program, function).unwrap()).unwrap();
        let origins = function
            .body()
            .commands()
            .map(|(_, command)| command.origin())
            .collect();
        (rendered, origins)
    }

    fn render_jump(
        core: &CoreProgram,
        plan: &LoweringPlan,
        function: FunctionId,
        source: BlockId,
        destination: BlockId,
        origin: OriginId,
    ) -> (String, Vec<OriginId>) {
        let mut construction = TargetConstruction::declare(plan).unwrap();
        construction.define_initialization(plan).unwrap();
        let body = core.function(function).unwrap().body().unwrap();
        let source_planned = plan.block_function(function, source).unwrap();
        for block in body.block_order().iter().copied() {
            let planned = plan.block_function(function, block).unwrap();
            let mut context = construction.begin_function(body, plan, planned).unwrap();
            if block == source {
                lower_jump(&mut context, function, source, destination, origin).unwrap();
            }
            context.finish();
        }
        let target = construction
            .declarations()
            .function(source_planned)
            .unwrap();
        let program = construction.finish().unwrap();
        let target_function = program.function(target).unwrap();
        let origins = target_function
            .body()
            .commands()
            .map(|(_, command)| command.origin())
            .collect();
        let rendered =
            String::from_utf8(render_function(&program, target_function).unwrap()).unwrap();
        (rendered, origins)
    }

    fn render_return(
        core: &CoreProgram,
        plan: &LoweringPlan,
        function: FunctionId,
        values: &[crate::lower::minecraft::plan::HomeId],
        origin: OriginId,
    ) -> (String, Vec<OriginId>) {
        let mut construction = TargetConstruction::declare(plan).unwrap();
        construction.define_initialization(plan).unwrap();
        let body = core.function(function).unwrap().body().unwrap();
        let planned = plan.function_entry(function).unwrap();
        let mut context = construction.begin_function(body, plan, planned).unwrap();
        lower_return(&mut context, function, values, origin).unwrap();
        context.finish();
        let target = construction.declarations().function(planned).unwrap();
        let program = construction.finish().unwrap();
        let target_function = program.function(target).unwrap();
        let origins = target_function
            .body()
            .commands()
            .map(|(_, command)| command.origin())
            .collect();
        let rendered =
            String::from_utf8(render_function(&program, target_function).unwrap()).unwrap();
        (rendered, origins)
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
