use crate::diagnostic::Diagnostics;
use crate::ir::core::{CoreType, FunctionId, InstId};
use crate::ir::minecraft::{
    CommandKind, DataCommand, DataModifyMode, DataSource, ExecuteCommand, ExecuteModifier,
    ExecuteModifierKind, ExecuteModifiers, FiniteF64, FunctionCall, InternalCallableRef, NbtValue,
    ScoreCommand, StoreChannel, StoreDestination,
};
use crate::source::OriginId;

use super::construct::{FunctionLoweringCx, command, invariant_diagnostics};
use super::plan::{CallResultDestination, HomeId, RecursiveSpill};
use super::scalar::copy_home;

#[cfg(test)]
pub(crate) fn lower_call(
    context: &mut FunctionLoweringCx<'_, '_>,
    callee: FunctionId,
    arguments: &[HomeId],
    results: &[HomeId],
    origin: OriginId,
) -> Result<(), Diagnostics> {
    lower_call_with_destinations(
        context,
        None,
        callee,
        arguments,
        &results.iter().copied().map(Some).collect::<Vec<_>>(),
        origin,
    )
}

/// Emits one call from its verified indexed physical plan.
pub(crate) fn lower_planned_call(
    context: &mut FunctionLoweringCx<'_, '_>,
    calling_function: FunctionId,
    instruction: InstId,
    called_function: FunctionId,
    arguments: &[HomeId],
    results: &[Option<CallResultDestination>],
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let destinations = results
        .iter()
        .enumerate()
        .map(|(index, destination)| {
            let destination = destination.as_ref().copied();
            if destination.is_some_and(|destination| destination.result_index() != index) {
                Err(invariant_diagnostics(
                    "planned call result index disagrees with its position",
                    origin,
                ))
            } else {
                Ok(destination.map(CallResultDestination::home))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    lower_call_with_destinations(
        context,
        Some((calling_function, instruction)),
        called_function,
        arguments,
        &destinations,
        origin,
    )
}

fn lower_call_with_destinations(
    context: &mut FunctionLoweringCx<'_, '_>,
    occurrence: Option<(FunctionId, InstId)>,
    called_function: FunctionId,
    arguments: &[HomeId],
    results: &[Option<HomeId>],
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let recursive = occurrence
        .is_some_and(|(caller, instruction)| context.plan().call_is_recursive(caller, instruction));
    let spill_homes = if recursive {
        let (calling_function, instruction) = occurrence.expect("recursive call has an occurrence");
        context
            .plan()
            .recursive_spill_homes(calling_function, instruction)
            .ok_or_else(|| {
                invariant_diagnostics("recursive call has no frozen spill plan", origin)
            })?
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if recursive {
        push_recursive_frame(context, origin)?;
        for spill in spill_homes.iter().copied() {
            store_home_in_frame(context, spill, origin)?;
        }
    }
    let parameter_count = context
        .plan()
        .function_parameters(called_function)
        .ok_or_else(|| invariant_diagnostics("call has no planned callee ABI", origin))?
        .len();
    let result_count = context
        .plan()
        .function_results(called_function)
        .ok_or_else(|| invariant_diagnostics("call has no planned result ABI", origin))?
        .len();
    if parameter_count != arguments.len() || result_count != results.len() {
        return Err(invariant_diagnostics(
            "verified call disagrees with the planned callee ABI",
            origin,
        ));
    }
    for (index, argument) in arguments.iter().copied().enumerate() {
        let parameter = context
            .plan()
            .function_parameters(called_function)
            .and_then(|parameters| parameters.get(index))
            .copied()
            .ok_or_else(|| invariant_diagnostics("planned call parameter disappeared", origin))?;
        context.require_same_type(parameter, argument)?;
        copy_home(context, parameter, argument, origin)?;
    }
    let entry = context
        .plan()
        .function_entry(called_function)
        .ok_or_else(|| invariant_diagnostics("callee has no planned entry function", origin))?;
    let target = context.function(entry)?;
    context.push(command(
        CommandKind::Function(FunctionCall::new(
            InternalCallableRef::Function(target).into(),
        )),
        origin,
    )?)?;
    if recursive {
        for spill in spill_homes.iter().copied() {
            load_home_from_frame(context, spill, origin)?;
        }
    }
    for (index, destination) in results.iter().copied().enumerate() {
        let Some(destination) = destination else {
            continue;
        };
        let result = context
            .plan()
            .function_results(called_function)
            .and_then(|results| results.get(index))
            .copied()
            .ok_or_else(|| invariant_diagnostics("planned call result disappeared", origin))?;
        context.require_same_type(destination, result)?;
        copy_home(context, destination, result, origin)?;
    }
    if recursive {
        context.push(command(
            CommandKind::Data(DataCommand::Remove {
                target: context.plan().activation_frame(),
            }),
            origin,
        )?)?;
    }
    Ok(())
}

fn push_recursive_frame(
    context: &mut FunctionLoweringCx<'_, '_>,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    let empty = NbtValue::compound(Vec::new()).map_err(|error| {
        invariant_diagnostics(
            format!("recursive frame literal is invalid: {error}"),
            origin,
        )
    })?;
    context.push(command(
        CommandKind::Data(DataCommand::Modify {
            target: context.plan().activation_frames(),
            mode: DataModifyMode::Append,
            source: DataSource::Value(empty),
        }),
        origin,
    )?)
}

fn store_home_in_frame(
    context: &mut FunctionLoweringCx<'_, '_>,
    spill: RecursiveSpill,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    if matches!(spill.ty(), CoreType::ListI32 | CoreType::String) {
        let source = match spill.ty() {
            CoreType::ListI32 => context.plan().list_storage(spill.home()),
            CoreType::String => context.plan().string_storage(spill.home()),
            CoreType::Bool | CoreType::I32 => None,
        };
        return context.push(command(
            CommandKind::Data(DataCommand::Modify {
                target: context
                    .plan()
                    .activation_frame_spill(spill.storage_ordinal()),
                mode: DataModifyMode::Set,
                source: DataSource::From(source.ok_or_else(|| {
                    invariant_diagnostics("recursive NBT spill has no storage", origin)
                })?),
            }),
            origin,
        )?);
    }
    let score = context.score(spill.home())?;
    let numeric_type = match spill.ty() {
        CoreType::Bool => crate::ir::minecraft::StorageNumericType::Byte,
        CoreType::I32 => crate::ir::minecraft::StorageNumericType::Int,
        CoreType::ListI32 | CoreType::String => unreachable!("NBT spills use direct copies"),
    };
    let get = command(
        CommandKind::Score(ScoreCommand::PlayersGet { score }),
        origin,
    )?;
    let modifier = ExecuteModifier::new(
        ExecuteModifierKind::Store(
            StoreChannel::Result,
            StoreDestination::Storage {
                target: context
                    .plan()
                    .activation_frame_spill(spill.storage_ordinal()),
                numeric_type,
                scale: FiniteF64::new(1.0).expect("one is finite"),
            },
        ),
        origin,
    );
    context.push(command(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(modifier, Vec::new()),
            get,
        )),
        origin,
    )?)
}

fn load_home_from_frame(
    context: &mut FunctionLoweringCx<'_, '_>,
    spill: RecursiveSpill,
    origin: OriginId,
) -> Result<(), Diagnostics> {
    if matches!(spill.ty(), CoreType::ListI32 | CoreType::String) {
        let target = match spill.ty() {
            CoreType::ListI32 => context.plan().list_storage(spill.home()),
            CoreType::String => context.plan().string_storage(spill.home()),
            CoreType::Bool | CoreType::I32 => None,
        };
        return context.push(command(
            CommandKind::Data(DataCommand::Modify {
                target: target.ok_or_else(|| {
                    invariant_diagnostics("recursive NBT spill has no storage", origin)
                })?,
                mode: DataModifyMode::Set,
                source: DataSource::From(
                    context
                        .plan()
                        .activation_frame_spill(spill.storage_ordinal()),
                ),
            }),
            origin,
        )?);
    }
    let get = command(
        CommandKind::Data(DataCommand::Get {
            source: context
                .plan()
                .activation_frame_spill(spill.storage_ordinal()),
            scale: None,
        }),
        origin,
    )?;
    let modifier = ExecuteModifier::new(
        ExecuteModifierKind::Store(
            StoreChannel::Result,
            StoreDestination::Score(context.score(spill.home())?),
        ),
        origin,
    );
    context.push(command(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(modifier, Vec::new()),
            get,
        )),
        origin,
    )?)
}

