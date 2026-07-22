# Advancement-Triggered Events — Giving MDL a Push Model

Date: 2026-07-21
Status: **proposed design, not yet scheduled as a milestone.**

## Why this note exists

Every MDL program today runs because something *pulls* it: a tick schedule, another function
calling it, a player typing `/function ...`, or a `#minecraft:load` tag. There is no push/event
model anywhere in the compiler — `ir::minecraft::CommandKind` has no advancement variant, and
nothing in `datapack/emit.rs` emits anything but `.mcfunction` files, `pack.mcmeta`, and function
tags. This matters concretely for the question that started this investigation: Minecraft has **no
native "a player took an item out of a container" event** exposed to commands at all (confirmed by
research, not assumed — see Sources). The vanilla-idiomatic way every real datapack detects "the
player just received an item" is the advancement system's `minecraft:inventory_changed` trigger.
Without any advancement modeling, MDL simply cannot express this class of program, no matter how
good its NBT-reading story ([`block-entity-nbt-paths.md`](block-entity-nbt-paths.md)) gets — reading
is pull, detecting is push, and MDL only has pull today.

This is a standalone subsystem, not an extension of the entity-NBT path system. It doesn't reuse
`SchemaNode`, the crossings engine, or anything else PS-11/PS-12 built. It composes *with* them
(a triggered function body can read block-entity NBT to correlate the event with a specific chest)
but doesn't depend on them structurally.

## Part 1 — Real Minecraft mechanics (researched)

### 1.1 Advancement JSON shape

`data/<namespace>/advancement/<path>.json`:

```json
{
  "criteria": {
    "<criterion name>": {
      "trigger": "minecraft:inventory_changed",
      "conditions": { "items": [ { "items": ["minecraft:diamond"] } ] }
    }
  },
  "requirements": [["<criterion name>"]],
  "rewards": { "function": "mynamespace:on_diamond_taken" }
}
```

`display` is **optional**. Omitting it produces a fully hidden advancement — no toast, no tree
entry, no chat message, purely a criteria-to-function hook. This is exactly the shape MDL wants to
emit; there is no reason a compiler-generated event hook should ever be player-visible in the
advancement tab.

### 1.2 The one-shot problem, and the fix every real datapack uses

An advancement's reward fires **once per player, ever** — the moment its criteria first becomes
satisfied, like any other achievement. It does not re-fire on repeated satisfaction. The universal
datapack idiom to make this a *repeatable* event is: the reward function's **first command** is
`advancement revoke @s only <namespace>:<path>`, immediately un-granting the advancement so its
criteria can be satisfied — and its reward re-triggered — again later. Forgetting this silently
turns "every time" into "only the first time in that player's save," which is exactly the kind of
footgun this compiler consistently designs away rather than documents around (see Part 2.3).

### 1.3 Trigger vocabulary

Vanilla ships dozens of trigger types (`minecraft:inventory_changed`, `minecraft:placed_block`,
`minecraft:item_used_on_block`, `minecraft:consume_item`, `minecraft:enter_block`, and many more),
each with its own `conditions` shape. MDL does not need, and should not attempt, to model all of
them at once — see staging.

## Part 2 — Design: how this fits MDL's architecture

### 2.1 Emission: a drop-in fourth artifact kind

`datapack/footprint.rs::ArtifactFileKind` is `{ Metadata, Function, FunctionTag }` today, and
`datapack/emit.rs::emit_datapack` already has a uniform pattern for each: serialize a
program-owned table to JSON, push a `PackFile`, let the existing sort/dedupe-path-collision pass
handle the rest (see `serialize_metadata`/`serialize_tag`). Adding `ArtifactFileKind::Advancement`
plus a `serialize_advancement` following the identical shape is additive, not architectural — the
emission pipeline was already built generic over "kinds of files," never hardcoded to `.mcfunction`
alone.

### 2.2 A new Minecraft-IR declaration table, mirroring `FunctionTag`

`FunctionTag { resource, origin, merge, entries }` (`ir/minecraft/program.rs`) is the closest
existing precedent — a named JSON artifact wired to callables, dense-tabled with an `entity_id!`
identity (`FunctionTagId`), declared through a builder method, verified, printed, emitted. An
`Advancement` declaration follows the same template:

```rust
entity_id!(pub struct AdvancementId;);

pub struct AdvancementDecl {
    resource: AdvancementResourceId,
    criterion: Criterion,          // closed enum, see 2.3
    reward: InternalCallableRef,   // the MDL function to run
    origin: OriginId,
}
```

`Criterion` starts as a **small closed enum**, not a general JSON predicate DSL — matching every
other closed-vocabulary table in this compiler (`MinecraftSemanticKey`, `SchemaNode`,
`RunModifierRecipeId`). Each variant owns its own typed `conditions` shape; an unrecognized trigger
is a compile error, exactly like an unrecognized schema key. Growing the vocabulary is "add a
variant + its condition struct + its JSON serialization," not a new subsystem each time.

### 2.3 The load-bearing decision: auto-inject the revoke, don't ask the source program to remember it

