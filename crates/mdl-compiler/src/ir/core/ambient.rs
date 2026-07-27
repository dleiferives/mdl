//! Target-independent ambient execution-context analysis for Core.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use super::{
    ControlFlowGraph, CoreOp, CoreProgram, EntityNbtReadId, EntityNbtWriteId, ExternalOpId,
    ExternalSemanticBinding, FunctionId, MinecraftOperationId, Reachability, RunModifierInstance,
    RunScopeId,
};
use crate::entity::EntityId;
use crate::ir::semantic::{AmbientContextRequirements, ContextRequirement, EntityCapability};

/// Failure to derive or independently verify Core ambient requirements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CoreAmbientAnalysisError {
    /// A function declaration has no body.
    MissingDefinition { function: FunctionId },
    /// Reachable Core names a function outside this program.
    InvalidFunctionReference {
        caller: FunctionId,
        callee: FunctionId,
    },
    /// Reachable Core names an external declaration outside this program.
    InvalidExternalOperation {
        function: FunctionId,
        operation: ExternalOpId,
    },
    /// An external binding names a typed operation outside this program.
    InvalidMinecraftOperation {
        function: FunctionId,
        operation: MinecraftOperationId,
    },
    /// An external binding names an entity-NBT path read outside this program.
    InvalidEntityNbtRead {
        function: FunctionId,
        read: EntityNbtReadId,
    },
    /// An external binding names an entity-NBT path write outside this program.
    InvalidEntityNbtWrite {
        function: FunctionId,
        write: EntityNbtWriteId,
    },
    /// An external binding names a malformed or absent run scope.
    InvalidRunScope {
        function: FunctionId,
        scope: RunScopeId,
    },
    /// A run modifier cannot satisfy the outlined body's precise requirement.
    IncompatibleRunScopeRequirement {
        function: FunctionId,
        scope: RunScopeId,
    },
    /// A retained result differs from independent recomputation.
    SummaryMismatch { function: FunctionId },
    /// A dense identity cannot be represented on this host.
    IdentitySpaceExhausted,
}

impl fmt::Display for CoreAmbientAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDefinition { function } => {
                write!(formatter, "Core function {function:?} has no definition")
            }
            Self::InvalidFunctionReference { caller, callee } => write!(
                formatter,
                "Core function {caller:?} references invalid function {callee:?}"
            ),
            Self::InvalidExternalOperation {
                function,
                operation,
            } => write!(
                formatter,
                "Core function {function:?} references invalid external operation {operation:?}"
            ),
            Self::InvalidMinecraftOperation {
                function,
                operation,
            } => write!(
                formatter,
                "Core function {function:?} references invalid Minecraft operation {operation:?}"
            ),
            Self::InvalidEntityNbtRead { function, read } => write!(
                formatter,
                "Core function {function:?} references invalid entity-NBT read {read:?}"
            ),
            Self::InvalidEntityNbtWrite { function, write } => write!(
                formatter,
                "Core function {function:?} references invalid entity-NBT write {write:?}"
            ),
            Self::InvalidRunScope { function, scope } => write!(
                formatter,
                "Core function {function:?} references invalid run scope {scope:?}"
            ),
            Self::IncompatibleRunScopeRequirement { function, scope } => write!(
                formatter,
                "Core function {function:?} has incompatible requirements in run scope {scope:?}"
            ),
            Self::SummaryMismatch { function } => write!(
                formatter,
                "retained ambient summary for Core function {function:?} does not match recomputation"
            ),
            Self::IdentitySpaceExhausted => {
                formatter.write_str("Core ambient-analysis identity space is exhausted")
            }
        }
    }
}

impl Error for CoreAmbientAnalysisError {}

/// Dense, target-independent ambient requirements for every Core function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreAmbientAnalysis {
    requirements: Box<[AmbientContextRequirements]>,
}

