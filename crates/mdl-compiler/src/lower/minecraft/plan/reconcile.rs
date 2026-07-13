//! Reconciliation of frozen control recipes with constructed Minecraft commands.

use crate::analysis::minecraft::{CommandStepCounts, classify_constructed_command};
use crate::diagnostic::Diagnostics;
use crate::entity::EntityId;
use crate::ir::core::{BlockId, CoreProgram, FunctionId, TerminatorKind};
use crate::ir::minecraft::{
    CallableRef, CommandKind, Condition, ExecuteModifierKind, InternalCallableRef, McFunctionId,
    MinecraftProgram, ReturnCommand, ScoreRange,
};

use super::{BranchArm, EdgeTransfer, LoweringPlan, PlannedFunctionId};
use crate::lower::minecraft::construct::invariant_diagnostics;
use crate::lower::minecraft::placement::BranchArmRecipe;

/// Recounts every selected recipe from the immutable target syntax.
///
/// This is deliberately a construction verifier, not a chooser: it neither searches
/// for recipes nor runs whole-program cost analysis. The frozen recipe already won;
/// this boundary only proves that its actual commands still implement the same closed
/// local fragment and command-step summary.
pub(crate) fn verify_constructed_control_recipes(
    core: &CoreProgram,
    plan: &LoweringPlan,
    program: &MinecraftProgram,
) -> Result<(), Diagnostics> {
    for (function, layout) in plan.functions.iter() {
        for (source_index, branch_recipe) in layout.branch_recipes.iter().enumerate() {
            let Some(branch_recipe) = branch_recipe else {
                continue;
            };
            if [BranchArm::Then, BranchArm::Else].into_iter().all(|arm| {
                !matches!(
                    branch_recipe.arm(arm),
                    BranchArmRecipe::InlineZeroAbiTerminalCall(_)
                )
            }) {
                continue;
            }
            let source = u32::try_from(source_index)
                .ok()
                .map(BlockId::from_index)
                .ok_or_else(|| recipe_mismatch("branch source exceeds the Core identity domain"))?;
            let source_planned = layout
                .block_functions
                .get(source_index)
                .copied()
                .flatten()
                .ok_or_else(|| recipe_mismatch("selected recipe source is not materialized"))?;
            reconcile_branch(
                core,
                plan,
                program,
                function,
                source_planned,
                source,
                *branch_recipe,
            )?;
        }
    }
    Ok(())
}

fn reconcile_branch(
    core: &CoreProgram,
    plan: &LoweringPlan,
    program: &MinecraftProgram,
    function: FunctionId,
    source_planned: PlannedFunctionId,
    source: BlockId,
    branch_recipe: crate::lower::minecraft::placement::BranchRecipe,
) -> Result<(), Diagnostics> {
    let source_function = target_function(plan, program, source_planned)?;
    let commands = source_function
        .body()
        .commands()
        .map(|(_, command)| command)
        .collect::<Vec<_>>();
    let first_index = commands
        .len()
        .checked_sub(2)
        .ok_or_else(|| recipe_mismatch("selected dispatcher emitted no conditional command"))?;
    let first = commands[first_index];
    let second = commands[first_index + 1];

    let first_target = conditional_tail_target(first)?;
    let second_target = tail_target(second)?;
    let branch_origin = core
        .functions
        .get(function)
        .and_then(crate::ir::core::Function::body)
        .and_then(|body| body.block(source))
        .and_then(crate::ir::core::BlockData::terminator)
        .map(crate::ir::core::Terminator::origin)
        .ok_or_else(|| recipe_mismatch("selected recipe source branch disappeared"))?;

    if first.origin() != branch_origin
        || first_execute_modifier_origin(first) != Some(branch_origin)
    {
        return Err(recipe_mismatch(
            "constructed branch guard lost its recorded semantic origin",
        ));
    }

    for (arm, actual_target, command) in [
        (BranchArm::Then, first_target, first),
        (BranchArm::Else, second_target, second),
    ] {
        let arm_recipe = branch_recipe.arm(arm);
        let expected_target =
            expected_arm_target(core, plan, program, function, source, arm, arm_recipe)?;
        if actual_target != expected_target {
            return Err(recipe_mismatch(format!(
                "constructed {function:?} {source:?} {arm:?} target {actual_target:?} differs from planned {expected_target:?}"
            )));
        }
        let expected_origin = match arm_recipe {
            BranchArmRecipe::Materialized { .. } => branch_origin,
            BranchArmRecipe::InlineZeroAbiTerminalCall(recipe) => {
                if recipe.terminal_arm() != arm || recipe.origins().branch() != branch_origin {
                    return Err(recipe_mismatch(
                        "selected recipe arm or branch origin changed after verification",
                    ));
                }
                reconcile_selected_cost(first, second, recipe)?;
                recipe.origins().call()
            }
        };
        if !tail_origins(command, arm).is_some_and(|origins_in_target| {
            origins_in_target
                .iter()
                .all(|origin| *origin == expected_origin)
        }) {
            return Err(recipe_mismatch(format!(
                "constructed {arm:?} tail lost its planned origin"
            )));
        }
    }
    Ok(())
}

