//! Reconciliation of frozen control recipes with constructed Minecraft commands.

use crate::analysis::minecraft::{CommandStepCounts, classify_constructed_command};
use crate::diagnostic::Diagnostics;
use crate::entity::EntityId;
use crate::ir::core::{BlockId, CoreOp, CoreProgram, FunctionId, TerminatorKind};
use crate::ir::minecraft::{
    CallableRef, CommandKind, Condition, ExecuteModifierKind, InternalCallableRef, McFunctionId,
    MinecraftProgram, ReturnCommand, ScoreRange,
};
use crate::ir::semantic::minecraft_descriptor;

use super::{BranchArm, EdgeTransfer, InstructionPlan, LoweringPlan, PlannedFunctionId};
use crate::lower::minecraft::construct::invariant_diagnostics;
use crate::lower::minecraft::emit::ConstructionMap;
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
    construction: &ConstructionMap,
) -> Result<(), Diagnostics> {
    reconcile_semantic_commands(core, plan, program, construction)?;
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

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive per-plan-kind dispatch keeps every closed construction correlation check together"
)]
fn reconcile_semantic_commands(
    core: &CoreProgram,
    plan: &LoweringPlan,
    program: &MinecraftProgram,
    construction: &ConstructionMap,
) -> Result<(), Diagnostics> {
    for (function, declaration) in core.functions() {
        let body = declaration
            .body()
            .ok_or_else(|| recipe_mismatch("semantic reconciliation lost a Core definition"))?;
        for block in body.block_order().iter().copied() {
            let Some(block_data) = body.block(block) else {
                continue;
            };
            for instruction in block_data.instructions().iter().copied() {
                let planned = plan.instruction_plan(function, instruction);
                let location = construction.location(function, instruction);
                let expects_direct_command = matches!(
                    planned,
                    Some(InstructionPlan::Minecraft { .. } | InstructionPlan::EntityNbtRead { .. })
                );
                if expects_direct_command != location.is_some() {
                    return Err(recipe_mismatch(format!(
                        "post-construction command correlation disagrees for {function:?} {instruction:?}"
                    )));
                }
                let (Some(planned), Some(location)) = (planned, location) else {
                    continue;
                };
                let expected_function = plan
                    .block_function(function, block)
                    .map(|planned| McFunctionId::from_index(planned.index()))
                    .ok_or_else(|| {
                        recipe_mismatch(
                            "direct semantic command has no materialized containing block",
                        )
                    })?;
                if location.function() != expected_function {
                    return Err(recipe_mismatch(
                        "direct semantic command was correlated to the wrong target function",
                    ));
                }
                let data = body.instruction(instruction).ok_or_else(|| {
                    recipe_mismatch("direct semantic command lost its Core instruction")
                })?;
                match planned {
                    InstructionPlan::Minecraft {
                        external, recipe, ..
                    } => {
                        if !matches!(data.op(), CoreOp::External(actual) if actual == external) {
                            return Err(recipe_mismatch(
                                "direct semantic command no longer names its planned external declaration",
                            ));
                        }
                        let selected =
                            plan.selected_semantic_recipe(*external).ok_or_else(|| {
                                recipe_mismatch(
                                    "direct semantic command lost its selected preflight recipe",
                                )
                            })?;
                        if selected.recipe_id() != *recipe {
                            return Err(recipe_mismatch(
                                "direct semantic command recipe differs from retained preflight",
                            ));
                        }
                        let command = program
                            .function(location.function())
                            .and_then(|function| function.body().command(location.command()))
                            .ok_or_else(|| {
                                recipe_mismatch("correlated target command does not exist")
                            })?;
                        if command.origin() != data.origin() {
                            return Err(recipe_mismatch(
                                "direct semantic command lost its exact occurrence origin",
                            ));
                        }
                        reconcile_selected_semantic_command(core, selected, command)?;
                    }
                    InstructionPlan::EntityNbtRead { external, .. } => {
                        if !matches!(data.op(), CoreOp::External(actual) if actual == external) {
                            return Err(recipe_mismatch(
                                "entity-NBT read command no longer names its planned external declaration",
                            ));
                        }
                        let resolved =
                            plan.preflight().selected_entity_nbt_read(*external).ok_or_else(|| {
                                recipe_mismatch(
                                    "entity-NBT read command lost its selected preflight resolution",
                                )
                            })?;
                        if resolved.is_unusable_inline() {
                            return Err(recipe_mismatch(
                                "inline entity-NBT read command retains a runtime index segment",
                            ));
                        }
                        let command = program
                            .function(location.function())
                            .and_then(|function| function.body().command(location.command()))
                            .ok_or_else(|| {
                                recipe_mismatch("correlated target command does not exist")
                            })?;
                        if command.origin() != data.origin() {
                            return Err(recipe_mismatch(
                                "entity-NBT read command lost its exact occurrence origin",
                            ));
                        }
                        reconcile_entity_nbt_read_command(
                            plan, program, resolved, location, command,
                        )?;
                    }
                    InstructionPlan::OmittedPure
                    | InstructionPlan::Scalar { .. }
                    | InstructionPlan::Call { .. }
                    | InstructionPlan::External { .. } => {}
                }
            }
        }
    }
    Ok(())
}