impl CoreAmbientAnalysis {
    /// Computes entry-reachable requirements to a deterministic fixed point.
    ///
    /// # Errors
    ///
    /// Returns a structured invariant failure for malformed program references or
    /// incompatible ordered context transfer.
    pub fn analyze(program: &CoreProgram) -> Result<Self, CoreAmbientAnalysisError> {
        let equations = build_equations(program)?;
        let mut requirements = vec![AmbientContextRequirements::NONE; equations.len()];
        let mut callers = vec![Vec::new(); equations.len()];
        for (caller, equation) in equations.iter().enumerate() {
            for dependency in &equation.dependencies {
                callers[dependency.callee].push(caller);
            }
        }

        let mut queue = (0..equations.len()).collect::<VecDeque<_>>();
        let mut queued = vec![true; equations.len()];
        while let Some(function) = queue.pop_front() {
            queued[function] = false;
            let mut next = equations[function].direct;
            for dependency in &equations[function].dependencies {
                let callee = requirements[dependency.callee];
                let transferred = match dependency.transfer {
                    DependencyTransfer::Identity => callee,
                    DependencyTransfer::RunScope(scope) => transfer_run_scope_requirements(
                        program,
                        equations[function].function,
                        scope,
                        callee,
                    )?,
                };
                next = next.join(transferred);
            }
            if next == requirements[function] {
                continue;
            }
            requirements[function] = next;
            for &caller in &callers[function] {
                if !queued[caller] {
                    queued[caller] = true;
                    queue.push_back(caller);
                }
            }
        }
        Ok(Self {
            requirements: requirements.into_boxed_slice(),
        })
    }

    /// Returns one function's derived requirement.
    #[must_use]
    pub fn requirement(&self, function: FunctionId) -> Option<AmbientContextRequirements> {
        function_index(function)
            .ok()
            .and_then(|index| self.requirements.get(index))
            .copied()
    }

