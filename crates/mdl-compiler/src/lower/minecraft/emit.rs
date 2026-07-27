use crate::diagnostic::Diagnostics;
use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{
    BlockId, CoreOp, CoreProgram, CoreType, ExternalSemanticBinding, FunctionId, InstData, InstId,
    Operand, TargetFragment, TerminatorKind, ValueId,
};
use crate::ir::minecraft::{
    AtMostOneSelector, CommandId, CommandKind, DataCommand, DataModifyMode, DataSource,
    ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers, FunctionCall,
    InternalCallableRef, ItemReplaceBlockCommand, McFunctionId, MinecraftProgram,
    NbtMatchValueKind, NbtPath, NbtPathKey, NbtPathSegment, NbtValue, ScheduleClearCommand,
    ScheduleCommand, Selector, StoreChannel, StoreDestination, SyntaxSlot, UnsafeRawCommand,
};
use crate::source::OriginId;

use super::call::lower_planned_call;
use super::construct::{FunctionLoweringCx, TargetConstruction, command, invariant_diagnostics};
use super::control::{lower_branch, lower_branch_helper, lower_jump, lower_return};
use super::plan::{
    BranchArm, EdgeTransfer, HomeId, InstructionPlan, LoweringPlan, PlannedFunctionId,
};
use super::scalar::{ScalarLowering, lower_scalar_operation};

/// Exact generated command produced for one Core semantic-operation occurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConstructedCommandLocation {
    function: McFunctionId,
    command: CommandId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConstructedRunModifierLocation {
    pub(crate) function: McFunctionId,
    pub(crate) command: CommandId,
    pub(crate) modifier_index: usize,
    pub(crate) recipe: super::RunModifierRecipeId,
}

impl ConstructedCommandLocation {
    pub(crate) const fn function(self) -> McFunctionId {
        self.function
    }

    pub(crate) const fn command(self) -> CommandId {
        self.command
    }
}

/// Dense post-construction correlation, indexed by Core function and instruction.
#[derive(Debug)]
pub(crate) struct ConstructionMap {
    functions: EntityVec<FunctionId, Box<[Option<ConstructedCommandLocation>]>>,
    run_modifiers:
        EntityVec<crate::ir::core::RunScopeId, Box<[Option<ConstructedRunModifierLocation>]>>,
}

impl ConstructionMap {
    fn new(core: &CoreProgram) -> Result<Self, Diagnostics> {
        let mut functions = EntityVec::new();
        for (function, declaration) in core.functions() {
            let body = declaration.body().ok_or_else(|| {
                invariant_diagnostics(
                    "planned function definition disappeared",
                    declaration.origin(),
                )
            })?;
            let slots = vec![None; body.instruction_counts().allocated].into_boxed_slice();
            let mapped = functions.push(slots).map_err(|EntityLimitError| {
                invariant_diagnostics(
                    "post-construction correlation identity space is exhausted",
                    declaration.origin(),
                )
            })?;
            if mapped != function {
                return Err(invariant_diagnostics(
                    "post-construction correlation is not dense over Core functions",
                    declaration.origin(),
                ));
            }
        }
        let mut run_modifiers = EntityVec::new();
        for (scope, declaration) in core.run_scopes() {
            let allocated = run_modifiers
                .push(vec![None; declaration.modifiers().len()].into_boxed_slice())
                .map_err(|EntityLimitError| {
                    invariant_diagnostics(
                        "construction run-modifier map identity space exhausted",
                        declaration.origin(),
                    )
                })?;
            if allocated != scope {
                return Err(invariant_diagnostics(
                    "construction run-modifier map is not dense",
                    declaration.origin(),
                ));
            }
        }
        Ok(Self {
            functions,
            run_modifiers,
        })
    }

    fn record(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        location: ConstructedCommandLocation,
        origin: OriginId,
    ) -> Result<(), Diagnostics> {
        let slot = usize::try_from(instruction.index())
            .ok()
            .and_then(|index| self.functions.get_mut(function)?.get_mut(index))
            .ok_or_else(|| {
                invariant_diagnostics(
                    "constructed semantic command has no dense Core correlation slot",
                    origin,
                )
            })?;
        if slot.replace(location).is_some() {
            return Err(invariant_diagnostics(
                "one Core semantic occurrence produced multiple top-level commands",
                origin,
            ));
        }
        Ok(())
    }

    pub(crate) fn location(
        &self,
        function: FunctionId,
        instruction: InstId,
    ) -> Option<ConstructedCommandLocation> {
        usize::try_from(instruction.index())
            .ok()
            .and_then(|index| self.functions.get(function)?.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn function_slots(
        &self,
        function: FunctionId,
    ) -> Option<&[Option<ConstructedCommandLocation>]> {
        self.functions.get(function).map(Box::as_ref)
    }

    pub(crate) fn run_scopes(
        &self,
    ) -> impl ExactSizeIterator<
        Item = (
            crate::ir::core::RunScopeId,
            &[Option<ConstructedRunModifierLocation>],
        ),
    > + '_ {
        self.run_modifiers
            .iter()
            .map(|(scope, slots)| (scope, slots.as_ref()))
    }

    fn record_run_modifiers(
        &mut self,
        scope: crate::ir::core::RunScopeId,
        values: Vec<ConstructedRunModifierLocation>,
        origin: OriginId,
    ) -> Result<(), Diagnostics> {
        let slots = self
            .run_modifiers
            .get_mut(scope)
            .ok_or_else(|| invariant_diagnostics("construction run scope is absent", origin))?;
        if slots.len() != values.len() || slots.iter().any(Option::is_some) {
            return Err(invariant_diagnostics(
                "construction run-modifier correlation shape mismatch",
                origin,
            ));
        }
        for (slot, value) in slots.iter_mut().zip(values) {
            *slot = Some(value);
        }
        Ok(())
    }
}

/// Complete construction product. No partial target or correlation escapes failure.
#[derive(Debug)]
pub(crate) struct ConstructedProgram {
    program: MinecraftProgram,
    commands: ConstructionMap,
}

impl ConstructedProgram {
    pub(crate) fn into_parts(self) -> (MinecraftProgram, ConstructionMap) {
        (self.program, self.commands)
    }
}

/// Constructs every frozen block and helper, returning no partial target on failure.
pub(crate) fn construct_program(
    core: &CoreProgram,
    plan: &LoweringPlan,
) -> Result<ConstructedProgram, Diagnostics> {
    let mut target = TargetConstruction::declare(core, plan)?;
    let mut commands = ConstructionMap::new(core)?;
    target.define_initialization(plan)?;
    define_external_helpers(&mut target, core, plan, &mut commands)?;
    for (function, declaration) in core.functions() {
        let body = declaration.body().ok_or_else(|| {
            invariant_diagnostics(
                "planned function definition disappeared",
                declaration.origin(),
            )
        })?;
        lower_blocks(&mut target, function, body, plan, &mut commands)?;
        lower_branch_helpers(&mut target, function, body, plan)?;
    }
    Ok(ConstructedProgram {
        program: target.finish()?,
        commands,
    })
}

fn lower_blocks(
    target: &mut TargetConstruction,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
    commands: &mut ConstructionMap,
) -> Result<(), Diagnostics> {
    for block in body.block_order().iter().copied() {
        if let Some(planned) = plan.block_function(function, block) {
            lower_block(target, function, block, body, plan, planned, commands)?;
        }
    }
    Ok(())
}

fn lower_block(
    target: &mut TargetConstruction,
    function: FunctionId,
    block: BlockId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
    planned: super::plan::PlannedFunctionId,
    commands: &mut ConstructionMap,
) -> Result<(), Diagnostics> {
    let target_function = target.function(planned)?;
    let mut context = target.begin_function(body, plan, planned)?;
    let data = body.block(block).ok_or_else(|| {
        invariant_diagnostics("planned Core block disappeared", OriginId::UNKNOWN)
    })?;
    for instruction in data.instructions().iter().copied() {
        if let Some(command) = lower_instruction(
            &mut context,
            function,
            instruction,
            body,
            plan,
            data.origin(),
        )? {
            let origin = body
                .instruction(instruction)
                .map_or(data.origin(), InstData::origin);
            commands.record(
                function,
                instruction,
                ConstructedCommandLocation {
                    function: target_function,
                    command,
                },
                origin,
            )?;
        }
    }
    lower_terminator(&mut context, function, block, data, plan)?;
    context.finish();
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "construction exhaustively consumes the closed frozen instruction-plan vocabulary"
)]
fn lower_instruction(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    instruction: InstId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
    block_origin: OriginId,
) -> Result<Option<CommandId>, Diagnostics> {
    let data = body.instruction(instruction).ok_or_else(|| {
        invariant_diagnostics("planned Core instruction disappeared", block_origin)
    })?;
    let instruction_plan = plan
        .instruction_plan(function, instruction)
        .ok_or_else(|| {
            invariant_diagnostics(
                "materialized Core instruction has no frozen instruction plan",
                data.origin(),
            )
        })?;
    match instruction_plan {
        InstructionPlan::OmittedPure => Ok(None),
        InstructionPlan::External { helper } => {
            let CoreOp::External(external) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-external instruction has an external physical plan",
                    data.origin(),
                ));
            };
            let target = context.function(*helper)?;
            if let Some(recipe) = plan.selected_semantic_recipe(*external) {
                if recipe.is_unusable_inline() {
                    let args = super::crossings::seed_macro_frame(context, data.origin())?;
                    let base_cmd = recipe.command_kind();
                    let operands = super::crossings::collect_runtime_operands(&base_cmd);
                    if let Some(frame) = super::crossings::build_frame(&operands) {
                        super::crossings::emit_bridges(
                            context,
                            plan,
                            function,
                            &frame,
                            data.origin(),
                        )?;
                    }
                    super::crossings::push_macro_call(context, target, args, data.origin())?;
                    return Ok(None);
                }
            } else if let Some(resolved) = plan.preflight().selected_entity_nbt_read(*external) {
                if resolved.is_unusable_inline() {
                    let args = super::crossings::seed_macro_frame(context, data.origin())?;
                    let operands = entity_nbt_runtime_operands(resolved);
                    if let Some(frame) = super::crossings::build_frame(&operands) {
                        super::crossings::emit_bridges(
                            context,
                            plan,
                            function,
                            &frame,
                            data.origin(),
                        )?;
                    }
                    super::crossings::push_macro_call(context, target, args, data.origin())?;
                    return Ok(None);
                }
            } else if let Some(resolved) = plan.preflight().selected_entity_nbt_write(*external) {
                if resolved.is_unusable_inline() {
                    let args = super::crossings::seed_macro_frame(context, data.origin())?;
                    let operands = entity_nbt_write_runtime_operands(resolved);
                    if let Some(frame) = super::crossings::build_frame(&operands) {
                        super::crossings::emit_bridges(
                            context,
                            plan,
                            function,
                            &frame,
                            data.origin(),
                        )?;
                    }
                    super::crossings::push_macro_call(context, target, args, data.origin())?;
                    return Ok(None);
                }
            }
            context.push(command(
                CommandKind::Function(FunctionCall::new(
                    InternalCallableRef::Function(target).into(),
                )),
                data.origin(),
            )?)?;
            Ok(None)
        }
        InstructionPlan::Minecraft {
            external, recipe, ..
        } => {
            let CoreOp::External(actual) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-external instruction has a Minecraft command plan",
                    data.origin(),
                ));
            };
            if actual != external {
                return Err(invariant_diagnostics(
                    "Minecraft command plan names the wrong external declaration",
                    data.origin(),
                ));
            }
            let selected = plan.selected_semantic_recipe(*external).ok_or_else(|| {
                invariant_diagnostics(
                    "Minecraft command plan has no retained preflight recipe",
                    data.origin(),
                )
            })?;
            if selected.recipe_id() != *recipe {
                return Err(invariant_diagnostics(
                    "Minecraft command plan disagrees with retained preflight",
                    data.origin(),
                ));
            }
            context
                .push_correlated(command(selected.command_kind(), data.origin())?)
                .map(Some)
        }
        InstructionPlan::EntityNbtRead { external, results } => {
            let CoreOp::External(actual) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-external instruction has an entity-NBT read physical plan",
                    data.origin(),
                ));
            };
            if actual != external {
                return Err(invariant_diagnostics(
                    "entity-NBT read plan names the wrong external declaration",
                    data.origin(),
                ));
            }
            let resolved = plan
                .preflight()
                .selected_entity_nbt_read(*external)
                .ok_or_else(|| {
                    invariant_diagnostics(
                        "entity-NBT read plan has no retained preflight resolution",
                        data.origin(),
                    )
                })?;
            let [result] = results.as_ref() else {
                return Err(invariant_diagnostics(
                    "entity-NBT read requires exactly one result home",
                    data.origin(),
                ));
            };
            let path = entity_nbt_path_from_segments(resolved.segments(), data.origin())?;
            let read_source = entity_nbt_read_source(resolved, path);
            emit_entity_nbt_read_result(context, result.home(), read_source, data.origin())
        }
        InstructionPlan::EntityNbtWrite { external, results } => {
            let CoreOp::External(actual) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-external instruction has an entity-NBT write physical plan",
                    data.origin(),
                ));
            };
            if actual != external {
                return Err(invariant_diagnostics(
                    "entity-NBT write plan names the wrong external declaration",
                    data.origin(),
                ));
            }
            if !results.is_empty() {
                return Err(invariant_diagnostics(
                    "entity-NBT write plan unexpectedly demands a result",
                    data.origin(),
                ));
            }
            let resolved = plan
                .preflight()
                .selected_entity_nbt_write(*external)
                .ok_or_else(|| {
                    invariant_diagnostics(
                        "entity-NBT write plan has no retained preflight resolution",
                        data.origin(),
                    )
                })?;
            let write_command = item_replace_block_command(resolved, data.origin())?;
            context
                .push_correlated(command(
                    CommandKind::ItemReplaceBlock(write_command),
                    data.origin(),
                )?)
                .map(Some)
        }
        InstructionPlan::Scalar { operands, results } => {
            if matches!(data.op(), CoreOp::Call(_)) {
                return Err(invariant_diagnostics(
                    "call instruction has a scalar physical plan",
                    data.origin(),
                ));
            }
            let result_homes = results
                .iter()
                .copied()
                .map(super::plan::ScalarResultPlacement::home)
                .collect::<Vec<_>>();
            if lower_scalar_operation(context, data.op(), operands, &result_homes, data.origin())?
                == ScalarLowering::Lowered
            {
                Ok(None)
            } else {
                Err(invariant_diagnostics(
                    "scalar physical plan was deferred as a call",
                    data.origin(),
                ))
            }
        }
        InstructionPlan::Call {
            arguments,
            result_destinations,
        } => {
            let CoreOp::Call(callee) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-call instruction has a call physical plan",
                    data.origin(),
                ));
            };
            lower_planned_call(
                context,
                function,
                instruction,
                *callee,
                arguments,
                result_destinations,
                data.origin(),
            )?;
            Ok(None)
        }
        InstructionPlan::Schedule => {
            let CoreOp::Schedule(callee, delay_ticks, mode) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-schedule instruction has a schedule physical plan",
                    data.origin(),
                ));
            };
            let entry = context.plan().function_entry(*callee).ok_or_else(|| {
                invariant_diagnostics(
                    "schedule target has no planned entry function",
                    data.origin(),
                )
            })?;
            let target = context.function(entry)?;
            context.push(command(
                CommandKind::Schedule(ScheduleCommand::new(target, *delay_ticks, *mode)),
                data.origin(),
            )?)?;
            Ok(None)
        }
        InstructionPlan::ScheduleClear => {
            let CoreOp::ScheduleClear(callee) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-schedule-clear instruction has a schedule-clear physical plan",
                    data.origin(),
                ));
            };
            let entry = context.plan().function_entry(*callee).ok_or_else(|| {
                invariant_diagnostics(
                    "schedule-clear target has no planned entry function",
                    data.origin(),
                )
            })?;
            let target = context.function(entry)?;
            context.push(command(
                CommandKind::ScheduleClear(ScheduleClearCommand::new(target)),
                data.origin(),
            )?)?;
            Ok(None)
        }
    }
}

