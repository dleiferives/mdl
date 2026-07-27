//! Closed, program-owned declarations for behavior outside Core's scalar vocabulary.

use std::error::Error;
use std::fmt;

use super::{
    CoreProgram, CoreType, EntityNbtReadId, EntityNbtWriteId, ExternalOpId, FunctionReference,
    ProgramError, RunScopeId, TargetFragmentId,
};
use crate::entity::EntityLimitError;
use crate::ir::command_line::{CommandLineShapeError, validate_command_line_shape};
use crate::ir::semantic::{RuntimeValueType, SemanticType, minecraft_descriptor};
use crate::source::OriginId;

/// Why an unsafe Minecraft command fragment was rejected before target selection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnsafeCommandFragmentError {
    /// No command text was supplied.
    Empty,
    /// The fragment contains a physical CR or LF.
    PhysicalNewline,
    /// The fragment begins or ends with whitespace.
    BoundaryWhitespace,
    /// The first character is reserved for slash input, comments, or macros.
    ReservedPrefix(char),
    /// A terminal backslash would continue the next physical line.
    TerminalContinuation,
}

impl From<CommandLineShapeError> for UnsafeCommandFragmentError {
    fn from(error: CommandLineShapeError) -> Self {
        match error {
            CommandLineShapeError::Empty => Self::Empty,
            CommandLineShapeError::PhysicalNewline => Self::PhysicalNewline,
            CommandLineShapeError::BoundaryWhitespace => Self::BoundaryWhitespace,
            CommandLineShapeError::ReservedPrefix(prefix) => Self::ReservedPrefix(prefix),
            CommandLineShapeError::TerminalContinuation => Self::TerminalContinuation,
        }
    }
}

impl fmt::Display for UnsafeCommandFragmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("unsafe command fragment cannot be empty"),
            Self::PhysicalNewline => {
                formatter.write_str("unsafe command fragment cannot contain CR or LF")
            }
            Self::BoundaryWhitespace => {
                formatter.write_str("unsafe command fragment cannot begin or end with whitespace")
            }
            Self::ReservedPrefix(prefix) => {
                write!(
                    formatter,
                    "unsafe command fragment cannot start with {prefix:?}"
                )
            }
            Self::TerminalContinuation => formatter
                .write_str("unsafe command fragment cannot end with a continuation backslash"),
        }
    }
}

impl Error for UnsafeCommandFragmentError {}

/// One target-independent, physically valid, unparsed Minecraft command line.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnsafeMinecraftCommandFragment(Box<str>);

impl UnsafeMinecraftCommandFragment {
    #[cfg(test)]
    pub(crate) fn from_unchecked(line: &str) -> Self {
        Self(line.into())
    }

    /// Validates physical-line shape without assuming a target encoding limit or
    /// parsing Brigadier syntax.
    ///
    /// # Errors
    ///
    /// Rejects empty text, physical newlines, boundary whitespace, reserved
    /// prefixes, and terminal line continuation.
    pub fn new(line: &str) -> Result<Self, UnsafeCommandFragmentError> {
        validate_command_line_shape(line).map_err(UnsafeCommandFragmentError::from)?;
        Ok(Self(line.into()))
    }

    /// Returns the unparsed physical command text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One immutable target-facing fragment owned by a Core program.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TargetFragment {
    /// One explicitly unsafe, unparsed Minecraft command line.
    UnsafeMinecraftCommand(UnsafeMinecraftCommandFragment),
}

impl TargetFragment {
    /// Constructs an unsafe Minecraft command fragment after physical validation.
    ///
    /// # Errors
    ///
    /// Returns the target-independent line-shape failure.
    pub fn unsafe_minecraft_command(line: &str) -> Result<Self, UnsafeCommandFragmentError> {
        UnsafeMinecraftCommandFragment::new(line).map(Self::UnsafeMinecraftCommand)
    }
}

/// Closed semantic identity resolved by one external Core declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExternalSemanticBinding {
    /// Emit one program-owned target fragment without assigning it safe semantics.
    UnsafeTargetFragment(TargetFragmentId),
    /// Invoke one program-owned ordered Minecraft execution scope.
    MinecraftRunScope(RunScopeId),
    /// Invoke one program-owned normalized typed Minecraft operation.
    MinecraftOperation(super::MinecraftOperationId),
    /// Read one program-owned schema-typed entity-NBT path.
    EntityNbtRead(EntityNbtReadId),
    /// Write one program-owned whole-slot entity-NBT path (PS-16, BE-2).
    EntityNbtWrite(EntityNbtWriteId),
}