    /// Returns the number of dense function summaries.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.requirements.len()
    }

    /// Returns whether no function summaries are present.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.requirements.is_empty()
    }

    /// Iterates summaries in stable function identity order.
    ///
    /// # Panics
    ///
    /// Panics only if a previously verified Core inventory exceeds its typed
    /// 32-bit function identity domain, which program construction prevents.
    #[must_use]
    pub fn requirements(
        &self,
    ) -> impl ExactSizeIterator<Item = (FunctionId, AmbientContextRequirements)> + '_ {
        self.requirements
            .iter()
            .copied()
            .enumerate()
            .map(|(index, requirement)| {
                let index = u32::try_from(index)
                    .expect("ambient analysis length is constrained by Core function IDs");
                (FunctionId::from_index(index), requirement)
            })
    }

    /// Independently recomputes and compares this retained analysis.
    ///
    /// # Errors
    ///
    /// Returns the recomputation failure or the first stable-identity mismatch.
    pub fn verify(&self, program: &CoreProgram) -> Result<(), CoreAmbientAnalysisError> {
        let recomputed = Self::analyze(program)?;
        let count = self.requirements.len().max(recomputed.requirements.len());
        for index in 0..count {
            if self.requirements.get(index) != recomputed.requirements.get(index) {
                let index = u32::try_from(index)
                    .map_err(|_| CoreAmbientAnalysisError::IdentitySpaceExhausted)?;
                return Err(CoreAmbientAnalysisError::SummaryMismatch {
                    function: FunctionId::from_index(index),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct FunctionEquation {
    function: FunctionId,
    direct: AmbientContextRequirements,
    dependencies: Vec<Dependency>,
}

#[derive(Clone, Copy, Debug)]
struct Dependency {
    callee: usize,
    transfer: DependencyTransfer,
}

#[derive(Clone, Copy, Debug)]
enum DependencyTransfer {
    Identity,
    RunScope(RunScopeId),
}

fn build_equations(
    program: &CoreProgram,
) -> Result<Vec<FunctionEquation>, CoreAmbientAnalysisError> {
    let mut equations = Vec::with_capacity(program.len());
    for (function, declaration) in program.functions() {
        let body = declaration
            .body()
            .ok_or(CoreAmbientAnalysisError::MissingDefinition { function })?;
        let cfg = ControlFlowGraph::new(body);
        let reachable = Reachability::new(&cfg);
        let mut equation = FunctionEquation {
            function,
            direct: AmbientContextRequirements::NONE,
            dependencies: Vec::new(),
        };
        for block in body.block_order().iter().copied() {
            if !reachable.contains(block) {
                continue;
            }
            let data = body.block(block).expect("verified attached block has data");
            for instruction in data.instructions() {
                let instruction = body
                    .instruction(*instruction)
                    .expect("verified attached instruction has data");
                include_operation(program, &mut equation, instruction.op())?;
            }
        }
        equations.push(equation);
    }
    Ok(equations)
}

fn include_operation(
    program: &CoreProgram,
    equation: &mut FunctionEquation,
    operation: &CoreOp,
) -> Result<(), CoreAmbientAnalysisError> {
    match operation {
        CoreOp::Call(callee) => equation.dependencies.push(Dependency {
            callee: checked_callee(program, equation.function, *callee)?,
            transfer: DependencyTransfer::Identity,
        }),
        CoreOp::External(operation) => {
            let declaration = program.external_op(*operation).ok_or(
                CoreAmbientAnalysisError::InvalidExternalOperation {
                    function: equation.function,
                    operation: *operation,
                },
            )?;
            match declaration.binding() {
                ExternalSemanticBinding::UnsafeTargetFragment(_) => {
                    equation.direct = equation.direct.join(AmbientContextRequirements::UNKNOWN);
                }
                ExternalSemanticBinding::MinecraftOperation(operation) => {
                    let operation = program.minecraft_operation(operation).ok_or(
                        CoreAmbientAnalysisError::InvalidMinecraftOperation {
                            function: equation.function,
                            operation,
                        },
                    )?;
                    let requirement = operation
                        .attributes()
                        .ambient_requirements(operation.receiver_kind());
                    equation.direct = equation.direct.join(requirement);
                }
                ExternalSemanticBinding::MinecraftRunScope(scope) => {
                    let declaration = program.run_scope(scope).ok_or(
                        CoreAmbientAnalysisError::InvalidRunScope {
                            function: equation.function,
                            scope,
                        },
                    )?;
                    equation.dependencies.push(Dependency {
                        callee: checked_callee(program, equation.function, declaration.callee())?,
                        transfer: DependencyTransfer::RunScope(scope),
                    });
                }
                ExternalSemanticBinding::EntityNbtRead(read) => {
                    let declaration = program.entity_nbt_read(read).ok_or(
                        CoreAmbientAnalysisError::InvalidEntityNbtRead {
                            function: equation.function,
                            read,
                        },
                    )?;
                    let requirement = match declaration.receiver() {
                        super::EntityNbtReceiver::Entity(kind) => AmbientContextRequirements::NONE
                            .with_executor(ContextRequirement::Required(kind)),
                        super::EntityNbtReceiver::Block(..) => AmbientContextRequirements::NONE,
                    };
                    equation.direct = equation.direct.join(requirement);
                }
                ExternalSemanticBinding::EntityNbtWrite(write) => {
                    // Always a block receiver (`EntityNbtWriteDecl::is_well_formed`),
                    // self-contained in its position — no ambient context, like
                    // `EntityNbtRead`'s own `Block` case.
                    program.entity_nbt_write(write).ok_or(
                        CoreAmbientAnalysisError::InvalidEntityNbtWrite {
                            function: equation.function,
                            write,
                        },
                    )?;
                    equation.direct = equation.direct.join(AmbientContextRequirements::NONE);
                }
            }
        }
        // A schedule statement's target is a compile-time literal resource id,
        // not a synchronous invocation — its ambient context requirement
        // (self-rooting) is checked independently on the target's own root,
        // not inherited into this function's equation (Stage 9B).
        CoreOp::Schedule(..) | CoreOp::ScheduleClear(_) => {}
        CoreOp::BoolConstant(_)
        | CoreOp::I32Constant(_)
        | CoreOp::I32AddWrapping
        | CoreOp::I32SubWrapping
        | CoreOp::I32AddOverflowing
        | CoreOp::I32Compare(_)
        | CoreOp::I32InClosedRange(_)
        | CoreOp::BoolNot
        | CoreOp::ListI32Empty
        | CoreOp::ListI32Length
        | CoreOp::ListI32Push
        | CoreOp::ListI32LastOrZero
        | CoreOp::ListI32WithoutLast
        | CoreOp::StringConstant(_)
        | CoreOp::StringLength
        | CoreOp::StringEndsWithAscii(_)
        | CoreOp::StringWithoutLastUnit => {}
    }
    Ok(())
}

fn checked_callee(
    program: &CoreProgram,
    source: FunctionId,
    target: FunctionId,
) -> Result<usize, CoreAmbientAnalysisError> {
    let index = function_index(target)?;
    if index >= program.len() || program.function(target).is_none() {
        return Err(CoreAmbientAnalysisError::InvalidFunctionReference {
            caller: source,
            callee: target,
        });
    }
    Ok(index)
}

fn function_index(function: FunctionId) -> Result<usize, CoreAmbientAnalysisError> {
    usize::try_from(function.index()).map_err(|_| CoreAmbientAnalysisError::IdentitySpaceExhausted)
}

fn transfer_run_scope_requirements(
    program: &CoreProgram,
    function: FunctionId,
    scope: RunScopeId,
    mut requirements: AmbientContextRequirements,
) -> Result<AmbientContextRequirements, CoreAmbientAnalysisError> {
    let declaration = program
        .run_scope(scope)
        .ok_or(CoreAmbientAnalysisError::InvalidRunScope { function, scope })?;
    for modifier in declaration.modifiers().iter().cloned().rev() {
        match modifier {
            RunModifierInstance::AsEntityQuery { query, .. } => {
                let query = program
                    .entity_query(query)
                    .ok_or(CoreAmbientAnalysisError::InvalidRunScope { function, scope })?;
                let kind = query.semantic().kind();
                if !kind
                    .capabilities()
                    .contains(EntityCapability::CommandExecutor)
                {
                    return Err(CoreAmbientAnalysisError::InvalidRunScope { function, scope });
                }
                if matches!(
                    requirements.executor(),
                    ContextRequirement::Required(required) if required != kind
                ) {
                    return Err(CoreAmbientAnalysisError::IncompatibleRunScopeRequirement {
                        function,
                        scope,
                    });
                }
                requirements = requirements.with_executor(ContextRequirement::None).join(
                    AmbientContextRequirements::NONE
                        .with_position(ContextRequirement::Required(()))
                        .with_dimension(ContextRequirement::Required(())),
                );
            }
            RunModifierInstance::AtEntityQuery { query, .. } => {
                program
                    .entity_query(query)
                    .ok_or(CoreAmbientAnalysisError::InvalidRunScope { function, scope })?;
                requirements = requirements
                    .with_position(ContextRequirement::None)
                    .with_rotation(ContextRequirement::None)
                    .with_dimension(ContextRequirement::None)
                    .join(
                        AmbientContextRequirements::NONE
                            .with_position(ContextRequirement::Required(()))
                            .with_dimension(ContextRequirement::Required(())),
                    );
            }
            RunModifierInstance::AtExecutor { kind, .. } => {
                requirements = requirements
                    .with_position(ContextRequirement::None)
                    .with_rotation(ContextRequirement::None)
                    .with_dimension(ContextRequirement::None)
                    .join(
                        AmbientContextRequirements::NONE
                            .with_executor(ContextRequirement::Required(kind)),
                    );
            }
            RunModifierInstance::Positioned { position, .. } => {
                requirements = requirements.with_position(ContextRequirement::None);
                match position {
                    crate::ir::semantic::PositionSpec::World(position)
                        if position.reads_position() =>
                    {
                        requirements = requirements.with_position(ContextRequirement::Required(()));
                    }
                    crate::ir::semantic::PositionSpec::Local(_) => {
                        requirements = requirements
                            .with_position(ContextRequirement::Required(()))
                            .with_rotation(ContextRequirement::Required(()))
                            .with_anchor(ContextRequirement::Required(()));
                    }
                    crate::ir::semantic::PositionSpec::World(_) => {}
                }
            }
            RunModifierInstance::Rotated { rotation, .. } => {
                requirements = requirements.with_rotation(if rotation.reads_rotation() {
                    ContextRequirement::Required(())
                } else {
                    ContextRequirement::None
                });
            }
            RunModifierInstance::In { .. } => {
                requirements = requirements
                    .with_position(ContextRequirement::Required(()))
                    .with_dimension(ContextRequirement::Required(()));
            }
            RunModifierInstance::Anchored { .. } => {
                requirements = requirements.with_anchor(ContextRequirement::None);
            }
            RunModifierInstance::Align { .. } => {
                requirements = requirements.with_position(ContextRequirement::Required(()));
            }
        }
    }
    Ok(requirements)
}

#[cfg(test)]
mod tests {
    use super::{CoreAmbientAnalysis, CoreAmbientAnalysisError};
    use crate::entity::EntityId;
    use crate::ir::core::{
        CoreProgram, EntityQueryDecl, ExternalOpId, ExternalSemanticBinding, FunctionBuilder,
        FunctionId, MinecraftOperationAttributes, MinecraftOperationOrigins, RunModifierInstance,
        TargetFragment, Terminator, TerminatorKind,
    };
    use crate::ir::semantic::{
        AmbientContextRequirements, ContextRequirement, EntityKind, MessageLiteral,
        MinecraftSemanticKey, StaticEntityQuery,
    };
    use crate::source::{OriginId, SourceContext};

    fn declare_void_function(program: &mut CoreProgram, name: &str) -> FunctionId {
        program
            .declare_function(Some(name), vec![], vec![], OriginId::UNKNOWN)
            .unwrap()
    }

    fn define_void_function(
        program: &mut CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
        build: impl FnOnce(&mut FunctionBuilder<'_>),
    ) {
        let body = {
            let mut builder = FunctionBuilder::new(program, sources, function).unwrap();
            build(&mut builder);
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            builder.finish().unwrap()
        };
        program.define_function(function, body).unwrap();
    }

    fn declare_say(program: &mut CoreProgram, message: &str) -> ExternalOpId {
        let operation = program
            .declare_minecraft_operation(
                MinecraftSemanticKey::Say,
                EntityKind::ArmorStand,
                MinecraftOperationAttributes::Say {
                    message: MessageLiteral::new(message).unwrap(),
                    message_origin: OriginId::UNKNOWN,
                },
                MinecraftOperationOrigins::new(
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                ),
            )
            .unwrap();
        program
            .declare_external_op(
                ExternalSemanticBinding::MinecraftOperation(operation),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap()
    }

    fn armor_stand_executor_requirement() -> AmbientContextRequirements {
        AmbientContextRequirements::NONE
            .with_executor(ContextRequirement::Required(EntityKind::ArmorStand))
    }

    #[test]
    fn typed_say_requires_exactly_its_receiver_as_current_executor() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "hello");
        let announce = declare_void_function(&mut program, "announce");
        define_void_function(&mut program, &sources, announce, |builder| {
            builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
        });

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(analysis.len(), 1);
        assert_eq!(
            analysis.requirement(announce),
            Some(armor_stand_executor_requirement())
        );
        assert_eq!(
            analysis.requirements().collect::<Vec<_>>(),
            vec![(announce, armor_stand_executor_requirement())]
        );
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn ordinary_calls_propagate_requirements_through_multiple_functions() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "through calls");
        let leaf = declare_void_function(&mut program, "leaf");
        let relay = declare_void_function(&mut program, "relay");
        let root = declare_void_function(&mut program, "root");
        define_void_function(&mut program, &sources, leaf, |builder| {
            builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
        });
        define_void_function(&mut program, &sources, relay, |builder| {
            builder.call(leaf, vec![], OriginId::UNKNOWN).unwrap();
        });
        define_void_function(&mut program, &sources, root, |builder| {
            builder.call(relay, vec![], OriginId::UNKNOWN).unwrap();
        });

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        for function in [leaf, relay, root] {
            assert_eq!(
                analysis.requirement(function),
                Some(armor_stand_executor_requirement())
            );
        }
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn empty_run_scope_preserves_the_outlined_body_requirement() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "identity scope");
        let body = declare_void_function(&mut program, "body");
        let caller = declare_void_function(&mut program, "caller");
        let scope = program
            .declare_run_scope(vec![], body, OriginId::UNKNOWN)
            .unwrap();
        let run = program
            .declare_external_op(
                ExternalSemanticBinding::MinecraftRunScope(scope),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        define_void_function(&mut program, &sources, body, |builder| {
            builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
        });
        define_void_function(&mut program, &sources, caller, |builder| {
            builder.external(run, vec![], OriginId::UNKNOWN).unwrap();
        });

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(
            analysis.requirement(caller),
            Some(armor_stand_executor_requirement())
        );
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn execute_as_discharges_executor_and_requires_query_frame_components() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "scoped");
        let body = declare_void_function(&mut program, "body");
        let caller = declare_void_function(&mut program, "caller");
        let query = program
            .declare_entity_query(EntityQueryDecl::from_semantic(
                StaticEntityQuery::entities(EntityKind::ArmorStand)
                    .limit(1)
                    .unwrap(),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let scope = program
            .declare_run_scope(
                vec![RunModifierInstance::AsEntityQuery {
                    query,
                    origin: OriginId::UNKNOWN,
                }],
                body,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let run = program
            .declare_external_op(
                ExternalSemanticBinding::MinecraftRunScope(scope),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        define_void_function(&mut program, &sources, body, |builder| {
            builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
        });
        define_void_function(&mut program, &sources, caller, |builder| {
            builder.external(run, vec![], OriginId::UNKNOWN).unwrap();
        });

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(
            analysis.requirement(body),
            Some(armor_stand_executor_requirement())
        );
        assert_eq!(
            analysis.requirement(caller),
            Some(
                AmbientContextRequirements::NONE
                    .with_position(ContextRequirement::Required(()))
                    .with_dimension(ContextRequirement::Required(()))
            )
        );
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn unsafe_target_fragments_force_unknown_for_every_ambient_component() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let fragment = program
            .declare_target_fragment(
                TargetFragment::unsafe_minecraft_command("say opaque").unwrap(),
            )
            .unwrap();
        let unsafe_operation = program
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = declare_void_function(&mut program, "opaque");
        define_void_function(&mut program, &sources, function, |builder| {
            builder
                .external(unsafe_operation, vec![], OriginId::UNKNOWN)
                .unwrap();
        });

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(
            analysis.requirement(function),
            Some(AmbientContextRequirements::UNKNOWN)
        );
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn attached_but_entry_unreachable_operations_do_not_contribute() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "unreachable");
        let function = declare_void_function(&mut program, "reachable-empty");
        let body = {
            let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
            let unreachable = builder.create_block(OriginId::UNKNOWN).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            builder.switch_to_block(unreachable).unwrap();
            builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            builder.finish().unwrap()
        };
        program.define_function(function, body).unwrap();

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(
            analysis.requirement(function),
            Some(AmbientContextRequirements::NONE)
        );
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn recursive_fixed_point_propagates_around_a_large_cycle() {
        const FUNCTION_COUNT: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "cycle");
        let functions = (0..FUNCTION_COUNT)
            .map(|index| declare_void_function(&mut program, &format!("cycle-{index}")))
            .collect::<Vec<_>>();
        for (index, function) in functions.iter().copied().enumerate() {
            let callee = functions[(index + 1) % functions.len()];
            define_void_function(&mut program, &sources, function, |builder| {
                builder.call(callee, vec![], OriginId::UNKNOWN).unwrap();
                if index + 1 == functions.len() {
                    builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
                }
            });
        }

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(analysis.len(), FUNCTION_COUNT);
        for function in functions {
            assert_eq!(
                analysis.requirement(function),
                Some(armor_stand_executor_requirement())
            );
        }
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn iterative_fixed_point_scales_to_a_twenty_thousand_function_chain() {
        const FUNCTION_COUNT: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "chain");
        let functions = (0..FUNCTION_COUNT)
            .map(|index| declare_void_function(&mut program, &format!("chain-{index}")))
            .collect::<Vec<_>>();
        for (index, function) in functions.iter().copied().enumerate() {
            define_void_function(&mut program, &sources, function, |builder| {
                if let Some(callee) = functions.get(index + 1) {
                    builder.call(*callee, vec![], OriginId::UNKNOWN).unwrap();
                } else {
                    builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
                }
            });
        }

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(analysis.len(), FUNCTION_COUNT);
        assert_eq!(
            analysis.requirement(functions[0]),
            Some(armor_stand_executor_requirement())
        );
        assert_eq!(
            analysis.requirement(functions[FUNCTION_COUNT - 1]),
            Some(armor_stand_executor_requirement())
        );
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn iterative_fixed_point_scales_to_twenty_thousand_function_fanout() {
        const FUNCTION_COUNT: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "fanout");
        let leaves = (0..FUNCTION_COUNT - 1)
            .map(|index| declare_void_function(&mut program, &format!("leaf-{index}")))
            .collect::<Vec<_>>();
        let root = declare_void_function(&mut program, "fanout-root");
        for leaf in leaves.iter().copied() {
            define_void_function(&mut program, &sources, leaf, |builder| {
                builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
            });
        }
        define_void_function(&mut program, &sources, root, |builder| {
            for leaf in leaves.iter().copied() {
                builder.call(leaf, vec![], OriginId::UNKNOWN).unwrap();
            }
        });

        let analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        assert_eq!(analysis.len(), FUNCTION_COUNT);
        assert_eq!(
            analysis.requirement(root),
            Some(armor_stand_executor_requirement())
        );
        analysis.verify(&program).unwrap();
    }

    #[test]
    fn retained_analysis_verification_rejects_a_changed_summary() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let say = declare_say(&mut program, "verify");
        let function = declare_void_function(&mut program, "verified");
        define_void_function(&mut program, &sources, function, |builder| {
            builder.external(say, vec![], OriginId::UNKNOWN).unwrap();
        });

        let mut analysis = CoreAmbientAnalysis::analyze(&program).unwrap();
        analysis.verify(&program).unwrap();
        analysis.requirements[usize::try_from(function.index()).unwrap()] =
            AmbientContextRequirements::NONE;
        assert_eq!(
            analysis.verify(&program),
            Err(CoreAmbientAnalysisError::SummaryMismatch { function })
        );
    }
}
