# PS-17 — Capstone: `tests/programs/chest-menu`

Status: **implemented and landed (2026-07-23).** The recommended minimal design shipped
unchanged: `crates/mdl-compiler/tests/source-fixtures/pre-scheduler/ps17_chest_menu_capstone.mdl`
(three `on inventory_changed` handlers, zero new Core ops/recipes/checker special-cases)
and `crates/mdl-test-bot/tests/ps17_chest_menu_capstone.rs` (a real bot clicking through
both screens on the pinned Java 26.2 server), both passing. Both open questions below were
confirmed directly before implementation: `player.teleport(...)` is callable on an
`on`-handler's `player` capture the same way `say` is (`frontend/check.rs`'s
`check_event_handler` establishes the same executor-capture context both methods dispatch
through — no `run.as(player) { ... }` wrap needed), and PS-16's write API cleanly clears a
slot via `.{id: "minecraft:air", ...}` with no "Serialization errors" warning. Writing the
pinned-server walkthrough also surfaced two real findings not anticipated by this document:
this test harness's flat world floor sits at `y=-60`, not `y=4` (every earlier PS-13–17 test
floats blocks/players at `y=4` without ever needing them to rest there — this milestone's own
position-check assertion was the first to depend on standing still), and leaving a clicked
button item in the player's inventory lets an unrelated later `tp` spuriously re-satisfy the
auto-revoked `inventory_changed` criterion and refire the handler mid-reposition, worked
around by clearing the item before moving the bot back to the chest. This document is
retained for the research and design that shaped the implementation; it was originally
written for a fresh agent with no memory of prior sessions, and is grounded in PS-15's actual
landed test
(`crates/mdl-test-bot/tests/ps15_advancement_inventory_changed.rs` and its fixture
`ps15_advancement_inventory_changed.mdl`) and BE-2's real fixture
(`be2_block_entity_nbt_runtime_slot.mdl`) — read both before starting, they are the
closest real, working precedent for everything this milestone composes.

## Why this exists

This is the actual feature the whole PS-13–17 sequence was requested for: a player
opens a real placed chest, clicks an item, and the compiled program reacts —
teleporting them, giving them an item, or presenting the next "screen" of a menu by
rewriting the chest. Every piece it needs already has a home elsewhere in this
sequence; PS-17's only job is proving they compose, on the real pinned server, driven
by a real bot.

## What it composes, concretely, with real (not sketched) syntax

- **PS-15's event handlers** — confirmed real, working syntax from the landed fixture:
  ```mdl
  on inventory_changed(.items = ["minecraft:emerald"]) |player| {
      player.say("MDL_TELEPORTED");
      run.as(player) { /* whatever TeleportCurrentExecutor needs, confirm exact call shape */ }
  }
  ```
  `player` is a fully-capable executor-capture binding (`player.say(...)` already proven
  live) — confirm whether `TeleportCurrentExecutor`/`MoveCurrentExecutorBy` are callable
  directly on `player` the same way, or need an explicit `run.as(player) { ... }` wrap;
  check how the PS-2/stage7 fixtures call `Teleport` today and mirror that shape exactly
  rather than guessing. Each distinct clickable "button" needs its **own** `on
  inventory_changed` handler, one per distinct item ID — `ItemMatch` is item-ID-only
  (PS-15 Slice 1), so a chest with two buttons showing different items needs two
  top-level `on` declarations, not one handler branching on which item fired.
- **BE-1's reads** (`mc.block(Chest, x, y, z).Items[slot].count`/`.id` once readable —
  confirm `.id` is actually in the registered schema before depending on it; the schema
  table historically left `id` unregistered as explicit follow-up, check
  `frontend/entity_schema.rs`'s current `CHEST_ITEM_ENTRY` before assuming it's there)
  — needed only if a handler wants to confirm which screen is currently showing before
  acting. Not required for the minimal design below (see "Recommended minimal design").
- **PS-16's writes** (`mc.block(Chest, x, y, z).Items[slot] = .{id: "...", count: N}`,
  once implemented) — rewrite a chest slot to present the next screen's button.
  **Confirm slot-clearing works before depending on it**: `item replace block <pos>
  container.<slot> with minecraft:air` is already a proven pattern elsewhere in this
  codebase (`crates/mdl-test-bot/tests/ps14_player_equipment_read.rs:98` clears an
  armor slot this way) but PS-16's own write API accepting `.{id: "minecraft:air", ...}`
  as a value hasn't been exercised — check this specifically, since "clear the old
  button" is part of a clean screen transition.
- **`TeleportCurrentExecutor`** (already implemented) — advancement rewards run
  `as`/`at` the triggering player automatically, so no explicit executor query is
  needed to teleport `player`.
- **PS-13's bot** — drives the actual multi-step walkthrough the test asserts against,
  exactly like `ps15_advancement_inventory_changed.rs` already does (reuse its
  connect/teleport-into-forceloaded-chunk/give/assert/shutdown shape directly).

## Real gotcha, confirmed from PS-15's own test: don't use `minecraft:diamond`

