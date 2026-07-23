//! Program-owned, target-independent schema-typed entity-NBT path reads.
//!
//! Mirrors `ir/core/minecraft.rs`'s `MinecraftOperationDecl` shape, but is
//! deliberately **not** part of that closed-verb system: there is no
//! `MinecraftSemanticKey`/`minecraft_descriptor` triple-verification here,
//! because a schema-driven path has no single fixed command shape to verify
//! against. See `notes/compiler/entity-nbt-path-composability.md` §2.5.

use std::fmt;

use super::{CoreProgram, CoreType, Operand, ProgramError};
use crate::entity::EntityLimitError;
use crate::ir::semantic::{BlockEntityKind, BlockPosition, EntityCapability, EntityKind};
use crate::source::OriginId;

/// One step of a checked entity-NBT path, after the root. A `Key` step is
/// always compile-time-constant (schema keys are never runtime-derived,
/// `nbt-schema-system.md` §7); `Index`/`Match` steps may be const or
/// runtime. `Match` (BE-1) selects a list element by a schema-known
/// compound-field match (e.g. a container slot) instead of by position.
///
/// Plain `Box<str>` keys, not `ir::minecraft::NbtPathKey`: Core stays
/// target-independent (matching `MinecraftOperationAttributes`, which uses
/// only `ir::semantic` types, never `ir::minecraft` rendering types). The
/// string becomes a real `NbtPathKey` only during Minecraft-target lowering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityNbtPathSegment {
    Key(Box<str>),
    Index(Operand<i32>),
    /// `match_key` is the schema-known compound field matched against
    /// (e.g. `"slot"`) — always a plain string, same target-independence
    /// rationale as `Key`.
    Match {
        match_key: Box<str>,
        value: Operand<i32>,
    },
}

/// The root of an entity-NBT path read (BE-1,
/// `notes/compiler/block-entity-nbt-paths.md` §2.3). An entity receiver
/// resolves its real selector from ambient executor context at lowering
/// time, unchanged from PS-12. A block receiver carries its position
/// directly — it is self-contained in the command it lowers to and needs no
/// ambient context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityNbtReceiver {
    Entity(EntityKind),
    Block(BlockEntityKind, BlockPosition),
}

impl fmt::Display for EntityNbtReceiver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entity(kind) => write!(formatter, "Entity<{kind}>"),
            Self::Block(kind, position) => {
                write!(
                    formatter,
                    "Block<{kind}>@{} {} {}",
                    position.x, position.y, position.z
                )
            }
        }
    }
}

/// One normalized entity-NBT path read declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityNbtReadDecl {
    receiver: EntityNbtReceiver,
    segments: Box<[EntityNbtPathSegment]>,
    result_ty: CoreType,
    receiver_origin: OriginId,
}

impl EntityNbtReadDecl {
    /// Returns the read's root receiver.
    #[must_use]
    pub const fn receiver(&self) -> EntityNbtReceiver {
        self.receiver
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

    /// Returns the number of `Index`/`Match` segments carrying a runtime
    /// `ValueId` — exactly the number of instruction operands this
    /// declaration expects.
    #[must_use]
    pub fn runtime_index_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    EntityNbtPathSegment::Index(Operand::Runtime(_))
                        | EntityNbtPathSegment::Match {
                            value: Operand::Runtime(_),
                            ..
                        }
                )
            })
            .count()
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        if self.segments.is_empty() {
            return false;
        }
        match self.receiver {
            EntityNbtReceiver::Entity(kind) => kind
                .capabilities()
                .contains(EntityCapability::CommandExecutor),
            EntityNbtReceiver::Block(..) => true,
        }
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
        receiver: EntityNbtReceiver,
        segments: Vec<EntityNbtPathSegment>,
        result_ty: CoreType,
        receiver_origin: OriginId,
    ) -> Result<super::EntityNbtReadId, ProgramError> {
        let declaration = EntityNbtReadDecl {
            receiver,
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

    /// Declares one normalized whole-slot entity-NBT write in stable order
    /// (PS-16, BE-2).
    ///
    /// # Errors
    ///
    /// Returns an error for an empty path, a non-block receiver, an empty
    /// item id, or exhaustion of the declaration identity space.
    pub fn declare_entity_nbt_write(
        &mut self,
        receiver: EntityNbtReceiver,
        segments: Vec<EntityNbtPathSegment>,
        item_id: Box<str>,
        count: i32,
        receiver_origin: OriginId,
    ) -> Result<super::EntityNbtWriteId, ProgramError> {
        let declaration = EntityNbtWriteDecl {
            receiver,
            segments: segments.into_boxed_slice(),
            item_id,
            count,
            receiver_origin,
        };
        if !declaration.is_well_formed() {
            return Err(ProgramError::InvalidEntityNbtWrite);
        }
        self.entity_nbt_writes
            .push(declaration)
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Returns an entity-NBT write declaration, or `None` for a foreign identity.
    #[must_use]
    pub fn entity_nbt_write(&self, write: super::EntityNbtWriteId) -> Option<&EntityNbtWriteDecl> {
        self.entity_nbt_writes.get(write)
    }

    /// Iterates entity-NBT write declarations in stable declaration order.
    #[must_use]
    pub fn entity_nbt_writes(
        &self,
    ) -> impl ExactSizeIterator<Item = (super::EntityNbtWriteId, &EntityNbtWriteDecl)> + '_ {
        self.entity_nbt_writes.iter()
    }
}

/// One normalized whole-slot entity-NBT write declaration (PS-16, BE-2) —
/// lowers unconditionally to `item replace block <pos> container.<slot> with
/// <item> <count>`, confirmed clean for both an occupied and unoccupied slot
/// by direct measurement against the real pinned server (see
/// `notes/compiler/pre-scheduler/ps-16-block-entity-nbt-writes.md`), so no
/// occupancy check is needed. `item_id`/`count` are always compile-time
/// constants — only `segments`' final `Match`/`Index` value may ever be a
/// runtime `Operand`, reusing `EntityNbtPathSegment` unchanged, the same as
/// `EntityNbtReadDecl`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityNbtWriteDecl {
    receiver: EntityNbtReceiver,
    segments: Box<[EntityNbtPathSegment]>,
    item_id: Box<str>,
    count: i32,
    receiver_origin: OriginId,
}