#[cfg(test)]
mod tests {
    use super::lower_call;
    use crate::ir::core::{CoreProgram, CoreType, FunctionBuilder, Terminator, TerminatorKind};
    use crate::ir::minecraft::{ObjectiveName, PackNamespace, render_function};
    use crate::lower::minecraft::LoweringOptions;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::construct::TargetConstruction;
    use crate::lower::minecraft::plan::PlanBuilder;
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the ABI golden keeps arguments, call, and ordered results together"
    )]
    fn forward_multi_argument_result_call_uses_fixed_slots() {
        let mut sources = SourceContext::new();
        let call_origin = sources.add_origin(Origin::Unknown).unwrap();
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
        let mut entry_body_builder = FunctionBuilder::new(&core, &sources, entry_function).unwrap();
        let integer = entry_body_builder.i32_constant(7, call_origin).unwrap();
        let boolean = entry_body_builder.bool_constant(true, call_origin).unwrap();
        let call_results = entry_body_builder
            .call(target_function, vec![integer, boolean], call_origin)
            .unwrap();
        entry_body_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(entry_function, entry_body_builder.finish().unwrap())
            .unwrap();
        let mut target_body_builder =
            FunctionBuilder::new(&core, &sources, target_function).unwrap();
        let parameters = target_body_builder
            .body()
            .block(target_body_builder.entry_block())
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect::<Vec<_>>();
        target_body_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![parameters[1], parameters[0]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(target_function, target_body_builder.finish().unwrap())
            .unwrap();
        let analyses = SemanticInventory::new(&core).unwrap();
        let mut builder = PlanBuilder::new(&core, options()).unwrap();
        builder
            .physicalize(
                &core,
                &analyses,
                &crate::lower::minecraft::demand::RuntimeDemand::optimization_disabled(),
            )
            .unwrap();
        let plan = builder.finish(&core, &analyses).unwrap();
        let entry_block = core
            .function(entry_function)
            .unwrap()
            .body()
            .unwrap()
            .entry();
        let target_block = core
            .function(target_function)
            .unwrap()
            .body()
            .unwrap()
            .entry();
        let entry_planned = plan.block_function(entry_function, entry_block).unwrap();
        let target_planned = plan.block_function(target_function, target_block).unwrap();
        let mut construction = TargetConstruction::declare(&core, &plan).unwrap();
        construction.define_initialization(&plan).unwrap();
        let mut context = construction
            .begin_function(
                core.function(entry_function).unwrap().body().unwrap(),
                &plan,
                entry_planned,
            )
            .unwrap();
        lower_call(
            &mut context,
            target_function,
            &[
                plan.value_home(entry_function, integer).unwrap(),
                plan.value_home(entry_function, boolean).unwrap(),
            ],
            &[
                plan.value_home(entry_function, call_results[0]).unwrap(),
                plan.value_home(entry_function, call_results[1]).unwrap(),
            ],
            call_origin,
        )
        .unwrap();
        context.finish();
        construction
            .begin_function(
                core.function(target_function).unwrap().body().unwrap(),
                &plan,
                target_planned,
            )
            .unwrap()
            .finish();
        let target = construction.declarations().function(entry_planned).unwrap();
        let program = construction.finish().unwrap();
        let function = program.function(target).unwrap();

        assert_eq!(
            String::from_utf8(render_function(&program, function).unwrap()).unwrap(),
            concat!(
                "scoreboard players operation #f1v0 mdl.reg = #f0v0 mdl.reg\n",
                "scoreboard players operation #f1v1 mdl.reg = #f0v1 mdl.reg\n",
                "function mdl:__mdl/f1/b0\n",
                "scoreboard players operation #f0v2 mdl.reg = #f1r0 mdl.reg\n",
                "scoreboard players operation #f0v3 mdl.reg = #f1r1 mdl.reg\n"
            )
        );
        assert!(
            function
                .body()
                .commands()
                .all(|(_, command)| command.origin() == call_origin)
        );
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the nested-call fixture constructs and lowers all three functions explicitly"
    )]
    fn zero_abi_nested_calls_preserve_each_call_origin() {
        let mut sources = SourceContext::new();
        let outer_origin = sources.add_origin(Origin::Unknown).unwrap();
        let relay_origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut core = CoreProgram::new();
        let outer = core
            .declare_function(Some("outer"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let relay = core
            .declare_function(Some("relay"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let terminal = core
            .declare_function(Some("terminal"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();

        let mut outer_builder = FunctionBuilder::new(&core, &sources, outer).unwrap();
        outer_builder.call(relay, vec![], outer_origin).unwrap();
        outer_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(outer, outer_builder.finish().unwrap())
            .unwrap();

        let mut relay_builder = FunctionBuilder::new(&core, &sources, relay).unwrap();
        relay_builder.call(terminal, vec![], relay_origin).unwrap();
        relay_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(relay, relay_builder.finish().unwrap())
            .unwrap();

        let mut terminal_builder = FunctionBuilder::new(&core, &sources, terminal).unwrap();
        terminal_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(terminal, terminal_builder.finish().unwrap())
            .unwrap();

        let analyses = SemanticInventory::new(&core).unwrap();
        let mut plan_builder = PlanBuilder::new(&core, options()).unwrap();
        plan_builder
            .physicalize(
                &core,
                &analyses,
                &crate::lower::minecraft::demand::RuntimeDemand::optimization_disabled(),
            )
            .unwrap();
        let plan = plan_builder.finish(&core, &analyses).unwrap();
        let outer_planned = plan.function_entry(outer).unwrap();
        let relay_planned = plan.function_entry(relay).unwrap();
        let terminal_planned = plan.function_entry(terminal).unwrap();
        let mut construction = TargetConstruction::declare(&core, &plan).unwrap();
        construction.define_initialization(&plan).unwrap();

        let mut outer_context = construction
            .begin_function(
                core.function(outer).unwrap().body().unwrap(),
                &plan,
                outer_planned,
            )
            .unwrap();
        lower_call(&mut outer_context, relay, &[], &[], outer_origin).unwrap();
        outer_context.finish();

        let mut relay_context = construction
            .begin_function(
                core.function(relay).unwrap().body().unwrap(),
                &plan,
                relay_planned,
            )
            .unwrap();
        lower_call(&mut relay_context, terminal, &[], &[], relay_origin).unwrap();
        relay_context.finish();
        construction
            .begin_function(
                core.function(terminal).unwrap().body().unwrap(),
                &plan,
                terminal_planned,
            )
            .unwrap()
            .finish();

        let outer_target = construction.declarations().function(outer_planned).unwrap();
        let relay_target = construction.declarations().function(relay_planned).unwrap();
        let program = construction.finish().unwrap();
        let outer_function = program.function(outer_target).unwrap();
        let relay_function = program.function(relay_target).unwrap();
        assert_eq!(
            String::from_utf8(render_function(&program, outer_function).unwrap()).unwrap(),
            "function mdl:__mdl/f1/b0\n"
        );
        assert_eq!(
            String::from_utf8(render_function(&program, relay_function).unwrap()).unwrap(),
            "function mdl:__mdl/f2/b0\n"
        );
        assert_eq!(
            outer_function.body().commands().next().unwrap().1.origin(),
            outer_origin
        );
        assert_eq!(
            relay_function.body().commands().next().unwrap().1.origin(),
            relay_origin
        );
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