/// Verifies a constructed `DataSource` matches its resolved receiver and
/// path (BE-1): an entity receiver's selector must be the ambient
/// self-executor (unchanged from PS-12); a block receiver's position must
/// match exactly, with no ambient context to fall back on.
fn entity_nbt_source_matches(
    resolved: &super::super::preflight::ResolvedEntityNbtRead,
    source: &crate::ir::minecraft::DataSource,
    expected_path: &crate::ir::minecraft::NbtPath,
) -> bool {
    match (resolved.receiver(), source) {
        (
            crate::ir::core::EntityNbtReceiver::Entity(_),
            crate::ir::minecraft::DataSource::Entity { selector, path },
        ) => {
            *selector
                == crate::ir::minecraft::Selector::from(
                    crate::ir::minecraft::AtMostOneSelector::SelfExecutor,
                )
                && path == expected_path
        }
        (
            crate::ir::core::EntityNbtReceiver::Block(_, expected_position),
            crate::ir::minecraft::DataSource::Block { position, path },
        ) => *position == expected_position && path == expected_path,
        _ => false,
    }
}

/// Verifies one constructed inline entity-NBT read against its resolved
/// segments. Two shapes are legal, matching `emit_entity_nbt_read_result`'s
/// two branches:
/// - `String` results: one `data modify <home> set from entity …` command,
///   immediately preceded by an empty-string initialization of the same home
///   (generalizes the retired `BookPage` reconciliation's exact shape).
/// - `Bool`/`I32` results: `execute store result score … run data get
///   storage <scratch>`, preceded by two commands writing the read (or its
///   type-appropriate default on failure) into the shared scratch slot —
///   `data get` has no entity-source form in this IR yet, so the read lands
///   in NBT storage first and converts to a score last.
#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive match keeps both legal constructed shapes and their fallback checks together"
)]
fn reconcile_entity_nbt_read_command(
    plan: &LoweringPlan,
    program: &MinecraftProgram,
    resolved: &super::super::preflight::ResolvedEntityNbtRead,
    location: crate::lower::minecraft::emit::ConstructedCommandLocation,
    command: &crate::ir::minecraft::CommandNode,
) -> Result<(), Diagnostics> {
    let expected_path =
        super::super::emit::entity_nbt_path_from_segments(resolved.segments(), command.origin())?;
    match command.kind() {
        CommandKind::Data(crate::ir::minecraft::DataCommand::Modify {
            target: read_target,
            mode: crate::ir::minecraft::DataModifyMode::Set,
            source,
        }) if matches!(
            source,
            crate::ir::minecraft::DataSource::Entity { .. }
                | crate::ir::minecraft::DataSource::Block { .. }
        ) =>
        {
            if !entity_nbt_source_matches(resolved, source, &expected_path) {
                return Err(recipe_mismatch(
                    "entity-NBT read command receiver or path differs from its resolved segments",
                ));
            }
            let fallback = preceding_command(program, location, 1)?;
            if fallback.origin() != command.origin() {
                return Err(recipe_mismatch(
                    "entity-NBT read empty initialization lost its occurrence origin",
                ));
            }
            match fallback.kind() {
                CommandKind::Data(crate::ir::minecraft::DataCommand::Modify {
                    target,
                    mode: crate::ir::minecraft::DataModifyMode::Set,
                    source: crate::ir::minecraft::DataSource::Value(value),
                }) if target == read_target
                    && value == &crate::ir::minecraft::NbtValue::string("") => {}
                _ => {
                    return Err(recipe_mismatch(
                        "entity-NBT read is not preceded by an empty-string initialization of its result home",
                    ));
                }
            }
            Ok(())
        }
        CommandKind::Execute(execute) => {
            let [modifier] = execute.modifiers().as_slice() else {
                return Err(recipe_mismatch(
                    "entity-NBT scalar read has an unexpected modifier chain",
                ));
            };
            if !matches!(
                modifier.kind(),
                crate::ir::minecraft::ExecuteModifierKind::Store(
                    crate::ir::minecraft::StoreChannel::Result,
                    crate::ir::minecraft::StoreDestination::Score(_),
                )
            ) {
                return Err(recipe_mismatch(
                    "entity-NBT scalar read does not store its result into a score",
                ));
            }
            let CommandKind::Data(crate::ir::minecraft::DataCommand::Get { source, .. }) =
                execute.run().kind()
            else {
                return Err(recipe_mismatch(
                    "entity-NBT scalar read does not convert from a storage read",
                ));
            };
            let scratch = plan.entity_nbt_scalar_scratch();
            if source != &scratch {
                return Err(recipe_mismatch(
                    "entity-NBT scalar read converts from an unexpected storage location",
                ));
            }
            let read_attempt = preceding_command(program, location, 1)?;
            let CommandKind::Data(crate::ir::minecraft::DataCommand::Modify {
                target,
                mode: crate::ir::minecraft::DataModifyMode::Set,
                source,
            }) = read_attempt.kind()
            else {
                return Err(recipe_mismatch(
                    "entity-NBT scalar read is not preceded by an entity/block read into the scratch slot",
                ));
            };
            if target != &scratch || !entity_nbt_source_matches(resolved, source, &expected_path) {
                return Err(recipe_mismatch(
                    "entity-NBT scalar read attempt differs from its resolved segments",
                ));
            }
            let fallback = preceding_command(program, location, 2)?;
            match fallback.kind() {
                CommandKind::Data(crate::ir::minecraft::DataCommand::Modify {
                    target,
                    mode: crate::ir::minecraft::DataModifyMode::Set,
                    source: crate::ir::minecraft::DataSource::Value(value),
                }) if target == &scratch
                    && (value == &crate::ir::minecraft::NbtValue::byte(0)
                        || value == &crate::ir::minecraft::NbtValue::int(0)) => {}
                _ => {
                    return Err(recipe_mismatch(
                        "entity-NBT scalar read is not preceded by a default initialization of its scratch slot",
                    ));
                }
            }
            Ok(())
        }
        _ => Err(recipe_mismatch(
            "entity-NBT read command has an unexpected shape",
        )),
    }
}

