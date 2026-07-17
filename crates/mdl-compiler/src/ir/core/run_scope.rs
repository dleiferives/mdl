//! Program-owned structured Minecraft execution scopes.

use super::{
    CoreFunctionLinkage, CoreProgram, EntityQueryId, FunctionId, FunctionReference,
    FunctionReferenceKind, ProgramError, RunScopeId,
};
use crate::entity::EntityLimitError;
use crate::ir::semantic::{
    Axes, DimensionKey, EntityAnchor, EntityCapability, EntityKind, ExecutionContext,
    InvocationBounds, MAX_RUN_MODIFIERS_PER_SCOPE, PositionSpec, RotationSpec,
};
use crate::source::OriginId;

/// Exact identity of one Core run-modifier context transition.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CoreContextStep {
    scope: RunScopeId,
    modifier_index: usize,
    origin: OriginId,
}

impl CoreContextStep {
    /// Returns the owning structured run scope.
    #[must_use]
    pub const fn scope(self) -> RunScopeId {
        self.scope
    }

    /// Returns the modifier's zero-based position in the ordered scope.
    #[must_use]
    pub const fn modifier_index(self) -> usize {
        self.modifier_index
    }

    /// Returns the modifier occurrence provenance.
    #[must_use]
    pub const fn origin(self) -> OriginId {
        self.origin
    }
}

/// The execution context obtained by replaying Core run modifiers.
pub type CoreExecutionContext = ExecutionContext<CoreContextStep>;

/// One ordered, target-independent execution-context modifier.
///
/// The enum stays deliberately closed. Later modifier families add typed variants
/// rather than erasing semantic meaning into strings or target command fragments.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum RunModifierInstance {
    /// Establish the current executor once per entity selected by a static query.
    AsEntityQuery {
        /// Program-owned semantic query.
        query: EntityQueryId,
        /// Provenance of this modifier call.
        origin: OriginId,
    },
    AtEntityQuery {
        query: EntityQueryId,
        origin: OriginId,
    },
    AtExecutor {
        kind: EntityKind,
        origin: OriginId,
    },
    Positioned {
        position: PositionSpec,
        origin: OriginId,
    },
    Rotated {
        rotation: RotationSpec,
        origin: OriginId,
    },
    In {
        dimension: DimensionKey,
        origin: OriginId,
    },
    Anchored {
        anchor: EntityAnchor,
        origin: OriginId,
    },
    Align {
        axes: Axes,
        origin: OriginId,
    },
}

impl RunModifierInstance {
    /// Returns this modifier's provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        match self {
            Self::AsEntityQuery { origin, .. }
            | Self::AtEntityQuery { origin, .. }
            | Self::AtExecutor { origin, .. }
            | Self::Positioned { origin, .. }
            | Self::Rotated { origin, .. }
            | Self::In { origin, .. }
            | Self::Anchored { origin, .. }
            | Self::Align { origin, .. } => *origin,
        }
    }

    fn invocation_bounds(&self, program: &CoreProgram) -> Option<InvocationBounds> {
        match self {
            Self::AsEntityQuery { query, .. } | Self::AtEntityQuery { query, .. } => {
                program.entity_query(*query).and_then(|query| {
                    query
                        .semantic()
                        .kind()
                        .capabilities()
                        .contains(EntityCapability::CommandExecutor)
                        .then(|| {
                            InvocationBounds::from_query_cardinality(
                                query.semantic().ty().cardinality(),
                            )
                        })
                })
            }
            Self::AtExecutor { .. }
            | Self::Positioned { .. }
            | Self::Rotated { .. }
            | Self::In { .. }
            | Self::Anchored { .. }
            | Self::Align { .. } => Some(InvocationBounds::EXACTLY_ONCE),
        }
    }

    fn function_reference(&self, _modifier_index: usize) -> Option<FunctionReference> {
        match self {
            Self::AsEntityQuery { .. }
            | Self::AtEntityQuery { .. }
            | Self::AtExecutor { .. }
            | Self::Positioned { .. }
            | Self::Rotated { .. }
            | Self::In { .. }
            | Self::Anchored { .. }
            | Self::Align { .. } => None,
        }
    }
}

/// One contextual invocation of an outlined internal function.
#[derive(Clone, Debug)]
pub struct RunScopeDecl {
    modifiers: Box<[RunModifierInstance]>,
    callee: FunctionId,
    invocation_bounds: InvocationBounds,
    origin: OriginId,
}