impl EntityNbtWriteDecl {
    /// Returns the write's root receiver.
    #[must_use]
    pub const fn receiver(&self) -> EntityNbtReceiver {
        self.receiver
    }

    /// Returns the checked path steps, root-relative, ending in the segment
    /// selecting the written element.
    #[must_use]
    pub fn segments(&self) -> &[EntityNbtPathSegment] {
        &self.segments
    }

    /// Returns the written item's `namespace:path` resource id.
    #[must_use]
    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    /// Returns the written stack count.
    #[must_use]
    pub const fn count(&self) -> i32 {
        self.count
    }

    /// Returns retained receiver source provenance.
    #[must_use]
    pub const fn receiver_origin(&self) -> OriginId {
        self.receiver_origin
    }

    /// Returns the number of `Index`/`Match` segments carrying a runtime
    /// `ValueId` — exactly the number of instruction operands this
    /// declaration expects (mirrors `EntityNbtReadDecl::runtime_index_count`;
    /// always `0` until Stage 2 lifts the checker's literal-only slot
    /// restriction).
    #[must_use]
    pub fn runtime_operand_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    EntityNbtPathSegment::Index(Operand::Runtime(_))
                        | EntityNbtPathSegment::Match {
                            value: Operand::Runtime(_),
                            ..
                        }
                )
            })
            .count()
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        !self.segments.is_empty()
            && !self.item_id.is_empty()
            && matches!(self.receiver, EntityNbtReceiver::Block(..))
    }
}

#[cfg(test)]
mod tests {
    use super::{EntityNbtPathSegment, EntityNbtReceiver};
    use crate::entity::EntityId;
    use crate::ir::core::{CoreProgram, CoreType, Operand, ProgramError, ValueId};
    use crate::ir::semantic::{BlockEntityKind, BlockPosition, EntityKind};
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

    fn container_segments() -> Vec<EntityNbtPathSegment> {
        vec![
            EntityNbtPathSegment::Key("Items".into()),
            EntityNbtPathSegment::Match {
                match_key: "Slot".into(),
                value: Operand::Const(0),
            },
            EntityNbtPathSegment::Key("count".into()),
        ]
    }

