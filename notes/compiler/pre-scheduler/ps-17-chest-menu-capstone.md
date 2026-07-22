# PS-17 — Capstone: `tests/programs/chest-menu`

Status: **planned, not yet implemented.** Depends on PS-13 through PS-16. No new
compiler capability — same role PS-3 (Brainfuck) played for PS-2: compose existing
capabilities into one real program, don't add to the compiler.

## Why this exists

This is the actual feature the whole PS-13–17 sequence was requested for: a player
opens a real placed chest, clicks an item, and the compiled program reacts —
teleporting them, giving them an item, or presenting the next "screen" of a menu by
rewriting the chest. Every piece it needs already has a home elsewhere in this
sequence; PS-17's only job is proving they compose, on the real pinned server, driven
by a real bot.

## What it composes

- BE-1 (already landed) — read the chest's current contents to know which "screen" is
  active and correlate *which* chest an `inventory_changed` event came from.
- PS-14's `Player` kind — the reward function's receiver.
- PS-15's advancement-triggered events — the actual click-detection mechanism
  (`minecraft:inventory_changed` fires when the clicked item lands in the player's
  inventory).
- PS-16's block-entity writes — rewrite the chest for the next screen.
- `TeleportCurrentExecutor` (already implemented, no new work) — advancement rewards
  run `as`/`at` the triggering player automatically, so this composes for free.
- PS-13's bot — drives the actual multi-step walkthrough the test asserts against.

## Not yet determined

- **`give` has no typed builtin today** — only `Say`/`TeleportCurrentExecutor`/
  `MoveCurrentExecutorBy` exist in `MinecraftSemanticKey`
  (`ir/semantic/minecraft.rs`). PS-17 needs *some* way to give the player an item as a
  menu outcome. Two options, not yet chosen between: (a) a small new typed
  `GiveItem`-shaped builtin, cheap given how mechanical `Say`/`Teleport` already are as
  templates; (b) `unsafe minecraft("give @s ...")`, which the checker already validates
  as raw command text (`check_unsafe_minecraft`, `check.rs:1515`) and other test
  programs already lean on for capability gaps. Decide based on whether "give an item"
  is likely to recur beyond this one capstone — if yes, it deserves the typed builtin;
  if this is the only place it's ever used, `unsafe` is proportionate and matches how
  this project has handled one-off capability gaps before (see PS-12E's own history of
  what stayed `unsafe` vs. what got promoted to a first-class construct).
- **How many "screens" the menu program needs.** At least two, or PS-16's write
  capability is never actually exercised by anything observable — a single-screen
  program can't distinguish "the write worked" from "the write was never attempted."
  The exact screen count/branching shape isn't decided; it should be the smallest
  structure that forces a real write-then-re-read round trip, not a large or elaborate
  "real" menu.
- **What each "button" item actually is.** `ItemMatch` (PS-15) is item-ID-only for
  v1 — so each distinct menu option in the same chest needs a distinct item type (or
  the design accepts only one clickable button per chest per screen, with different
  screens reusing the same item ID safely because only one screen's advancement is
  "live" at a time via the chest's rewritten contents). This needs to be worked out
  concretely once PS-15's `ItemMatch` shape is final, not guessed now.

## Testing

Full pinned-server bot walkthrough via PS-13: connect, click a button, assert the
observable outcome (teleport position, given item, or the chest's rewritten contents
for the next screen), click again on the new screen, assert the second outcome. This
is the first test in the whole compiler that exercises a real multi-step,
state-carrying interaction end to end rather than a single compiled-function
invocation — worth flagging to whoever implements it as a genuinely new test shape,
not a bigger version of an existing one.

## Structural ideas to carry forward

- Mirror PS-3's own discipline exactly: this milestone adds **zero** new Core ops,
  recipes, or checker special-cases. If implementing the test program surfaces a need
  for one, that's a sign a piece of PS-13 through PS-16's scope was mis-sized, not a
  reason to quietly add capability inside the capstone.
- Keep the program itself minimal on purpose — the smallest chest-menu shape that
  forces every dependency (a real click, a real event fire, a real read, a real write,
  a real second event fire proving the screen actually changed) to be exercised
  honestly, not a showcase program.

## Dependencies

PS-13, PS-14, PS-15, PS-16, in that order — this is the one milestone in the sequence
that cannot start early or in parallel with its dependencies, since it has no scope of
its own beyond composing them.
