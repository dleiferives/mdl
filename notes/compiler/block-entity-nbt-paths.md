# Block-Entity NBT Paths — Reading Container Contents (Chests First)

Date: 2026-07-21
Status: **proposed design, not yet scheduled as a milestone. Extends
[`entity-nbt-path-composability.md`](entity-nbt-path-composability.md) and answers the deferred
question both [`references-design.md`](references-design.md) §11 and
[`nbt-schema-system.md`](nbt-schema-system.md) §7 left open: "block-entity-rooted references... no
schema table exists for block NBT yet."**

## Why this note exists

A user asked whether MDL can currently express "a player interacting with a chest and grabbing
items out of it, detected." It cannot, for two independent reasons: MDL has no way to read
block-entity NBT (chest contents) at all, and MDL has no way to detect a Minecraft *event*
(there is no native "container take" event in vanilla anyway — see
[`advancement-triggers.md`](advancement-triggers.md) for that half). This note is the first half
only: **reading** container contents, generalizing PS-12's entity-NBT path system rather than
building a second, parallel one.

The PS-12 system was deliberately built to generalize. This note is the test of that claim.

## Part 1 — What's actually different about block-entity NBT (researched, not assumed)

Four concrete differences from entity-NBT reads, each with a real consequence for the design:

### 1.1 Addressing is by position, not selector

Entity reads use `data modify storage <target> set from entity <selector> <path>`. Block reads use
`data get/modify block <x> <y> <z> <path>` — a block position, not a selector. There is no
"selected inventory-holder capability" concept; any block coordinate is a valid (if possibly
empty) read target.

### 1.2 Container contents are match-indexed, not position-indexed

This is the one genuinely new piece of grammar needed. A chest's inventory is *not* a dense,
positionally-ordered list the way a book's `pages` is. Per the measured Java Edition data-component
format: the `minecraft:container` component is a list where **only occupied slots appear**, each
entry a compound `{slot: <0-255>, item: {id, count, components}}` — you find an entry by matching
its `slot` field, not by list position. Minecraft's own NBT path grammar supports this directly
(`components."minecraft:container"[{slot:0}].item`), via a predicate-matching list-element
selector. MDL's `NbtPathSegment` (`ir/minecraft/nbt.rs`) has exactly three variants today —
`Key`, `Index(Operand<i32>)` (plain positional `[n]`), `AllElements` (`[]`) — and none of them
express "the element whose `slot` field equals N." A real new segment variant is required; this
cannot be expressed as a table row the way `.count` was.

### 1.3 Block coordinates are a different value shape than entity spatial args

