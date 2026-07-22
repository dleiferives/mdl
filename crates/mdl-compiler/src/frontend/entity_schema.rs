//! Compiler-known entity/item/NBT schema tables (PS-12, S-042).
//!
//! See `notes/compiler/nbt-schema-system.md` for the full design and
//! `notes/compiler/entity-nbt-path-composability.md` §2.4 for the worked
//! table this module implements. The table is closed, compile-time data:
//! growing it is "add a table row," never a new checker branch.

use super::hir::ValueType;
use crate::ir::semantic::EntityKind;

/// One key a `Compound` schema node is reachable by.
///
/// The variant *is* the disambiguation rule from
/// `entity-paths-and-general-indexing.md` §1: a key is `Identifier` if and
/// only if it is spellable as `.name`; otherwise it is `ResourceId` and only
/// reachable via `."string literal"`. The two lookup paths
/// (`SchemaNode::field_by_name`/`field_by_resource_id`) are structurally
/// separate, so a key can never be reached through the wrong source form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SchemaKey {
    Identifier(&'static str),
    ResourceId(&'static str),
}

/// One node in the schema graph. Copy: every recursive field is a `'static`
/// reference into the fixed table below, never owned/heap data.
#[derive(Clone, Copy, Debug)]
pub(super) enum SchemaNode {
    /// A terminal — reaching this node ends the chain at an ordinary value
    /// of this type.
    Scalar(ValueType),
    /// Reachable by `.name` or `."string"` steps; each field is one more key.
    Compound(&'static [(SchemaKey, SchemaNode)]),
    /// Reachable by `[Expression]` steps (const or runtime `Int32`).
    List(&'static SchemaNode),
}

impl SchemaNode {
    /// Narrows through a `.name` step. `None` if this node isn't a
    /// `Compound` or has no `Identifier` field with this spelling.
    pub(super) fn field_by_name(self, name: &str) -> Option<Self> {
        let Self::Compound(fields) = self else {
            return None;
        };
        fields.iter().find_map(|(key, node)| match key {
            SchemaKey::Identifier(candidate) if *candidate == name => Some(*node),
            _ => None,
        })
    }

    /// Narrows through a `."string"` step. `None` if this node isn't a
    /// `Compound` or has no `ResourceId` field with this content.
    pub(super) fn field_by_resource_id(self, content: &str) -> Option<Self> {
        let Self::Compound(fields) = self else {
            return None;
        };
        fields.iter().find_map(|(key, node)| match key {
            SchemaKey::ResourceId(candidate) if *candidate == content => Some(*node),
            _ => None,
        })
    }

    /// Narrows through a `[Expression]` step. `None` if this node isn't a
    /// `List`.
    pub(super) fn list_element(self) -> Option<Self> {
        match self {
            Self::List(element) => Some(*element),
            Self::Compound(_) | Self::Scalar(_) => None,
        }
    }

    /// Returns the terminal `ValueType` if this node is a `Scalar`.
    pub(super) const fn scalar_type(self) -> Option<ValueType> {
        match self {
            Self::Scalar(ty) => Some(ty),
            Self::Compound(_) | Self::List(_) => None,
        }
    }

    /// Human-readable node-kind label for diagnostics.
    pub(super) const fn kind_label(self) -> &'static str {
        match self {
            Self::Scalar(_) => "a scalar value",
            Self::Compound(_) => "a compound with known fields",
            Self::List(_) => "a list",
        }
    }
}

// ── The V26_2 / ArmorStand table (`nbt-schema-system.md` §8) ──
//
// Ships what the book-page chain needs, plus `ItemStack.count` as PS-12E's
// worked extensibility proof: a new schema field is a pure table-row change,
// with no new checker/HIR/lowering branch required. `ItemStack.id` remains
// unregistered — still explicit follow-up, not required by either chain.

static BOOK_PAGE_ENTRY: SchemaNode = SchemaNode::Compound(&[(
    SchemaKey::Identifier("raw"),
    SchemaNode::Scalar(ValueType::String),
)]);

static WRITTEN_BOOK_CONTENT: SchemaNode = SchemaNode::Compound(&[
    (
        SchemaKey::Identifier("pages"),
        SchemaNode::List(&BOOK_PAGE_ENTRY),
    ),
    (
        SchemaKey::Identifier("author"),
        SchemaNode::Scalar(ValueType::String),
    ),
    (SchemaKey::Identifier("title"), BOOK_PAGE_ENTRY),
    (
        SchemaKey::Identifier("resolved"),
        SchemaNode::Scalar(ValueType::Bool),
    ),
]);

static COMPONENTS: SchemaNode = SchemaNode::Compound(&[(
    SchemaKey::ResourceId("minecraft:written_book_content"),
    WRITTEN_BOOK_CONTENT,
)]);

static ITEM_STACK: SchemaNode = SchemaNode::Compound(&[
    (SchemaKey::Identifier("components"), COMPONENTS),
    (
        SchemaKey::Identifier("count"),
        SchemaNode::Scalar(ValueType::Int32),
    ),
]);

static EQUIPMENT_SLOTS: SchemaNode = SchemaNode::Compound(&[
    (SchemaKey::Identifier("mainhand"), ITEM_STACK),
    (SchemaKey::Identifier("offhand"), ITEM_STACK),
    (SchemaKey::Identifier("head"), ITEM_STACK),
    (SchemaKey::Identifier("chest"), ITEM_STACK),
    (SchemaKey::Identifier("legs"), ITEM_STACK),
    (SchemaKey::Identifier("feet"), ITEM_STACK),
]);

static ARMOR_STAND_ROOT: SchemaNode =
    SchemaNode::Compound(&[(SchemaKey::Identifier("equipment"), EQUIPMENT_SLOTS)]);

/// Returns the root schema node for `.equipment`-style chains starting from
/// the current executor, narrowed by its nominal entity kind.
///
/// Target-version selection (`nbt-schema-system.md` §6: `JavaEditionTarget ->
/// RootSchemaTable`) is not wired here — the frontend checker is already
/// target-independent everywhere else (only `LoweringOptions` carries a
/// `JavaEditionTarget`), so there is exactly one table today. Keying this
/// function by target is explicit follow-up once a second target exists,
/// not a gap introduced by this design.
pub(super) const fn root_schema(kind: EntityKind) -> SchemaNode {
    match kind {
        EntityKind::ArmorStand => ARMOR_STAND_ROOT,
    }
}

#[cfg(test)]
mod tests {
    use super::{SchemaKey, SchemaNode, root_schema};
    use crate::ir::semantic::EntityKind;

    fn is_valid_identifier_spelling(name: &str) -> bool {
        let mut chars = name.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    /// `nbt-schema-system.md` §4.3: an `Identifier` key must be spellable as
    /// `.name`; a `ResourceId` key must not be (otherwise it should have been
    /// registered as `Identifier`, and would be unreachable via `."string"`
    /// disambiguation by construction).
    fn assert_registration_is_well_formed(node: SchemaNode) {
        let SchemaNode::Compound(fields) = node else {
            return;
        };
        let mut seen = std::collections::BTreeSet::new();
        for (key, child) in fields {
            let spelling = match key {
                SchemaKey::Identifier(name) => {
                    assert!(
                        is_valid_identifier_spelling(name),
                        "{name} is registered as Identifier but is not a valid MDL Name"
                    );
                    *name
                }
                SchemaKey::ResourceId(content) => {
                    assert!(
                        !is_valid_identifier_spelling(content),
                        "{content} is registered as ResourceId but is a valid MDL Name \
                         (should be Identifier)"
                    );
                    *content
                }
            };
            assert!(
                seen.insert((matches!(key, SchemaKey::Identifier(_)), spelling)),
                "duplicate schema key {spelling} in one Compound node"
            );
            assert_registration_is_well_formed(*child);
        }
    }

    #[test]
    fn every_registered_table_satisfies_key_form_and_uniqueness_invariants() {
        assert_registration_is_well_formed(root_schema(EntityKind::ArmorStand));
    }

    #[test]
    fn book_page_chain_walks_the_table_end_to_end() {
        let node = root_schema(EntityKind::ArmorStand)
            .field_by_name("equipment")
            .unwrap()
            .field_by_name("mainhand")
            .unwrap()
            .field_by_name("components")
            .unwrap()
            .field_by_resource_id("minecraft:written_book_content")
            .unwrap()
            .field_by_name("pages")
            .unwrap()
            .list_element()
            .unwrap()
            .field_by_name("raw")
            .unwrap();
        assert_eq!(node.scalar_type(), Some(super::ValueType::String));
    }

    /// PS-12E's extensibility proof: `.count` was added as a pure table-row
    /// change (no new checker/HIR/lowering branch), mirroring the book-page
    /// chain test above.
    #[test]
    fn item_count_chain_walks_the_table_end_to_end() {
        let node = root_schema(EntityKind::ArmorStand)
            .field_by_name("equipment")
            .unwrap()
            .field_by_name("mainhand")
            .unwrap()
            .field_by_name("count")
            .unwrap();
        assert_eq!(node.scalar_type(), Some(super::ValueType::Int32));
    }

    #[test]
    fn unknown_keys_and_wrong_step_kinds_are_rejected() {
        let equipment = root_schema(EntityKind::ArmorStand)
            .field_by_name("equipment")
            .unwrap();
        assert!(equipment.field_by_name("nonexistent").is_none());
        // A ResourceId key is not reachable through the Identifier lookup path.
        let components = equipment
            .field_by_name("mainhand")
            .unwrap()
            .field_by_name("components")
            .unwrap();
        assert!(
            components
                .field_by_name("minecraft:written_book_content")
                .is_none()
        );
        // list_element on a non-List node fails.
        assert!(equipment.list_element().is_none());
        // field_by_name on a Scalar fails.
        let raw = components
            .field_by_resource_id("minecraft:written_book_content")
            .unwrap()
            .field_by_name("pages")
            .unwrap()
            .list_element()
            .unwrap()
            .field_by_name("raw")
            .unwrap();
        assert!(raw.field_by_name("anything").is_none());
    }
}
