# PS-14 — `Player` Entity Kind

Status: **implemented (2026-07-22).** `EntityKind::Player` landed exactly as scoped
below: the four production-code sites, `PLAYER_ROOT` scoped to armor slots only
(`mainhand`/`offhand` deferred), a unit test in `entity_schema.rs`, a structural/
differential test (`crates/mdl-compiler/tests/ps14_player_entity_kind.rs`), and a
pinned-server test (`crates/mdl-test-bot/tests/ps14_player_equipment_read.rs`) —
this compiler's first genuinely player-backed proof, run live against the pinned
26.2 server. Depends on [PS-13](ps-13-bot-driven-test-infrastructure.md) (complete).
This document is written as an implementation handoff — a fresh agent with no
memory of this session should be able to execute it directly from what's here,
without re-deriving the research.

## Why this exists

`ir::semantic::mod.rs`'s `EntityKind` is `ArmorStand`-only today. PS-15's advancement
reward function needs a typed `player` receiver (the reward runs `as`/`at` the
triggering player automatically; the source program needs a way to name that
receiver's type). `advancement-triggers.md` §2.4: "**`EntityKind` has no `Player`
variant today**... A `Player` kind... is a prerequisite for this feature."

## The implementation methodology to use

**Add the enum variant first, then let `rustc`'s exhaustiveness checker enumerate
every real call site — do not try to hand-audit the codebase for every place
`EntityKind::ArmorStand` appears.** A `grep -rn "EntityKind::ArmorStand"` across
`crates/mdl-compiler/src` returns **~50 hits**, and it is easy to over-scope from
that number alone. Verified directly (checked every non-`lower/minecraft/query.rs`,
non-`entity_schema.rs`, non-`selector.rs` hit): **all but four are inside
`#[cfg(test)] mod tests` blocks**, constructing a concrete `EntityKind::ArmorStand`
value to exercise already-generic machinery (`ContextRequirement::Required`,
`StaticEntityQuery::entities`, `ExecutorType::new`, etc.) — none of those need a
`Player` counterpart added for correctness, only new tests if `Player`-specific
behavior is worth covering separately. The pattern this codebase already uses
everywhere (`for_executor_kind`, `behavior_for_executor_kind`, `AmbientContextRequirements`)
takes `kind: EntityKind` as a plain parameter and threads it through generically —
confirmed by reading `ir/semantic/minecraft.rs:93-99` directly: `for_executor_kind`
matches on `self` (the operation kind: `CurrentExecutorKind`), never on the `kind`
parameter. This means `Say`/`TeleportCurrentExecutor`/`MoveCurrentExecutorBy` and the
whole ambient-context/executor-capture system already work for *any* `EntityKind`,
`Player` included, with zero changes.

## The four real production-code changes (verified, not estimated)

1. **`ir/semantic/mod.rs:65-94`** — add `EntityKind::Player`:
   ```rust
   pub enum EntityKind {
       ArmorStand,
       Player,
   }
   ```
   plus a `from_source_name("Player")` arm (line 73-78), a `source_name` arm (line
   82-86, returns `"Player"`), and a `capabilities()` arm (line 90-94). For
   capabilities, add a new named constant rather than reusing `ARMOR_STAND`'s bit
   pattern by reference — `EntityCapabilities::PLAYER = Self(Self::COMMAND_EXECUTOR_BIT
   | Self::INVENTORY_HOLDER_BIT)` (same bits as `ARMOR_STAND` today, since a player is
   both a valid command executor and an inventory holder, but a distinct named
   constant keeps room for the two to diverge later without a silent behavior change).
   `Display` needs no separate change — it already delegates to `source_name()`.

2. **`frontend/entity_schema.rs:173-177`** — `root_schema`'s match:
   ```rust
   pub(super) const fn root_schema(kind: EntityKind) -> SchemaNode {
       match kind {
           EntityKind::ArmorStand => ARMOR_STAND_ROOT,
           EntityKind::Player => PLAYER_ROOT,
       }
   }
   ```
   `PLAYER_ROOT`'s actual shape is **not** a straight reuse of `ARMOR_STAND_ROOT` — see
   "The measured schema divergence" below, this is the one place this document
   overturns the original plan.

3. **`ir/minecraft/selector.rs`** — add `SelectedEntityKind::Player` (line 62-65),
   an `EntitySelector::players(tags, limit) -> Result<Self, EntitySelectorError>`
   constructor mirroring `armor_stands` (lines 82-102) exactly (same body, just
   `kind: SelectedEntityKind::Player`), and a `Display` arm (line 141-146) rendering
   `"minecraft:player"`. Confirmed: `EntitySelector` always renders as `@e[type=<kind>,
   tag=...,limit=...]` — there is no structural support for an `@a`-shorthand base
   selector anywhere in this type, and adding one would be new architecture, not a
   table row. `@e[type=minecraft:player,...]` is functionally equivalent to `@a` in
   real Minecraft (both select player-type entities) and is the correct, minimal,
   consistent choice — resolves what the previous draft of this document left open.

4. **`lower/minecraft/query.rs:51-53`** — `lower_static_entity_query`'s match:
   ```rust
   match query.kind() {
       EntityKind::ArmorStand => EntitySelector::armor_stands(tags, query.maximum()),
       EntityKind::Player => EntitySelector::players(tags, query.maximum()),
   }
   ```

That's the complete required production surface. After making these four changes,
run `cargo check -p mdl-compiler --lib` and fix whatever `rustc` reports — if it
reports anything outside these four sites, that's new information this document
didn't have; trust the compiler over this document in that case.

**`frontend/check.rs`'s `check_entity_query_root` (line 2543-2587, the checker for
`mc.entities(Kind)`) needs zero changes** — confirmed by reading it directly. It
already just calls `EntityKind::from_source_name(&spelling)` and uses whatever comes
back; `mc.entities(Player)` will parse and check correctly the moment step 1 above
lands, with no new checker branch.

