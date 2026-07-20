//! Program-owned, target-independent typed Minecraft operations.

use super::{CoreProgram, MacroOrStatic, MinecraftOperationId, ProgramError};
use crate::entity::EntityLimitError;
use crate::ir::semantic::{
    AmbientContextRequirements, ContextRequirement, EntityCapability, EntityKind, MessageLiteral,
    MinecraftSemanticKey, PositionSpec, RelativeWorldOffset, minecraft_descriptor,
};
use crate::source::OriginId;

/// Closed static attributes carried by one normalized Minecraft operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MinecraftOperationAttributes {
    /// Attributes of [`MinecraftSemanticKey::Say`].
    Say {
        /// Plain compile-time message.
        message: MessageLiteral,
        /// Exact source literal provenance.
        message_origin: OriginId,
    },
    /// Static frame-relative teleport destination.
    Teleport {
        position: PositionSpec,
        component_origins: [OriginId; 3],
    },
    /// Static receiver-relative movement offset.
    MoveBy {
        offset: RelativeWorldOffset,
        component_origins: [OriginId; 3],
    },
    /// Static or dynamic-index main-hand written-book literal-page read.
    BookPage {
        page_index: MacroOrStatic<u8>,
        page_origin: OriginId,
    },
}

impl MinecraftOperationAttributes {
    /// Returns the semantic key whose attribute shape this value implements.
    #[must_use]
    pub const fn semantic_key(&self) -> MinecraftSemanticKey {
        match self {
            Self::Say { .. } => MinecraftSemanticKey::Say,
            Self::Teleport { .. } => MinecraftSemanticKey::TeleportCurrentExecutor,
            Self::MoveBy { .. } => MinecraftSemanticKey::MoveCurrentExecutorBy,
            Self::BookPage { .. } => MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage,
        }
    }

    /// Returns the message for a `Say` operation.
    #[must_use]
    pub const fn say_message(&self) -> Option<&MessageLiteral> {
        match self {
            Self::Say { message, .. } => Some(message),
            Self::Teleport { .. } | Self::MoveBy { .. } | Self::BookPage { .. } => None,
        }
    }

    /// Returns the message literal's source provenance.
    #[must_use]
    pub const fn message_origin(&self) -> OriginId {
        match self {
            Self::Say { message_origin, .. } => *message_origin,
            Self::Teleport {
                component_origins, ..
            }
            | Self::MoveBy {
                component_origins, ..
            } => component_origins[0],
            Self::BookPage { page_origin, .. } => *page_origin,
        }
    }

    /// Derives the exact ambient requirements of this attribute instance.
    #[must_use]
    pub fn ambient_requirements(&self, receiver_kind: EntityKind) -> AmbientContextRequirements {
        let base = AmbientContextRequirements::NONE
            .with_executor(ContextRequirement::Required(receiver_kind));
        match self {
            Self::Say { .. } | Self::MoveBy { .. } | Self::BookPage { .. } => base,
            Self::Teleport { position, .. } => {
                let mut requirements = base.with_dimension(ContextRequirement::Required(()));
                match position {
                    PositionSpec::World(position) if position.reads_position() => {
                        requirements = requirements.with_position(ContextRequirement::Required(()));
                    }
                    PositionSpec::Local(_) => {
                        requirements = requirements
                            .with_position(ContextRequirement::Required(()))
                            .with_rotation(ContextRequirement::Required(()))
                            .with_anchor(ContextRequirement::Required(()));
                    }
                    PositionSpec::World(_) => {}
                }
                requirements
            }
        }
    }
}

/// Exact source locations retained after lexical executor proof erasure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MinecraftOperationOrigins {
    call: OriginId,
    member: OriginId,
    receiver: OriginId,
}

impl MinecraftOperationOrigins {
    /// Creates complete provenance for one typed method occurrence.
    #[must_use]
    pub const fn new(call: OriginId, member: OriginId, receiver: OriginId) -> Self {
        Self {
            call,
            member,
            receiver,
        }
    }