fn define_external_helpers(
    target: &mut TargetConstruction,
    core: &CoreProgram,
    plan: &LoweringPlan,
    commands: &mut ConstructionMap,
) -> Result<(), Diagnostics> {
    for (function, declaration) in core.functions() {
        let body = declaration.body().ok_or_else(|| {
            invariant_diagnostics(
                "planned function definition disappeared",
                declaration.origin(),
            )
        })?;
        for block in body.block_order().iter().copied() {
            let Some(block) = body.block(block) else {
                continue;
            };
            for instruction in block.instructions().iter().copied() {
                let Some(InstructionPlan::External { helper }) =
                    plan.instruction_plan(function, instruction)
                else {
                    continue;
                };
                let data = body.instruction(instruction).ok_or_else(|| {
                    invariant_diagnostics(
                        "planned external instruction disappeared",
                        block.origin(),
                    )
                })?;
                let CoreOp::External(operation) = data.op() else {
                    return Err(invariant_diagnostics(
                        "external helper belongs to a non-external instruction",
                        data.origin(),
                    ));
                };
                define_external_helper(
                    target, core, plan, function, *helper, *operation, data, commands,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "each argument is a disjoint piece of the construction/plan context the closed binding vocabulary needs"
)]
#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive match keeps every closed external-binding helper construction together"
)]
fn define_external_helper(
    target: &mut TargetConstruction,
    core: &CoreProgram,
    plan: &LoweringPlan,
    function: FunctionId,
    helper: PlannedFunctionId,
    operation: crate::ir::core::ExternalOpId,
    data: &InstData,
    commands: &mut ConstructionMap,
) -> Result<(), Diagnostics> {
    let declaration = core.external_op(operation).ok_or_else(|| {
        invariant_diagnostics("planned external operation disappeared", data.origin())
    })?;
    match declaration.binding() {
        ExternalSemanticBinding::UnsafeTargetFragment(fragment) => {
            let Some(TargetFragment::UnsafeMinecraftCommand(fragment)) =
                core.target_fragment(fragment)
            else {
                return Err(invariant_diagnostics(
                    "planned unsafe target fragment disappeared",
                    data.origin(),
                ));
            };
            let raw = UnsafeRawCommand::new_for_target(fragment.as_str(), plan.target()).map_err(
                |error| {
                    invariant_diagnostics(
                        format!("audited unsafe target fragment became invalid: {error}"),
                        data.origin(),
                    )
                },
            )?;
            target.define_external_helper(helper, command(CommandKind::Raw(raw), data.origin())?)
        }
        ExternalSemanticBinding::MinecraftRunScope(scope) => {
            define_run_scope_helper(target, core, plan, helper, scope, data, commands)
        }
        ExternalSemanticBinding::MinecraftOperation(_) => {
            let recipe = plan.selected_semantic_recipe(operation).ok_or_else(|| {
                invariant_diagnostics(
                    "typed Minecraft operation helper has no retained preflight recipe",
                    data.origin(),
                )
            })?;
            if !recipe.is_unusable_inline() {
                return Err(invariant_diagnostics(
                    "typed Minecraft operation incorrectly received an external helper",
                    data.origin(),
                ));
            }
            let base_cmd = retarget_to_result_home(recipe.command_kind(), function, data, plan)?;
            let operands = super::crossings::collect_runtime_operands(&base_cmd);
            let frame = super::crossings::build_frame(&operands)
                .expect("unusable-inline recipe must have runtime operands");
            let macro_cmd = super::crossings::render_as_macro(&base_cmd, &frame, data.origin());
            let body = command(CommandKind::Macro(macro_cmd), data.origin())?;
            target.define_external_helper(helper, body)
        }
        ExternalSemanticBinding::EntityNbtRead(_) => {
            let resolved = plan
                .preflight()
                .selected_entity_nbt_read(operation)
                .ok_or_else(|| {
                    invariant_diagnostics(
                        "entity-NBT read helper has no retained preflight resolution",
                        data.origin(),
                    )
                })?;
            if !resolved.is_unusable_inline() {
                return Err(invariant_diagnostics(
                    "all-constant entity-NBT read incorrectly received an external helper",
                    data.origin(),
                ));
            }
            let [result] = data.results() else {
                return Err(invariant_diagnostics(
                    "macro-routed entity-NBT read requires exactly one result",
                    data.origin(),
                ));
            };
            let home = plan.value_home(function, *result).ok_or_else(|| {
                invariant_diagnostics(
                    "macro-routed entity-NBT read result has no planned home",
                    data.origin(),
                )
            })?;
            let macro_cmd = match plan.home_type(home) {
                Some(CoreType::String) => {
                    let result_target = plan.string_storage(home).ok_or_else(|| {
                        invariant_diagnostics(
                            "macro-routed entity-NBT read result has no string storage",
                            data.origin(),
                        )
                    })?;
                    let read_path =
                        entity_nbt_path_from_segments(resolved.segments(), data.origin())?;
                    let base_cmd = CommandKind::Data(DataCommand::Modify {
                        target: result_target,
                        mode: DataModifyMode::Set,
                        source: entity_nbt_read_source(resolved, read_path),
                    });
                    let operands = super::crossings::collect_runtime_operands(&base_cmd);
                    let frame = super::crossings::build_frame(&operands)
                        .expect("unusable-inline entity-NBT read must have runtime operands");
                    super::crossings::render_as_macro(&base_cmd, &frame, data.origin())
                }
                Some(ty @ (CoreType::Bool | CoreType::I32)) => {
                    // BE-1 Slice 2: a runtime-matched container slot (`.count`
                    // etc.) reaches here — mirrors `emit_entity_nbt_read_result`'s
                    // inline scratch/score conversion, one level up across the
                    // macro-helper boundary instead of within one function.
                    let score = plan.score(home).ok_or_else(|| {
                        invariant_diagnostics(
                            "macro-routed entity-NBT scalar read result has no score home",
                            data.origin(),
                        )
                    })?;
                    let scratch = plan.entity_nbt_scalar_scratch();
                    let default = if ty == CoreType::Bool {
                        NbtValue::byte(0)
                    } else {
                        NbtValue::int(0)
                    };
                    let read_path =
                        entity_nbt_path_from_segments(resolved.segments(), data.origin())?;
                    let read_source = entity_nbt_read_source(resolved, read_path);
                    let operands = entity_nbt_runtime_operands(resolved);
                    let frame = super::crossings::build_frame(&operands)
                        .expect("unusable-inline entity-NBT read must have runtime operands");
                    super::crossings::render_entity_nbt_scalar_read_as_macro(
                        &scratch,
                        &read_source,
                        &default,
                        &score,
                        &frame,
                        data.origin(),
                    )
                }
                Some(CoreType::ListI32) | None => {
                    return Err(invariant_diagnostics(
                        "macro-routed entity-NBT read result has an unsupported physical type",
                        data.origin(),
                    ));
                }
            };
            let body = command(CommandKind::Macro(macro_cmd), data.origin())?;
            target.define_external_helper(helper, body)
        }
        ExternalSemanticBinding::EntityNbtWrite(_) => {
            let resolved = plan
                .preflight()
                .selected_entity_nbt_write(operation)
                .ok_or_else(|| {
                    invariant_diagnostics(
                        "entity-NBT write helper has no retained preflight resolution",
                        data.origin(),
                    )
                })?;
            if !resolved.is_unusable_inline() {
                return Err(invariant_diagnostics(
                    "all-constant entity-NBT write incorrectly received an external helper",
                    data.origin(),
                ));
            }
            let base_cmd =
                CommandKind::ItemReplaceBlock(item_replace_block_command(resolved, data.origin())?);
            let operands = super::crossings::collect_runtime_operands(&base_cmd);
            let frame = super::crossings::build_frame(&operands)
                .expect("unusable-inline entity-NBT write must have runtime operands");
            let macro_cmd = super::crossings::render_as_macro(&base_cmd, &frame, data.origin());
            let body = command(CommandKind::Macro(macro_cmd), data.origin())?;
            target.define_external_helper(helper, body)
        }
    }
}

/// Redirects a macro-routed data-modify recipe's target from the recipe's
/// placeholder scratch path to the instruction's real, per-occurrence result
/// home, so the macro helper's read reaches the caller instead of a scratch
/// storage path nothing else ever consumes.
fn retarget_to_result_home(
    base_cmd: CommandKind,
    function: FunctionId,
    data: &InstData,
    plan: &LoweringPlan,
) -> Result<CommandKind, Diagnostics> {
    let CommandKind::Data(DataCommand::Modify { mode, source, .. }) = base_cmd else {
        return Err(invariant_diagnostics(
            "macro-routed external helper base command is not a data-modify command",
            data.origin(),
        ));
    };
    let [result] = data.results() else {
        return Err(invariant_diagnostics(
            "macro-routed external read requires exactly one result",
            data.origin(),
        ));
    };
    let home = plan.value_home(function, *result).ok_or_else(|| {
        invariant_diagnostics(
            "macro-routed external result has no planned home",
            data.origin(),
        )
    })?;
    let target = plan.string_storage(home).ok_or_else(|| {
        invariant_diagnostics(
            "macro-routed external result has no string storage",
            data.origin(),
        )
    })?;
    Ok(CommandKind::Data(DataCommand::Modify {
        target,
        mode,
        source,
    }))
}

/// Extracts the runtime operands of a macro-routed entity-NBT read directly
/// from its resolved segments — the caller-side mirror of
/// `crossings::collect_runtime_operands`, which instead walks a constructed
/// `CommandKind`. There is no base command to walk yet at the call site (only
/// the helper body, built separately in `define_external_helper`, has one),
/// so this reads the same information straight from the resolved read.
fn entity_nbt_runtime_operands(
    resolved: &super::preflight::ResolvedEntityNbtRead,
) -> Vec<super::crossings::RuntimeOperand> {
    entity_nbt_segment_runtime_operands(resolved.segments())
}

/// Same rationale as `entity_nbt_runtime_operands`, for a resolved whole-slot
/// write (PS-16, BE-2) — additionally includes `item_id`/`count` when either
/// is runtime, in the same order `EntityNbtWriteDecl::runtime_operand_types`
/// declared them.
fn entity_nbt_write_runtime_operands(
    resolved: &super::preflight::ResolvedEntityNbtWrite,
) -> Vec<super::crossings::RuntimeOperand> {
    let mut operands = entity_nbt_segment_runtime_operands(resolved.segments());
    if let Operand::Runtime(value_id) = resolved.item_id() {
        operands.push(super::crossings::RuntimeOperand {
            value_id: *value_id,
            slot: SyntaxSlot::ResourceId,
        });
    }
    if let Operand::Runtime(value_id) = resolved.count() {
        operands.push(super::crossings::RuntimeOperand {
            value_id,
            slot: SyntaxSlot::Int,
        });
    }
    operands
}

/// Shared by `entity_nbt_runtime_operands`/`entity_nbt_write_runtime_operands`.
fn entity_nbt_segment_runtime_operands(
    segments: &[super::preflight::ResolvedEntityNbtSegment],
) -> Vec<super::crossings::RuntimeOperand> {
    segments
        .iter()
        .filter_map(|segment| match segment {
            super::preflight::ResolvedEntityNbtSegment::Index(Operand::Runtime(value_id))
            | super::preflight::ResolvedEntityNbtSegment::Match {
                value: Operand::Runtime(value_id),
                ..
            } => Some(super::crossings::RuntimeOperand {
                value_id: *value_id,
                slot: SyntaxSlot::NbtIndex,
            }),
            super::preflight::ResolvedEntityNbtSegment::Key(_)
            | super::preflight::ResolvedEntityNbtSegment::Index(Operand::Const(_))
            | super::preflight::ResolvedEntityNbtSegment::Match {
                value: Operand::Const(_),
                ..
            } => None,
        })
        .collect()
}

/// Builds the real `NbtPath` from a fully resolved (no more `Operand::Runtime`
/// placeholders) entity-NBT read's segments. Shared by both the inline
/// (all-`Const`, `emit_entity_nbt_read_result`) and macro-helper
/// (≥1 `Runtime`, `define_external_helper`) routes — `NbtPathSegment::Index`
/// already natively carries `Operand<i32>`, so there is nothing route-specific
/// to decide here; which route a read takes was already fixed at plan time.
pub(crate) fn entity_nbt_path_from_segments(
    segments: &[super::preflight::ResolvedEntityNbtSegment],
    origin: OriginId,
) -> Result<NbtPath, Diagnostics> {
    use super::preflight::ResolvedEntityNbtSegment;

    let mut iter = segments.iter();
    let Some(ResolvedEntityNbtSegment::Key(first_key)) = iter.next() else {
        return Err(invariant_diagnostics(
            "entity-NBT read path is empty or does not start with a key",
            origin,
        ));
    };
    let root = NbtPathKey::new(first_key).map_err(|_| {
        invariant_diagnostics("entity-NBT read path has an invalid root key", origin)
    })?;
    let mut rest = Vec::with_capacity(segments.len().saturating_sub(1));
    for segment in iter {
        let core_segment = match segment {
            ResolvedEntityNbtSegment::Key(key) => {
                NbtPathSegment::Key(NbtPathKey::new(key).map_err(|_| {
                    invariant_diagnostics("entity-NBT read path has an invalid key", origin)
                })?)
            }
            ResolvedEntityNbtSegment::Index(operand) => NbtPathSegment::Index(*operand),
            ResolvedEntityNbtSegment::Match { match_key, value } => NbtPathSegment::Match {
                key: NbtPathKey::new(match_key).map_err(|_| {
                    invariant_diagnostics("entity-NBT read path has an invalid match key", origin)
                })?,
                value: *value,
                value_kind: match_value_kind(match_key),
            },
        };
        rest.push(core_segment);
    }
    Ok(NbtPath::new(NbtPathSegment::Key(root), rest))
}

/// The real NBT primitive type tag for one schema-known match key — a
/// closed table mirroring the schema itself (`entity_schema.rs` never
/// registers a `MatchList` whose `match_key` isn't listed here). Currently
/// exactly one: a chest's `Slot` field is measured (against the real pinned
/// server, not assumed) to be a `Byte`, and Minecraft's compound-match NBT
/// syntax requires the match value's type tag to agree exactly or the match
/// silently finds nothing.
fn match_value_kind(match_key: &str) -> NbtMatchValueKind {
    match match_key {
        "Slot" => NbtMatchValueKind::Byte,
        _ => NbtMatchValueKind::Int32,
    }
}

/// Builds the real `DataSource` for a resolved entity-NBT read, branching on
/// its receiver (BE-1): an entity receiver always reads via the ambient
/// self-executor selector (unchanged from PS-12 — the op always executes
/// inside its caller's already-`as`'d context); a block receiver reads via
/// its absolute position directly, with no selector at all.
fn entity_nbt_read_source(
    resolved: &super::preflight::ResolvedEntityNbtRead,
    path: NbtPath,
) -> DataSource {
    match resolved.receiver() {
        crate::ir::core::EntityNbtReceiver::Entity(_) => DataSource::Entity {
            selector: Selector::from(AtMostOneSelector::SelfExecutor),
            path,
        },
        crate::ir::core::EntityNbtReceiver::Block(_, position) => {
            DataSource::Block { position, path }
        }
    }
}

/// Builds the real `ItemReplaceBlockCommand` for a resolved whole-slot
/// entity-NBT write (PS-16, BE-2). The written slot is the value carried by
/// the resolved path's final `Match`/`Index` segment — `container.<slot>` is
/// a Brigadier `slot` command argument (a plain decimal integer), not an NBT
/// path match, so unlike the read side's `[{Slot:Nb}]` rendering there is no
/// byte-suffix concern here at all.
fn item_replace_block_command(
    resolved: &super::preflight::ResolvedEntityNbtWrite,
    origin: OriginId,
) -> Result<ItemReplaceBlockCommand, Diagnostics> {
    let crate::ir::core::EntityNbtReceiver::Block(_, position) = resolved.receiver() else {
        return Err(invariant_diagnostics(
            "entity-NBT write has a non-block receiver",
            origin,
        ));
    };
    let Some(
        super::preflight::ResolvedEntityNbtSegment::Match { value: slot, .. }
        | super::preflight::ResolvedEntityNbtSegment::Index(slot),
    ) = resolved.segments().last()
    else {
        return Err(invariant_diagnostics(
            "entity-NBT write path does not end in a container index",
            origin,
        ));
    };
    Ok(ItemReplaceBlockCommand::new(
        position,
        *slot,
        resolved.item_id().clone(),
        resolved.count(),
    ))
}

/// Emits the fail-soft two-command read (type-appropriate default, then
/// attempt) for one entity-NBT read result, generalizing the retired
/// `BookPage` static arm's exact shape to any schema scalar type. `String`
/// results write directly to their NBT string home; `Bool`/`I32` results
/// (score-based homes, per PS-11's representation-selection: Minecraft
/// scoreboards only hold integers) go through a shared scratch NBT slot and
/// an `execute store result score … run data get storage …` conversion,
/// since `data get` has no entity-source form in this IR yet.
fn emit_entity_nbt_read_result(
    context: &mut FunctionLoweringCx<'_, '_>,
    home: HomeId,
    read_source: DataSource,
    origin: OriginId,
) -> Result<Option<CommandId>, Diagnostics> {
    match context.plan().home_type(home) {
        Some(CoreType::String) => {
            let target = context.plan().string_storage(home).ok_or_else(|| {
                invariant_diagnostics("entity-NBT string result has no string storage", origin)
            })?;
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: target.clone(),
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(NbtValue::string("")),
                }),
                origin,
            )?)?;
            let read = CommandKind::Data(DataCommand::Modify {
                target,
                mode: DataModifyMode::Set,
                source: read_source,
            });
            context.push_correlated(command(read, origin)?).map(Some)
        }
        Some(ty @ (CoreType::Bool | CoreType::I32)) => {
            let score = context.score(home)?;
            let scratch = context.plan().entity_nbt_scalar_scratch();
            let default = if ty == CoreType::Bool {
                NbtValue::byte(0)
            } else {
                NbtValue::int(0)
            };
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: scratch.clone(),
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(default),
                }),
                origin,
            )?)?;
            context.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: scratch.clone(),
                    mode: DataModifyMode::Set,
                    source: read_source,
                }),
                origin,
            )?)?;
            let store_mod = ExecuteModifier::new(
                ExecuteModifierKind::Store(StoreChannel::Result, StoreDestination::Score(score)),
                origin,
            );
            let get_cmd = command(
                CommandKind::Data(DataCommand::Get {
                    source: scratch,
                    scale: None,
                }),
                origin,
            )?;
            let execute = CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(store_mod, vec![]),
                get_cmd,
            ));
            context.push_correlated(command(execute, origin)?).map(Some)
        }
        Some(CoreType::ListI32) | None => Err(invariant_diagnostics(
            "entity-NBT read result has an unsupported physical type",
            origin,
        )),
    }
}