fn reconcile_selected_cost(
    first: &crate::ir::minecraft::CommandNode,
    second: &crate::ir::minecraft::CommandNode,
    recipe: crate::lower::minecraft::placement::InlineZeroAbiTerminalCall,
) -> Result<(), Diagnostics> {
    let first_cost = classify_constructed_command(first.kind())
        .ok_or_else(|| recipe_mismatch("constructed conditional command could not be recounted"))?;
    let second_cost = classify_constructed_command(second.kind())
        .ok_or_else(|| recipe_mismatch("constructed fallback command could not be recounted"))?;
    let rejected_guard = CommandStepCounts::new(1, 1, 0, 0);
    let else_counts = rejected_guard
        .checked_add(second_cost.counts())
        .ok_or_else(|| recipe_mismatch("constructed recipe path cost overflowed"))?;
    let selected_cost = recipe
        .selected_cost()
        .map_err(|error| recipe_mismatch(format!("selected recipe cost failed: {error}")))?;
    if selected_cost.path(BranchArm::Then).counts() != first_cost.counts()
        || selected_cost.path(BranchArm::Else).counts() != else_counts
        || selected_cost
            .path(BranchArm::Then)
            .maximum_chain_expansion()
            != crate::analysis::minecraft::CountBound::exact(1)
        || selected_cost
            .path(BranchArm::Else)
            .maximum_chain_expansion()
            != crate::analysis::minecraft::CountBound::exact(0)
    {
        return Err(recipe_mismatch(
            "constructed recipe command-step recount differs from its frozen prediction",
        ));
    }

    let structured = selected_cost.structured_size();
    let actual_nodes = command_node_count(first)
        .checked_add(command_node_count(second))
        .ok_or_else(|| recipe_mismatch("constructed recipe node count overflowed"))?;
    if structured.functions() != 1
        || structured.helpers() != 0
        || structured.top_level_commands() != 2
        || structured.command_nodes() != actual_nodes
    {
        return Err(recipe_mismatch(
            "constructed recipe structure differs from its frozen prediction",
        ));
    }
    Ok(())
}

fn expected_arm_target(
    core: &CoreProgram,
    plan: &LoweringPlan,
    program: &MinecraftProgram,
    function: FunctionId,
    source: BlockId,
    arm: BranchArm,
    recipe: BranchArmRecipe,
) -> Result<McFunctionId, Diagnostics> {
    let layout = plan
        .functions
        .get(function)
        .ok_or_else(|| recipe_mismatch("selected recipe function layout disappeared"))?;
    let source_index = usize::try_from(source.index())
        .map_err(|_| recipe_mismatch("selected recipe source exceeds the host index domain"))?;
    let (then_target, else_target) = core
        .function(function)
        .and_then(crate::ir::core::Function::body)
        .and_then(|body| body.block(source))
        .and_then(crate::ir::core::BlockData::terminator)
        .and_then(|terminator| match terminator.kind() {
            TerminatorKind::Branch {
                then_target,
                else_target,
                ..
            } => Some((then_target, else_target)),
            TerminatorKind::Jump(_) | TerminatorKind::Return(_) | TerminatorKind::Unreachable => {
                None
            }
        })
        .ok_or_else(|| recipe_mismatch("selected recipe no longer belongs to a branch"))?;
    let destination = match arm {
        BranchArm::Then => then_target.block(),
        BranchArm::Else => else_target.block(),
    };
    let EdgeTransfer::Branch {
        then_edge,
        else_edge,
    } = layout
        .edge_transfers
        .get(source_index)
        .and_then(Option::as_ref)
        .ok_or_else(|| recipe_mismatch("selected recipe edge transfer disappeared"))?
    else {
        return Err(recipe_mismatch(
            "selected recipe edge transfer is not a branch",
        ));
    };
    let transfer = match arm {
        BranchArm::Then => then_edge,
        BranchArm::Else => else_edge,
    };
    let planned = match recipe {
        BranchArmRecipe::Materialized { .. } => transfer.helper().or_else(|| {
            usize::try_from(destination.index())
                .ok()
                .and_then(|index| layout.block_functions.get(index))
                .copied()
                .flatten()
        }),
        BranchArmRecipe::InlineZeroAbiTerminalCall(recipe) => {
            plan.functions.get(recipe.callee()).and_then(|callee| {
                usize::try_from(callee.abi.entry_block.index())
                    .ok()
                    .and_then(|index| callee.block_functions.get(index))
                    .copied()
                    .flatten()
            })
        }
    }
    .ok_or_else(|| recipe_mismatch("selected recipe target planned function disappeared"))?;
    target_id(plan, program, planned)
}