impl RunScopeDecl {
    /// Returns modifiers in exact semantic/source order.
    #[must_use]
    pub fn modifiers(&self) -> &[RunModifierInstance] {
        &self.modifiers
    }

    /// Returns the outlined internal body.
    #[must_use]
    pub const fn callee(&self) -> FunctionId {
        self.callee
    }

    /// Returns the conservative per-input invocation interval.
    #[must_use]
    pub const fn invocation_bounds(&self) -> InvocationBounds {
        self.invocation_bounds
    }

    /// Returns the complete scope provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Enumerates modifier-owned function edges followed by the outlined body.
    ///
    /// The current `.as` modifier owns no edge, but the ordered modifier walk makes
    /// this genuinely plural before call-like modifiers such as `if function` land.
    pub(crate) fn function_references(&self) -> impl Iterator<Item = FunctionReference> + '_ {
        self.modifiers
            .iter()
            .enumerate()
            .filter_map(|(index, modifier)| modifier.function_reference(index))
            .chain(std::iter::once(FunctionReference::new(
                self.callee,
                self.origin,
                FunctionReferenceKind::RunScopeBody,
            )))
    }

    pub(crate) fn is_well_formed(&self, program: &CoreProgram) -> bool {
        let Some(callee) = program.function(self.callee) else {
            return false;
        };
        self.modifiers.len() <= MAX_RUN_MODIFIERS_PER_SCOPE
            && callee.linkage() == CoreFunctionLinkage::Internal
            && callee.parameters().is_empty()
            && callee.results().is_empty()
            && derive_invocation_bounds(program, &self.modifiers) == Some(self.invocation_bounds)
    }
}

fn derive_invocation_bounds(
    program: &CoreProgram,
    modifiers: &[RunModifierInstance],
) -> Option<InvocationBounds> {
    let mut bounds = InvocationBounds::EXACTLY_ONCE;
    for modifier in modifiers {
        bounds = bounds.multiply(modifier.invocation_bounds(program)?);
    }
    Some(bounds)
}

impl CoreProgram {
    /// Declares one structured contextual invocation in stable allocation order.
    ///
    /// # Errors
    ///
    /// Returns an error for a foreign query, an absent or incompatible outlined
    /// body, or exhaustion of the run-scope identity space.
    pub fn declare_run_scope(
        &mut self,
        modifiers: Vec<RunModifierInstance>,
        callee: FunctionId,
        origin: OriginId,
    ) -> Result<RunScopeId, ProgramError> {
        if modifiers.len() > MAX_RUN_MODIFIERS_PER_SCOPE {
            let origin = modifiers
                .get(MAX_RUN_MODIFIERS_PER_SCOPE)
                .map_or(origin, RunModifierInstance::origin);
            return Err(ProgramError::InvalidRunScopeModifier { origin });
        }
        for modifier in &modifiers {
            match modifier {
                RunModifierInstance::AsEntityQuery { query, .. }
                | RunModifierInstance::AtEntityQuery { query, .. }
                    if self.entity_query(*query).is_none() =>
                {
                    return Err(ProgramError::InvalidEntityQuery { query: *query });
                }
                RunModifierInstance::AsEntityQuery { query, origin } => {
                    let supports_executor = self.entity_query(*query).is_some_and(|query| {
                        query
                            .semantic()
                            .kind()
                            .capabilities()
                            .contains(EntityCapability::CommandExecutor)
                    });
                    if !supports_executor {
                        return Err(ProgramError::InvalidRunScopeModifier { origin: *origin });
                    }
                }
                RunModifierInstance::AtEntityQuery { .. }
                | RunModifierInstance::AtExecutor { .. }
                | RunModifierInstance::Positioned { .. }
                | RunModifierInstance::Rotated { .. }
                | RunModifierInstance::In { .. }
                | RunModifierInstance::Anchored { .. }
                | RunModifierInstance::Align { .. } => {}
            }
        }
        let Some(callee_decl) = self.function(callee) else {
            return Err(ProgramError::InvalidFunction { function: callee });
        };
        if callee_decl.linkage() != CoreFunctionLinkage::Internal
            || !callee_decl.parameters().is_empty()
            || !callee_decl.results().is_empty()
        {
            return Err(ProgramError::InvalidRunScopeBody { function: callee });
        }
        let Some(invocation_bounds) = derive_invocation_bounds(self, &modifiers) else {
            let origin = modifiers
                .first()
                .map_or(origin, RunModifierInstance::origin);
            return Err(ProgramError::InvalidRunScopeModifier { origin });
        };
        self.run_scopes
            .push(RunScopeDecl {
                modifiers: modifiers.into_boxed_slice(),
                callee,
                invocation_bounds,
                origin,
            })
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Returns one structured run-scope declaration.
    #[must_use]
    pub fn run_scope(&self, scope: RunScopeId) -> Option<&RunScopeDecl> {
        self.run_scopes.get(scope)
    }

    /// Iterates run-scope declarations in stable identity order.
    #[must_use]
    pub fn run_scopes(&self) -> impl ExactSizeIterator<Item = (RunScopeId, &RunScopeDecl)> + '_ {
        self.run_scopes.iter()
    }

