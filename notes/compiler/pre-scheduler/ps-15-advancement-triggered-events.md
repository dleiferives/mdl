# PS-15 — Advancement-Triggered Events

Status: **planned, not yet implemented.** Depends on [PS-13](ps-13-bot-driven-test-infrastructure.md)
and [PS-14](ps-14-player-entity-kind.md).

## Why this exists

This is the push half of MDL's world-interaction story. Every MDL program today runs
because something pulls it — a schedule, a call, a player typing `/function`. There is
no event model anywhere in the compiler, and Minecraft has no native "a player took an
item out of a container" event exposed to commands at all. The vanilla-idiomatic way
every real datapack detects that is the advancement system's `minecraft:inventory_changed`
trigger. The full researched mechanics — the advancement JSON shape, the one-shot
re-fire problem and its standard `advancement revoke @s only <id>` fix, the trigger
vocabulary, and how this composes with block-entity NBT reads — already live in
[`advancement-triggers.md`](../advancement-triggers.md); this document does not repeat
that research. It's the PS-15 execution wrapper: scope, dependencies, and evidence
strategy for turning that design into code.

## Scope

Exactly `advancement-triggers.md` Part 3's **Slice 1** ("prove the mechanism with
exactly one trigger"):

- `AdvancementId`/`AdvancementDecl` Core and Minecraft-IR declaration tables, mirroring
  `FunctionTag`'s existing shape (`ir/minecraft/program.rs`).
- A new `ArtifactFileKind::Advancement` in `datapack/footprint.rs` (today `{ Metadata,
  Function, FunctionTag }`) plus `serialize_advancement`, following
  `serialize_metadata`/`serialize_tag`'s existing pattern — additive, not architectural.
- The auto-revoke guarantee: the compiler, not the source program, emits
  `advancement revoke @s only <resource>` as the unconditional first command of every
  generated reward function body. `advancement-triggers.md` §2.3 calls this "the single
  most important design choice in this note" — it is not optional or configurable.
- `Criterion::InventoryChanged { items: Vec<ItemMatch> }` as the only closed `Criterion`
  variant, `ItemMatch` minimal (item ID only, no count/component predicates yet).
- Source syntax for declaring a handler. `advancement-triggers.md` §2.4 sketches
  `on inventory_changed(items: [...]) |player| { ... }` but deliberately does not commit
  to it — "the exact keyword/attribute form is a real language-design question... that
  deserves its own syntax-decision document under `notes/syntax/`, the way S-042
  preceded PS-12's entity-NBT grammar." That S-0xx document is part of this milestone's
  scope, not a separate one — the feature isn't usable without it.

## Non-goals (inherited from `advancement-triggers.md`)

- A general predicate/condition DSL or multi-criterion AND/OR composition.
- Any trigger type beyond `minecraft:inventory_changed` (that's Slice 2, an explicit
  future milestone, not PS-15).
- Player-visible advancement trees — this stays a fully hidden (`display`-omitted)
  event hook, never surfaced in the advancement tab.
- Anything to do with reading NBT — that's `block-entity-nbt-paths.md`/BE-1 entirely;
  this milestone is push, that one is pull. They compose at the source-program level
  (a reward function body can contain an ordinary entity-NBT or block-NBT read), not at
  the compiler level — neither note hardcodes the composition.

## Verify against current implementation before starting

`advancement-triggers.md`'s own design leans on two structural claims about the
current codebase — confirmed accurate as of this check, but re-confirm before building
on them, since the design doc was written before this milestone's turn arrived:

- `datapack/footprint.rs:9`'s `ArtifactFileKind` is still exactly `{ Metadata,
  Function, FunctionTag }` — no fourth kind has been added by anything else in the
  meantime. Adding `Advancement` is still additive, not a conflict with other
  in-flight work.
- `ir/minecraft/program.rs`'s `FunctionTag`/`FunctionTagId` (`entity_id!`-declared,
  `FunctionTagEntry` at line 95, `FunctionTag` at line 217) is still the closest
  structural precedent — confirm its exact field shape hasn't drifted before modeling
  `AdvancementDecl` after it line-for-line.

Re-check both with a fresh grep immediately before implementation, not from this
document's word — this project's own convention (BE-1's two bugs) is to distrust
anything not verified against the current, actual source at the moment of use.

## Not yet determined

- The S-0xx source syntax decision itself — `on inventory_changed(items: [...])
  |player| { ... }` is a sketch in `advancement-triggers.md` §2.4, explicitly not a
  commitment. This needs its own document under `notes/syntax/`, written and decided
  before (or as part of) this milestone's implementation, not deferred past it — the
  feature has no way to be exercised from source without it.
- Where the auto-revoke command sits relative to program-author-written reward-function
  body code — first line, unconditionally, per `advancement-triggers.md` §2.3 — but
  whether the source-level function body the programmer writes is the *entire* reward
  function (with the revoke silently prepended by the compiler) or a *called-into*
  body (with the compiler generating a separate wrapper function that revokes then
  calls it) is an implementation-shape choice with real consequences for stack
  traces/debugging and hasn't been picked yet.
- Exact `Criterion`/`ItemMatch` Core and Minecraft-IR representation — `advancement-
  triggers.md` sketches the JSON shape and the closed-enum principle but not the actual
  Rust types. Model these once this milestone starts, following `RunModifierRecipeId`/
  `MinecraftSemanticKey`'s existing closed-table conventions rather than inventing a
  new pattern.

## Dependencies

- PS-14's `Player` `EntityKind` — the reward function's `|player|` receiver needs a
  real type to bind.
- PS-13's bot — see Testing below. This is the one milestone in the whole sequence
  where the design doc itself already flagged the evidence problem before any code
  existed: `advancement-triggers.md` Part 3.3, "there is no meaningful Core-evaluator
  story for 'the player did X'... Real coverage is necessarily pinned-server-only."

## Testing

No Core-evaluator differential is possible or attempted — `CoreEvaluator` has no
player, no advancement state, nothing to simulate, and pretending otherwise would be
dishonest evidence. The gate is pinned-server-only, via PS-13's bot:

1. Install the compiled pack, connect the bot.
2. Drive the bot to take a matching item from a chest (or otherwise satisfy the
   criterion directly, e.g. via `/give` to the bot if isolating the criterion from
   container interaction is useful for a narrower unit-style test).
3. Assert the reward function ran (existing `say`-marker log-line pattern).
4. Repeat the same action and assert the reward fires a **second** time — this is the
   proof the auto-revoke actually re-armed the criterion, not just that it fired once
   (which every advancement does by default, revoke or not). Skipping this half of the
   assertion would validate nothing beyond vanilla's own baseline behavior.

## Deferred

- Slice 2 (trigger vocabulary growth beyond `InventoryChanged`) — explicit future PS,
  grown as closed-table rows once Slice 1's JSON-emission and auto-revoke machinery is
  proven reusable infrastructure.
- `ItemMatch` predicates beyond item ID (count, components/custom data).