## The measured schema divergence — the one real research finding here

The original plan assumed `PLAYER_ROOT` would reuse `ARMOR_STAND_ROOT` verbatim (a
"prove by table row" job like `.count` was in PS-12E). **Measured directly against a
real connected player via PS-13's bot infrastructure — this assumption is only half
right.**

Setup: connected a bot, gave it a diamond chestplate (`item replace entity
mdl-test-bot armor.chest with minecraft:diamond_chestplate`) and a written book to
the mainhand slot (`item replace entity mdl-test-bot weapon.mainhand with
minecraft:written_book 1`, then `data merge entity mdl-test-bot {equipment:{mainhand:
{components:{...}}}}` for the book content — the exact two-step pattern
`crates/mdl-test/tests/ps12e_entity_nbt_scalar_semantics.rs:78-79` already uses for
ArmorStand). Queried `data get entity mdl-test-bot equipment` and `data get entity
mdl-test-bot Inventory`:

```
mdl-test-bot has the following entity data: {chest: {count: 1, id: "minecraft:diamond_chestplate"}}
mdl-test-bot has the following entity data: [{count: 1, Slot: 0b, id: "minecraft:written_book"}]
```

**Armor slots (`chest`, and by the same mechanism presumably `head`/`legs`/`feet`)
show up under `equipment` for a player exactly like they do for an ArmorStand** —
`ITEM_STACK`'s shape (`count`, `components`, unregistered-but-present `id`) matches.

**`mainhand` does not appear in the `equipment` compound at all**, even after forcing
`data merge entity mdl-test-bot {SelectedItemSlot:0}` to make the book's inventory
slot (confirmed `Slot: 0b`) match the selected hotbar slot exactly. Re-queried after
forcing this — still absent. This was checked twice, including the explicit
`SelectedItemSlot` correction, not a one-off fluke.

**Interpretation:** a player's mainhand/offhand items live in the ordinary
`Inventory` list (matched by `Slot`, with the selected slot tracked separately in
`SelectedItemSlot`), not in a dedicated `equipment.mainhand` field the way an
ArmorStand's do. The unified `equipment` query view appears to only synthesize armor
slots for players, not hand slots — this is inference from the measurement, not
confirmed against Minecraft's own source, and is exactly the kind of claim a future
agent should re-verify before depending on it further.

**What this means for scope:** reading a player's *held* item honestly needs a
two-hop chain this codebase has no mechanism for yet — read `SelectedItemSlot`
(a plain `Int32` scalar), then use that value to match into `Inventory`
(`Inventory[{Slot: <that value as a Byte>}]`). This is structurally close to BE-1's
`SchemaNode::MatchList`/`NbtPathSegment::Match` (a list matched by a compound field
rather than positional index) but with a new twist neither BE-1 nor PS-12 needed:
**the match value must come from reading a different NBT field on the same entity
first**, not from a compile-time constant or an ordinary MDL runtime expression. No
existing part of the entity-NBT path system supports "the index is itself the result
of another read" — this would be new design work, not a table-row addition.

**Recommendation: scope `PLAYER_ROOT` to armor slots only for this pass.** Define it
as a `Compound` with `head`/`chest`/`legs`/`feet` (each `ITEM_STACK`), omit
`mainhand`/`offhand` entirely rather than wiring them to something that silently
returns the fail-soft default forever. If a source program writes
`player.equipment.mainhand...`, it should get a clear "field not found" checker
diagnostic (the existing schema-walk failure path — confirmed generic, no special
casing needed) — not compile successfully into a read that can never produce a
non-default value. Treat `player.equipment.mainhand`/`.offhand` as explicit, called-out
future work (see "Deferred"), not something this pass should silently under-deliver.
Before committing to this, it would be worth one more experiment: check whether an
item a bot picks up "for real" (via `open_container_and_click`, already working)
rather than one placed with `item replace ... weapon.mainhand` behaves differently —
this document did not check that variant.

