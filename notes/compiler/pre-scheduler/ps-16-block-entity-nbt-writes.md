# PS-16 — Block-Entity NBT Writes (BE-2)

Status: **planned, researched to an implementation handoff (2026-07-22).** Depends on
BE-1 (block-entity NBT reads, landed) for the schema/segment groundwork this reuses.
Does **not** depend on PS-13/14/15 — writes can be tested via the existing
`crates/mdl-test` `ServerSandbox` pattern (console commands + `data get`), no live bot
needed, unlike PS-14/15. This document is written for a fresh agent with no memory of
prior sessions.

## Why this exists

A PS-15 reward function needs to rewrite a chest's contents to present the next
"screen" of a menu. `block-entity-nbt-paths.md`/BE-1 is explicitly read-only. This is
real new scope, and — confirmed by direct measurement against the real pinned server,
not assumed — **genuinely more dangerous than a naive "mirror the read side" plan
would produce.** Read this whole document before writing any code; it overturns part
of the original plan with a real, surprising, measured finding.

## The critical measured finding: `data modify` on an unoccupied slot is not safe

The original plan assumed a write to an empty (never-populated) chest slot would
either cleanly no-op or cleanly error, the same fail-soft-or-fail-loud binary every
other part of this compiler's NBT story exhibits. **Measured directly against the real
pinned server — it does neither.**

Setup: a chest at `0 4 0` with only slot 3 occupied (`item replace block 0 4 0
container.3 with minecraft:diamond 5`). Ran `data modify block 0 4 0 Items[{Slot:7b}]
.count set value 10` against **slot 7, which had never been populated**:

```
[Server thread/INFO]: Modified block data of 0, 4, 0
```

— reports **success**. But immediately after (on the very next `data get`):

```
[Server thread/WARN]: [...BlockDataAccessor] Serialization errors:
minecraft:chest // ...ChestBlockEntity@BlockPos{x=0, y=4, z=0}: Failed to decode
value '{Slot:7b,count:10}' from field 'Items' at index 1': No key id in
MapLike[{Slot:7b,count:10}]
```

What actually happened: the match-then-modify command, finding no existing element to
match, **synthesized a new list entry** containing only the matched key and the
modified field (`{Slot:7b, count:10}`) — missing the mandatory `id` field every real
item-stack entry needs. That malformed entry gets rejected at the next deserialization
pass (hence the warning) and doesn't persist (confirmed: a follow-up `data get`
showed only the original diamond entry, slot 7 still absent) — so there's no lasting
state corruption, but **a real, alarming warning gets written to the server's log for
what the MDL program considers ordinary, successful, expected behavior.** A generated
datapack doing this in a real deployment would spam server operators with spurious
"Serialization errors" warnings.

Confirmed clean by contrast: `item replace block 0 4 0 container.7 with
minecraft:emerald 3` on the same empty slot 7 populates it with no warning at all, and
once a slot is populated this way, `data modify ... Items[{Slot:7b}].count set value
20` on it afterward is then also clean (confirmed) — the danger is specifically
`data modify`'s match-then-modify shape targeting a slot with no existing entry, not
`data modify` in general.

**Conclusion, and the scope recommendation that follows from it:** don't build a
`.count = value`-shaped partial-field write as the primary primitive. Build a
**whole-slot write**, lowered to `item replace block <pos> container.<slot> with
<item> <count>` unconditionally — this command is safe and warning-free for *both* an
occupied and an unoccupied slot (confirmed above), so it needs no occupancy check at
all, which is a real simplification, not just a safety fix. This also better matches
what PS-17 actually needs: "present the next screen" is about placing whole items
into slots, not incrementing an existing item's count. Recommend deferring
`.count`-only (or any other single-field) partial writes entirely — they're the
genuinely harder, occupancy-sensitive case this measurement just proved, and nothing
in PS-17's own plan asks for them.

## Source syntax — a concrete recommendation, not settled

`frontend/check.rs:2981`'s `check_assignment` (re-verify this line number, it has
already drifted once from an earlier draft of this document due to PS-14/PS-15's own
edits elsewhere in the file) resolves `assignment.target` as a bare *name* — `let name
= self.spelling(assignment.target.span)?; let target = self.active_binding(name.as_ref());`
— not a general expression. Confirmed still true as of this research pass. There is no
existing "assign through a member/index chain" mechanism anywhere in the checker.

**Recommendation: widen `AstAssignment.target` from a bare name to a general
`AstExpression`, and reuse the ordinary `=` assignment statement** — don't invent a
dedicated `.write(...)` builtin/method call. Two concrete, already-existing pieces of
machinery make this cheaper than it sounds, and are why this is a real recommendation
and not just a hopeful sketch:

1. **Recognizing the target as an entity-NBT-path receiver is already solved.**
   `check.rs` already has `expression_roots_in_nbt_path_receiver`-style logic (from
   BE-1/PS-12's own read-path checking) that recognizes an expression chain rooted at
   `mc.block(...)`/`mc.entities(...)`. `check_assignment` needs to try that same
   recognition on `assignment.target` before falling back to today's bare-name path —
   if it matches, build a write declaration instead of resolving a local binding.
2. **Expected-type-driven inference for the right-hand side already exists and
   already flows through assignment.** `check_assignment` already computes an
   `expected` type from the target and calls `self.check_expression_expected
   (&assignment.value, assigned, expected)` (confirmed at the current
   `check_assignment` body) — and PS-5's anonymous struct literals
   (`HirExpressionKind::AnonymousStructConstruct`, `check_expression_expected`
   around line 4155-4286) are *already* expected-type-driven the same way ordinary
   `.{}` literals are for a declared variable type. This means the natural
   right-hand-side shape for a whole-slot write —
   ```mdl
   mc.block(Chest, x, y, z).Items[slot] = .{ id: "minecraft:diamond", count: 5 };
   ```
   — should need **no new inference logic**, only teaching the checker to compute an
   `{id: String, count: Int32}`-shaped expected type for an `Items[slot]` write
   target, then handing it to the same `check_expression_expected` call
   `check_assignment` already makes. Verify this actually works end to end before
   trusting it fully (write the smallest possible test first) — this is a strong,
   well-grounded hypothesis from reading the code, not something run against the
   compiler yet.

If this turns out not to hold together cleanly once tried, that's better learned by
attempting the smallest version of it first (a structural test with no server
involved) than by committing to a full plan around an unverified hypothesis — same
discipline as everywhere else in this codebase.

## Scope

- Whole-slot writes only: `mc.block(Chest, x, y, z).Items[slot] = .{id: ..., count:
  ...}`. No `.count`-only partial-field writes (deferred, see above).
- Literal position, `Chest` only, mirroring BE-1 Slice 1's own narrowing.
- Both literal and runtime slot index — PS-17 will need a runtime-computed slot (which
  "menu button" fired isn't always known at compile time), and BE-1's read side
  already proved the runtime-match-index mechanism works
  (`ir/core/entity_nbt.rs:27-37`'s `EntityNbtPathSegment::Match { match_key, value:
  Operand<i32> }` already carries a full `Operand`, not just a constant — reuse this
  type directly on the write side rather than a parallel one).

## Structural ideas to carry forward

- **Reuse BE-1's receiver/segment sum types in the write direction rather than
  inventing parallel ones** — confirmed still exactly `EntityNbtReceiver { Entity
  (EntityKind), Block(BlockEntityKind, BlockPosition) }` / `EntityNbtPathSegment {
  Key, Index, Match }` at `ir/core/entity_nbt.rs:27-49` (re-verify line numbers before
  trusting them). A write declaration should reuse both unchanged and add a new
  Core-level write operation alongside the existing `EntityNbtReadDecl`, not a
  parallel receiver/segment model.
- Whatever the write's own contract is (does a write to a slot always "succeed" from
  the source program's point of view, mirroring reads' fail-soft default?) should be a
  **structural** decision baked into the emitted command shape, not a documented
  convention — the same "auto-revoke, don't ask the programmer to remember it"
  principle PS-15 already committed to. Given the write lowers unconditionally to
  `item replace`, which always succeeds regardless of prior slot occupancy, this may
  already be free — confirm rather than assume.

## Testing

- Differential/structural lowering tests, mirroring BE-1's own
  `be1_block_entity_nbt_lowering.rs` style: compile a whole-slot write and assert the
  emitted command is `item replace block <pos> container.<slot> with <item> <count>`
  for both a literal and a runtime slot index.
- Pinned-server tests via the existing (non-bot) `ServerSandbox` pattern
  (`crates/mdl-test`, ordinary stable-toolchain `cargo test`, no PS-13 bot needed):
  install a pack, run a compiled write against both an occupied and an unoccupied
  slot, `data get` to confirm the result, and — importantly, given the finding above —
  **assert the server log contains no "Serialization errors" warning** after an
  unoccupied-slot write. That assertion is this milestone's proof that the whole-slot-
  write recommendation actually avoids the danger this document measured, not just a
  nice-to-have check.

## Deferred, explicitly

- `.count`-only (or any other single-field) partial writes — the genuinely harder,
  occupancy-sensitive case. Revisit only if a concrete future milestone needs it.
- Block-entity kinds beyond `Chest`, runtime block positions — same deferrals BE-1
  itself already carries.