fn preceding_command(
    program: &MinecraftProgram,
    location: crate::lower::minecraft::emit::ConstructedCommandLocation,
    steps_back: u32,
) -> Result<&crate::ir::minecraft::CommandNode, Diagnostics> {
    let index = location
        .command()
        .index()
        .checked_sub(steps_back)
        .ok_or_else(|| recipe_mismatch("entity-NBT read has no preceding command"))?;
    let id = crate::ir::minecraft::CommandId::from_index(index);
    program
        .function(location.function())
        .and_then(|function| function.body().command(id))
        .ok_or_else(|| recipe_mismatch("entity-NBT read's preceding command does not exist"))
}

fn reconcile_selected_semantic_command(
    core: &CoreProgram,
    selected: &crate::lower::minecraft::SelectedSemanticRecipe,
    command: &crate::ir::minecraft::CommandNode,
) -> Result<(), Diagnostics> {
    let declaration = core
        .minecraft_operation(selected.operation())
        .ok_or_else(|| recipe_mismatch("selected recipe lost its semantic declaration"))?;
    if declaration.key() != selected.semantic_key() {
        return Err(recipe_mismatch(
            "selected recipe semantic key differs from its Core declaration",
        ));
    }
    let projection = selected.contract_projection(declaration);
    if !projection.matches_semantic_descriptor(
        minecraft_descriptor(declaration.key()),
        declaration.receiver_kind(),
    ) {
        return Err(recipe_mismatch(
            "selected recipe differs from its instantiated semantic descriptor",
        ));
    }

    match (selected, command.kind()) {
        (
            crate::lower::minecraft::SelectedSemanticRecipe::Java26_2Say { message, .. },
            CommandKind::Say(command),
        ) if command.message() == message => {}
        (
            crate::lower::minecraft::SelectedSemanticRecipe::Java26_2Teleport {
                destination, ..
            },
            CommandKind::Teleport(command),
        ) if command.destination() == destination => {}
        (
            crate::lower::minecraft::SelectedSemanticRecipe::Java26_2MoveBy { destination, .. },
            CommandKind::Execute(execute),
        ) if matches!(execute.modifiers().as_slice(), [modifier]
            if matches!(modifier.kind(), crate::ir::minecraft::ExecuteModifierKind::At(_)))
            && matches!(execute.run().kind(), CommandKind::Teleport(command) if command.destination() == destination) =>
            {}
        _ => {
            return Err(recipe_mismatch(
                "constructed semantic command differs from its exact selected recipe",
            ));
        }
    }
    if !projection.matches_target_contract(command.contract()) {
        return Err(recipe_mismatch(
            "constructed semantic command contract differs from the selected recipe",
        ));
    }
    let cost = classify_constructed_command(command.kind()).ok_or_else(|| {
        recipe_mismatch("constructed semantic command has no local cost classification")
    })?;
    if !projection.matches_local_cost(&cost) {
        return Err(recipe_mismatch(
            "constructed semantic command cost differs from the selected recipe",
        ));
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
        | CommandKind::Say(_)
        | CommandKind::Teleport(_)
        | CommandKind::Function(_)
        | CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail)
        | CommandKind::Raw(_)
        | CommandKind::Macro(_)
        | CommandKind::FunctionWithStorage(_) => 0,
    }
}