`TargetWorldPosition`/`JavaDecimal` (`ir/minecraft/spatial.rs`) were built for entity
teleport/positioned targets, which accept fractional coordinates (`~0.5`). Block positions
(`block_pos` in Minecraft's own argument-type vocabulary) are strictly integer, with `~`-relative
support but no decimals. `TargetWorldPosition` is the wrong type to reuse as-is; a block position
needs its own small integer-triple type.

### 1.4 Precedent for "literal-only first" already exists in this exact area

`frontend/check.rs::check_spatial_decimal` already rejects any non-compile-time-literal spatial
coordinate today, with the diagnostic text *"runtime expressions are not spatial attributes in
Stage 7.5"* — an explicit, already-shipped restriction on `teleport`/`move_by`. This is a gift for
scoping: a first block-entity-read slice can adopt the identical restriction (literal-only block
position) and sidestep the harder question of "what does a runtime-computed block position mean as
a first-class value" entirely, exactly the way PS-12's own book-page chain shipped constant indices
before generalizing to runtime ones.

## Part 2 — Design: what reuses PS-12 unchanged, what's genuinely new

### 2.1 Reuses unchanged

- `SchemaNode`/`SchemaKey` and their walking methods (`field_by_name`, `field_by_resource_id`,
  `list_element`) — fully generic already, no entity-specific assumption anywhere in
  `entity_schema.rs`'s node-walking logic.
- The fail-soft contract (type-appropriate default, attempt the read, keep the default on any
  broken link) — "slot not occupied" is structurally identical to "book not present": both are a
  `data get`/`data modify ... set from ...` source that resolves to nothing, which is already a
  no-op in Minecraft's command semantics, not an error.
- The inline-vs-macro lowering split (`is_unusable_inline()` as a derived predicate: "does any
  segment carry a runtime operand") and the whole `crossings.rs` macro engine — both are already
  written generically over "some number of runtime index/match segments," not "one runtime page
  index."
- **`ItemStack`'s existing schema node, verbatim.** A container slot's `item` field is structurally
  the same shape already registered for equipment (`id`/`count`/`components`). This is a genuine,
  unplanned composability win: the same `ITEM_STACK` static can be the element type of the new
  container match-list, with zero duplication.

### 2.2 Genuinely new

- **A second schema root.** Today `root_schema(EntityKind) -> SchemaNode` is keyed by the one
  closed `EntityKind` enum. Add a parallel `BlockEntityKind` enum and `block_root_schema
  (BlockEntityKind) -> SchemaNode`, starting with exactly one variant, `Chest` — mirroring how
  `EntityKind` itself started (and still is) `ArmorStand`-only. Do **not** try to unify
  `EntityKind`/`BlockEntityKind` into one enum; they have different capability sets, different
  invalidation facts (§1.4 of `references-design.md`'s table already keeps them as separate rows),
  and conflating them would force irrelevant entity concepts (capabilities like
  `CommandExecutor`/`InventoryHolder`) onto blocks, which are never executors.
- **`SchemaNode::MatchList { element: &'static SchemaNode, match_key: SchemaKey }`** — a list whose
  elements are found by one compound-field match instead of position. `match_key` names the field
  used for matching (`"slot"` for containers); the checker's list-index-step handling grows one new
  case (`MatchList` behaves like `List` for "what does `[expr]` narrow to," but the expr's *role* at
  lowering time is "the match value," not "the position").
- **`EntityNbtPathSegment::Match(Box<str> /* match key */, Operand<i32>)`** in Core (parallel to
  the existing `Index(Operand<i32>)`), and a matching `NbtPathSegment`-level addition
  (`ir::minecraft::nbt.rs`) for Minecraft-target rendering. Scope the match *value* to `Int32` only
  for v1 — `slot` is the only field MDL needs to match on for containers, and general
  arbitrary-compound-predicate matching is exactly the kind of open-ended surface this codebase
  consistently declines to build until a second real use case demands it (same discipline as the
  closed `SchemaKey`/`MinecraftSemanticKey` tables everywhere else).
- **A block position value.** A new `BlockPos { x: TargetIntegerAxis, y: ..., z: ... }` (integer,
  `~`-relative, no decimals — distinct from `TargetWorldPosition`). Source-level: `mc.block(x, y,
  z)` — literal-only in v1 per §1.4 — producing a typed value analogous to `EntityRefType` (a
  `SemanticType::BlockRef(BlockRefType::new(BlockEntityKind::Chest))`, say), **not** an
  `Executor<T>`. This is a real simplification versus entities: a block position is
  self-contained in the command it lowers to, so reading through it needs **no ambient context
  capture at all** — no `run.at(...) |chest| { ... }` construct is required the way `run.as(...)
  |reader|` is required for entities. `mc.block(0, 4, 0).components."minecraft:container"[{slot:
  0}].item.id` can be an ordinary expression anywhere a value is expected.

### 2.3 HIR/Core representation

Generalize `EntityNbtReadDecl.receiver_kind: EntityKind` to a small closed sum,
`EntityNbtReceiver { Entity(EntityKind), Block(BlockEntityKind, BlockPos) }` — an entity receiver
still carries only its kind (the real selector is resolved from ambient executor context at
lowering time, unchanged); a block receiver carries its kind *and* its position, since there is no
ambient context to resolve it from. `EntityNbtPathSegment` gains `Match`, as above. Everything else
in `EntityNbtReadDecl` (segments after the root, `result_ty`, well-formedness checking) is
unchanged — Core stays representation-agnostic about which physical addressing mode a `Key`/`Index`/
`Match` segment will become, exactly as it already is target-independent about `NbtPathKey` vs.
plain `Box<str>`.

### 2.4 Minecraft-target lowering

New `DataSource::Block { position: BlockPos, path: NbtPath }` (mirrors `DataSource::Entity`
exactly). `NbtPathSegment::Match` needs a render arm in both the inline path (regular `NbtPath`
`Display`, quoting every key, matching the existing convention) and the macro path
(`crossings.rs::render_data_modify_as_macro`) — **whoever implements this must not repeat this
session's leading-dot bug**: the macro renderer's loop must treat the *first* path segment specially
(no separator before it) regardless of whether that segment is `Key`, `Index`, or the new `Match`,
since the bug was exactly a missing `index == 0` check, and a new segment kind is a new place for
the same mistake to recur if the guard isn't structured to cover it generically.

## Part 3 — Staging

1. **Slice 1 — inline, literal-only, `Chest` only.** `BlockPos` + literal `mc.block(x,y,z)` syntax
   + `BlockEntityKind::Chest` + the container schema (`MatchList` keyed on `slot`, element =
   existing `ITEM_STACK`) + `DataSource::Block` inline lowering. No macro/runtime route yet (mirrors
   PS-12B/C's own const-first staging).
2. **Slice 2 — runtime slot matching.** Generalize `Match`'s value operand to `Operand::Runtime`,
   proving the macro/crossings engine handles a `Match` segment exactly as it already handles
   `Index` — this is the real test that PS-11's engine generalizes to a second segment kind, not
   just a second use of the same one.
3. **Slice 3 — differential + pinned-server evidence**, mirroring PS-12D exactly: a fixture with a
   summoned chest (`setblock`/`data merge block`) and a genuinely runtime slot number, four-policy
   differential, one `#[ignore]`d pinned-server test.
4. **Slice 4 — more block-entity kinds** (`Barrel`, `Furnace`'s input/output/fuel slots, `Hopper`,
   `ShulkerBox`) as additional `BlockEntityKind` variants and schema rows — near-free once the
   mechanism is proven, the same "prove with one, generalize by table row" story `.count` already
   demonstrated for entities.

### Non-goals (explicit, matching PS-12's own discipline)

- Block-entity **writes** — read-only, matching PS-12's own entity-NBT scope.
- Runtime-computed block positions (as opposed to a runtime match *value* within a fixed position)
  — literal position only, matching the existing Stage 7.5 spatial-literal restriction. Lifting
  this is a language-wide decision (does MDL get first-class position values at all?), not scoped
  to this feature.
- General compound-predicate matching beyond one `Int32`-valued field — `slot` only.
- Any block-entity type beyond what a real client program needs; do not pre-populate the table.
- Detecting *when* a player interacts with a chest — that is
  [`advancement-triggers.md`](advancement-triggers.md), a wholly separate subsystem. This note
  only makes "read what's in the chest right now" possible; combining it with an event trigger is
  what actually answers the original question.

## Sources

- [Data component format/container – Minecraft Wiki](https://minecraft.wiki/w/Data_component_format/container)
  — exact `minecraft:container` NBT shape (`slot`/`item` per entry, only occupied slots present).
- [Block entity format – Minecraft Wiki](https://minecraft.wiki/w/Block_entity_format) — common
  block-entity NBT fields, `components` compound.
- [Chest – Minecraft Wiki](https://minecraft.wiki/w/Chest) — chest-specific slot numbering (0-26).
- [`references-design.md`](references-design.md) §4, §11 — the block-rooted-reference invalidation
  row and the original "no schema table exists for block NBT yet" deferral this note resolves.
- [`nbt-schema-system.md`](nbt-schema-system.md) §7 — "a block-entity handle... register their own
  entry in an analogous root table, reusing the same `SchemaNode` walking logic unchanged," the
  design this note follows through on.
- [`entity-nbt-path-composability.md`](entity-nbt-path-composability.md) — the system being
  extended; Part 2's architectural decisions (schema-typed not arbitrary, fail-soft by default,
  bypass the closed-verb recipe system) all carry over unchanged.