    #[test]
    fn well_formed_declarations_round_trip() {
        let mut program = CoreProgram::new();
        let read = program
            .declare_entity_nbt_read(
                EntityNbtReceiver::Entity(EntityKind::ArmorStand),
                book_page_segments(),
                CoreType::String,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let declaration = program.entity_nbt_read(read).unwrap();
        assert_eq!(
            declaration.receiver(),
            EntityNbtReceiver::Entity(EntityKind::ArmorStand)
        );
        assert_eq!(declaration.result_ty(), CoreType::String);
        assert_eq!(declaration.segments().len(), 7);
        assert_eq!(declaration.runtime_index_count(), 0);
        assert_eq!(program.entity_nbt_reads().len(), 1);
    }

    #[test]
    fn well_formed_block_declarations_round_trip() {
        let mut program = CoreProgram::new();
        let position = BlockPosition { x: 0, y: 4, z: 0 };
        let read = program
            .declare_entity_nbt_read(
                EntityNbtReceiver::Block(BlockEntityKind::Chest, position),
                container_segments(),
                CoreType::I32,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let declaration = program.entity_nbt_read(read).unwrap();
        assert_eq!(
            declaration.receiver(),
            EntityNbtReceiver::Block(BlockEntityKind::Chest, position)
        );
        assert_eq!(declaration.result_ty(), CoreType::I32);
        assert_eq!(declaration.runtime_index_count(), 0);
    }

    #[test]
    fn runtime_index_segments_are_counted() {
        let mut program = CoreProgram::new();
        let mut segments = book_page_segments();
        segments[5] = EntityNbtPathSegment::Index(Operand::Runtime(ValueId::from_index(0)));
        let read = program
            .declare_entity_nbt_read(
                EntityNbtReceiver::Entity(EntityKind::ArmorStand),
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
    fn runtime_match_segments_are_counted() {
        let mut program = CoreProgram::new();
        let mut segments = container_segments();
        segments[1] = EntityNbtPathSegment::Match {
            match_key: "Slot".into(),
            value: Operand::Runtime(ValueId::from_index(0)),
        };
        let read = program
            .declare_entity_nbt_read(
                EntityNbtReceiver::Block(
                    BlockEntityKind::Chest,
                    BlockPosition { x: 0, y: 4, z: 0 },
                ),
                segments,
                CoreType::I32,
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
                EntityNbtReceiver::Entity(EntityKind::ArmorStand),
                vec![],
                CoreType::String,
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidEntityNbtRead)
        );
    }

    fn container_write_segments() -> Vec<EntityNbtPathSegment> {
        vec![
            EntityNbtPathSegment::Key("Items".into()),
            EntityNbtPathSegment::Match {
                match_key: "Slot".into(),
                value: Operand::Const(0),
            },
        ]
    }

    #[test]
    fn well_formed_write_declarations_round_trip() {
        let mut program = CoreProgram::new();
        let position = BlockPosition { x: 0, y: 4, z: 0 };
        let write = program
            .declare_entity_nbt_write(
                EntityNbtReceiver::Block(BlockEntityKind::Chest, position),
                container_write_segments(),
                "minecraft:diamond".into(),
                5,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let declaration = program.entity_nbt_write(write).unwrap();
        assert_eq!(
            declaration.receiver(),
            EntityNbtReceiver::Block(BlockEntityKind::Chest, position)
        );
        assert_eq!(declaration.item_id(), "minecraft:diamond");
        assert_eq!(declaration.count(), 5);
        assert_eq!(declaration.segments().len(), 2);
        assert_eq!(declaration.runtime_operand_count(), 0);
        assert_eq!(program.entity_nbt_writes().len(), 1);
    }

    #[test]
    fn runtime_write_match_segments_are_counted() {
        let mut program = CoreProgram::new();
        let mut segments = container_write_segments();
        segments[1] = EntityNbtPathSegment::Match {
            match_key: "Slot".into(),
            value: Operand::Runtime(ValueId::from_index(0)),
        };
        let write = program
            .declare_entity_nbt_write(
                EntityNbtReceiver::Block(
                    BlockEntityKind::Chest,
                    BlockPosition { x: 0, y: 4, z: 0 },
                ),
                segments,
                "minecraft:diamond".into(),
                5,
                OriginId::UNKNOWN,
            )
            .unwrap();
        assert_eq!(
            program
                .entity_nbt_write(write)
                .unwrap()
                .runtime_operand_count(),
            1
        );
    }

    #[test]
    fn an_empty_write_path_is_rejected() {
        let mut program = CoreProgram::new();
        assert_eq!(
            program.declare_entity_nbt_write(
                EntityNbtReceiver::Block(
                    BlockEntityKind::Chest,
                    BlockPosition { x: 0, y: 4, z: 0 }
                ),
                vec![],
                "minecraft:diamond".into(),
                5,
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidEntityNbtWrite)
        );
    }

    #[test]
    fn an_empty_write_item_id_is_rejected() {
        let mut program = CoreProgram::new();
        assert_eq!(
            program.declare_entity_nbt_write(
                EntityNbtReceiver::Block(
                    BlockEntityKind::Chest,
                    BlockPosition { x: 0, y: 4, z: 0 }
                ),
                container_write_segments(),
                "".into(),
                5,
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidEntityNbtWrite)
        );
    }

    #[test]
    fn an_entity_receiver_write_is_rejected() {
        let mut program = CoreProgram::new();
        assert_eq!(
            program.declare_entity_nbt_write(
                EntityNbtReceiver::Entity(EntityKind::ArmorStand),
                container_write_segments(),
                "minecraft:diamond".into(),
                5,
                OriginId::UNKNOWN,
            ),
            Err(ProgramError::InvalidEntityNbtWrite)
        );
    }
}