fn define_run_scope_helper(
    target: &mut TargetConstruction,
    core: &CoreProgram,
    plan: &LoweringPlan,
    helper: PlannedFunctionId,
    scope: crate::ir::core::RunScopeId,
    data: &InstData,
    commands: &mut ConstructionMap,
) -> Result<(), Diagnostics> {
    let scope_id = scope;
    let scope = core
        .run_scope(scope)
        .ok_or_else(|| invariant_diagnostics("planned run scope disappeared", data.origin()))?;
    let body_entry = plan.function_entry(scope.callee()).ok_or_else(|| {
        invariant_diagnostics("planned run body has no function entry", data.origin())
    })?;
    let body_target = target.function(body_entry)?;
    let invoke_body = command(
        CommandKind::Function(FunctionCall::new(
            InternalCallableRef::Function(body_target).into(),
        )),
        data.origin(),
    )?;

    let mut modifiers = Vec::with_capacity(scope.modifiers().len());
    for (modifier_index, modifier) in scope.modifiers().iter().enumerate() {
        let selected = plan
            .preflight()
            .selected_run_modifier(scope_id, modifier_index)
            .ok_or_else(|| {
                invariant_diagnostics(
                    "planned run modifier has no retained preflight recipe",
                    modifier.origin(),
                )
            })?;
        modifiers.push(ExecuteModifier::new(
            selected.target_kind(),
            modifier.origin(),
        ));
    }
    let mut modifiers = modifiers.into_iter();
    let Some(first) = modifiers.next() else {
        return target.define_external_helper(helper, invoke_body);
    };
    let modifiers = ExecuteModifiers::new(first, modifiers.collect());
    let target_function = target.function(helper)?;
    let command_id = CommandId::from_index(0);
    let correlations = scope
        .modifiers()
        .iter()
        .enumerate()
        .map(|(modifier_index, _modifier)| {
            let recipe = plan
                .preflight()
                .selected_run_modifier(scope_id, modifier_index)
                .expect("verified selected run modifier exists")
                .recipe_id();
            ConstructedRunModifierLocation {
                function: target_function,
                command: command_id,
                modifier_index,
                recipe,
            }
        })
        .collect::<Vec<_>>();
    target.define_external_helper(
        helper,
        command(
            CommandKind::Execute(ExecuteCommand::new(modifiers, invoke_body)),
            data.origin(),
        )?,
    )?;
    commands.record_run_modifiers(scope_id, correlations, data.origin())
}

