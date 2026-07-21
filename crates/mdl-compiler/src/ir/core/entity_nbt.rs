//! Program-owned, target-independent schema-typed entity-NBT path reads.
//!
//! Mirrors `ir/core/minecraft.rs`'s `MinecraftOperationDecl` shape, but is
//! deliberately **not** part of that closed-verb system: there is no
//! `MinecraftSemanticKey`/`minecraft_descriptor` triple-verification here,
//! because a schema-driven path has no single fixed command shape to verify
//! against. See `notes/compiler/entity-nbt-path-composability.md` §2.5.

use super::{CoreProgram, CoreType, Operand, ProgramError};
use crate::entity::EntityLimitError;
use crate::ir::semantic::{EntityCapability, EntityKind};
use crate::source::OriginId;

/// One step of a checked entity-NBT path, after the root. A `Key` step is
/// always compile-time-constant (schema keys are never runtime-derived,
/// `nbt-schema-system.md` §7); an `Index` step may be const or runtime.
///
/// Plain `Box<str>` keys, not `ir::minecraft::NbtPathKey`: Core stays
/// target-independent (matching `MinecraftOperationAttributes`, which uses
/// only `ir::semantic` types, never `ir::minecraft` rendering types). The
/// string becomes a real `NbtPathKey` only during Minecraft-target lowering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityNbtPathSegment {
    Key(Box<str>),
    Index(Operand<i32>),
}

/// One normalized entity-NBT path read declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityNbtReadDecl {
    receiver_kind: EntityKind,
    segments: Box<[EntityNbtPathSegment]>,
    result_ty: CoreType,
    receiver_origin: OriginId,
}

impl EntityNbtReadDecl {
    /// Returns the nominal kind required of Minecraft's current executor.
    #[must_use]
    pub const fn receiver_kind(&self) -> EntityKind {
        self.receiver_kind
    }

    /// Returns the checked path steps, root-relative.
    #[must_use]
    pub fn segments(&self) -> &[EntityNbtPathSegment] {
        &self.segments
    }

    /// Returns the terminal scalar type this read produces.
    #[must_use]
    pub const fn result_ty(&self) -> CoreType {
        self.result_ty
    }

    /// Returns retained receiver source provenance.
    #[must_use]
    pub const fn receiver_origin(&self) -> OriginId {
        self.receiver_origin
    }

    /// Returns the number of `Index` segments carrying a runtime `ValueId` —
    /// exactly the number of instruction operands this declaration expects.
    #[must_use]
    pub fn runtime_index_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| matches!(segment, EntityNbtPathSegment::Index(Operand::Runtime(_))))
            .count()
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        !self.segments.is_empty()
            && self
                .receiver_kind
                .capabilities()
                .contains(EntityCapability::CommandExecutor)
    }
}

impl CoreProgram {
    /// Declares one normalized entity-NBT path read in stable order.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty path, an incapable receiver kind, or
    /// exhaustion of the declaration identity space.
    pub fn declare_entity_nbt_read(
        &mut self,
        receiver_kind: EntityKind,
        segments: Vec<EntityNbtPathSegment>,
        result_ty: CoreType,
        receiver_origin: OriginId,
    ) -> Result<super::EntityNbtReadId, ProgramError> {
        let declaration = EntityNbtReadDecl {
            receiver_kind,
            segments: segments.into_boxed_slice(),
            result_ty,
            receiver_origin,
        };
        if !declaration.is_well_formed() {
            return Err(ProgramError::InvalidEntityNbtRead);
        }
        self.entity_nbt_reads
            .push(declaration)
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Returns an entity-NBT read declaration, or `None` for a foreign identity.
    #[must_use]
    pub fn entity_nbt_read(&self, read: super::EntityNbtReadId) -> Option<&EntityNbtReadDecl> {
        self.entity_nbt_reads.get(read)
    }

    /// Iterates entity-NBT read declarations in stable declaration order.
    #[must_use]
    pub fn entity_nbt_reads(
        &self,
    ) -> impl ExactSizeIterator<Item = (super::EntityNbtReadId, &EntityNbtReadDecl)> + '_ {
        self.entity_nbt_reads.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::EntityNbtPathSegment;
    use crate::entity::EntityId;
    use crate::ir::core::{CoreProgram, CoreType, Operand, ProgramError, ValueId};
    use crate::ir::semantic::EntityKind;
    use crate::source::OriginId;

    fn book_page_segments() -> Vec<EntityNbtPathSegment> {
        vec![
            EntityNbtPathSegment::Key("equipment".into()),
            EntityNbtPathSegment::Key("mainhand".into()),
            EntityNbtPathSegment::Key("components".into()),
            EntityNbtPathSegment::Key("minecraft:written_book_content".into()),
            EntityNbtPathSegment::Key("pages".into()),
            EntityNbtPathSegment::Index(Operand::Const(0)),
            EntityNbtPathSegment::Key("raw".into()),
        ]
    }

    #[test]
    fn well_formed_declarations_round_trip() {
        let mut program = CoreProgram::new();
        let read = program
            .declare_entity_nbt_read(
                EntityKind::ArmorStand,
                book_page_segments(),
                CoreType::String,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let declaration = program.entity_nbt_read(read).unwrap();
        assert_eq!(declaration.receiver_kind(), EntityKind::ArmorStand);
        assert_eq!(declaration.result_ty(), CoreType::String);
        assert_eq!(declaration.segments().len(), 7);
        assert_eq!(declaration.runtime_index_count(), 0);
        assert_eq!(program.entity_nbt_reads().len(), 1);
    }

    #[test]
    fn runtime_index_segments_are_counted() {
        let mut program = CoreProgram::new();
        let mut segments = book_page_segments();
        segments[5] = EntityNbtPathSegment::Index(Operand::Runtime(ValueId::from_index(0)));
        let read = program
            .declare_entity_nbt_read(
                EntityKind::ArmorStand,
                segments,
                CoreType::String,
                OriginId::UNKNOWN,
            )
            .unwrap();
        assert_eq!(
            program.entity_nbt_read(read).unwrap().runtime_index_count(),
            1
        );
    }

    #[test]
    fn an_empty_path_is_rejected() {
        let mut program = CoreProgram::new();
        assert_eq!(
            program.declare_entity_nbt_read(
                EntityKind::ArmorStand,
                vec![],
                CoreType::String,
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidEntityNbtRead)
        );
    }
}