fn recipe_mismatch(message: impl Into<String>) -> Diagnostics {
    invariant_diagnostics(
        format!("constructed control recipe mismatch: {}", message.into()),
        crate::source::OriginId::UNKNOWN,
    )
}

#[cfg(test)]
mod tests {
    use super::{reconcile_selected_cost, tail_target, verify_constructed_control_recipes};
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind,
    };
    use crate::ir::minecraft::{
        CommandKind, CommandNode, ExecuteCommand, ExecuteModifier, ExecuteModifiers, FunctionCall,
        InternalCallableRef, McFunctionId, MinecraftProgram, MinecraftProgramBuilder,
        ObjectiveName, PackNamespace, ReturnCommand,
    };
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::assignment::HomeAssignment;
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::lower::minecraft::edge_transfer::EdgeTransferPlan;
    use crate::lower::minecraft::emit::{ConstructionMap, construct_program};
    use crate::lower::minecraft::placement::{
        BranchArmRecipe, ControlRecipePlan, InlineZeroAbiTerminalCall,
    };
    use crate::lower::minecraft::plan::{BranchArm, LoweringPlan};
    use crate::lower::minecraft::resources::ResourceInventory;
    use crate::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    struct Fixture {
        core: CoreProgram,
        plan: LoweringPlan,
        target: MinecraftProgram,
        construction: ConstructionMap,
        dispatcher: FunctionId,
        source: BlockId,
        wrong_origin: OriginId,
    }

    #[test]
    fn rejects_a_constructed_recipe_with_a_changed_target() {
        let fixture = selected_terminal_call_fixture();
        let source_target = source_target(&fixture);
        let commands = source_commands(&fixture);
        let wrong_target = tail_target(commands[1]).expect("fallback must be a tail call");
        let corrupted = rebuild_with_replaced_command(
            &fixture.target,
            source_target,
            0,
            retarget_conditional(commands[0], wrong_target),
        );

        assert_reconciliation_error(
            verify_constructed_control_recipes(
                &fixture.core,
                &fixture.plan,
                &corrupted,
                &fixture.construction,
            ),
            "differs from planned",
        );
    }

    #[test]
    fn rejects_a_constructed_recipe_with_a_changed_guard_origin() {
        let fixture = selected_terminal_call_fixture();
        let source_target = source_target(&fixture);
        let commands = source_commands(&fixture);
        let CommandKind::Execute(execute) = commands[0].kind() else {
            panic!("fixture guard must be an execute command")
        };
        let [modifier] = execute.modifiers().as_slice() else {
            panic!("fixture guard must have one modifier")
        };
        let changed_guard = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(modifier.kind().clone(), fixture.wrong_origin),
                    vec![],
                ),
                execute.run().clone(),
            )),
            commands[0].origin(),
        )
        .unwrap();
        let corrupted =
            rebuild_with_replaced_command(&fixture.target, source_target, 0, changed_guard);

        assert_reconciliation_error(
            verify_constructed_control_recipes(
                &fixture.core,
                &fixture.plan,
                &corrupted,
                &fixture.construction,
            ),
            "branch guard lost its recorded semantic origin",
        );
    }

    #[test]
    fn rejects_a_constructed_recipe_with_a_changed_command_shape() {
        let fixture = selected_terminal_call_fixture();
        let source_target = source_target(&fixture);
        let commands = source_commands(&fixture);
        let fallback_target = tail_target(commands[1]).expect("fallback must be a tail call");
        let bare_call = function_call(fallback_target, commands[1].origin());
        let corrupted = rebuild_with_replaced_command(&fixture.target, source_target, 1, bare_call);

        assert_reconciliation_error(
            verify_constructed_control_recipes(
                &fixture.core,
                &fixture.plan,
                &corrupted,
                &fixture.construction,
            ),
            "recipe target is not `return run`",
        );
    }

    #[test]
    fn rejects_a_constructed_recipe_with_an_extra_zero_cost_node() {
        let fixture = selected_terminal_call_fixture();
        let commands = source_commands(&fixture);
        let recipe = selected_recipe(&fixture);
        let nested_return = CommandNode::new(
            CommandKind::Return(ReturnCommand::run(commands[1].clone())),
            commands[1].origin(),
        )
        .unwrap();

        assert_reconciliation_error(
            reconcile_selected_cost(commands[0], &nested_return, recipe),
            "recipe structure differs from its frozen prediction",
        );
    }

    #[test]
    fn rejects_actual_command_steps_that_disagree_with_the_selected_recipe() {
        let fixture = selected_terminal_call_fixture();
        let commands = source_commands(&fixture);
        let recipe = selected_recipe(&fixture);
        let CommandKind::Execute(execute) = commands[0].kind() else {
            panic!("fixture guard must be an execute command")
        };
        let [modifier] = execute.modifiers().as_slice() else {
            panic!("fixture guard must have one modifier")
        };
        let extra_guard = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(modifier.clone(), vec![]),
                commands[1].clone(),
            )),
            commands[1].origin(),
        )
        .unwrap();

        assert_reconciliation_error(
            reconcile_selected_cost(commands[0], &extra_guard, recipe),
            "command-step recount differs from its frozen prediction",
        );
    }

    fn selected_terminal_call_fixture() -> Fixture {
        let (core, dispatcher, source, wrong_origin) = terminal_call_core();
        let plan = selected_baseline_plan(&core);
        let (target, construction) = construct_program(&core, &plan).unwrap().into_parts();
        verify_constructed_control_recipes(&core, &plan, &target, &construction).unwrap();

        Fixture {
            core,
            plan,
            target,
            construction,
            dispatcher,
            source,
            wrong_origin,
        }
    }

    fn terminal_call_core() -> (CoreProgram, FunctionId, BlockId, OriginId) {
        let mut sources = SourceContext::new();
        let branch_origin = sources.add_origin(Origin::Unknown).unwrap();
        let call_origin = sources.add_origin(Origin::Unknown).unwrap();
        let terminal_return_origin = sources.add_origin(Origin::Unknown).unwrap();
        let wrong_origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let leaf = core
            .declare_function(Some("leaf"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let dispatcher = core
            .declare_function(
                Some("dispatcher"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut leaf_builder = FunctionBuilder::new(&core, &sources, leaf).unwrap();
        leaf_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                terminal_return_origin,
            ))
            .unwrap();
        core.define_function(leaf, leaf_builder.finish().unwrap())
            .unwrap();

        let mut builder = FunctionBuilder::new(&core, &sources, dispatcher).unwrap();
        let source = builder.entry_block();
        let condition = builder.body().block(source).unwrap().parameters()[0].value();
        let selected = builder.create_block(OriginId::UNKNOWN).unwrap();
        let fallback = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(selected, vec![]),
                    else_target: BlockTarget::new(fallback, vec![]),
                },
                branch_origin,
            ))
            .unwrap();
        builder.switch_to_block(selected).unwrap();
        builder.call(leaf, vec![], call_origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                terminal_return_origin,
            ))
            .unwrap();
        builder.switch_to_block(fallback).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(dispatcher, builder.finish().unwrap())
            .unwrap();

        (core, dispatcher, source, wrong_origin)
    }

    fn selected_baseline_plan(core: &CoreProgram) -> LoweringPlan {
        let options = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
        .with_optimization_level(MinecraftOptimizationLevel::Baseline);
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
        let demand = RuntimeDemand::for_level(
            core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
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
        let control = ControlRecipePlan::new(
            core,
            &inventory,
            &assignment,
            &transfers,
            MinecraftOptimizationLevel::Baseline,
        )
        .unwrap();
        let resources = ResourceInventory::for_control_plan(
            core, &inventory, &transfers, &control, &preflight, &options,
        )
        .unwrap();
        LoweringPlan::from_selected_parts(
            core,
            &options,
            preflight,
            ambient,
            assignment,
            physical,
            physical_preflight,
            transfers,
            control,
            resources,
        )
        .unwrap()
    }

    fn source_target(fixture: &Fixture) -> McFunctionId {
        let planned = fixture
            .plan
            .block_function(fixture.dispatcher, fixture.source)
            .expect("fixture source must be materialized");
        McFunctionId::from_index(planned.index())
    }

    fn source_commands(fixture: &Fixture) -> Vec<&CommandNode> {
        fixture
            .target
            .function(source_target(fixture))
            .unwrap()
            .body()
            .commands()
            .map(|(_, command)| command)
            .collect()
    }

    fn selected_recipe(fixture: &Fixture) -> InlineZeroAbiTerminalCall {
        let Some(BranchArmRecipe::InlineZeroAbiTerminalCall(recipe)) = fixture
            .plan
            .branch_arm_recipe(fixture.dispatcher, fixture.source, BranchArm::Then)
        else {
            panic!("fixture then arm must select the terminal-call recipe")
        };
        recipe
    }

    fn function_call(target: McFunctionId, origin: OriginId) -> CommandNode {
        CommandNode::new(
            CommandKind::Function(FunctionCall::new(
                InternalCallableRef::Function(target).into(),
            )),
            origin,
        )
        .unwrap()
    }

    fn tail_call(target: McFunctionId, origin: OriginId) -> CommandNode {
        CommandNode::new(
            CommandKind::Return(ReturnCommand::run(function_call(target, origin))),
            origin,
        )
        .unwrap()
    }

    fn retarget_conditional(command: &CommandNode, target: McFunctionId) -> CommandNode {
        let CommandKind::Execute(execute) = command.kind() else {
            panic!("fixture guard must be an execute command")
        };
        CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                execute.modifiers().clone(),
                tail_call(target, execute.run().origin()),
            )),
            command.origin(),
        )
        .unwrap()
    }

    fn rebuild_with_replaced_command(
        program: &MinecraftProgram,
        owner: McFunctionId,
        command_index: usize,
        replacement: CommandNode,
    ) -> MinecraftProgram {
        let mut builder = MinecraftProgramBuilder::new(program.target());
        for (function, declaration) in program.functions() {
            let rebuilt = builder
                .declare_function(declaration.resource().clone(), declaration.origin())
                .unwrap();
            assert_eq!(rebuilt, function);
        }
        for (tag, declaration) in program.function_tags() {
            let rebuilt = builder
                .declare_function_tag(
                    declaration.resource().clone(),
                    declaration.origin(),
                    declaration.merge(),
                )
                .unwrap();
            assert_eq!(rebuilt, tag);
        }

        let mut replacement = Some(replacement);
        for (function, declaration) in program.functions() {
            let mut body = builder.begin_function(function).unwrap();
            for (index, (_, command)) in declaration.body().commands().enumerate() {
                let command = if function == owner && index == command_index {
                    replacement.take().expect("replacement site must be unique")
                } else {
                    command.clone()
                };
                body.push(command).unwrap();
            }
            body.finish();
        }
        assert!(replacement.is_none(), "replacement site must exist");
        for (tag, declaration) in program.function_tags() {
            let mut rebuilt = builder.begin_function_tag(tag).unwrap();
            for entry in declaration.entries() {
                rebuilt.push(entry.clone());
            }
            rebuilt.finish();
        }
        builder.finish().unwrap()
    }

    fn assert_reconciliation_error(
        result: Result<(), crate::diagnostic::Diagnostics>,
        expected_message: &str,
    ) {
        let diagnostics = result.expect_err("corrupted recipe output must be rejected");
        assert_eq!(diagnostics.len(), 1, "{diagnostics}");
        assert!(
            diagnostics.findings()[0]
                .message()
                .contains(expected_message),
            "expected {expected_message:?}, got {diagnostics}"
        );
    }
}
