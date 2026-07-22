# Block-Entity NBT Paths — Reading Container Contents (Chests First)

Date: 2026-07-21
Status: **Slices 1-2 implemented and landed — literal and runtime container-slot reads, `Chest`
only, both the inline and macro-helper lowering routes, proven against the real pinned Java 26.2
server. Slices 3-4 (more block-entity kinds, `~`-relative positions) remain future work. Extends
[`entity-nbt-path-composability.md`](entity-nbt-path-composability.md) and answers the deferred
question both [`references-design.md`](references-design.md) §11 and
[`nbt-schema-system.md`](nbt-schema-system.md) §7 left open: "block-entity-rooted references... no
schema table exists for block NBT yet."**

**Second correction (Slice 2, found by testing against the real server before trusting a
plausible-sounding assumption):** storing a runtime macro-bridged value with a `Byte` NBT tag does
**not** make its `$(key)` substitution render with a `b` suffix — measured directly: `$(key)`
always substitutes as bare decimal text, regardless of the stored value's NBT tag. The `Byte`
suffix a chest's `Slot` match needs must be a literal character in the macro command *template*,
immediately after the substitution marker (`Items[{Slot:$(i0)b}]`), not derived from the bridged
value at all. This is the same class of mistake as §1.2's correction below — a documented or
"obviously true" assumption about NBT/macro behavior that turned out to be wrong on contact with
the real server — logged here for the same reason.