    /// Returns complete call-expression provenance.
    #[must_use]
    pub const fn call(self) -> OriginId {
        self.call
    }

    /// Returns method-member provenance.
    #[must_use]
    pub const fn member(self) -> OriginId {
        self.member
    }

    /// Returns receiver provenance.
    #[must_use]
    pub const fn receiver(self) -> OriginId {
        self.receiver
    }
}

/// One normalized typed Minecraft operation declaration.
///
/// The source-only executor proof is deliberately absent. Core retains the
/// instantiated receiver kind and independently derives ambient requirements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MinecraftOperationDecl {
    key: MinecraftSemanticKey,
    receiver_kind: EntityKind,
    attributes: MinecraftOperationAttributes,
    origins: MinecraftOperationOrigins,
}

impl MinecraftOperationDecl {
    /// Returns the normalized target-independent operation identity.
    #[must_use]
    pub const fn key(&self) -> MinecraftSemanticKey {
        self.key
    }

    /// Returns the nominal kind required of Minecraft's current executor.
    #[must_use]
    pub const fn receiver_kind(&self) -> EntityKind {
        self.receiver_kind
    }

    /// Returns the operation's closed compile-time attributes.
    #[must_use]
    pub const fn attributes(&self) -> &MinecraftOperationAttributes {
        &self.attributes
    }

    /// Returns retained source occurrence provenance.
    #[must_use]
    pub const fn origins(&self) -> MinecraftOperationOrigins {
        self.origins
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        let descriptor = minecraft_descriptor(self.key);
        descriptor.key() == self.key
            && self.attributes.semantic_key() == self.key
            && self
                .receiver_kind
                .capabilities()
                .contains(EntityCapability::CommandExecutor)
    }
}

impl CoreProgram {
    /// Declares one normalized typed Minecraft operation in stable order.
    ///
    /// # Errors
    ///
    /// Returns an error for a key/attribute mismatch, an incapable receiver
    /// kind, or exhaustion of the operation identity space.
    pub fn declare_minecraft_operation(
        &mut self,
        key: MinecraftSemanticKey,
        receiver_kind: EntityKind,
        attributes: MinecraftOperationAttributes,
        origins: MinecraftOperationOrigins,
    ) -> Result<MinecraftOperationId, ProgramError> {
        let declaration = MinecraftOperationDecl {
            key,
            receiver_kind,
            attributes,
            origins,
        };
        if !declaration.is_well_formed() {
            return Err(ProgramError::InvalidMinecraftOperation);
        }
        self.minecraft_operations
            .push(declaration)
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Replaces the page index of a previously declared `BookPage` operation
    /// with a dynamically resolved `ValueId`.
    #[allow(dead_code, reason = "used by Stage 10 macro lowering when fully wired")]
    pub(crate) fn patch_book_page_index(
        &mut self,
        operation: MinecraftOperationId,
        value: super::ValueId,
    ) {
        let decl = self
            .minecraft_operations
            .get_mut(operation)
            .expect("patch_book_page_index called with valid operation id");
        let MinecraftOperationAttributes::BookPage { page_index, .. } = &mut decl.attributes else {
            panic!("patch_book_page_index called on non-BookPage operation");
        };
        *page_index = MacroOrStatic::Macro(value);
    }

    /// Returns a typed Minecraft operation, or `None` for a foreign identity.
    #[must_use]
    pub fn minecraft_operation(
        &self,
        operation: MinecraftOperationId,
    ) -> Option<&MinecraftOperationDecl> {
        self.minecraft_operations.get(operation)
    }

    /// Iterates typed Minecraft operations in stable declaration order.
    #[must_use]
    pub fn minecraft_operations(
        &self,
    ) -> impl ExactSizeIterator<Item = (MinecraftOperationId, &MinecraftOperationDecl)> + '_ {
        self.minecraft_operations.iter()
    }
}