    /// Replays one scope's context transfer over an incoming execution frame.
    ///
    /// This is a transfer function rather than a cached absolute context because
    /// the same outlined scope may be invoked under different callers. `None`
    /// indicates a malformed scope or query reference.
    #[must_use]
    pub fn apply_run_scope_context(
        &self,
        scope: RunScopeId,
        input: CoreExecutionContext,
    ) -> Option<CoreExecutionContext> {
        let declaration = self.run_scope(scope)?;
        let mut context = input;
        for (modifier_index, modifier) in declaration.modifiers.iter().enumerate() {
            match modifier {
                RunModifierInstance::AsEntityQuery { query, origin } => {
                    let kind = self.entity_query(*query)?.semantic().kind();
                    if !kind
                        .capabilities()
                        .contains(EntityCapability::CommandExecutor)
                    {
                        return None;
                    }
                    context = context.establish_executor(
                        kind,
                        CoreContextStep {
                            scope,
                            modifier_index,
                            origin: *origin,
                        },
                    );
                }
                RunModifierInstance::AtEntityQuery { query, origin } => {
                    self.entity_query(*query)?;
                    let step = CoreContextStep {
                        scope,
                        modifier_index,
                        origin: *origin,
                    };
                    context = context
                        .establish_position(step)
                        .establish_rotation(step)
                        .establish_dimension(step);
                }
                RunModifierInstance::AtExecutor { origin, .. } => {
                    let step = CoreContextStep {
                        scope,
                        modifier_index,
                        origin: *origin,
                    };
                    context = context
                        .establish_position(step)
                        .establish_rotation(step)
                        .establish_dimension(step);
                }
                RunModifierInstance::Positioned { origin, .. }
                | RunModifierInstance::Align { origin, .. } => {
                    context = context.establish_position(CoreContextStep {
                        scope,
                        modifier_index,
                        origin: *origin,
                    });
                }
                RunModifierInstance::Rotated { origin, .. } => {
                    context = context.establish_rotation(CoreContextStep {
                        scope,
                        modifier_index,
                        origin: *origin,
                    });
                }
                RunModifierInstance::In { origin, .. } => {
                    let step = CoreContextStep {
                        scope,
                        modifier_index,
                        origin: *origin,
                    };
                    context = context.establish_position(step).establish_dimension(step);
                }
                RunModifierInstance::Anchored { origin, .. } => {
                    context = context.establish_anchor(CoreContextStep {
                        scope,
                        modifier_index,
                        origin: *origin,
                    });
                }
            }
        }
        Some(context)
    }
}

#[cfg(test)]
mod tests {
    use super::RunModifierInstance;
    use crate::entity::EntityId;
    use crate::ir::core::{
        CoreOp, CoreProgram, EntityQueryDecl, ExternalSemanticBinding, FunctionBuilder, FunctionId,
        ProgramError, RunScopeId, Terminator, TerminatorKind, verify_program,
    };
    use crate::ir::semantic::{
        ContextFact, EntityKind, InvocationBounds, MAX_RUN_MODIFIERS_PER_SCOPE, StaticEntityQuery,
    };
    use crate::source::{OriginId, SourceContext};

