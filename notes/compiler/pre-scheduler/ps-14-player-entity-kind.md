# PS-14 — `Player` Entity Kind

Status: **planned, not yet implemented.** Depends on [PS-13](ps-13-bot-driven-test-infrastructure.md).

## Why this exists

`ir::semantic::mod.rs`'s `EntityKind` is `ArmorStand`-only today:

```rust
pub enum EntityKind {
    /// A Minecraft armor stand.
    ArmorStand,
}
```

Every entity-facing surface in the compiler — `mc.entities(Kind)` selector queries,
the PS-12 entity-NBT path system's receiver, the capability/schema table in
`frontend/entity_schema.rs` — is written generically over `EntityKind`, but has never
had a second variant exercise that genericity for real. PS-15's advancement reward
function needs a typed `player` receiver (the reward runs `as`/`at` the triggering
player automatically; the source program needs a way to name that receiver's type).
`advancement-triggers.md` §2.4 flags this exact prerequisite: "**`EntityKind` has no
`Player` variant today**... A `Player` kind... is a prerequisite for this feature,
independent of the trigger-JSON machinery."

## Scope

- Add `EntityKind::Player` alongside `ArmorStand` in `ir/semantic/mod.rs:65`, including
  `from_source_name("Player")` (`mod.rs:73`) and the matching `source_name` arm.
- Selector rendering: `EntityKind` currently reaches selector text through at least two
  confirmed call sites — `ir/minecraft/selector.rs`'s `SelectedEntityKind::ArmorStand`
  (lines 64, 98, 145, rendering `"minecraft:armor_stand"`), and
  `lower/minecraft/query.rs:52`'s `EntityKind::ArmorStand => EntitySelector::
  armor_stands(tags, query.maximum())`. Both need a `Player` arm. Whether that renders
  as `type=minecraft:player` or an `@a` shorthand is a real choice — `@a` already
  implicitly means "all players" in vanilla, so a `type=minecraft:player` predicate on
  an `@e` base would be redundant with just using `@a` directly. Decide by checking how
  `EntitySelector`'s existing base-selector choice (`@e` vs `@a` vs `@s`) is modeled
  structurally before picking, not by guessing which reads better.
- Schema/capability table: `entity_schema.rs:173`'s `root_schema(EntityKind) ->
  SchemaNode` gains a `Player` arm, alongside `ArmorStand`'s existing
  `ARMOR_STAND_ROOT` (`entity_schema.rs:161-162`, itself
  `Compound(&[("equipment", EQUIPMENT_SLOTS)])`, `EQUIPMENT_SLOTS` at line 152). The
  expectation, to be confirmed rather than assumed, is that this is a "prove by table
  row" job like `.count` was in PS-12E — a player's equipment components are the same
  NBT shape an armor stand's are, so `PLAYER_ROOT` likely reuses `ARMOR_STAND_ROOT`
  verbatim. See "Verify against current implementation" below before assuming that.
- Capability set: `advancement-triggers.md` notes players have real capabilities
  `ArmorStand` doesn't (inventory, command permission level) but PS-14 itself should
  scope narrowly to whatever `check_entity_query_root` (`check.rs:2543`) structurally
  requires to accept `Player` as a valid receiver — not a general player-capability
  model. Broader capability modeling is explicit future work, added by table rows,
  exactly like every other schema growth in this codebase.

## Verify against current implementation before starting

- The `ARMOR_STAND_ROOT` reuse assumption above is exactly the kind of claim BE-1's own
  history shows this codebase should not trust without measurement — BE-1 assumed a
  container's NBT shape from web research and shipped a real bug; PS-14 should not
  repeat that pattern by assuming a player's equipment schema from precedent alone.
  Before writing `PLAYER_ROOT`, measure a real connected bot's equipment NBT against
  the pinned server (`data get entity <bot> equipment` or the per-slot component path
  BE-1's own read chain already exercises) and compare it field-for-field against what
  `ARMOR_STAND_ROOT` encodes, rather than finding a mismatch via a failing test later.
- Re-grep `EntityKind::ArmorStand` across the tree before starting — the search that
  grounded this document found matches in `selector.rs`, `query.rs`, `audit.rs`, and
  `preflight.rs`, several of them test-only fixture values (e.g. `preflight.rs`'s test
  module uses `EntityKind::ArmorStand` as a fixed test input, not something that
  structurally needs a `Player` counterpart). Distinguish "needs a new match arm for
  correctness" from "is a test fixture that can stay `ArmorStand`-only" per call site —
  don't mechanically add a `Player` arm everywhere the type name appears.

## Not yet determined

- Exact selector text for `Player` (`@a` vs. `type=minecraft:player` vs. something
  else) — see Scope above.
- Whether `Player`'s capability set needs anything beyond what `check_entity_query_root`
  requires structurally for this milestone specifically, versus later broader
  capability work.

## Non-goals

- No advancement/event machinery (PS-15).
- No new selector predicate vocabulary beyond what's needed to name "a player" as a
  query target.
- No general "player capabilities" (permission level, gamemode, experience, etc.)
  beyond what's needed to make `Player` a legitimate entity-NBT and selector receiver.

## Dependencies

PS-13's bot gives this milestone something that has never existed in this project's
test suite before: a real player entity to test against. Structural/differential tests
(selector text, schema-table reachability) don't need it, but the actual behavioral
claim — "reading `player.equipment.mainhand...` against a genuinely connected player
resolves the way `reader.equipment.mainhand...` already does for an armor stand" — does.

## Testing

- Unit: schema-table reachability/typing test for the new `Player` root, mirroring
  `entity_schema.rs`'s existing `book_page_chain_walks_the_table_end_to_end`.
- Structural/differential: selector rendering and entity-NBT path lowering for a
  `Player`-typed receiver, mirroring `ps12c_entity_nbt_path_lowering.rs`'s style.
- Pinned server (PS-13-gated): connect the bot, give it a known item, read it back via
  an MDL-compiled `player.equipment.mainhand...` chain, assert the value matches —
  the first genuinely player-backed proof this compiler has ever produced.

## Deferred

- Full player capability modeling (permission level, gamemode, experience, advancement
  state beyond what PS-15 needs).
- Any selector predicate beyond identifying "a/the player" as a target kind.