fn target_function<'a>(
    plan: &LoweringPlan,
    program: &'a MinecraftProgram,
    planned: PlannedFunctionId,
) -> Result<&'a crate::ir::minecraft::McFunction, Diagnostics> {
    let target = target_id(plan, program, planned)?;
    program
        .function(target)
        .ok_or_else(|| recipe_mismatch("constructed recipe function is absent"))
}

fn target_id(
    plan: &LoweringPlan,
    program: &MinecraftProgram,
    planned: PlannedFunctionId,
) -> Result<McFunctionId, Diagnostics> {
    let target = McFunctionId::from_index(planned.index());
    let expected_resource = plan
        .target_functions
        .get(planned)
        .map(|function| &function.resource)
        .ok_or_else(|| recipe_mismatch("selected recipe planned function is absent"))?;
    if program
        .function(target)
        .is_none_or(|function| function.resource() != expected_resource)
    {
        return Err(recipe_mismatch(
            "planned and constructed recipe function identities diverged",
        ));
    }
    Ok(target)
}

fn conditional_tail_target(
    command: &crate::ir::minecraft::CommandNode,
) -> Result<McFunctionId, Diagnostics> {
    let CommandKind::Execute(execute) = command.kind() else {
        return Err(recipe_mismatch("recipe guard is not an execute command"));
    };
    let [modifier] = execute.modifiers().as_slice() else {
        return Err(recipe_mismatch("recipe guard does not have one modifier"));
    };
    if !matches!(
        modifier.kind(),
        ExecuteModifierKind::If(Condition::ScoreMatches(_, range))
            if *range == ScoreRange::exact(1)
    ) {
        return Err(recipe_mismatch(
            "recipe guard is not the normalized Boolean score test",
        ));
    }
    tail_target(execute.run())
}

fn first_execute_modifier_origin(
    command: &crate::ir::minecraft::CommandNode,
) -> Option<crate::source::OriginId> {
    let CommandKind::Execute(execute) = command.kind() else {
        return None;
    };
    let [modifier] = execute.modifiers().as_slice() else {
        return None;
    };
    Some(modifier.origin())
}

fn tail_target(command: &crate::ir::minecraft::CommandNode) -> Result<McFunctionId, Diagnostics> {
    let CommandKind::Return(ReturnCommand::Run(nested)) = command.kind() else {
        return Err(recipe_mismatch("recipe target is not `return run`"));
    };
    let CommandKind::Function(call) = nested.kind() else {
        return Err(recipe_mismatch("recipe tail does not invoke a function"));
    };
    match call.target() {
        CallableRef::Internal(InternalCallableRef::Function(target)) => Ok(*target),
        CallableRef::Internal(InternalCallableRef::Tag(_)) | CallableRef::External(_) => Err(
            recipe_mismatch("recipe tail does not invoke an owned function"),
        ),
    }
}

fn tail_origins(
    command: &crate::ir::minecraft::CommandNode,
    arm: BranchArm,
) -> Option<[crate::source::OriginId; 2]> {
    let command = if arm == BranchArm::Then {
        let CommandKind::Execute(execute) = command.kind() else {
            return None;
        };
        execute.run()
    } else {
        command
    };
    let CommandKind::Return(ReturnCommand::Run(nested)) = command.kind() else {
        return None;
    };
    Some([command.origin(), nested.origin()])
}

fn command_node_count(command: &crate::ir::minecraft::CommandNode) -> u64 {
    1 + match command.kind() {
        CommandKind::Execute(execute) => command_node_count(execute.run()),
        CommandKind::Return(ReturnCommand::Run(nested)) => command_node_count(nested),
        CommandKind::Score(_)
        | CommandKind::Data(_)
        | CommandKind::Function(_)
        | CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail)
        | CommandKind::Raw(_) => 0,
    }
}

fn recipe_mismatch(message: impl Into<String>) -> Diagnostics {
    invariant_diagnostics(
        format!("constructed control recipe mismatch: {}", message.into()),
        crate::source::OriginId::UNKNOWN,
    )
}