`minecraft:diamond` collides with vanilla's own real, permanent, one-shot-forever
"Diamonds!" achievement (`minecraft:story/mine_diamond`) — PS-15's own test had to
work around this by only checking the hidden-advancement claim in a log window *after*
that vanilla advancement was already permanently earned. **Pick different items for
this capstone's buttons/reward** — anything without an obvious vanilla single-item
achievement tied to it (e.g. `minecraft:emerald`, `minecraft:gold_ingot`,
`minecraft:redstone_block`, `minecraft:stick` are all plausible; don't assume any of
these are collision-free without checking, but they're less likely than diamond).

## Recommended minimal design

Two screens, three buttons total — the smallest shape that exercises every dependency
(a real click, a real event fire, a real write, a real re-click on the *rewritten*
state, proving the write had an observable effect, not just that it compiled):

- **Screen 1**: chest slot 0 holds `minecraft:emerald` (teleport button), slot 1 holds
  `minecraft:gold_ingot` (advance button).
- Clicking the emerald: `player.say(...)` + teleport, screen unchanged (no write).
- Clicking the gold ingot: rewrites slot 0 to `minecraft:air` (clear), slot 1 to
  `minecraft:redstone_block` (screen 2's only button) — **this is the write PS-16 exists
  for, and the moment this milestone actually needs it**.
- **Screen 2**: clicking the redstone block gives the player a reward item
  (`unsafe minecraft("give @s minecraft:stick 1")` — see "The `give` decision" below)
  and says a distinct marker.

This forces: two real advancement fires (different criteria), one real block write with
a real re-read afterward to confirm it landed, and a third advancement fire that could
only succeed if the write actually changed the chest's contents (since the redstone
block didn't exist in the world until the write created it) — that last fact is the
actual end-to-end proof, not just "the write command ran."

## The `give` decision — resolved, don't re-litigate

`MinecraftSemanticKey` (`ir/semantic/minecraft.rs`) still has only `Say`,
`TeleportCurrentExecutor`, `MoveCurrentExecutorBy` as of this research pass (verify
this hasn't changed). There is no precedent anywhere in this codebase for an MDL
*source program* issuing a `give` command — the existing `.command("give ...")` calls
you'll find via grep are all in Rust test harness code driving the server directly,
not compiled MDL. **Recommendation: `unsafe minecraft("give @s minecraft:stick 1")`,
not a new typed builtin.** This capstone is (by design) the only place in the whole
PS-13–17 sequence that needs "give," matching this project's own established pattern
of leaving a one-off vanilla capability as `unsafe` rather than promoting it to typed
status pre-emptively (see PS-12E's own history of what got promoted vs. what stayed
`unsafe`). If a later milestone needs `give` again, that recurrence is the actual
signal to add the typed builtin — not this one.

## Non-goals, explicit

- **Correlating which specific chest an event came from.** `minecraft:inventory_changed`
  fires globally on the triggering item ID, not scoped to a particular chest instance —
  if two different chests both offered an emerald button, both would fire the same
  handler. This capstone has exactly one chest, so this never surfaces, but it is a
  real limitation of Slice 1's `ItemMatch`, not something this milestone solves.
  `advancement-triggers.md` §2.5 already scoped chest-correlation as an explicit
  source-program-level composition (a handler body reading block NBT to check state),
  available if a future milestone needs it — not required here.
- Clearing the picked-up button item from the player's own inventory after a click —
  cosmetic, not required to prove the mechanism, skip it.
- Any real "menu" UX polish (multiple items per screen beyond what's needed to prove
  the write round-trip, back-navigation, etc.).

## Testing

One pinned-server test, `crates/mdl-test-bot/tests/ps17_chest_menu_capstone.rs`,
structured directly after `ps15_advancement_inventory_changed.rs`'s own shape (same
connect/forceload/teleport/give/assert/shutdown pattern — copy its structure, don't
redesign the harness usage):

1. Place the chest, stock screen 1 (emerald in slot 0, gold ingot in slot 1).
2. Connect the bot, position it at the chest.
3. Click the emerald slot (`open_container_and_click`) — assert the teleport marker
   and (if feasible) the bot's new position via console.
4. Click the gold ingot slot — assert the screen-transition marker, then `data get
   block <pos> Items` via console to confirm slot 0 is now air/absent and slot 1 is
   now `minecraft:redstone_block` — this is the real proof PS-16's write worked, not
   just that the handler ran.
5. Click the (now-present) redstone block slot — assert the reward marker, and
   (if feasible) confirm the stick actually landed in the bot's inventory via console
   `data get entity <bot> Inventory`, the same style BE-2/PS-14's own tests already use.

No Core-evaluator differential — same reasoning as PS-15, `CoreEvaluator` has no
player/advancement/block-write state to simulate honestly.

## Structural ideas to carry forward

- Mirror PS-3's own discipline exactly: this milestone adds **zero** new Core ops,
  recipes, or checker special-cases. If implementing the program surfaces a need for
  one, that's a sign a piece of PS-13 through PS-16's scope was mis-sized — go fix the
  milestone that actually owns it, don't quietly patch it in here.
- Keep the program itself minimal on purpose — the two-screen, three-button design
  above is already the smallest shape that forces every dependency to be exercised
  honestly; resist the urge to build a more elaborate "real" menu for this milestone.

## Dependencies

PS-13, PS-14, PS-15, PS-16, in that order — the one milestone in the sequence that
cannot start early or in parallel with its dependencies, since it has no scope of its
own beyond composing them. Confirm PS-16 has landed and passed its own pinned-server
gate (including the "no Serialization errors warning" assertion its own document
requires) before starting this one.
