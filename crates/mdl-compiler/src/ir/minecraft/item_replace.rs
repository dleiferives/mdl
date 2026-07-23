use crate::ir::core::Operand;
use crate::ir::semantic::BlockPosition;

/// One typed `item replace block <pos> container.<slot> with <item> <count>`
/// command (PS-16, BE-2) — the whole-slot entity-NBT write's only lowering
/// shape. Confirmed clean (no "Serialization errors" warning) for both an
/// occupied and an unoccupied slot by direct measurement against the real
/// pinned server (`notes/compiler/pre-scheduler/ps-16-block-entity-nbt-writes.md`),
/// unlike a `data modify ... Items[{Slot:N}].field set value ...` partial
/// write, so no occupancy check is needed. `item_id`/`count` are always
/// compile-time constants; `slot` may be `Operand::Runtime` once a macro
/// route exists (Stage 2) — rendered inline only when `Operand::Const`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemReplaceBlockCommand {
    position: BlockPosition,
    slot: Operand<i32>,
    item_id: Box<str>,
    count: i32,
}

impl ItemReplaceBlockCommand {
    /// Constructs a whole-slot container item-replace command.
    #[must_use]
    pub const fn new(
        position: BlockPosition,
        slot: Operand<i32>,
        item_id: Box<str>,
        count: i32,
    ) -> Self {
        Self {
            position,
            slot,
            item_id,
            count,
        }
    }

    /// Returns the target block position.
    #[must_use]
    pub const fn position(&self) -> BlockPosition {
        self.position
    }

    /// Returns the target container slot.
    #[must_use]
    pub const fn slot(&self) -> Operand<i32> {
        self.slot
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
}
