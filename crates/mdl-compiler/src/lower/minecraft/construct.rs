use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::minecraft::{
    BuildError, CommandId, CommandKind, CommandNode, Condition, DataCommand, DataModifyMode,
    DataSource, ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers,
    FunctionBodyBuilder, FunctionTagEntry, FunctionTagId, FunctionTagMerge, FunctionTagResourceId,
    InternalCallableRef, McFunctionId, MinecraftProgram, MinecraftProgramBuilder, NbtValue,
    ReturnCommand, ScoreCommand, ScoreRef,
};
use crate::source::OriginId;

use super::plan::{HomeId, LoweringPlan, PlannedFunctionId};

/// Complete Stage 3 declaration mapping, retained only during construction.
#[derive(Debug)]
pub(crate) struct DeclarationMap {
    functions: EntityVec<PlannedFunctionId, McFunctionId>,
    #[allow(
        dead_code,
        reason = "the frozen Stage 3 declaration map records the load-tag identity by contract"
    )]
    load_tag: FunctionTagId,
}

impl DeclarationMap {
    pub(crate) fn function(&self, planned: PlannedFunctionId) -> Option<McFunctionId> {
        self.functions.get(planned).copied()
    }

    #[cfg(test)]
    pub(crate) const fn load_tag(&self) -> FunctionTagId {
        self.load_tag
    }
}

/// Ephemeral target builder plus the only identities allowed to address it.
#[derive(Debug)]
pub(crate) struct TargetConstruction {
    builder: MinecraftProgramBuilder,
    declarations: DeclarationMap,
}

