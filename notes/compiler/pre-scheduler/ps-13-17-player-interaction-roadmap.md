# PS-13 through PS-17 — Player Interaction and the Chest-Menu Capstone

Status: **PS-13 complete; PS-14 fully researched and ready to implement (its own
document is a direct implementation handoff, not just a scope sketch); PS-15 through
PS-17 planned at the milestone level.** Each
milestone has its own document; this file is the index and the shared dependency
rationale, mirroring how `roadmap.md` itself indexes PS-1 through PS-5 rather than
containing their full detail inline.

## Why this sequence exists

The target capability is a chest-based interactive menu: a player opens a real placed
chest, clicks an item, and the compiled program reacts — teleport them, give them an
item, or rewrite the chest to present the next "screen." Each thing that capability
needs is its own PS milestone, not one combined pass, matching this project's existing
discipline (PS-1 through PS-5 are each one provable vertical slice; PS-3 only composed
PS-2's capabilities, it didn't add new ones). [`block-entity-nbt-paths.md`](../block-entity-nbt-paths.md)
(chest reads, implemented as BE-1) and [`advancement-triggers.md`](../advancement-triggers.md)
(the push/event model) are the two architecture notes this sequence executes.

## The connected-player blocker

PS-14 and PS-15 both need a real `minecraft:player` entity to test against — vanilla
has no way to summon or fake one, and this project has twice already, deliberately,
deferred solving that (`ps-1-handoff.md`, `ps-2-0-decisions.md` — quoted in full in
[PS-13's document](ps-13-bot-driven-test-infrastructure.md)). That boundary is reached
now; PS-13 crosses it.

## The documents

| Milestone | Scope | Document |
|---|---|---|
| PS-13 ✅ | Azalea-backed bot test infrastructure, `crates/mdl-test-bot/` — no compiler changes | [ps-13-bot-driven-test-infrastructure.md](ps-13-bot-driven-test-infrastructure.md) |
| PS-14 🔬 | `EntityKind::Player` — researched, ready to implement | [ps-14-player-entity-kind.md](ps-14-player-entity-kind.md) |
| PS-15 | Advancement-triggered events (the push model) | [ps-15-advancement-triggered-events.md](ps-15-advancement-triggered-events.md) |
| PS-16 | Block-entity NBT writes (BE-2) | [ps-16-block-entity-nbt-writes.md](ps-16-block-entity-nbt-writes.md) |
| PS-17 | Capstone: `tests/programs/chest-menu`, composition only | [ps-17-chest-menu-capstone.md](ps-17-chest-menu-capstone.md) |

Dependency order is strict and linear — each milestone depends on every one before it,
with the partial exception of PS-16 (depends only on BE-1, not PS-13/14/15; see its own
document for the caveat on that). None of PS-14 through PS-17 should start before the
milestone(s) they depend on have reached their own applicable gate, the same discipline
`roadmap.md` states for PS-2's slices: "a slice reaches its applicable server or
evaluator gate before the next dependent slice treats it as established."

## Cross-cutting things worth knowing before starting any of them

- **The bot-library research (Mineflayer ruled out as non-functional on 26.2, HeadlessMC
  considered and passed over, Azalea selected) lives in PS-13's document**, not here —
  read it there if the choice ever needs revisiting.
- **Every one of these five documents has its own "Verify against current
  implementation" and "Not yet determined" sections.** They exist because this note's
  claims were checked against the actual current source once, at write time
  (2026-07-22) — not because they're guaranteed still true whenever implementation
  starts. Re-check before building on any specific file/line reference.
- The recurring structural discipline worth carrying into all five: reuse existing
  closed-sum/table-row extension patterns rather than inventing parallel machinery
  (PS-14's `EntityKind`, PS-16's `EntityNbtReceiver`), and never assume a real
  Minecraft NBT/command shape from documentation or precedent when the pinned server
  can be asked directly — BE-1 shipped two real bugs from exactly that shortcut, and
  PS-16 already has a known instance of the same risk before implementation even
  starts (see its document's "Verify against current implementation" section).

## Open, not yet decided, and not owned by any single one of the five documents

- `give` has no typed builtin today (only `Say`/`TeleportCurrentExecutor`/
  `MoveCurrentExecutorBy` exist). Scoped to PS-17's document as a decision to make
  there, since it's the first and so far only milestone that needs it.
- Whether PS-13's bot capability belongs in `mdl-test` directly or a new sibling crate
  is scoped to PS-13's document as an implementation-time call.