**Correction (found by testing against the real server, not assumed):** §1.2 and §2.1 below
originally described container contents as living under a `minecraft:container` *data component*
(`components."minecraft:container"[{slot:N}].item...`), based on documentation describing item
stack encoding. That shape is wrong for a *placed* block entity's own storage. Measured directly:
a chest's real NBT is `{Items: [{Slot: 0b, id: "...", count: N, components: {...}}], components:
{}, ...}` — a flat top-level `Items` list (no wrapper), matched by a **`Byte`-typed** `Slot` field
(not `Int32` — Minecraft's compound-match NBT syntax requires the match value's type tag to agree
exactly, or the match silently finds nothing), with `id`/`count`/`components` flat on each element
(no nested `item` compound). The text below is left as originally written where it's still
accurate (the architectural shape: new schema root, `MatchList`, `Match` segment, no ambient
context) and corrected inline where the *NBT facts* were wrong.

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
positionally-ordered list the way a book's `pages` is — **measured** (see the correction above,
not the data-component documentation this section originally cited): `Items` is a list where
**only occupied slots appear**, each entry a flat compound `{Slot: <byte>, id, count, components}`
— you find an entry by matching its `Slot` field (a `Byte`), not by list position. Minecraft's own
NBT path grammar supports this directly (`Items[{Slot:0b}].count`), via a predicate-matching
list-element selector whose match value's NBT type tag must agree with the real field's stored
type. MDL's `NbtPathSegment` (`ir/minecraft/nbt.rs`) had exactly three variants before this —
`Key`, `Index(Operand<i32>)` (plain positional `[n]`), `AllElements` (`[]`) — and none of them
express "the element whose `Slot` field equals N, typed as a Byte." A real new segment variant
was required; this cannot be expressed as a table row the way `.count` was.

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
- **`COMPONENTS`' existing schema node, verbatim** (not the whole `ITEM_STACK` node — see the
  correction above: there is no nested `item` wrapper to reuse `ITEM_STACK` as). A container
  slot's own `components` field (for e.g. a written book placed in a chest) reuses the exact same
  `COMPONENTS` static equipment already registers, since item-level component data is identical
  regardless of which container holds the stack — a smaller but still real, unplanned reuse win.

### 2.2 Genuinely new

- **A second schema root.** Today `root_schema(EntityKind) -> SchemaNode` is keyed by the one
  closed `EntityKind` enum. Add a parallel `BlockEntityKind` enum and `block_root_schema
  (BlockEntityKind) -> SchemaNode`, starting with exactly one variant, `Chest` — mirroring how
  `EntityKind` itself started (and still is) `ArmorStand`-only. Do **not** try to unify
  `EntityKind`/`BlockEntityKind` into one enum; they have different capability sets, different
  invalidation facts (§1.4 of `references-design.md`'s table already keeps them as separate rows),
  and conflating them would force irrelevant entity concepts (capabilities like
  `CommandExecutor`/`InventoryHolder`) onto blocks, which are never executors.
- **`SchemaNode::MatchList { element: &'static SchemaNode, match_key: &'static str }`** — a list
  whose elements are found by one compound-field match instead of position. `match_key` names the
  field used for matching (`"Slot"` for containers); the checker's list-index-step handling grows
  one new case (`MatchList` behaves like `List` for "what does `[expr]` narrow to," but the expr's
  *role* at lowering time is "the match value," not "the position"). **MDL source syntax needs no
  change at all** — `[expr]` on a `MatchList` uses the exact same S-042 bracket-index grammar as an
  ordinary `List`; the schema node kind, not new grammar, decides which segment kind gets emitted.
- **`EntityNbtPathSegment::Match { match_key: Box<str>, value: Operand<i32> }`** in Core (parallel
  to the existing `Index(Operand<i32>)`), and a matching `ir::minecraft::NbtPathSegment::Match {
  key: NbtPathKey, value: Operand<i32>, value_kind: NbtMatchValueKind }` for Minecraft-target
  rendering — `value_kind` (`Byte | Int32`) is the real correction from measurement: the match
  value's NBT type tag must agree with the real field's stored type (a chest's `Slot` is `Byte`),
  resolved by a small closed lookup keyed on `match_key` at render time
  (`lower/minecraft/emit.rs::match_value_kind`), not threaded as source-level type information —
  MDL's `[expr]` index is always `Int32` regardless of which NBT tag the match ultimately renders
  as. Scope the match *value* to `Int32`-typed-in-MDL only for v1 — `Slot` is the only field MDL
  needs to match on for containers, and general arbitrary-compound-predicate matching is exactly
  the kind of open-ended surface this codebase consistently declines to build until a second real
  use case demands it (same discipline as the closed `SchemaKey`/`MinecraftSemanticKey` tables
  everywhere else).
- **A block position value.** A new `BlockPosition { x: i32, y: i32, z: i32 }`
  (`ir/semantic/spatial.rs`) — absolute-only, no `~`-relative or fractional form, distinct from
  `TargetWorldPosition`/`JavaDecimal` (built for *entity* teleport/positioned targets, which accept
  decimals). Source-level: `mc.block(Chest, x, y, z)` (kind first, mirroring `mc.entities(kind)`)
  — literal-only in v1 per §1.4 — checked by `frontend/check.rs::check_block_ref_root`, producing
  an `HirEntityNbtReceiver::Block { kind, position }` root, **not** an `Executor<T>`/ambient
  capture. This is a real simplification versus entities: a block position is self-contained in
  the command it lowers to, so reading through it needs **no ambient context capture at all** — no
  `run.at(...) |chest| { ... }` construct is required the way `run.as(...) |reader|` is required
  for entities. `mc.block(Chest, 0, 4, 0).Items[0].count` is an ordinary expression anywhere a
  value is expected; whether the block at that position really is a chest is never proven at
  compile time (no selector-style type filter exists for positions) — an incorrect kind is simply
  the existing fail-soft case, not a new verification story.

### 2.3 HIR/Core representation

Generalized `EntityNbtReadDecl.receiver_kind: EntityKind` to a small closed sum,
`EntityNbtReceiver { Entity(EntityKind), Block(BlockEntityKind, BlockPosition) }` — an entity
receiver still carries only its kind (the real selector is resolved from ambient executor context
at lowering time, unchanged); a block receiver carries its kind *and* its position, since there is
no ambient context to resolve it from. `EntityNbtPathSegment` gained `Match { match_key: Box<str>,
value: Operand<i32> }`. Everything else in `EntityNbtReadDecl` (segments after the root,
`result_ty`, well-formedness checking) is unchanged — Core stays representation-agnostic about
which physical addressing mode a `Key`/`Index`/`Match` segment will become, exactly as it already
is target-independent about `NbtPathKey` vs. plain `Box<str>`.

### 2.4 Minecraft-target lowering

`DataSource::Block { position: BlockPosition, path: NbtPath }` (mirrors `DataSource::Entity`
exactly). `NbtPathSegment::Match` has a render arm in both the inline path (regular `NbtPath`
`Display`, quoting every key, matching the existing convention, plus the `value_kind` byte-suffix
correction) and, since Slice 2, the macro path
(`crossings.rs::build_entity_nbt_read_line`, extracted from the old `render_data_modify_as_macro`
so both the `String` 2-line shape and the new `Bool`/`I32` 3-line scratch-conversion shape
(`render_entity_nbt_scalar_read_as_macro`) reuse the same segment-rendering loop instead of
duplicating it). The macro renderer's byte-suffix handling is the Slice 2 correction from the top
of this note: the suffix is emitted as literal template text immediately after the `$(key)`
substitution marker, never derived from the bridged value's stored NBT tag.

## Part 3 — Staging

1. **Slice 1 — inline, literal-only, `Chest` only. Implemented and landed.** `BlockPosition` +
   literal `mc.block(Chest, x, y, z)` syntax + `BlockEntityKind::Chest` + the container schema
   (`MatchList` keyed on the real `Slot` field, `Byte`-typed) + `DataSource::Block` inline
   lowering. No macro/runtime route (mirrors PS-12B/C's own const-first staging). Proven against
   the real pinned Java 26.2 server, both the success path (reading a real stack count out of a
   real chest) and the fail-soft path (block replaced with something that isn't a chest).
2. **Slice 2 — runtime slot matching. Implemented and landed**, together with differential and
   pinned-server evidence (folded into this slice rather than a separate Slice 3 — the same
   discipline PS-12C+D used). `check.rs`'s literal-only restriction on `Match` segments was
   removed (the checker already had every other guard `Index` needed —
   `runtime_index_contains_forbidden`, the `Int32` type check — so nothing new was needed there).
   `crossings.rs` gained a real `Match` rendering arm (replacing Slice 1's `unreachable!()` guard),
   applying the `index == 0` leading-separator lesson from PS-12.0/E deliberately (both
   `DataSource::Entity` and `DataSource::Block`'s header text are built before the segment loop
   starts, so no segment is ever "first" inside the loop itself). Also needed a genuinely new
   piece the design note didn't anticipate: the `Bool`/`I32` macro-helper shape needed its own
   3-line `MacroCommand` builder (`render_entity_nbt_scalar_read_as_macro`) — default-init,
   macro-substituted read, plain score-store conversion, all inside one helper function body —
   since the existing 2-line `String`-shape builder didn't have a slot for the extra conversion
   step. Confirmed against the real server that `.count`'s macro-helper route needed no
   String-only assertion lifted incorrectly: the byte-suffix correction above was found and fixed
   before any test was written, not after a test failed.
4. **Slice 4 — more block-entity kinds** (`Barrel`, `Furnace`'s input/output/fuel slots, `Hopper`,
   `ShulkerBox`) as additional `BlockEntityKind` variants and schema rows — near-free once the
   mechanism is proven, the same "prove with one, generalize by table row" story `.count` already
   demonstrated for entities. Re-measure each against the real server before trusting documentation
   — this note's own §1.2 correction is exactly why.

### Non-goals (explicit, matching PS-12's own discipline)

- Block-entity **writes** — read-only, matching PS-12's own entity-NBT scope.
- Runtime-computed block positions (as opposed to a runtime match *value* within a fixed position)
  — literal position only, matching the existing Stage 7.5 spatial-literal restriction. Lifting
  this is a language-wide decision (does MDL get first-class position values at all?), not scoped
  to this feature.
- General compound-predicate matching beyond one field — `Slot` only, and only the one NBT type
  tag (`Byte`) that field actually needs; the `NbtMatchValueKind` lookup is a two-line closed
  table, not a general per-field type registry.
- Any block-entity type beyond what a real client program needs; do not pre-populate the table.
- Detecting *when* a player interacts with a chest — that is
  [`advancement-triggers.md`](advancement-triggers.md), a wholly separate subsystem. This note
  only makes "read what's in the chest right now" possible; combining it with an event trigger is
  what actually answers the original question.

## Sources

- **The real pinned Java 26.2 server, queried directly** (`data get block <pos>` against a real
  placed chest) — the authoritative source once actually available; this is what found and fixed
  §1.2's original documentation-sourced error. Prefer this over documentation for any future
  block-entity kind added under Slice 4.
- [Data component format/container – Minecraft Wiki](https://minecraft.wiki/w/Data_component_format/container)
  — describes item-stack-scoped `minecraft:container` component encoding; **does not apply** to a
  placed block entity's own storage (see the correction at the top of this note) — kept as a
  citation of what turned out to be the wrong shape, not a recommendation.
- [Block entity format – Minecraft Wiki](https://minecraft.wiki/w/Block_entity_format) — common
  block-entity NBT fields, `components` compound (confirmed present but empty for a plain chest).
- [Chest – Minecraft Wiki](https://minecraft.wiki/w/Chest) — chest-specific slot numbering (0-26).
- [`references-design.md`](references-design.md) §4, §11 — the block-rooted-reference invalidation
  row and the original "no schema table exists for block NBT yet" deferral this note resolves.
- [`nbt-schema-system.md`](nbt-schema-system.md) §7 — "a block-entity handle... register their own
  entry in an analogous root table, reusing the same `SchemaNode` walking logic unchanged," the
  design this note follows through on.
- [`entity-nbt-path-composability.md`](entity-nbt-path-composability.md) — the system being
  extended; Part 2's architectural decisions (schema-typed not arbitrary, fail-soft by default,
  bypass the closed-verb recipe system) all carry over unchanged.