impl TargetConstruction {
    pub(crate) fn declare(plan: &LoweringPlan) -> Result<Self, Diagnostics> {
        let mut builder = MinecraftProgramBuilder::new(plan.target());
        let mut functions: EntityVec<PlannedFunctionId, McFunctionId> = EntityVec::new();
        for (planned, function) in plan.planned_functions() {
            let target = builder
                .declare_function(function.resource().clone(), function.origin())
                .map_err(|error| construction_diagnostics(&error, function.origin()))?;
            let mapped = functions
                .push(target)
                .map_err(|EntityLimitError| capacity_diagnostics(function.origin()))?;
            if mapped != planned || mapped.index() != target.index() {
                return Err(invariant_diagnostics(
                    "dense planned and Stage 3 function identities diverged",
                    function.origin(),
                ));
            }
        }
        let load_tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("minecraft:load")
                    .expect("the vanilla load-tag resource is valid"),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .map_err(|error| construction_diagnostics(&error, OriginId::UNKNOWN))?;
        let load_function = functions.get(plan.load()).copied().ok_or_else(|| {
            invariant_diagnostics(
                "the verified plan has no mapped load function",
                OriginId::UNKNOWN,
            )
        })?;
        let mut tag = builder
            .begin_function_tag(load_tag)
            .map_err(|error| construction_diagnostics(&error, OriginId::UNKNOWN))?;
        tag.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(load_function),
            OriginId::UNKNOWN,
        ));
        tag.finish();
        Ok(Self {
            builder,
            declarations: DeclarationMap {
                functions,
                load_tag,
            },
        })
    }

    #[cfg(test)]
    pub(crate) fn declarations(&self) -> &DeclarationMap {
        &self.declarations
    }

    pub(crate) fn finish(self) -> Result<MinecraftProgram, Diagnostics> {
        self.builder.finish()
    }

    pub(crate) fn begin_function<'a>(
        &'a mut self,
        body: &'a crate::ir::core::FunctionBody,
        plan: &'a LoweringPlan,
        planned: PlannedFunctionId,
    ) -> Result<FunctionLoweringCx<'a, 'a>, Diagnostics> {
        let target = self.declarations.function(planned).ok_or_else(|| {
            invariant_diagnostics(
                "verified planned function has no Stage 3 declaration",
                OriginId::UNKNOWN,
            )
        })?;
        let target_body = self
            .builder
            .begin_function(target)
            .map_err(|error| construction_diagnostics(&error, OriginId::UNKNOWN))?;
        #[cfg(not(test))]
        let _ = body;
        Ok(FunctionLoweringCx {
            #[cfg(test)]
            body,
            plan,
            planned,
            declarations: &self.declarations,
            target_body,
        })
    }

    pub(crate) fn define_initialization(&mut self, plan: &LoweringPlan) -> Result<(), Diagnostics> {
        let load = self.declared_function(plan.load())?;
        let init = self.declared_function(plan.init_try_create())?;
        let probe = command(
            CommandKind::Return(ReturnCommand::run(command(
                CommandKind::Score(ScoreCommand::ObjectiveAddDummy {
                    objective: plan.register_objective().clone(),
                }),
                OriginId::UNKNOWN,
            )?)),
            OriginId::UNKNOWN,
        )?;
        define_commands(&mut self.builder, init, vec![probe])?;

        let already_initialized = execute_if(
            Condition::DataExists(plan.init_sentinel().clone()),
            command(
                CommandKind::Return(ReturnCommand::Value(1)),
                OriginId::UNKNOWN,
            )?,
        )?;
        let commit = command(
            CommandKind::Data(DataCommand::Modify {
                target: plan.init_sentinel().clone(),
                mode: DataModifyMode::Set,
                source: DataSource::Value(NbtValue::byte(1)),
            }),
            OriginId::UNKNOWN,
        )?;
        let create_and_commit = execute_if(
            Condition::Function(init),
            command(
                CommandKind::Return(ReturnCommand::run(commit)),
                OriginId::UNKNOWN,
            )?,
        )?;
        let fail = command(CommandKind::Return(ReturnCommand::Fail), OriginId::UNKNOWN)?;
        let mut load_commands = Vec::with_capacity(if plan.has_recursive_activation() {
            4
        } else {
            3
        });
        if plan.has_recursive_activation() {
            let empty_frames = NbtValue::list(Vec::new()).map_err(|error| {
                invariant_diagnostics(
                    format!("compiler activation-list literal is invalid: {error}"),
                    OriginId::UNKNOWN,
                )
            })?;
            load_commands.push(command(
                CommandKind::Data(DataCommand::Modify {
                    target: plan.activation_frames(),
                    mode: DataModifyMode::Set,
                    source: DataSource::Value(empty_frames),
                }),
                OriginId::UNKNOWN,
            )?);
        }
        load_commands.extend([already_initialized, create_and_commit, fail]);
        define_commands(&mut self.builder, load, load_commands)
    }

    pub(crate) fn define_external_helper(
        &mut self,
        planned: PlannedFunctionId,
        body: CommandNode,
    ) -> Result<(), Diagnostics> {
        let function = self.declared_function(planned)?;
        define_commands(&mut self.builder, function, vec![body])
    }

    pub(crate) fn function(&self, planned: PlannedFunctionId) -> Result<McFunctionId, Diagnostics> {
        self.declared_function(planned)
    }

    fn declared_function(&self, planned: PlannedFunctionId) -> Result<McFunctionId, Diagnostics> {
        self.declarations.function(planned).ok_or_else(|| {
            invariant_diagnostics(
                "verified initialization function has no Stage 3 declaration",
                OriginId::UNKNOWN,
            )
        })
    }
}

/// Function-scoped target construction with no authority to alter planning.
pub(crate) struct FunctionLoweringCx<'a, 'builder> {
    #[cfg(test)]
    body: &'a crate::ir::core::FunctionBody,
    plan: &'a LoweringPlan,
    planned: PlannedFunctionId,
    declarations: &'a DeclarationMap,
    target_body: FunctionBodyBuilder<'builder>,
}

