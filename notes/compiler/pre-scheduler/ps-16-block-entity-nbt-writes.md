# PS-16 — Block-Entity NBT Writes (BE-2)

Status: **planned, not yet implemented.** Depends on BE-1 (block-entity NBT reads,
already landed) for the schema/segment groundwork this reuses in the write direction.

## Why this exists

A PS-15 reward function needs to rewrite a chest's contents to present the next
"screen" of a menu. `block-entity-nbt-paths.md`/BE-1 is explicitly read-only — its own
non-goals list "Block-entity writes" by name. This is real new scope, not leftover
BE-1 work, and it is **not** as symmetric to the read side as that framing first
suggests — see "Verify against current implementation" below, which found a structural
asymmetry between reads and writes that the naive "just mirror BE-1" plan misses.

## Scope (tentative — see open questions)

- A write path through an entity-NBT-shaped receiver, at minimum
  `mc.block(Chest, x, y, z).Items[slot].count = value` and probably the whole-slot form
  (`...Items[slot] = itemExpr`).
- Literal position, `Chest` only, mirroring BE-1 Slice 1's own narrowing — prove the
  mechanism once before generalizing to more block-entity kinds or runtime positions.
- Both literal and runtime slot index, since PS-17's capstone almost certainly needs to
  write to a runtime-computed slot (which "menu button" the reward function is
  reacting to is not always known at compile time).

## Verify against current implementation before starting

This section exists because two claims that sound obviously true from the read side
turned out, on inspection of the actual current source, to not transfer cleanly.
Confirm both before writing a plan doc:

1. **There is no general assignment-target/lvalue grammar to extend.**
   `frontend/check.rs:2731`'s `check_assignment` resolves `assignment.target` as a bare
   *name* (`self.spelling(assignment.target.span)` → `self.active_binding(name)`) — it
   is not a general expression-chain target. There is no existing "assign through a
   member/index chain" mechanism anywhere in the checker today. This means PS-16 is not
   "add a write arm to existing assignment handling" — it needs new checker support for
   recognizing an entity-NBT-path expression as a valid assignment target in the first
   place, closer in size to a small grammar addition than a mechanical generalization.
   Confirm this is still accurate (re-grep `check_assignment` and `AstAssignment`)
   before scoping a plan around it, since this note was written from one inspection
   pass, not exhaustive coverage of every assignment-adjacent code path.

2. **Reads and writes probably don't have symmetric failure semantics, and this needs
   real measurement, not assumption.** BE-1's own history is two real bugs from
   assuming NBT shape/behavior instead of measuring it (the wrong container-shape
   assumption, the macro substitution type-tag assumption) — both caught only by
   spinning up the pinned server and looking. The write side has its own version of
   this risk: a real chest's `Items` list only contains entries for **occupied** slots
   (confirmed during BE-1 — reading an empty slot fails soft to the default, which is
   consistent with the slot's compound simply not existing in the list, not existing
   with empty fields). A read's fail-soft contract handles "nothing matched" gracefully
   by keeping a default. A **write** to a slot that has no existing `Items` entry can't
   use the same `data modify ... Items[{Slot:Nb}]... set value ...` match-then-modify
   command BE-1's read side conceptually mirrors, because there is nothing for the
   match to find — Minecrans's NBT match-index syntax finds an existing element, it
   doesn't create one. Placing an item into a currently-empty slot almost certainly
   needs `item replace block <pos> container.<slot> with <item> <count>` instead, which
   is a structurally different command (and a different target-IR shape) from
   modifying a slot that's already occupied. This needs to be measured against the
   real pinned server — install a chest, target an empty slot, try both command shapes,
   see what actually happens — before assuming one write path covers both cases.
   Do this before writing any lowering code, the same discipline BE-1 itself used.

## Not yet determined

- Source syntax: does this reuse assignment-statement syntax once the checker accepts
  a path-chain target (`mc.block(...).Items[slot].count = v;`), or does it need a
  dedicated write statement/builtin (`mc.block(...).write(...)`)? This is a real
  language-design question, same category as PS-15's own deferred S-0xx decision — it
  should get one, not be decided implicitly by whatever's easiest to parse first.
- Whether "occupied vs. empty slot" needs to be a single write op that branches
  internally (checking occupancy first, then choosing `data modify` vs `item replace`),
  or whether it's cleaner as two distinct source-level operations the program author
  chooses between explicitly. Depends on what PS-17's actual menu program needs to
  express, which argues for deciding this once PS-17's shape is drafted, not before.
- Whether a whole-slot write (`Items[slot] = itemExpr`) is in scope for PS-16 at all,
  or whether PS-17 only ever needs the narrower `.count`/single-field write. Narrowing
  scope here is cheap insurance against building an unused general mechanism — decide
  by checking what PS-17's actual "next screen" transitions require.

## Structural ideas to carry forward

- **Reuse BE-1's receiver/segment sum types in the write direction rather than
  inventing parallel ones.** `EntityNbtReceiver`/`EntityNbtPathSegment`
  (`ir/core/entity_nbt.rs:27,46`) already model "which block, which path" generically;
  a write declaration should be able to reuse both unchanged and only add a new
  Core-level write operation, mirroring how BE-1 itself added `Block(...)` as one more
  `EntityNbtReceiver` arm instead of a parallel type.
- **"Verify against the real server before assuming" is not optional here** — call
  this out explicitly in whatever plan doc follows this one, since it's the exact
  discipline that caught BE-1's two real bugs and the write side has at least one
  known asymmetry (occupied vs. empty slot) already surfaced above without even
  starting implementation.
- Whatever fail-soft-equivalent contract is chosen for writes should be a **structural**
  decision (impossible to get wrong), not a documented convention — the same "auto-
  revoke, don't ask the programmer to remember it" principle PS-15 already commits to
  for the advancement side.

## Dependencies

BE-1 (already landed) for the schema/segment machinery. No PS-13/14/15 dependency in
principle — writes can be tested via the existing `ServerSandbox` pattern (install
pack, issue commands, `data get` to inspect the resulting NBT) without a live bot,
unlike PS-14/PS-15. Confirm this stays true once the exact write shape is settled;
if the chosen semantics ever need to observe a *player's* resulting state rather than
just the block's NBT, that would pull in a PS-13 dependency it doesn't have today.

## Testing

- Differential/structural lowering tests, mirroring BE-1's own
  `be1_block_entity_nbt_lowering.rs` style, for both the occupied-slot and empty-slot
  command shapes once "Verify against current implementation" above is resolved.
- Pinned-server tests via the existing (non-bot) `ServerSandbox` pattern.