## Bot infrastructure usage notes (PS-13 is done — use it, don't rebuild anything)

`crates/mdl-test-bot/` is a **separate, non-workspace crate** — build/test it from
inside that directory with `cargo +nightly`, never from the repo root. Concrete
patterns to reuse directly, not rediscover:

- `BotHandle::connect(&server, "name")` — connects, waits for `Event::Spawn`, returns
  a ready `BotHandle`. Bounded (30s timeout, `BotError::SpawnTimeout`/
  `SpawnChannelClosed`).
- `bot.open_container_and_click(BlockPos { x, y, z }, slot)` — already proven end to
  end (see PS-13's own `tests/chest_click.rs`), not needed for PS-14 itself, but the
  exact pattern to imitate if PS-14's own tests need the bot to interact with
  anything rather than just exist as a query target.
- **`bot.shutdown()` can legitimately return `Err(BotError::ShutdownTimeout)`** after
  any container interaction (measured, real, still not root-caused — see PS-13's own
  doc). Do not `.expect()` it as a hard test failure; log it and move on, the way
  `chest_click.rs` does. The underlying OS thread does not outlive the test process.
- Giving a player a written book: **two commands, not one** —
  `item replace entity <name> weapon.mainhand with minecraft:written_book 1` then
  `data merge entity <name> {equipment:{mainhand:{components:{'minecraft:written_book_content':
  {pages:[{raw:{text:'...'}}],title:{raw:'...'},author:'...',generation:0,resolved:true}}}}}`.
  A single `/give ... minecraft:written_book{written_book_content:{...}}`-style
  embedded-NBT command was tried first and silently produced an empty inventory —
  don't repeat that; use the two-step form other tests in this repo already prove
  works.
- Any assertion depending on server-side state settling after a bot action needs a
  short sleep (PS-13 used 500ms after a click) — packet round trips are not
  synchronous with the Rust call that sends them.

## Testing plan

- **Unit** (`entity_schema.rs`, no server needed): a schema-table reachability/typing
  test for `PLAYER_ROOT`'s armor slots, mirroring the existing
  `book_page_chain_walks_the_table_end_to_end`-style tests already in that file's
  `#[cfg(test)] mod tests` (starts around line 227 as of this reading) — walk
  `.equipment.chest.count`, assert `Int32`.
- **Structural/differential** (`crates/mdl-compiler/tests/`, no server needed):
  `mc.entities(Player)` selector rendering, and a `player.equipment.chest...` entity-NBT
  path lowering test, mirroring `ps12c_entity_nbt_path_lowering.rs`'s style (compile
  source, inspect the emitted `.mcfunction` text for the expected `@e[type=
  minecraft:player,...]`/NBT path shape).
- **Pinned server, PS-13-gated** (new test file under `crates/mdl-test-bot/tests/`,
  `cargo +nightly`): connect a bot, give it armor via the `item replace entity ...
  armor.chest with ...` pattern already proven above, compile and run an MDL program
  reading `player.equipment.chest.count` (or similar) through a proven `mc.entities
  (Player)`/executor-capture chain, assert against the console — the first genuinely
  player-backed proof this compiler has ever produced. This is the milestone's own
  calibrated evidence, the same role `chest_click.rs` played for PS-13.

## Non-goals

- No advancement/event machinery (PS-15).
- No new selector predicate vocabulary beyond identifying "a player" as a query
  target kind.
- No general "player capabilities" (permission level, gamemode, experience) beyond
  the `CommandExecutor`/`InventoryHolder` bits armor-slot reads and executor capture
  already need.
- `player.equipment.mainhand`/`.offhand` — see "Deferred," not silently attempted.

## Deferred

- **`player.equipment.mainhand`/`.offhand`** — needs a genuinely new mechanism (a
  match value sourced from reading a different NBT field first, not a constant or
  ordinary expression) that doesn't exist anywhere in this codebase yet. Real design
  work, not a table row. Worth its own note under `notes/compiler/` if picked up,
  given it likely also matters for any future entity kind with inventory-backed
  (rather than dedicated-field) equipment.
- Full player capability modeling (permission level, gamemode, experience,
  advancement state beyond what PS-15 needs).
- Whether an item picked up "for real" by the bot (vs. placed via `item replace`)
  behaves differently for the mainhand-equipment question — flagged above, not
  checked.