    fn define_void_function(
        program: &mut CoreProgram,
        sources: &SourceContext,
        name: &str,
    ) -> FunctionId {
        let function = program
            .declare_function(Some(name), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(program, sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        function
    }

    #[test]
    fn scope_owns_bounds_order_and_the_call_like_edge() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let body = define_void_function(&mut program, &sources, "body");
        let query = program
            .declare_entity_query(EntityQueryDecl::from_semantic(
                StaticEntityQuery::entities(EntityKind::ArmorStand)
                    .limit(1)
                    .unwrap(),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let modifier = RunModifierInstance::AsEntityQuery {
            query,
            origin: OriginId::UNKNOWN,
        };
        let scope = program
            .declare_run_scope(vec![modifier.clone()], body, OriginId::UNKNOWN)
            .unwrap();
        let external = program
            .declare_external_op(
                ExternalSemanticBinding::MinecraftRunScope(scope),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let declaration = program.run_scope(scope).unwrap();
        assert_eq!(declaration.modifiers(), [modifier]);
        assert_eq!(declaration.invocation_bounds().lower(), 0);
        assert_eq!(declaration.invocation_bounds().upper(), Some(1));
        assert_eq!(
            CoreOp::External(external)
                .function_references(&program, OriginId::UNKNOWN)
                .map(crate::ir::core::FunctionReference::function)
                .collect::<Vec<_>>(),
            [body]
        );

        let cloned = program.clone();
        assert_eq!(
            CoreOp::External(external)
                .function_references(&cloned, OriginId::UNKNOWN)
                .map(crate::ir::core::FunctionReference::function)
                .collect::<Vec<_>>(),
            [body]
        );
        verify_program(&cloned, &sources).unwrap();
    }

    #[test]
    fn context_transfer_replays_every_modifier_and_preserves_other_frame_components() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let body = define_void_function(&mut program, &sources, "body");
        let first = program
            .declare_entity_query(EntityQueryDecl::from_semantic(
                StaticEntityQuery::entities(EntityKind::ArmorStand)
                    .limit(1)
                    .unwrap(),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let second = program
            .declare_entity_query(EntityQueryDecl::from_semantic(
                StaticEntityQuery::entities(EntityKind::ArmorStand)
                    .limit(1)
                    .unwrap(),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let scope = program
            .declare_run_scope(
                vec![
                    RunModifierInstance::AsEntityQuery {
                        query: first,
                        origin: OriginId::UNKNOWN,
                    },
                    RunModifierInstance::AsEntityQuery {
                        query: second,
                        origin: OriginId::UNKNOWN,
                    },
                ],
                body,
                OriginId::UNKNOWN,
            )
            .unwrap();

        let input = super::CoreExecutionContext::function_entry();
        let output = program.apply_run_scope_context(scope, input).unwrap();
        let ContextFact::Established { by, .. } = output.executor() else {
            panic!("expected an established executor");
        };
        assert_eq!(by.scope(), scope);
        assert_eq!(by.modifier_index(), 1);
        assert_eq!(output.position(), input.position());
        assert_eq!(output.rotation(), input.rotation());
        assert_eq!(output.dimension(), input.dimension());
        assert_eq!(output.anchor(), input.anchor());
    }

    #[test]
    fn declaration_rejects_foreign_entities_and_external_rejects_foreign_scope() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let body = define_void_function(&mut program, &sources, "body");
        let foreign_query = crate::ir::core::EntityQueryId::from_index(7);
        assert_eq!(
            program.declare_run_scope(
                vec![RunModifierInstance::AsEntityQuery {
                    query: foreign_query,
                    origin: OriginId::UNKNOWN,
                }],
                body,
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidEntityQuery {
                query: foreign_query
            })
        );

        let foreign_function = FunctionId::from_index(9);
        assert_eq!(
            program.declare_run_scope(vec![], foreign_function, OriginId::UNKNOWN),
            Err(ProgramError::InvalidFunction {
                function: foreign_function
            })
        );

        let foreign_scope = RunScopeId::from_index(11);
        assert_eq!(
            program.declare_external_op(
                ExternalSemanticBinding::MinecraftRunScope(foreign_scope),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidRunScope {
                scope: foreign_scope
            })
        );
    }

    #[test]
    fn declaration_rejects_an_over_budget_modifier_chain_before_replay() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let body = define_void_function(&mut program, &sources, "body");
        let modifier = RunModifierInstance::AtExecutor {
            kind: EntityKind::ArmorStand,
            origin: OriginId::UNKNOWN,
        };

        assert_eq!(
            program.declare_run_scope(
                vec![modifier; MAX_RUN_MODIFIERS_PER_SCOPE + 1],
                body,
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidRunScopeModifier {
                origin: OriginId::UNKNOWN,
            })
        );
    }

    #[test]
    fn verifier_recomputes_bounds_instead_of_trusting_the_inventory() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let body = define_void_function(&mut program, &sources, "body");
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
        program.run_scopes.get_mut(scope).unwrap().invocation_bounds =
            InvocationBounds::EXACTLY_ONCE;

        let diagnostics = verify_program(&program, &sources).unwrap_err();
        assert!(diagnostics.contains_code("core.invalid-run-scope"));
    }
}
