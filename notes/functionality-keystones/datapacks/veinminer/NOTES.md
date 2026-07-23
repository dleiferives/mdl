# VeinMiner (datapack) — Feature-Scoping Notes

Source: [Modrinth](https://modrinth.com/datapack/veinminer), project `OhduvhIc`, version
`Veinminer DP 1.3.5` (version id `4klb3qNE`), loader `datapack`, `game_versions: ["26.2"]` —
the same Minecraft target this compiler targets, fetched via the Modrinth API rather than
assuming the site's latest listed build matches. Archive: `veinminer-1.3.5.zip`, sha1
`9851168ef1a105bec831781331161ffeedbc42d4` (matches the API's recorded hash), downloaded
2026-07-23. Read directly (every `.mcfunction`/`.json` in the pack), not summarized from the
store page — the store page's own feature list turned out to be inaccurate for this specific
version in more than one place (see "Corrections" below).

## How it's actually implemented

**Detection (`data/minecraft/tags/function/tick.json` → `veinminer:internal/tick`, every
tick):** one scoreboard objective per *configured, watched* block, created with vanilla's own
per-block mined-stat criteria and a macro-templated name:

```
$scoreboard objectives add veinminer.b.$(namespace).$(id) minecraft.mined:$(namespace).$(id)
```

Every tick, for every registered (namespace, id) pair, it checks whether any player's score on
that exact objective is `1..` (`internal/check/_loopi.mcfunction`):

```
$execute if entity @s[scores={veinminer.b.$(namespace).$(id)=1..}] anchored eyes positioned ^ ^ ^1.5 \
    at @n[type=item,nbt={Age:0s},distance=..5.0] run function veinminer:internal/mine/check_aligning
```

A stat objective only tells you *that* a block of that type was mined, not *where* — so it
locates the break position via a heuristic: reposition to the nearest freshly-spawned item
entity (`Age:0s`, within 5 blocks in front of the player's eyes) and use *that* position as the
origin. This is a real, slightly fragile trick worth knowing about if MDL ever wants the same
capability more robustly.

**Flood-fill (`internal/mine/check_aligning.mcfunction` → `_loop`/`_loopi`/`try`/`mine`):** pure
relative-frame recursion, never an absolute/stored position:

```
execute positioned ~ ~ ~1 run function veinminer:internal/mine/try with storage veinminer:data temp1.current
execute positioned ~ ~ ~-1 run function veinminer:internal/mine/try with storage veinminer:data temp1.current
... (all 6 axis-aligned neighbors)
```

`try.mcfunction` tests the block at the *current* (already-repositioned) frame origin against a
runtime/macro-templated resource id, and only recurses further if it matches:

```
$execute if block ~ ~ ~ $(namespace):$(id) run function veinminer:internal/mine/mine
```

`mine.mcfunction` breaks it (`setblock ~ ~ ~ air destroy`) and recurses into
`check_aligning` again. **No explicit step/chain-count cap exists anywhere in this datapack
version** — termination relies entirely on running out of matching neighbors. (The Modrinth
page's "maxChain, default 100, `/veinminer settings maxChain`" claim belongs to the separate
mod/plugin builds, not this datapack — confirmed by its absence in every function file here.)

**Enchantments:** read directly off the executor's held item via a selector NBT predicate, not
a general "read enchantment level" primitive:

```
execute if entity @s[nbt={SelectedItem:{components:{"minecraft:enchantments":{"minecraft:fortune": 1}}}}] ...
```

**Fortune/silk-touch:** hand-rolled, not `/loot`. Fortune duplication is
`setblock ~ ~ ~ air destroy` then `$setblock ~ ~ ~ $(namespace):$(id)` gated behind a
`minecraft:random_chance` predicate (`predicate/fortune1.json`, chance `0.33333`, stacked up to
3 times for higher fortune levels — a crude but real approximation of drop-count scaling).
Silk touch kills the natural item/xp drop and `$summon`s a single custom item stack instead.
**No loot table or item modifier is used anywhere in this pack.**

**Config UI:** clickable `tellraw` JSON text components (`click_event: run_command` /
`suggest_command`, `hover_event: show_text`) that invoke ordinary `/scoreboard` and
`/function <ns>:<path> {arg: "value"}` calls. Not real custom slash-commands, and not a chat-text
parser — "chat-based configuration" (the store page's phrase) means "a menu rendered in chat,"
not "type a command in chat."

## What this means for MDL — corrected against the earlier (pre-inspection) guess

An earlier discussion in this session guessed MDL would need general first-class runtime block
*positions* (Tier C of `macro-reference-crossing-model.md`) to express a flood-fill. **That
guess was wrong, or at least overstated**, once the real implementation was read: the pack never
holds a position as data at all — it re-enters relative `execute positioned` frames recursively,
which is structurally close to what `run.at(...)`/`.positioned(...)` chains plus Stage 8's
existing bounded recursion already give MDL. This is the concrete value of reading the real
artifact instead of reasoning from a feature description: it can shrink a gap as easily as
reveal one.

What's genuinely still missing, grounded in the actual commands above, roughly ordered by how
self-contained each gap is:

1. **A typed "if block `<frame-relative-or-literal position>` matches `<id>`" test**, where the
   block id can be a runtime/macro value (a `Subst` on a `ResourceId` `SyntaxSlot`, in
   `macro-reference-crossing-model.md`'s own vocabulary — not a new *kind* of machinery, a new
   typed op built on an encoding the architecture already names). No equivalent exists anywhere
   in MDL today (confirmed: no `execute if block`-shaped construct in the frontend at all).
2. **Native vanilla stat-scoreboard readback**, including a runtime/macro-constructed *objective
   name* (`veinminer.b.$(namespace).$(id)`) — not just a runtime score value. MDL's scoreboard
   machinery today is for its own compiler-managed objectives only.
3. **Block mutation at the current frame** (`setblock ~ ~ ~ air destroy`, `setblock ~ ~ ~ <id>`)
   — structurally simple once (1) exists, no runtime-position problem since it's always the
   current relative frame, same pattern PS-16's whole-slot write already established for
   container slots.
4. **Entity summon/kill with literal-ish NBT** (`summon item ~ ~ ~ {...}`, `kill
   @n[type=item,...]`) — no typed equivalent exists; would likely start as a well-scoped typed op
   rather than needing full raw-command generality, mirroring how `give` stayed `unsafe` in PS-17
   rather than becoming a typed builtin prematurely.
5. **Predicate evaluation** (`if predicate <ns:path>`) — no first-class predicate-reference
   concept exists in MDL yet.
6. **Held-item enchantment reads** — narrower than it first looks: extending PS-14's equipment
   schema to expose `mainhand` (currently explicitly deferred and non-resolving,
   `entity_schema.rs:366-367`) plus enchantment-component fields would cover it, following the
   exact same schema-row-extension pattern already used for armor slots. Not a new subsystem.
7. **Persistent per-tick scheduling (Stage 9)** — confirmed necessary, not avoidable: detection
   is fundamentally "poll every player's stat scores every tick, forever." This is the one piece
   of the original analysis that gets *more* certain, not less, after reading the real pack.
8. **Loot tables / item modifiers / general mainhand durability writes** — **not actually needed
   for this specific datapack.** Drop this from "required for VeinMiner" (it may still matter for
   a different real datapack later — that's exactly what cataloguing more of them is for).

## Open questions this note doesn't answer

- Whether MDL's existing `mc.entities(...)` selector builder supports the NBT-predicate +
  distance + "nearest of type" filtering the item-drop-position heuristic needs
  (`@n[type=item,nbt={Age:0s},distance=..5.0]`) — not verified against the current checker in
  this pass; check before assuming either way.
- Whether relative `execute positioned` reframing composed *recursively* across many nested
  function calls (as this pack's flood-fill does) is something MDL's current `run.at`/`.positioned`
  modifier + Stage 8 recursion combination can already express structurally, or whether nesting
  frame modifiers inside a recursive call has never actually been exercised by any existing test.