The compiler, not the programmer, emits `advancement revoke @s only <resource>` as the
unconditional first command of the generated reward function body. This is the single most
important design choice in this note. Every other closed system in this compiler chooses
"structurally impossible to get wrong" over "documented, but the programmer must remember it" —
PS-12's fail-soft default, the closed schema tables, the triple-verified recipe contracts. A
one-shot-by-default event primitive that silently stops firing after the first use is exactly the
kind of foot-gun this codebase has never shipped, and there's no reason to start here. If a source
program ever legitimately wants one-shot (achievement-style) semantics, that should be an explicit,
separately-named construct later — not the default behavior of "detect this repeatedly."

### 2.4 Source syntax (sketch only — needs its own S-0xx decision)

Something in the shape of:

```
on inventory_changed(items: [ItemMatch("minecraft:diamond")]) |player| {
    player.say("MDL_GOT_DIAMOND");
}
```

is plausible, but the exact keyword/attribute form is a real language-design question (declaration
syntax? an attribute on an ordinary `fn`? a `run.on(...)` form parallel to `run.as(...)`?) that
deserves its own syntax-decision document under `notes/syntax/`, the way S-042 preceded PS-12's
entity-NBT grammar. This note deliberately does not pick one — its job is the compiler-side
composition story (IR shape, emission, the auto-revoke guarantee), not the keyword.

One hard prerequisite this surfaces: the triggering player needs a typed receiver, and
**`EntityKind` has no `Player` variant today** (`ir/semantic/mod.rs`'s `EntityKind` is
`ArmorStand`-only). A `Player` kind — with its own capability set, since players aren't
`ArmorStand`s and have real capabilities (inventory, command permission level) `ArmorStand` doesn't
— is a prerequisite for this feature, independent of the trigger-JSON machinery above.

### 2.5 How this composes with [`block-entity-nbt-paths.md`](block-entity-nbt-paths.md)

This is the payoff the original question was actually asking about. Neither note depends on the
other structurally, but together: a reward function triggered by `inventory_changed` receives a
`player`, and inside that body an ordinary `mc.block(x, y, z).components."minecraft:container"...`
read (Doc A) can check whether a specific chest is now missing the item the player just gained —
correlating *which* container an item came from, something the `inventory_changed` trigger alone
cannot tell you (it only knows the player's inventory changed, not the source). Two independently
useful, independently buildable features that only become "detect a player grabbing items from a
chest" when composed at the source-program level, not the compiler level. Neither note should try
to hardcode that composition — it falls out for free once both exist.

## Part 3 — Staging

1. **Slice 1 — prove the mechanism with exactly one trigger.** `Player` `EntityKind` variant +
   `Advancement`/`AdvancementId` Core and Minecraft IR tables + hidden-advancement JSON emission +
   the auto-revoke guarantee + `Criterion::InventoryChanged { items: Vec<ItemMatch> }` as the only
   variant, with `ItemMatch` itself minimal (item ID only, no count/component predicates yet).
   Mirrors PS-12's own "prove with the book-page chain before generalizing" discipline.
2. **Slice 2 — grow the trigger vocabulary** as closed-table rows (`Criterion::PlacedBlock`, etc.),
   each independently small once the JSON-emission and auto-revoke machinery from Slice 1 is
   reusable infrastructure.
3. **Slice 3 — evidence, and this is the hard one.** Every other PS-milestone in this codebase
   validates against a Core evaluator differential *and* a pinned server. There is no meaningful
   Core-evaluator story for "the player did X" — `CoreEvaluator` has no player, no advancement
   state, nothing to simulate. Real coverage is necessarily **pinned-server-only**: install the
   pack, issue the real commands that satisfy the criteria (e.g. `/give` an item to a fake player
   to satisfy `inventory_changed`), assert the reward function ran, then assert it can fire a
   *second* time (proving the auto-revoke actually works) before the next assertion. Flag this
   explicitly rather than discover it mid-implementation: this feature's test architecture looks
   structurally different from every other PS milestone's, because the thing being tested (an
   event firing) has no target-independent semantics to differentially check against.

### Non-goals

- A general predicate/condition DSL, or multi-criterion `requirements` AND/OR composition — v1 is
  one criterion, one trigger kind, one reward function.
- Modeling every vanilla trigger type — grow the closed `Criterion` enum only as real programs need
  specific triggers.
- Player-visible advancement trees / un-hiding advancements — this is an event-hook mechanism, not
  an achievements feature.
- Anything to do with reading NBT — that's [`block-entity-nbt-paths.md`](block-entity-nbt-paths.md)
  entirely; this note is push, that note is pull.

## Sources

- [Advancement definition – Minecraft Wiki](https://minecraft.wiki/w/Advancement_definition) —
  JSON schema (`criteria`/`requirements`/`rewards`, hidden-advancement behavior when `display` is
  omitted).
- Search-confirmed community consensus on the `advancement revoke @s only <id>` re-arm pattern
  (`minecraftcommands.github.io/wiki`'s "activate a command once" guide and multiple independent
  datapack-technique writeups) — this is the standard idiom, not a novel proposal.
- [`references-design.md`](references-design.md) — the effect/invalidation-analysis machinery this
  note's reward-function bodies would need to interoperate with if they read/write references.