impl ExternalSemanticBinding {
    pub(crate) fn function_references(
        self,
        program: &CoreProgram,
    ) -> impl Iterator<Item = FunctionReference> + '_ {
        let scope = match self {
            Self::MinecraftRunScope(scope) => program.run_scope(scope),
            Self::UnsafeTargetFragment(_)
            | Self::MinecraftOperation(_)
            | Self::EntityNbtRead(_)
            | Self::EntityNbtWrite(_) => None,
        };
        scope
            .into_iter()
            .flat_map(super::RunScopeDecl::function_references)
    }

    pub(crate) fn is_well_formed(
        self,
        program: &CoreProgram,
        parameters: &[CoreType],
        results: &[CoreType],
    ) -> bool {
        match self {
            Self::UnsafeTargetFragment(fragment) => {
                program.target_fragment(fragment).is_some()
                    && parameters.is_empty()
                    && results.is_empty()
            }
            Self::MinecraftRunScope(scope) => {
                program
                    .run_scope(scope)
                    .is_some_and(|scope| scope.is_well_formed(program))
                    && parameters.is_empty()
                    && results.is_empty()
            }
            Self::MinecraftOperation(operation) => program
                .minecraft_operation(operation)
                .is_some_and(|operation| {
                    let signature = minecraft_descriptor(operation.key()).signature();
                    // Extra runtime operands beyond the semantic descriptor's
                    // static operands come from macro-typed attributes (e.g. a
                    // runtime book-page index). The first N parameters must
                    // match the descriptor; additional ones are runtime values
                    // validated when the body is lowered.
                    let static_count = signature.operands().len();
                    let static_params = &parameters[..parameters.len().min(static_count)];
                    operation.is_well_formed()
                        && parameters.len() >= static_count
                        && semantic_runtime_types_match_core(signature.operands(), static_params)
                        && semantic_runtime_types_match_core(signature.results(), results)
                }),
            Self::EntityNbtRead(read) => program.entity_nbt_read(read).is_some_and(|read| {
                read.is_well_formed()
                    && parameters.len() == read.runtime_index_count()
                    && parameters.iter().all(|ty| *ty == CoreType::I32)
                    && results == [read.result_ty()]
            }),
            Self::EntityNbtWrite(write) => program.entity_nbt_write(write).is_some_and(|write| {
                write.is_well_formed()
                    && *parameters == *write.runtime_operand_types()
                    && results.is_empty()
            }),
        }
    }
}

fn semantic_runtime_types_match_core(semantic: &[SemanticType], core: &[CoreType]) -> bool {
    semantic.len() == core.len()
        && semantic.iter().zip(core).all(|(semantic, core)| {
            matches!(
                (semantic.runtime(), core),
                (Some(RuntimeValueType::Bool), CoreType::Bool)
                    | (Some(RuntimeValueType::Int32), CoreType::I32)
                    | (Some(RuntimeValueType::String), CoreType::String)
            )
        })
}

/// One closed linked declaration referenced by [`super::CoreOp::External`].
#[derive(Clone, Debug)]
pub struct ExternalOpDecl {
    pub(crate) binding: ExternalSemanticBinding,
    pub(crate) parameters: Vec<CoreType>,
    pub(crate) results: Vec<CoreType>,
    pub(crate) origin: OriginId,
}

impl ExternalOpDecl {
    /// Returns the closed external semantic binding.
    #[must_use]
    pub const fn binding(&self) -> ExternalSemanticBinding {
        self.binding
    }

    /// Returns ordered SSA operand types.
    #[must_use]
    pub fn parameters(&self) -> &[CoreType] {
        &self.parameters
    }

    /// Returns ordered SSA result types.
    #[must_use]
    pub fn results(&self) -> &[CoreType] {
        &self.results
    }

    /// Returns declaration provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    pub(crate) fn function_references<'a>(
        &'a self,
        program: &'a CoreProgram,
    ) -> impl Iterator<Item = FunctionReference> + 'a {
        self.binding.function_references(program)
    }

    pub(crate) fn is_well_formed(&self, program: &CoreProgram) -> bool {
        self.binding
            .is_well_formed(program, &self.parameters, &self.results)
    }
}

impl CoreProgram {
    /// Stores one immutable target fragment in stable allocation order.
    ///
    /// # Errors
    ///
    /// Returns an error if the fragment ID space is exhausted.
    pub fn declare_target_fragment(
        &mut self,
        fragment: TargetFragment,
    ) -> Result<TargetFragmentId, ProgramError> {
        self.target_fragments
            .push(fragment)
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Returns a target fragment, or `None` for an ID outside this program.
    #[must_use]
    pub fn target_fragment(&self, fragment: TargetFragmentId) -> Option<&TargetFragment> {
        self.target_fragments.get(fragment)
    }

    /// Iterates target fragments in stable ID order.
    #[must_use]
    pub fn target_fragments(
        &self,
    ) -> impl ExactSizeIterator<Item = (TargetFragmentId, &TargetFragment)> + '_ {
        self.target_fragments.iter()
    }