fn lower_terminator(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    block: BlockId,
    data: &crate::ir::core::BlockData,
    plan: &LoweringPlan,
) -> Result<(), Diagnostics> {
    let terminator = data.terminator().ok_or_else(|| {
        invariant_diagnostics("planned Core block lost its terminator", data.origin())
    })?;
    match terminator.kind() {
        TerminatorKind::Jump(destination) => lower_jump(
            context,
            function,
            block,
            destination.block(),
            terminator.origin(),
        ),
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => lower_branch(
            context,
            function,
            block,
            value_home(plan, function, *condition, terminator.origin())?,
            then_target.block(),
            else_target.block(),
            terminator.origin(),
        ),
        TerminatorKind::Return(values) => lower_return(
            context,
            function,
            &value_homes(plan, function, values, terminator.origin())?,
            terminator.origin(),
        ),
        TerminatorKind::Unreachable => Err(invariant_diagnostics(
            "reachable `unreachable` survived the legality audit",
            terminator.origin(),
        )),
    }
}

fn lower_branch_helpers(
    target: &mut TargetConstruction,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
) -> Result<(), Diagnostics> {
    for block in body.block_order().iter().copied() {
        if plan.block_function(function, block).is_none() {
            continue;
        }
        let terminator = body
            .block(block)
            .and_then(crate::ir::core::BlockData::terminator)
            .ok_or_else(|| {
                invariant_diagnostics("planned Core block lost its terminator", OriginId::UNKNOWN)
            })?;
        let TerminatorKind::Branch {
            then_target,
            else_target,
            ..
        } = terminator.kind()
        else {
            continue;
        };
        let Some(EdgeTransfer::Branch {
            then_edge,
            else_edge,
        }) = plan.edge_transfer(function, block)
        else {
            return Err(invariant_diagnostics(
                "planned branch helper transfer disappeared",
                terminator.origin(),
            ));
        };
        for (arm, edge, destination) in [
            (BranchArm::Then, then_edge, then_target.block()),
            (BranchArm::Else, else_edge, else_target.block()),
        ] {
            let Some(helper) = edge.helper() else {
                continue;
            };
            let mut context = target.begin_function(body, plan, helper)?;
            lower_branch_helper(
                &mut context,
                function,
                block,
                arm,
                destination,
                terminator.origin(),
            )?;
            context.finish();
        }
    }
    Ok(())
}

fn value_homes(
    plan: &LoweringPlan,
    function: FunctionId,
    values: &[ValueId],
    origin: OriginId,
) -> Result<Vec<HomeId>, Diagnostics> {
    values
        .iter()
        .copied()
        .map(|value| value_home(plan, function, value, origin))
        .collect()
}