impl<'a> FunctionLoweringCx<'a, '_> {
    #[cfg(test)]
    pub(crate) const fn body(&self) -> &'a crate::ir::core::FunctionBody {
        self.body
    }

    pub(crate) fn score(&self, home: HomeId) -> Result<ScoreRef, Diagnostics> {
        self.plan.score(home).ok_or_else(|| {
            invariant_diagnostics(
                "verified plan references an absent score home",
                OriginId::UNKNOWN,
            )
        })
    }

    pub(crate) fn function(&self, planned: PlannedFunctionId) -> Result<McFunctionId, Diagnostics> {
        self.declarations.function(planned).ok_or_else(|| {
            invariant_diagnostics(
                "verified plan references an absent target declaration",
                OriginId::UNKNOWN,
            )
        })
    }

    pub(crate) fn require_type(
        &self,
        home: HomeId,
        expected: crate::ir::core::CoreType,
    ) -> Result<(), Diagnostics> {
        match self.plan.home_type(home) {
            Some(actual) if actual == expected => Ok(()),
            actual => Err(invariant_diagnostics(
                format!("score home {home:?} has type {actual:?}, expected {expected}"),
                OriginId::UNKNOWN,
            )),
        }
    }

    pub(crate) fn require_same_type(&self, left: HomeId, right: HomeId) -> Result<(), Diagnostics> {
        let left_type = self.plan.home_type(left);
        let right_type = self.plan.home_type(right);
        if left_type.is_some() && left_type == right_type {
            Ok(())
        } else {
            Err(invariant_diagnostics(
                format!("score homes {left:?} and {right:?} have incompatible types"),
                OriginId::UNKNOWN,
            ))
        }
    }

    pub(crate) const fn plan(&self) -> &'a LoweringPlan {
        self.plan
    }

    pub(crate) const fn planned_function(&self) -> PlannedFunctionId {
        self.planned
    }

    pub(crate) fn push(&mut self, command: CommandNode) -> Result<(), Diagnostics> {
        self.push_correlated(command).map(|_| ())
    }

    /// Appends one top-level command and retains its exact function-local identity.
    pub(crate) fn push_correlated(
        &mut self,
        command: CommandNode,
    ) -> Result<CommandId, Diagnostics> {
        let origin = command.origin();
        self.target_body
            .push(command)
            .map_err(|error| construction_diagnostics(&error, origin))
    }

    pub(crate) fn finish(self) {
        self.target_body.finish();
    }
}

fn construction_diagnostics(error: &BuildError, origin: OriginId) -> Diagnostics {
    match error {
        BuildError::EntityLimit => capacity_diagnostics(origin),
        BuildError::DuplicateFunctionResource(_)
        | BuildError::DuplicateFunctionTagResource(_)
        | BuildError::InvalidFunction(_)
        | BuildError::InvalidFunctionTag(_)
        | BuildError::FunctionAlreadyDefined(_)
        | BuildError::FunctionTagAlreadyDefined(_) => invariant_diagnostics(
            format!("verified target declaration failed: {error}"),
            origin,
        ),
    }
}

pub(super) fn command(kind: CommandKind, origin: OriginId) -> Result<CommandNode, Diagnostics> {
    CommandNode::new(kind, origin).map_err(|error| {
        invariant_diagnostics(
            format!("compiler-generated command exceeds structural depth: {error}"),
            origin,
        )
    })
}

fn execute_if(condition: Condition, run: CommandNode) -> Result<CommandNode, Diagnostics> {
    command(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(
                ExecuteModifier::new(ExecuteModifierKind::If(condition), OriginId::UNKNOWN),
                vec![],
            ),
            run,
        )),
        OriginId::UNKNOWN,
    )
}

fn define_commands(
    builder: &mut MinecraftProgramBuilder,
    function: McFunctionId,
    commands: Vec<CommandNode>,
) -> Result<(), Diagnostics> {
    let mut body = builder
        .begin_function(function)
        .map_err(|error| construction_diagnostics(&error, OriginId::UNKNOWN))?;
    for command in commands {
        let origin = command.origin();
        body.push(command)
            .map_err(|error| construction_diagnostics(&error, origin))?;
    }
    body.finish();
    Ok(())
}

fn capacity_diagnostics(origin: OriginId) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.target-entity-limit",
        "Minecraft target entity ID space is exhausted",
        origin,
    ))
}

pub(super) fn invariant_diagnostics(message: impl Into<String>, origin: OriginId) -> Diagnostics {
    one_diagnostic(Diagnostic::new(
        "lower.construction-invariant",
        message,
        origin,
    ))
}

fn one_diagnostic(finding: Diagnostic) -> Diagnostics {
    Diagnostics::from_findings(vec![finding]).expect("one finding always forms diagnostics")
}