    /// Declares one linked external operation with an ordered typed signature.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding refers outside this program or the
    /// external-operation ID space is exhausted.
    pub fn declare_external_op(
        &mut self,
        binding: ExternalSemanticBinding,
        parameters: Vec<CoreType>,
        results: Vec<CoreType>,
        origin: OriginId,
    ) -> Result<ExternalOpId, ProgramError> {
        match binding {
            ExternalSemanticBinding::UnsafeTargetFragment(fragment)
                if self.target_fragment(fragment).is_none() =>
            {
                return Err(ProgramError::InvalidTargetFragment { fragment });
            }
            ExternalSemanticBinding::UnsafeTargetFragment(_) => {}
            ExternalSemanticBinding::MinecraftRunScope(scope) => {
                if self.run_scope(scope).is_none() {
                    return Err(ProgramError::InvalidRunScope { scope });
                }
            }
            ExternalSemanticBinding::MinecraftOperation(operation) => {
                if self.minecraft_operation(operation).is_none() {
                    return Err(ProgramError::InvalidMinecraftOperation);
                }
            }
            ExternalSemanticBinding::EntityNbtRead(read) => {
                if self.entity_nbt_read(read).is_none() {
                    return Err(ProgramError::InvalidEntityNbtReadReference { read });
                }
            }
            ExternalSemanticBinding::EntityNbtWrite(write) => {
                if self.entity_nbt_write(write).is_none() {
                    return Err(ProgramError::InvalidEntityNbtWriteReference { write });
                }
            }
        }
        if !binding.is_well_formed(self, &parameters, &results) {
            return Err(ProgramError::InvalidExternalSignature { binding });
        }
        self.external_ops
            .push(ExternalOpDecl {
                binding,
                parameters,
                results,
                origin,
            })
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Returns an external declaration, or `None` for an ID outside this program.
    #[must_use]
    pub fn external_op(&self, operation: ExternalOpId) -> Option<&ExternalOpDecl> {
        self.external_ops.get(operation)
    }

    /// Iterates external declarations in stable ID order.
    #[must_use]
    pub fn external_ops(
        &self,
    ) -> impl ExactSizeIterator<Item = (ExternalOpId, &ExternalOpDecl)> + '_ {
        self.external_ops.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExternalSemanticBinding, TargetFragment, UnsafeCommandFragmentError};
    use crate::entity::EntityId;
    use crate::ir::core::{
        CoreFunctionLinkage, CoreProgram, CoreType, EntityQueryDecl, ProgramError,
        RunModifierInstance, TargetFragmentId,
    };
    use crate::ir::semantic::{EntityKind, StaticEntityQuery};
    use crate::source::OriginId;

    #[test]
    fn fragments_are_physically_validated_before_target_selection() {
        assert_eq!(
            TargetFragment::unsafe_minecraft_command("say\nno"),
            Err(UnsafeCommandFragmentError::PhysicalNewline)
        );
        let fragment = TargetFragment::unsafe_minecraft_command("say yes").unwrap();
        let TargetFragment::UnsafeMinecraftCommand(line) = fragment;
        assert_eq!(line.as_str(), "say yes");
    }

    #[test]
    fn declarations_are_typed_and_cannot_reference_foreign_fragments() {
        let mut program = CoreProgram::new();
        assert_eq!(
            program.declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(TargetFragmentId::from_index(4)),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidTargetFragment {
                fragment: TargetFragmentId::from_index(4),
            })
        );

        let fragment = program
            .declare_target_fragment(TargetFragment::unsafe_minecraft_command("say yes").unwrap())
            .unwrap();
        assert_eq!(
            program.declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![CoreType::I32],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidExternalSignature {
                binding: ExternalSemanticBinding::UnsafeTargetFragment(fragment),
            })
        );
        let operation = program
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let declaration = program.external_op(operation).unwrap();
        assert!(declaration.parameters().is_empty());
        assert!(declaration.results().is_empty());
        assert_eq!(program.target_fragments().len(), 1);
        assert_eq!(program.external_ops().len(), 1);
    }

    #[test]
    fn large_linked_inventories_allocate_linearly_in_source_order() {
        const COUNT: usize = 20_000;
        let mut program = CoreProgram::new();
        for index in 0..COUNT {
            let fragment = program
                .declare_target_fragment(
                    TargetFragment::unsafe_minecraft_command(&format!("say {index}")).unwrap(),
                )
                .unwrap();
            let operation = program
                .declare_external_op(
                    ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                    vec![],
                    vec![],
                    OriginId::UNKNOWN,
                )
                .unwrap();
            assert_eq!(usize::try_from(fragment.index()).unwrap(), index);
            assert_eq!(usize::try_from(operation.index()).unwrap(), index);
        }
        assert_eq!(program.target_fragments().len(), COUNT);
        assert_eq!(program.external_ops().len(), COUNT);
    }

    #[test]
    fn run_scope_requires_an_internal_void_zero_argument_body() {
        let mut program = CoreProgram::new();
        let query = program
            .declare_entity_query(EntityQueryDecl::from_semantic(
                StaticEntityQuery::entities(EntityKind::ArmorStand),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let parameterized = program
            .declare_function(
                Some("parameterized"),
                vec![CoreType::I32],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let exported = program
            .declare_function_with_linkage(
                Some("exported"),
                CoreFunctionLinkage::DatapackExport,
                false,
                false,
                false,
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();

        for body in [parameterized, exported] {
            assert_eq!(
                program.declare_run_scope(
                    vec![RunModifierInstance::AsEntityQuery {
                        query,
                        origin: OriginId::UNKNOWN,
                    }],
                    body,
                    OriginId::UNKNOWN,
                ),
                Err(ProgramError::InvalidRunScopeBody { function: body })
            );
        }
    }
}