fn value_home(
    plan: &LoweringPlan,
    function: FunctionId,
    value: ValueId,
    origin: OriginId,
) -> Result<HomeId, Diagnostics> {
    plan.value_home(function, value).ok_or_else(|| {
        invariant_diagnostics("reachable Core value has no planned score home", origin)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fmt::Debug;

    use super::construct_program;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, I32Predicate,
        Terminator, TerminatorKind,
    };
    use crate::ir::minecraft::{
        CallableRef, CommandKind, CommandNode, Condition, ExecuteModifierKind, InternalCallableRef,
        McFunctionId, MinecraftDebugDumper, MinecraftProgram, ObjectiveName, PackNamespace,
        ReturnCommand, ScoreCommand, ScoreComparison, ScoreHolders, ScoreOperation, ScoreRangeKind,
        ScoreRef, ScoreSelection, SingleScoreHolder, render_function, verify_program,
    };
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::audit::audit_legality;
    use crate::lower::minecraft::plan::{LoweringPlan, PlanBuilder};
    use crate::lower::minecraft::{
        LoweredFunction, LoweringOptions, LoweringOutput, MinecraftOptimizationLevel, RegisterSlot,
        lower_to_minecraft,
    };
    use crate::opt::core::{
        CoreOptimizationLevel, CoreOptimizationOptions, CorePassStatistics, CorePipelineStep,
        StatisticCount, optimize_core,
    };
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    trait GeneratedFixtureResult<T> {
        #[track_caller]
        fn expect_generated(self, context: &str) -> T;
    }

    impl<T, E: Debug> GeneratedFixtureResult<T> for Result<T, E> {
        #[track_caller]
        fn expect_generated(self, context: &str) -> T {
            self.unwrap_or_else(|error| {
                panic!("{context}: generated fixture construction failed: {error:?}")
            })
        }
    }

    #[track_caller]
    fn run_generated_case<T>(context: &str, case: impl FnOnce() -> T) -> T {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(case)).unwrap_or_else(|payload| {
            if let Some(message) = payload.downcast_ref::<String>() {
                panic!("{context}: generated lowering case failed: {message}");
            }
            if let Some(message) = payload.downcast_ref::<&str>() {
                panic!("{context}: generated lowering case failed: {message}");
            }
            panic!("{context}: generated lowering case failed with a non-string panic payload");
        })
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the loop fixture mirrors the canonical Core proof and checks its target result"
    )]
    fn sum_down_lowers_and_executes_with_ordinary_loop_copies() {
        let sources = SourceContext::new();
        let (core, function) = sum_down_program(&sources);
        let (plan, target) = lower_fixture(&core, &sources);
        let rendered = render_all(&target);
        assert!(!rendered.contains("#f0t0"));
        assert!(rendered.contains("mdl:__mdl/f0/e1_0"));

        let mut executor = TestExecutor::new(&target, 10_000);
        set_abi_parameters(&mut executor, &plan, function, &[4, 10]);
        let outcome = executor.run(entry_target(&target, function));
        assert!(outcome.success);
        assert_eq!(outcome.value, 1);
        assert_eq!(result_values(&executor, &plan, function), vec![20]);
    }

    #[test]
    fn baseline_executes_loop_carried_wrapping_left_reuse_with_fewer_commands() {
        let sources = SourceContext::new();
        let (core, function, parameter, sum) = wrapping_reuse_program(&sources, false);

        let none = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::None),
        )
        .unwrap();
        let baseline = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
        )
        .unwrap();
        let none_rendered = render_all(none.program());
        let baseline_rendered = render_all(baseline.program());
        let baseline_report = baseline.dump_lowering();
        assert!(accepted_coalescing_merges(&baseline_report) > 0);
        assert_eq!(
            dumped_value_home(&baseline_report, function, parameter),
            dumped_value_home(&baseline_report, function, sum),
            "the executed wrapping add must reuse its left operand's exact home"
        );
        assert!(
            baseline_rendered
                .matches("scoreboard players operation")
                .count()
                < none_rendered
                    .matches("scoreboard players operation")
                    .count(),
            "None:\n{none_rendered}\nBaseline:\n{baseline_rendered}"
        );
        assert!(!has_self_assignment(&baseline_rendered));

        let observation = execute_lowered(&baseline, function, &[], &[], "wrapping-left-reuse");
        assert_eq!(observation.results, vec![3]);
        assert!(observation.effect_calls.is_empty());
    }

    #[test]
    fn baseline_executes_same_value_x_plus_x_in_its_coalesced_home() {
        let sources = SourceContext::new();
        let (core, function, parameter, sum) = wrapping_reuse_program(&sources, true);
        let baseline = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
        )
        .unwrap();
        let rendered = render_all(baseline.program());
        let report = baseline.dump_lowering();
        assert!(accepted_coalescing_merges(&report) > 0);
        assert_eq!(
            dumped_value_home(&report, function, parameter),
            dumped_value_home(&report, function, sum)
        );
        assert!(!has_self_assignment(&rendered));
        assert!(has_self_addition(&rendered));

        let observation = execute_lowered(&baseline, function, &[], &[], "same-value-x-plus-x");
        assert_eq!(observation.results, vec![2]);
        assert!(observation.effect_calls.is_empty());
    }

    #[test]
    fn baseline_executes_a_call_result_reusing_its_dead_caller_argument() {
        let sources = SourceContext::new();
        let (core, calling_function, argument, result) = call_result_reuse_program(&sources);
        let none = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::None),
        )
        .unwrap();
        let baseline = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
        )
        .unwrap();
        let none_rendered = render_all(none.program());
        let baseline_rendered = render_all(baseline.program());
        let baseline_report = baseline.dump_lowering();
        assert!(accepted_coalescing_merges(&baseline_report) > 0);
        assert_eq!(
            dumped_value_home(&baseline_report, calling_function, argument),
            dumped_value_home(&baseline_report, calling_function, result),
            "the executed call result must reuse its exact dead argument home"
        );
        assert!(
            baseline_rendered
                .matches("scoreboard players operation")
                .count()
                < none_rendered
                    .matches("scoreboard players operation")
                    .count()
        );

        let observation = execute_lowered(
            &baseline,
            calling_function,
            &[],
            &[],
            "call-result-dead-argument-reuse",
        );
        assert_eq!(observation.results, vec![5]);
        assert!(observation.effect_calls.is_empty());
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the finite swap loop must expose the cyclic backedge in one complete fixture"
    )]
    fn finite_countdown_swap_lowers_and_executes_through_edge_temporary() {
        let sources = SourceContext::new();
        let (core, function, backedge) = countdown_swap_program(&sources);
        let (plan, target) = lower_fixture(&core, &sources);
        let rendered_backedge = render_resource(&target, &function_resource(function, backedge));
        assert_eq!(rendered_backedge.matches("#f0t0").count(), 2);
        assert!(
            rendered_backedge
                .contains("scoreboard players operation #f0v5 mdl.reg = #f0t0 mdl.reg")
        );

        let mut executor = TestExecutor::new(&target, 10_000);
        set_abi_parameters(&mut executor, &plan, function, &[3, 10, 20]);
        let outcome = executor.run(entry_target(&target, function));
        assert!(outcome.success);
        assert_eq!(outcome.value, 1);
        assert_eq!(result_values(&executor, &plan, function), vec![20, 10]);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one cross-layer fixture proves optimization, lowering, target results, and effect order"
    )]
    fn baseline_core_optimization_preserves_lowered_target_results_and_call_order() {
        let sources = SourceContext::new();
        let (core, fixture) = target_differential_program(&sources);

        let reference = optimize_core(
            core.clone(),
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        )
        .unwrap();
        let baseline = optimize_core(
            core.clone(),
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        )
        .unwrap();
        crate::ir::core::verify_program(reference.program(), &sources).unwrap();
        crate::ir::core::verify_program(baseline.program(), &sources).unwrap();

        let reference_body = reference
            .program()
            .function(fixture.entry)
            .unwrap()
            .body()
            .unwrap();
        let baseline_body = baseline
            .program()
            .function(fixture.entry)
            .unwrap()
            .body()
            .unwrap();
        assert_eq!(reference_body.block_counts().attached, 5);
        assert_eq!(baseline_body.block_counts().attached, 4);
        assert_eq!(reference_body.instruction_counts().attached, 9);
        assert_eq!(baseline_body.instruction_counts().attached, 8);

        let fusion = baseline
            .report()
            .steps()
            .iter()
            .find(|summary| summary.step() == CorePipelineStep::Fusion)
            .unwrap();
        let CorePassStatistics::Fusion(fusion) = fusion.statistics() else {
            panic!("fusion step exposed non-fusion statistics");
        };
        assert!(matches!(fusion.blocks_consumed, StatisticCount::Exact(1..)));
        let cse = baseline
            .report()
            .steps()
            .iter()
            .find(|summary| summary.step() == CorePipelineStep::Cse)
            .unwrap();
        let CorePassStatistics::Cse(cse) = cse.statistics() else {
            panic!("CSE step exposed non-CSE statistics");
        };
        assert!(matches!(
            cse.instructions_erased,
            StatisticCount::Exact(1..)
        ));

        let direct_reference = lower_to_minecraft(&core, &sources, &options()).unwrap();
        let reference_target =
            lower_to_minecraft(reference.program(), &sources, &options()).unwrap();
        let baseline_target = lower_to_minecraft(baseline.program(), &sources, &options()).unwrap();
        assert_eq!(
            MinecraftDebugDumper::program(direct_reference.program()),
            MinecraftDebugDumper::program(reference_target.program()),
            "Core optimization level `None` must retain the Stage 4 target exactly"
        );
        verify_program(reference_target.program(), &sources).unwrap();
        verify_program(baseline_target.program(), &sources).unwrap();

        for input in [i32::MIN, -1, 0, i32::MAX] {
            let reference_observation = execute_lowered(
                &reference_target,
                fixture.entry,
                &[input],
                &[fixture.increment, fixture.decrement],
                &format!("fixed target differential input={input}"),
            );
            let baseline_observation = execute_lowered(
                &baseline_target,
                fixture.entry,
                &[input],
                &[fixture.increment, fixture.decrement],
                &format!("fixed target differential input={input}"),
            );
            assert_eq!(
                baseline_observation, reference_observation,
                "lowered target differential failed for input {input}"
            );
            assert_eq!(reference_observation.results, vec![input.wrapping_add(5)]);
            let expected_calls = if input < 0 {
                vec![fixture.decrement, fixture.increment]
            } else {
                vec![fixture.increment, fixture.decrement]
            };
            assert_eq!(reference_observation.effect_calls, expected_calls);
        }
    }

    #[test]
    fn generated_typed_cfgs_match_their_core_models_under_both_lowering_policies() {
        const GENERATOR_VERSION: u32 = 1;
        const SEEDS: [u64; 4] = [0, 1, 0x5eed_5eed, u64::MAX];

        let sources = SourceContext::new();
        for seed in SEEDS {
            let suite_context = format!(
                "generator_version={GENERATOR_VERSION} shape=all seed={seed:#018x} inputs=all"
            );
            run_generated_case(&suite_context, || {
                let suite = generated_target_suite(&sources, GENERATOR_VERSION, seed);
                crate::ir::core::verify_program(&suite.core, &sources).unwrap_or_else(
                    |diagnostics| {
                        panic!(
                            "{suite_context}: generated Core verification failed: {diagnostics:?}"
                        )
                    },
                );
                let none = lower_to_minecraft(
                    &suite.core,
                    &sources,
                    &options().with_optimization_level(MinecraftOptimizationLevel::None),
                )
                .unwrap_or_else(|diagnostics| {
                    panic!("{suite_context}: Minecraft None lowering failed: {diagnostics:?}")
                });
                let baseline = lower_to_minecraft(
                    &suite.core,
                    &sources,
                    &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
                )
                .unwrap_or_else(|diagnostics| {
                    panic!("{suite_context}: Minecraft Baseline lowering failed: {diagnostics:?}")
                });
                verify_program(none.program(), &sources).unwrap_or_else(|diagnostics| {
                    panic!(
                        "{suite_context}: Minecraft None target verification failed: {diagnostics:?}"
                    )
                });
                verify_program(baseline.program(), &sources).unwrap_or_else(|diagnostics| {
                    panic!(
                        "{suite_context}: Minecraft Baseline target verification failed: {diagnostics:?}"
                    )
                });
                assert!(
                    accepted_coalescing_merges(&baseline.dump_lowering()) > 0,
                    "{suite_context}: generated Baseline suite must exercise a real coalescing decision"
                );

                for case in &suite.cases {
                    for inputs in case.inputs.iter().copied() {
                        let context = format!(
                            "generator_version={GENERATOR_VERSION} shape={} seed={seed:#018x} inputs={inputs:?}",
                            case.shape.name()
                        );
                        run_generated_case(&context, || {
                            let expected = case.oracle.observe(inputs);
                            let none_observation = execute_lowered(
                                &none,
                                case.entry,
                                &inputs,
                                &case.observable_functions,
                                &context,
                            );
                            let baseline_observation = execute_lowered(
                                &baseline,
                                case.entry,
                                &inputs,
                                &case.observable_functions,
                                &context,
                            );
                            assert_eq!(
                                none_observation.results, expected.results,
                                "{context}: Minecraft None results disagree with the Core fixture model"
                            );
                            assert_eq!(
                                none_observation.effect_calls, expected.effect_calls,
                                "{context}: Minecraft None effect order disagrees with the Core fixture model"
                            );
                            assert_eq!(
                                baseline_observation.results, expected.results,
                                "{context}: Minecraft Baseline results disagree with the Core fixture model"
                            );
                            assert_eq!(
                                baseline_observation.effect_calls, expected.effect_calls,
                                "{context}: Minecraft Baseline effect order disagrees with the Core fixture model"
                            );
                            assert_eq!(
                                baseline_observation, none_observation,
                                "{context}: Minecraft lowering policies disagree"
                            );
                        });
                    }
                }
            });
        }
    }

    fn accepted_coalescing_merges(report: &str) -> u64 {
        report
            .lines()
            .filter_map(|line| {
                line.split_whitespace()
                    .find_map(|field| field.strip_prefix("merges="))
            })
            .filter_map(|merges| merges.parse::<u64>().ok())
            .sum()
    }

    fn wrapping_reuse_program(
        sources: &SourceContext,
        same_operand: bool,
    ) -> (
        CoreProgram,
        FunctionId,
        crate::ir::core::ValueId,
        crate::ir::core::ValueId,
    ) {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("wrapping_reuse"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        let initial = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let increment = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let right = if same_operand { parameter } else { increment };
        let sum = builder
            .i32_add_wrapping(parameter, right, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(header, vec![sum]),
                    else_target: BlockTarget::new(exit, vec![sum]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![returned]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, parameter, sum)
    }

    fn call_result_reuse_program(
        sources: &SourceContext,
    ) -> (
        CoreProgram,
        FunctionId,
        crate::ir::core::ValueId,
        crate::ir::core::ValueId,
    ) {
        let mut core = CoreProgram::new();
        let identity_function = core
            .declare_function(
                Some("identity"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut identity_builder = FunctionBuilder::new(&core, sources, identity_function).unwrap();
        let identity_parameter = parameter(&identity_builder, identity_builder.entry_block(), 0);
        identity_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![identity_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(identity_function, identity_builder.finish().unwrap())
            .unwrap();

        let calling_function = core
            .declare_function(
                Some("call-result-reuse"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, calling_function).unwrap();
        let initial = builder.i32_constant(5, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let argument = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let result = builder
            .call(identity_function, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(header, vec![result]),
                    else_target: BlockTarget::new(exit, vec![result]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![returned]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(calling_function, builder.finish().unwrap())
            .unwrap();
        (core, calling_function, argument, result)
    }

    fn dumped_value_home(
        report: &str,
        function: FunctionId,
        value: crate::ir::core::ValueId,
    ) -> u64 {
        use crate::entity::EntityId;

        let function_prefix = format!("function {} ", function.index());
        let value_prefix = format!("  value {} -> ", value.index());
        let mut in_function = false;
        for line in report.lines() {
            if line.starts_with("function ") {
                in_function = line.starts_with(&function_prefix);
                continue;
            }
            if in_function {
                if let Some(home) = line.strip_prefix(&value_prefix) {
                    return home.parse().expect("lowering report home is numeric");
                }
            }
        }
        panic!(
            "lowering report omitted function {} value {}",
            function.index(),
            value.index()
        );
    }

    fn has_self_assignment(rendered: &str) -> bool {
        rendered.lines().any(|line| {
            let Some(operation) = line.strip_prefix("scoreboard players operation ") else {
                return false;
            };
            let Some((destination, source)) = operation.split_once(" = ") else {
                return false;
            };
            destination == source
        })
    }

    fn has_self_addition(rendered: &str) -> bool {
        rendered.lines().any(|line| {
            let Some(operation) = line.strip_prefix("scoreboard players operation ") else {
                return false;
            };
            let Some((destination, source)) = operation.split_once(" += ") else {
                return false;
            };
            destination == source
        })
    }

    #[derive(Clone, Copy)]
    struct TargetDifferentialFixture {
        entry: FunctionId,
        increment: FunctionId,
        decrement: FunctionId,
    }

    #[derive(Debug, Eq, PartialEq)]
    struct TargetObservation {
        results: Vec<i32>,
        effect_calls: Vec<FunctionId>,
    }

    #[derive(Clone, Copy, Debug)]
    enum GeneratedShape {
        StraightLine,
        BranchJoin,
        TerminatingLoop,
        MultiResultCalls,
    }

    impl GeneratedShape {
        const fn name(self) -> &'static str {
            match self {
                Self::StraightLine => "straight-line",
                Self::BranchJoin => "branch-join",
                Self::TerminatingLoop => "terminating-loop",
                Self::MultiResultCalls => "multi-result-effectful-calls",
            }
        }
    }

    struct GeneratedTargetSuite {
        core: CoreProgram,
        cases: Vec<GeneratedTargetCase>,
    }

    struct GeneratedTargetCase {
        shape: GeneratedShape,
        entry: FunctionId,
        observable_functions: Box<[FunctionId]>,
        inputs: Box<[[i32; 2]]>,
        oracle: GeneratedOracle,
    }

    #[derive(Clone, Copy)]
    enum GeneratedOracle {
        StraightLine {
            left_offset: i32,
            right_offset: i32,
            predicate: I32Predicate,
            negate: bool,
        },
        BranchJoin {
            condition: I32Predicate,
            then_swap: bool,
            else_swap: bool,
            then_offset: i32,
            else_offset: i32,
            then_predicate: I32Predicate,
            else_predicate: I32Predicate,
            then_negate: bool,
            else_negate: bool,
        },
        TerminatingLoop {
            iterations: i32,
            delta: i32,
            rotate: bool,
        },
        MultiResultCalls {
            pair: FunctionId,
            first: FunctionId,
            second: FunctionId,
            first_offset: i32,
            second_offset: i32,
            negate_condition: bool,
        },
    }

    impl GeneratedOracle {
        fn observe(self, [left, right]: [i32; 2]) -> TargetObservation {
            let (results, effect_calls) = match self {
                Self::StraightLine {
                    left_offset,
                    right_offset,
                    predicate,
                    negate,
                } => {
                    let left = left.wrapping_add(left_offset);
                    let right = right.wrapping_add(right_offset);
                    let (sum, overflowed) = left.overflowing_add(right);
                    let compared = evaluate_predicate(predicate, left, right) ^ negate;
                    (
                        vec![sum, bool_score(overflowed), bool_score(compared)],
                        vec![],
                    )
                }
                Self::BranchJoin {
                    condition,
                    then_swap,
                    else_swap,
                    then_offset,
                    else_offset,
                    then_predicate,
                    else_predicate,
                    then_negate,
                    else_negate,
                } => {
                    let (swap, offset, predicate, negate) =
                        if evaluate_predicate(condition, left, right) {
                            (then_swap, then_offset, then_predicate, then_negate)
                        } else {
                            (else_swap, else_offset, else_predicate, else_negate)
                        };
                    let (first, second) = if swap { (right, left) } else { (left, right) };
                    (
                        vec![
                            first.wrapping_add(offset),
                            bool_score(evaluate_predicate(predicate, first, second) ^ negate),
                        ],
                        vec![],
                    )
                }
                Self::TerminatingLoop {
                    iterations,
                    delta,
                    rotate,
                } => {
                    let mut count = iterations;
                    let mut accumulator = left;
                    let mut carried = right;
                    while count > 0 {
                        count = count.wrapping_add(-1);
                        let next_accumulator = accumulator.wrapping_add(carried);
                        let next_carried = carried.wrapping_add(delta);
                        (accumulator, carried) = if rotate {
                            (next_carried, next_accumulator)
                        } else {
                            (next_accumulator, next_carried)
                        };
                    }
                    (vec![accumulator, carried], vec![])
                }
                Self::MultiResultCalls {
                    pair,
                    first,
                    second,
                    first_offset,
                    second_offset,
                    negate_condition,
                } => {
                    let (sum, overflowed) = left.overflowing_add(right);
                    let first_then_second = overflowed ^ negate_condition;
                    let (first_call, first_delta, second_call, second_delta) = if first_then_second
                    {
                        (first, first_offset, second, second_offset)
                    } else {
                        (second, second_offset, first, first_offset)
                    };
                    let result = sum.wrapping_add(first_delta).wrapping_add(second_delta);
                    (
                        vec![result, bool_score(overflowed)],
                        vec![pair, first_call, second_call],
                    )
                }
            };
            TargetObservation {
                results,
                effect_calls,
            }
        }
    }

    fn generated_target_suite(
        sources: &SourceContext,
        generator_version: u32,
        seed: u64,
    ) -> GeneratedTargetSuite {
        let mut rng = GeneratedRng::new(generator_version, seed);
        let mut core = CoreProgram::new();
        let replay_inputs = "[[0,0],[i32::MAX,1],[i32::MIN,-1],[-7,11],[generated,generated]]";
        let straight_line_shape = GeneratedShape::StraightLine.name();
        let branch_join_shape = GeneratedShape::BranchJoin.name();
        let loop_shape = GeneratedShape::TerminatingLoop.name();
        let calls_shape = GeneratedShape::MultiResultCalls.name();
        let straight_line_context = format!(
            "generator_version={generator_version} seed={seed:#018x} shape={straight_line_shape} inputs={replay_inputs}"
        );
        let branch_join_context = format!(
            "generator_version={generator_version} seed={seed:#018x} shape={branch_join_shape} inputs={replay_inputs}"
        );
        let loop_context = format!(
            "generator_version={generator_version} seed={seed:#018x} shape={loop_shape} inputs={replay_inputs}"
        );
        let calls_context = format!(
            "generator_version={generator_version} seed={seed:#018x} shape={calls_shape} inputs={replay_inputs}"
        );
        let cases = vec![
            append_generated_straight_line(&mut core, sources, &mut rng, &straight_line_context),
            append_generated_branch_join(&mut core, sources, &mut rng, &branch_join_context),
            append_generated_loop(&mut core, sources, &mut rng, &loop_context),
            append_generated_calls(&mut core, sources, &mut rng, &calls_context),
        ];
        GeneratedTargetSuite { core, cases }
    }

    fn append_generated_straight_line(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
        context: &str,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_straight_line"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let left_offset = rng.next_i32();
        let right_offset = rng.next_i32();
        let predicate = rng.predicate();
        let negate = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut builder = FunctionBuilder::new(core, sources, entry).expect_generated(context);
        let block = builder.entry_block();
        let left = generated_parameter(&builder, block, 0, context);
        let right = generated_parameter(&builder, block, 1, context);
        let left_literal = builder
            .i32_constant(left_offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let right_literal = builder
            .i32_constant(right_offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let adjusted_left = builder
            .i32_add_wrapping(left, left_literal, OriginId::UNKNOWN)
            .expect_generated(context);
        let adjusted_right = builder
            .i32_add_wrapping(right, right_literal, OriginId::UNKNOWN)
            .expect_generated(context);
        let (sum, overflowed) = builder
            .i32_add_overflowing(adjusted_left, adjusted_right, OriginId::UNKNOWN)
            .expect_generated(context);
        let compared = builder
            .i32_compare(predicate, adjusted_left, adjusted_right, OriginId::UNKNOWN)
            .expect_generated(context);
        let compared = maybe_negate(&mut builder, compared, negate, context);
        let dead = builder
            .i32_constant(rng.next_i32(), OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .i32_add_wrapping(dead, adjusted_left, OriginId::UNKNOWN)
            .expect_generated(context);
        return_values(&mut builder, vec![sum, overflowed, compared], context);
        let body = builder.finish().expect_generated(context);
        core.define_function(entry, body).expect_generated(context);

        GeneratedTargetCase {
            shape: GeneratedShape::StraightLine,
            entry,
            observable_functions: Box::new([]),
            inputs,
            oracle: GeneratedOracle::StraightLine {
                left_offset,
                right_offset,
                predicate,
                negate,
            },
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the generated diamond keeps typed edge arguments and each arm's oracle knobs together"
    )]
    fn append_generated_branch_join(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
        context: &str,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_branch_join"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let condition = rng.predicate();
        let then_swap = rng.next_bool();
        let else_swap = rng.next_bool();
        let then_offset = rng.next_i32();
        let else_offset = rng.next_i32();
        let then_predicate = rng.predicate();
        let else_predicate = rng.predicate();
        let then_negate = rng.next_bool();
        let else_negate = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut builder = FunctionBuilder::new(core, sources, entry).expect_generated(context);
        let entry_block = builder.entry_block();
        let left = generated_parameter(&builder, entry_block, 0, context);
        let right = generated_parameter(&builder, entry_block, 1, context);
        let then_block = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let then_first = builder
            .append_block_parameter(then_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let then_second = builder
            .append_block_parameter(then_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let then_dead = builder
            .append_block_parameter(then_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let else_block = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let else_first = builder
            .append_block_parameter(else_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let else_second = builder
            .append_block_parameter(else_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let else_dead = builder
            .append_block_parameter(else_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let join = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let joined_integer = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let joined_boolean = builder
            .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
            .expect_generated(context);
        let branch_condition = builder
            .i32_compare(condition, left, right, OriginId::UNKNOWN)
            .expect_generated(context);
        let dead = builder
            .i32_constant(rng.next_i32(), OriginId::UNKNOWN)
            .expect_generated(context);
        let (then_left, then_right) = if then_swap {
            (right, left)
        } else {
            (left, right)
        };
        let (else_left, else_right) = if else_swap {
            (right, left)
        } else {
            (left, right)
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: branch_condition,
                    then_target: BlockTarget::new(then_block, vec![then_left, then_right, dead]),
                    else_target: BlockTarget::new(else_block, vec![else_left, else_right, dead]),
                },
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder
            .switch_to_block(then_block)
            .expect_generated(context);
        let offset = builder
            .i32_constant(then_offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let result = builder
            .i32_add_wrapping(then_first, offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let compared = builder
            .i32_compare(then_predicate, then_first, then_second, OriginId::UNKNOWN)
            .expect_generated(context);
        let compared = maybe_negate(&mut builder, compared, then_negate, context);
        builder
            .i32_add_wrapping(then_dead, then_second, OriginId::UNKNOWN)
            .expect_generated(context);
        jump(&mut builder, join, vec![result, compared], context);

        builder
            .switch_to_block(else_block)
            .expect_generated(context);
        let offset = builder
            .i32_constant(else_offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let result = builder
            .i32_add_wrapping(else_first, offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let compared = builder
            .i32_compare(else_predicate, else_first, else_second, OriginId::UNKNOWN)
            .expect_generated(context);
        let compared = maybe_negate(&mut builder, compared, else_negate, context);
        builder
            .i32_add_wrapping(else_dead, else_second, OriginId::UNKNOWN)
            .expect_generated(context);
        jump(&mut builder, join, vec![result, compared], context);

        builder.switch_to_block(join).expect_generated(context);
        return_values(&mut builder, vec![joined_integer, joined_boolean], context);
        let body = builder.finish().expect_generated(context);
        core.define_function(entry, body).expect_generated(context);

        GeneratedTargetCase {
            shape: GeneratedShape::BranchJoin,
            entry,
            observable_functions: Box::new([]),
            inputs,
            oracle: GeneratedOracle::BranchJoin {
                condition,
                then_swap,
                else_swap,
                then_offset,
                else_offset,
                then_predicate,
                else_predicate,
                then_negate,
                else_negate,
            },
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the generated loop keeps its bounded state transition and dead carried cycle visible"
    )]
    fn append_generated_loop(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
        context: &str,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_terminating_loop"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let iterations = rng.bounded_loop_iterations();
        let delta = rng.next_i32();
        let rotate = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut builder = FunctionBuilder::new(core, sources, entry).expect_generated(context);
        let entry_block = builder.entry_block();
        let initial_accumulator = generated_parameter(&builder, entry_block, 0, context);
        let initial_carried = generated_parameter(&builder, entry_block, 1, context);
        let header = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let count = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let accumulator = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let carried = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let dead = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let body = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let exit = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let result_accumulator = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let result_carried = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let initial_count = builder
            .i32_constant(iterations, OriginId::UNKNOWN)
            .expect_generated(context);
        let initial_dead = builder
            .i32_constant(rng.next_i32(), OriginId::UNKNOWN)
            .expect_generated(context);
        jump(
            &mut builder,
            header,
            vec![
                initial_count,
                initial_accumulator,
                initial_carried,
                initial_dead,
            ],
            context,
        );

        builder.switch_to_block(header).expect_generated(context);
        let zero = builder
            .i32_constant(0, OriginId::UNKNOWN)
            .expect_generated(context);
        let done = builder
            .i32_compare(I32Predicate::SignedLe, count, zero, OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: done,
                    then_target: BlockTarget::new(exit, vec![accumulator, carried]),
                    else_target: BlockTarget::new(body, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder.switch_to_block(body).expect_generated(context);
        let minus_one = builder
            .i32_constant(-1, OriginId::UNKNOWN)
            .expect_generated(context);
        let delta_value = builder
            .i32_constant(delta, OriginId::UNKNOWN)
            .expect_generated(context);
        let next_count = builder
            .i32_add_wrapping(count, minus_one, OriginId::UNKNOWN)
            .expect_generated(context);
        let next_accumulator = builder
            .i32_add_wrapping(accumulator, carried, OriginId::UNKNOWN)
            .expect_generated(context);
        let next_carried = builder
            .i32_add_wrapping(carried, delta_value, OriginId::UNKNOWN)
            .expect_generated(context);
        let next_dead = builder
            .i32_add_wrapping(dead, delta_value, OriginId::UNKNOWN)
            .expect_generated(context);
        let (next_accumulator, next_carried) = if rotate {
            (next_carried, next_accumulator)
        } else {
            (next_accumulator, next_carried)
        };
        jump(
            &mut builder,
            header,
            vec![next_count, next_accumulator, next_carried, next_dead],
            context,
        );

        builder.switch_to_block(exit).expect_generated(context);
        return_values(
            &mut builder,
            vec![result_accumulator, result_carried],
            context,
        );
        let body = builder.finish().expect_generated(context);
        core.define_function(entry, body).expect_generated(context);

        GeneratedTargetCase {
            shape: GeneratedShape::TerminatingLoop,
            entry,
            observable_functions: Box::new([]),
            inputs,
            oracle: GeneratedOracle::TerminatingLoop {
                iterations,
                delta,
                rotate,
            },
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the call fixture deliberately keeps its multi-result callee and both effect orders visible"
    )]
    fn append_generated_calls(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
        context: &str,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_effectful_calls"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let pair = core
            .declare_function(
                Some("generated_pair"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let first = core
            .declare_function(
                Some("generated_first_effect"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let second = core
            .declare_function(
                Some("generated_second_effect"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let first_offset = rng.next_i32();
        let second_offset = rng.next_i32();
        let extra_offset = rng.next_i32();
        let negate_condition = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut pair_builder = FunctionBuilder::new(core, sources, pair).expect_generated(context);
        let pair_entry = pair_builder.entry_block();
        let pair_left = generated_parameter(&pair_builder, pair_entry, 0, context);
        let pair_right = generated_parameter(&pair_builder, pair_entry, 1, context);
        let (sum, overflowed) = pair_builder
            .i32_add_overflowing(pair_left, pair_right, OriginId::UNKNOWN)
            .expect_generated(context);
        let extra_literal = pair_builder
            .i32_constant(extra_offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let extra = pair_builder
            .i32_add_wrapping(sum, extra_literal, OriginId::UNKNOWN)
            .expect_generated(context);
        return_values(&mut pair_builder, vec![sum, overflowed, extra], context);
        let pair_body = pair_builder.finish().expect_generated(context);
        core.define_function(pair, pair_body)
            .expect_generated(context);
        define_generated_wrapping_offset(core, sources, first, first_offset, context);
        define_generated_wrapping_offset(core, sources, second, second_offset, context);

        let mut builder = FunctionBuilder::new(core, sources, entry).expect_generated(context);
        let entry_block = builder.entry_block();
        let left = generated_parameter(&builder, entry_block, 0, context);
        let right = generated_parameter(&builder, entry_block, 1, context);
        let call_results = builder
            .call(pair, vec![left, right], OriginId::UNKNOWN)
            .expect_generated(context);
        let sum = generated_result(&call_results, 0, context);
        let overflowed = generated_result(&call_results, 1, context);
        let condition = maybe_negate(&mut builder, overflowed, negate_condition, context);
        let then_block = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let else_block = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let join = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let joined_integer = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let joined_boolean = builder
            .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder
            .switch_to_block(then_block)
            .expect_generated(context);
        let after_first_results = builder
            .call(first, vec![sum], OriginId::UNKNOWN)
            .expect_generated(context);
        let after_first = generated_result(&after_first_results, 0, context);
        let after_second_results = builder
            .call(second, vec![after_first], OriginId::UNKNOWN)
            .expect_generated(context);
        let after_second = generated_result(&after_second_results, 0, context);
        jump(&mut builder, join, vec![after_second, overflowed], context);

        builder
            .switch_to_block(else_block)
            .expect_generated(context);
        let after_second_results = builder
            .call(second, vec![sum], OriginId::UNKNOWN)
            .expect_generated(context);
        let after_second = generated_result(&after_second_results, 0, context);
        let after_first_results = builder
            .call(first, vec![after_second], OriginId::UNKNOWN)
            .expect_generated(context);
        let after_first = generated_result(&after_first_results, 0, context);
        jump(&mut builder, join, vec![after_first, overflowed], context);

        builder.switch_to_block(join).expect_generated(context);
        return_values(&mut builder, vec![joined_integer, joined_boolean], context);
        let body = builder.finish().expect_generated(context);
        core.define_function(entry, body).expect_generated(context);

        GeneratedTargetCase {
            shape: GeneratedShape::MultiResultCalls,
            entry,
            observable_functions: vec![pair, first, second].into_boxed_slice(),
            inputs,
            oracle: GeneratedOracle::MultiResultCalls {
                pair,
                first,
                second,
                first_offset,
                second_offset,
                negate_condition,
            },
        }
    }

    fn generated_inputs(rng: &mut GeneratedRng) -> Box<[[i32; 2]]> {
        vec![
            [0, 0],
            [i32::MAX, 1],
            [i32::MIN, -1],
            [-7, 11],
            [rng.next_i32(), rng.next_i32()],
        ]
        .into_boxed_slice()
    }

    fn maybe_negate(
        builder: &mut FunctionBuilder<'_>,
        value: crate::ir::core::ValueId,
        negate: bool,
        context: &str,
    ) -> crate::ir::core::ValueId {
        if negate {
            builder
                .bool_not(value, OriginId::UNKNOWN)
                .expect_generated(context)
        } else {
            value
        }
    }

    fn jump(
        builder: &mut FunctionBuilder<'_>,
        target: BlockId,
        arguments: Vec<crate::ir::core::ValueId>,
        context: &str,
    ) {
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(target, arguments)),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);
    }

    fn return_values(
        builder: &mut FunctionBuilder<'_>,
        values: Vec<crate::ir::core::ValueId>,
        context: &str,
    ) {
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(values),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);
    }

    fn generated_parameter(
        builder: &FunctionBuilder<'_>,
        block: BlockId,
        index: usize,
        context: &str,
    ) -> crate::ir::core::ValueId {
        builder
            .body()
            .block(block)
            .unwrap_or_else(|| panic!("{context}: generated block {block:?} is missing"))
            .parameters()
            .get(index)
            .unwrap_or_else(|| {
                panic!("{context}: generated block {block:?} omitted parameter {index}")
            })
            .value()
    }

    fn generated_result(
        results: &[crate::ir::core::ValueId],
        index: usize,
        context: &str,
    ) -> crate::ir::core::ValueId {
        results
            .get(index)
            .copied()
            .unwrap_or_else(|| panic!("{context}: generated call omitted result index {index}"))
    }

    const fn bool_score(value: bool) -> i32 {
        if value { 1 } else { 0 }
    }

    const fn evaluate_predicate(predicate: I32Predicate, left: i32, right: i32) -> bool {
        match predicate {
            I32Predicate::Eq => left == right,
            I32Predicate::Ne => left != right,
            I32Predicate::SignedLt => left < right,
            I32Predicate::SignedLe => left <= right,
            I32Predicate::SignedGt => left > right,
            I32Predicate::SignedGe => left >= right,
        }
    }

    struct GeneratedRng {
        state: u64,
    }

    impl GeneratedRng {
        fn new(generator_version: u32, seed: u64) -> Self {
            Self {
                state: seed ^ u64::from(generator_version).wrapping_mul(0x9e37_79b9_7f4a_7c15),
            }
        }

        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut value = self.state;
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^ (value >> 31)
        }

        fn next_i32(&mut self) -> i32 {
            let bytes = self.next_u64().to_le_bytes();
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }

        fn next_bool(&mut self) -> bool {
            self.next_u64() & 1 != 0
        }

        fn predicate(&mut self) -> I32Predicate {
            match self.next_u64() % 6 {
                0 => I32Predicate::Eq,
                1 => I32Predicate::Ne,
                2 => I32Predicate::SignedLt,
                3 => I32Predicate::SignedLe,
                4 => I32Predicate::SignedGt,
                _ => I32Predicate::SignedGe,
            }
        }

        fn bounded_loop_iterations(&mut self) -> i32 {
            match self.next_u64() % 5 {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 3,
                _ => 4,
            }
        }
    }

    fn target_differential_program(
        sources: &SourceContext,
    ) -> (CoreProgram, TargetDifferentialFixture) {
        let mut core = CoreProgram::new();
        let entry = core
            .declare_function(
                Some("target_differential"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let increment = core
            .declare_function(
                Some("observable_increment"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let decrement = core
            .declare_function(
                Some("observable_decrement"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();

        define_wrapping_offset(&mut core, sources, increment, 1);
        define_wrapping_offset(&mut core, sources, decrement, -1);
        define_target_differential_entry(&mut core, sources, entry, increment, decrement);
        (
            core,
            TargetDifferentialFixture {
                entry,
                increment,
                decrement,
            },
        )
    }

    fn define_target_differential_entry(
        core: &mut CoreProgram,
        sources: &SourceContext,
        entry: FunctionId,
        increment: FunctionId,
        decrement: FunctionId,
    ) {
        let mut builder = FunctionBuilder::new(core, sources, entry).unwrap();
        let entry_block = builder.entry_block();
        let input = parameter(&builder, entry_block, 0);
        let work = builder.create_block(OriginId::UNKNOWN).unwrap();
        let work_input = builder
            .append_block_parameter(work, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(work, vec![input])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(work).unwrap();
        let five = builder.i32_constant(5, OriginId::UNKNOWN).unwrap();
        let first_sum = builder
            .i32_add_wrapping(work_input, five, OriginId::UNKNOWN)
            .unwrap();
        let duplicate_sum = builder
            .i32_add_wrapping(work_input, five, OriginId::UNKNOWN)
            .unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let nonnegative = builder
            .i32_compare(I32Predicate::SignedGe, work_input, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: nonnegative,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(then_block).unwrap();
        let incremented = builder
            .call(increment, vec![first_sum], OriginId::UNKNOWN)
            .unwrap()[0];
        let restored = builder
            .call(decrement, vec![incremented], OriginId::UNKNOWN)
            .unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![restored])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(else_block).unwrap();
        let decremented = builder
            .call(decrement, vec![duplicate_sum], OriginId::UNKNOWN)
            .unwrap()[0];
        let restored = builder
            .call(increment, vec![decremented], OriginId::UNKNOWN)
            .unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![restored])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(entry, builder.finish().unwrap())
            .unwrap();
    }

    fn define_wrapping_offset(
        core: &mut CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
        offset: i32,
    ) {
        let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
        let input = parameter(&builder, builder.entry_block(), 0);
        let offset = builder.i32_constant(offset, OriginId::UNKNOWN).unwrap();
        let result = builder
            .i32_add_wrapping(input, offset, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
    }

    fn define_generated_wrapping_offset(
        core: &mut CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
        offset: i32,
        context: &str,
    ) {
        let mut builder = FunctionBuilder::new(core, sources, function).expect_generated(context);
        let input = generated_parameter(&builder, builder.entry_block(), 0, context);
        let offset = builder
            .i32_constant(offset, OriginId::UNKNOWN)
            .expect_generated(context);
        let result = builder
            .i32_add_wrapping(input, offset, OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);
        let body = builder.finish().expect_generated(context);
        core.define_function(function, body)
            .expect_generated(context);
    }

    fn execute_lowered(
        output: &LoweringOutput,
        entry: FunctionId,
        arguments: &[i32],
        observable_functions: &[FunctionId],
        context: &str,
    ) -> TargetObservation {
        let lowered = output
            .map()
            .function(entry)
            .unwrap_or_else(|| panic!("{context}: lowered entry {entry:?} is missing"));
        assert_eq!(
            lowered.parameter_homes().len(),
            arguments.len(),
            "{context}: entry parameter arity changed during lowering"
        );
        let mut executor = TestExecutor::new(output.program(), 10_000);
        set_public_abi_parameters(&mut executor, lowered, arguments, context);
        let outcome = executor.run(public_entry_target(output.program(), lowered, context));
        assert!(outcome.success, "{context}: target call failed");
        assert_eq!(
            outcome.value, 1,
            "{context}: target entry returned a non-success value"
        );
        let observable_targets = observable_functions
            .iter()
            .copied()
            .map(|function| {
                let lowered = output.map().function(function).unwrap_or_else(|| {
                    panic!("{context}: lowered observable function {function:?} is missing")
                });
                (
                    public_entry_target(output.program(), lowered, context),
                    function,
                )
            })
            .collect::<HashMap<_, _>>();
        let effect_calls = executor
            .called_functions
            .iter()
            .filter_map(|target| observable_targets.get(target).copied())
            .collect();
        let results = lowered
            .result_homes()
            .iter()
            .map(|(_, slot)| executor.score(&register_score(slot)))
            .collect();
        TargetObservation {
            results,
            effect_calls,
        }
    }

    fn set_public_abi_parameters(
        executor: &mut TestExecutor<'_>,
        function: &LoweredFunction,
        values: &[i32],
        context: &str,
    ) {
        for ((ty, slot), value) in function
            .parameter_homes()
            .iter()
            .zip(values.iter().copied())
        {
            assert!(
                *ty != CoreType::Bool || matches!(value, 0 | 1),
                "{context}: Boolean ABI input is not canonical"
            );
            executor.scores.insert(register_score(slot), value);
        }
    }

    fn public_entry_target(
        program: &MinecraftProgram,
        function: &LoweredFunction,
        context: &str,
    ) -> McFunctionId {
        program
            .functions()
            .find_map(|(id, data)| (data.resource() == function.entry_resource()).then_some(id))
            .unwrap_or_else(|| panic!("{context}: lowered public entry resource is missing"))
    }

    fn register_score(slot: &RegisterSlot) -> ScoreRef {
        ScoreRef::new(
            SingleScoreHolder::Fake(slot.holder().clone()),
            slot.objective().clone(),
        )
    }

    fn sum_down_program(sources: &SourceContext) -> (CoreProgram, FunctionId) {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("sum_down"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        let entry = builder.entry_block();
        let initial_counter = parameter(&builder, entry, 0);
        let initial_accumulator = parameter(&builder, entry, 1);
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let counter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let accumulator = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let body = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    header,
                    vec![initial_counter, initial_accumulator],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let done = builder
            .i32_compare(I32Predicate::SignedLe, counter, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: done,
                    then_target: BlockTarget::new(exit, vec![accumulator]),
                    else_target: BlockTarget::new(body, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(body).unwrap();
        let minus_one = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
        let next_counter = builder
            .i32_add_wrapping(counter, minus_one, OriginId::UNKNOWN)
            .unwrap();
        let next_accumulator = builder
            .i32_add_wrapping(accumulator, counter, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    header,
                    vec![next_counter, next_accumulator],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    fn countdown_swap_program(sources: &SourceContext) -> (CoreProgram, FunctionId, BlockId) {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("countdown_swap"),
                vec![CoreType::I32, CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        let entry = builder.entry_block();
        let initial_count = parameter(&builder, entry, 0);
        let initial_left = parameter(&builder, entry, 1);
        let initial_right = parameter(&builder, entry, 2);
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let count = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let left = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let right = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let backedge = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result_left = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let result_right = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    header,
                    vec![initial_count, initial_left, initial_right],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let done = builder
            .i32_compare(I32Predicate::SignedLe, count, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: done,
                    then_target: BlockTarget::new(exit, vec![left, right]),
                    else_target: BlockTarget::new(backedge, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(backedge).unwrap();
        let minus_one = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
        let next_count = builder
            .i32_add_wrapping(count, minus_one, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![next_count, right, left])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result_left, result_right]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, backedge)
    }

    fn parameter(
        builder: &FunctionBuilder<'_>,
        block: BlockId,
        index: usize,
    ) -> crate::ir::core::ValueId {
        builder.body().block(block).unwrap().parameters()[index].value()
    }

    fn lower_fixture(
        core: &CoreProgram,
        sources: &SourceContext,
    ) -> (LoweringPlan, MinecraftProgram) {
        crate::ir::core::verify_program(core, sources).unwrap();
        let analyses = SemanticInventory::new(core).unwrap();
        audit_legality(
            core,
            &analyses,
            options().target(),
            options().command_limit_assumptions(),
        )
        .unwrap();
        let mut builder = PlanBuilder::new(core, options()).unwrap();
        builder
            .physicalize(
                core,
                &analyses,
                &crate::lower::minecraft::demand::RuntimeDemand::optimization_disabled(),
            )
            .unwrap();
        let plan = builder.finish(core, &analyses).unwrap();
        let (target, _) = construct_program(core, &plan).unwrap().into_parts();
        verify_program(&target, sources).unwrap();
        (plan, target)
    }

    fn options() -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
    }

    fn render_all(program: &MinecraftProgram) -> String {
        program
            .functions()
            .map(|(_, function)| {
                String::from_utf8(render_function(program, function).unwrap()).unwrap()
            })
            .collect()
    }

    fn render_resource(program: &MinecraftProgram, resource: &str) -> String {
        let (_, function) = program
            .functions()
            .find(|(_, function)| function.resource().to_string() == resource)
            .unwrap();
        String::from_utf8(render_function(program, function).unwrap()).unwrap()
    }

    fn function_resource(function: FunctionId, block: BlockId) -> String {
        use crate::entity::EntityId;
        format!("mdl:__mdl/f{}/b{}", function.index(), block.index())
    }

    fn entry_target(program: &MinecraftProgram, function: FunctionId) -> McFunctionId {
        use crate::entity::EntityId;
        let resource = format!("mdl:__mdl/f{}/b0", function.index());
        program
            .functions()
            .find_map(|(id, data)| (data.resource().to_string() == resource).then_some(id))
            .unwrap()
    }

    fn set_abi_parameters(
        executor: &mut TestExecutor<'_>,
        plan: &LoweringPlan,
        function: FunctionId,
        values: &[i32],
    ) {
        for (home, value) in plan
            .function_parameters(function)
            .unwrap()
            .iter()
            .copied()
            .zip(values.iter().copied())
        {
            executor.scores.insert(plan.score(home).unwrap(), value);
        }
    }

    fn result_values(
        executor: &TestExecutor<'_>,
        plan: &LoweringPlan,
        function: FunctionId,
    ) -> Vec<i32> {
        plan.function_results(function)
            .unwrap()
            .iter()
            .map(|home| executor.score(&plan.score(*home).unwrap()))
            .collect()
    }

    #[derive(Clone, Copy, Debug)]
    struct Outcome {
        value: i32,
        success: bool,
    }

    enum Flow {
        Continue(Outcome),
        Return(Outcome),
    }

    struct TestExecutor<'a> {
        program: &'a MinecraftProgram,
        scores: HashMap<ScoreRef, i32>,
        called_functions: Vec<McFunctionId>,
        remaining_commands: usize,
    }

    impl<'a> TestExecutor<'a> {
        fn new(program: &'a MinecraftProgram, command_budget: usize) -> Self {
            Self {
                program,
                scores: HashMap::new(),
                called_functions: Vec::new(),
                remaining_commands: command_budget,
            }
        }

        fn run(&mut self, function: McFunctionId) -> Outcome {
            let commands = self
                .program
                .function(function)
                .unwrap()
                .body()
                .commands()
                .map(|(_, command)| command.clone())
                .collect::<Vec<_>>();
            let mut last = Outcome {
                value: 0,
                success: true,
            };
            for command in &commands {
                match self.command(command) {
                    Flow::Continue(outcome) => last = outcome,
                    Flow::Return(outcome) => return outcome,
                }
            }
            last
        }

        fn command(&mut self, command: &CommandNode) -> Flow {
            self.remaining_commands = self
                .remaining_commands
                .checked_sub(1)
                .expect("generated loop exceeded the test command budget");
            match command.kind() {
                CommandKind::Score(command) => Flow::Continue(self.score_command(command)),
                CommandKind::Function(call) => {
                    let CallableRef::Internal(InternalCallableRef::Function(target)) =
                        call.target()
                    else {
                        panic!("lowered test program called a non-function target");
                    };
                    self.called_functions.push(*target);
                    Flow::Continue(self.run(*target))
                }
                CommandKind::Return(ReturnCommand::Value(value)) => Flow::Return(Outcome {
                    value: *value,
                    success: true,
                }),
                CommandKind::Return(ReturnCommand::Fail) => Flow::Return(Outcome {
                    value: 0,
                    success: false,
                }),
                CommandKind::Return(ReturnCommand::Run(run)) => {
                    Flow::Return(self.command(run).outcome())
                }
                CommandKind::Execute(execute) => {
                    let selected = execute.modifiers().as_slice().iter().all(|modifier| {
                        match modifier.kind() {
                            ExecuteModifierKind::If(condition) => self.condition(condition),
                            ExecuteModifierKind::Unless(condition) => !self.condition(condition),
                            _ => panic!("loop lowering emitted a context-changing modifier"),
                        }
                    });
                    if selected {
                        self.command(execute.run())
                    } else {
                        Flow::Continue(Outcome {
                            value: 0,
                            success: false,
                        })
                    }
                }
                CommandKind::Data(_)
                | CommandKind::Say(_)
                | CommandKind::Teleport(_)
                | CommandKind::Raw(_)
                | CommandKind::Macro(_)
                | CommandKind::FunctionWithStorage(_)
                | CommandKind::AdvancementRevoke(_)
                | CommandKind::ItemReplaceBlock(_)
                | CommandKind::Schedule(_)
                | CommandKind::ScheduleClear(_) => {
                    panic!("loop lowering emitted a non-score primitive")
                }
            }
        }

        fn score_command(&mut self, command: &ScoreCommand) -> Outcome {
            match command {
                ScoreCommand::PlayersSet { target, value } => {
                    self.scores.insert(score_ref(target), *value);
                    Outcome {
                        value: *value,
                        success: true,
                    }
                }
                ScoreCommand::PlayersOperation { target, op, source } => {
                    let target = score_ref(target);
                    let source = score_ref(source);
                    let left = self.score(&target);
                    let right = self.score(&source);
                    let value = match op {
                        ScoreOperation::Assign => right,
                        ScoreOperation::Add => left.wrapping_add(right),
                        ScoreOperation::Subtract => left.wrapping_sub(right),
                        _ => panic!("loop lowering emitted an untested score operation"),
                    };
                    self.scores.insert(target, value);
                    Outcome {
                        value,
                        success: true,
                    }
                }
                _ => panic!("loop lowering emitted an untested score command"),
            }
        }

        fn condition(&self, condition: &Condition) -> bool {
            match condition {
                Condition::ScoreMatches(score, range) => {
                    let value = self.score(score);
                    match range.kind() {
                        ScoreRangeKind::Exact(expected) => value == expected,
                        ScoreRangeKind::AtLeast(minimum) => value >= minimum,
                        ScoreRangeKind::AtMost(maximum) => value <= maximum,
                        ScoreRangeKind::Between { min, max } => (min..=max).contains(&value),
                    }
                }
                Condition::ScoreCompare(left, comparison, right) => {
                    let left = self.score(left);
                    let right = self.score(right);
                    match comparison {
                        ScoreComparison::Equal => left == right,
                        ScoreComparison::LessThan => left < right,
                        ScoreComparison::LessOrEqual => left <= right,
                        ScoreComparison::GreaterThan => left > right,
                        ScoreComparison::GreaterOrEqual => left >= right,
                    }
                }
                _ => panic!("loop lowering emitted an untested condition"),
            }
        }

        fn score(&self, score: &ScoreRef) -> i32 {
            *self.scores.get(score).unwrap_or(&0)
        }
    }

    impl Flow {
        const fn outcome(self) -> Outcome {
            match self {
                Self::Continue(outcome) | Self::Return(outcome) => outcome,
            }
        }
    }

    fn score_ref(selection: &ScoreSelection) -> ScoreRef {
        let ScoreHolders::Fake(holder) = selection.holders() else {
            panic!("lowered loop used a selector score");
        };
        ScoreRef::new(
            SingleScoreHolder::Fake(holder.clone()),
            selection.objective().clone(),
        )
    }
}