#[cfg(test)]
mod tests {
    use super::{TargetConstruction, construction_diagnostics};
    use crate::entity::EntityId;
    use crate::ir::core::{CoreProgram, FunctionBuilder, Terminator, TerminatorKind};
    use crate::ir::minecraft::{
        BuildError, FunctionTagEntryKind, FunctionTagMerge, InternalCallableRef, McFunctionId,
        ObjectiveName, PackNamespace, render_function,
    };
    use crate::lower::minecraft::LoweringOptions;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::plan::PlanBuilder;
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    #[test]
    fn declares_dense_functions_and_the_append_only_load_tag() {
        let (core, analyses, plan, _) = plan();
        let mut construction = TargetConstruction::declare(&plan).unwrap();
        for (planned, _) in plan.planned_functions() {
            assert_eq!(
                construction.declarations.function(planned).unwrap().index(),
                planned.index()
            );
        }
        for (_, target) in construction.declarations.functions.iter() {
            construction
                .builder
                .begin_function(*target)
                .unwrap()
                .finish();
        }
        let load_tag = construction.declarations.load_tag();
        let program = construction.builder.finish().unwrap();
        let tag = program.function_tag(load_tag).unwrap();
        assert_eq!(tag.resource().to_string(), "minecraft:load");
        assert_eq!(tag.merge(), FunctionTagMerge::Append);
        assert_eq!(tag.entries().len(), 1);
        assert!(matches!(
            tag.entries()[0].kind(),
            FunctionTagEntryKind::Internal(InternalCallableRef::Function(function))
                if *function == construction.declarations.function(plan.load()).unwrap()
        ));
        drop((core, analyses));
    }

    #[test]
    fn function_context_resolves_only_typed_plan_and_declaration_ids() {
        let (core, _analyses, plan, value) = plan();
        let function = core.functions().next().unwrap().0;
        let body = core.function(function).unwrap().body().unwrap();
        let planned = plan.block_function(function, body.entry()).unwrap();
        let home = plan.value_home(function, value).unwrap();
        let mut construction = TargetConstruction::declare(&plan).unwrap();
        let expected_target = construction.declarations().function(planned).unwrap();

        let context = construction.begin_function(body, &plan, planned).unwrap();

        assert_eq!(context.body().entry(), body.entry());
        assert_eq!(context.score(home).unwrap().objective().as_str(), "mdl.reg");
        assert_eq!(context.function(planned).unwrap(), expected_target);
        context.finish();
    }

    #[test]
    fn initialization_is_exact_collision_safe_structured_ir() {
        let (_core, _analyses, plan, _) = plan();
        let mut construction = TargetConstruction::declare(&plan).unwrap();

        construction.define_initialization(&plan).unwrap();
        for (planned, _) in plan.planned_functions() {
            if planned == plan.load() || planned == plan.init_try_create() {
                continue;
            }
            let target = construction.declarations.function(planned).unwrap();
            construction
                .builder
                .begin_function(target)
                .unwrap()
                .finish();
        }
        let load = construction.declarations.function(plan.load()).unwrap();
        let init = construction
            .declarations
            .function(plan.init_try_create())
            .unwrap();
        let program = construction.builder.finish().unwrap();

        assert_eq!(
            String::from_utf8(render_function(&program, program.function(init).unwrap()).unwrap())
                .unwrap(),
            "return run scoreboard objectives add mdl.reg dummy\n"
        );
        assert_eq!(
            String::from_utf8(render_function(&program, program.function(load).unwrap()).unwrap())
                .unwrap(),
            concat!(
                "execute if data storage mdl:__mdl/init/v0/6d646c2e726567 \"initialized\" run return 1\n",
                "execute if function mdl:__mdl/init/try_create run return run data modify storage mdl:__mdl/init/v0/6d646c2e726567 \"initialized\" set value 1b\n",
                "return fail\n"
            )
        );
        for function in [
            program.function(load).unwrap(),
            program.function(init).unwrap(),
        ] {
            assert!(
                function
                    .body()
                    .commands()
                    .all(|(_, command)| command.origin() == OriginId::UNKNOWN)
            );
        }
    }

    #[test]
    fn synthetic_stage3_build_errors_have_stable_lowering_diagnostics() {
        let capacity = construction_diagnostics(&BuildError::EntityLimit, OriginId::UNKNOWN);
        assert!(capacity.contains_code("lower.target-entity-limit"));

        let invariant = construction_diagnostics(
            &BuildError::InvalidFunction(McFunctionId::from_index(99)),
            OriginId::UNKNOWN,
        );
        assert!(invariant.contains_code("lower.construction-invariant"));
    }

    fn plan() -> (
        CoreProgram,
        SemanticInventory,
        crate::lower::minecraft::plan::LoweringPlan,
        crate::ir::core::ValueId,
    ) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("entry"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut body = FunctionBuilder::new(&core, &sources, function).unwrap();
        let value = body.i32_constant(7, OriginId::UNKNOWN).unwrap();
        body.terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
        core.define_function(function, body.finish().unwrap())
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
        (core, analyses, plan, value)
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
