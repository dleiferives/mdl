# Dynamic Lights (Tschipcraft, datapack) — Feature-Scoping Notes

Source: [Modrinth](https://modrinth.com/datapack/dynamic-lights), project `7YjclEGc`, version
`[DP] Release v1.9.3` (version id `3SxZhstR`), loader `datapack`, `game_versions` includes
`26.2` — the same Minecraft target this compiler targets. Archive:
`dynamiclights-v1.9.3-mc1.17-26.2.9-datapack.zip`, sha1 `72044b84f1b7bd3d38d3cd6c79a47c742670b35b`
(matches the API's recorded hash), published 2026-06-24, downloaded 2026-07-23. Read directly
(function/predicate/tag files, not the store page) — this pack is far larger and more
sophisticated than VeinMiner (647 files vs. ~40), and its own changelog is real live evidence of
its own bug history (waterlogging/bubble-column/glowing-effect fixes), not marketing copy.

**Pack-format note, relevant to which files are "live" for 26.2:** `pack.mcmeta` declares
`pack_format: 15`, `supported_formats: [7,107]`, with per-range overlays (`overlay_16`,
`overlay_33`, `overlay_35`, `overlay_disable_trim`, `overlay_pre_62`, `overlay_pre_102`). None of
the overlays covers format 102+, so for `26.2` (format 107, same as VeinMiner's) the **base,
non-overlay tree is what's active** — and specifically its **singular**-named directories
(`function/`, `predicate/`, `tags/function/`), not the plural mirrors (`functions/`,
`predicates/`, `tags/functions/`) also present in the base pack for older-format compatibility
(Mojang renamed these directories plural→singular at some format boundary). All notes below read
the singular tree only. `extracted/` in this folder is trimmed accordingly — the plural mirrors
and all six `overlay_*` trees are dropped (none apply to format 107), keeping only what actually
resolves for `26.2`; the full original is still the downloaded `.zip` alongside it.

## How it's actually implemented

### Persistence: `schedule function`, not a `#minecraft:tick` tag

`data/minecraft/tags/function/load.json` only registers `dynamiclights:install_trigger` — there
is **no `tick.json` at all**. Instead, `install.mcfunction` kicks off two independent
self-rescheduling chains:

```
schedule function dynamiclights:internal/main 5t
schedule function dynamiclights:internal/loop 10s
```

`internal/main.mcfunction` re-schedules *itself* every tick as its first line
(`schedule function dynamiclights:internal/main 1t`) before doing any work — a perpetual chain,
not a tag-driven repeating function. `internal/loop.mcfunction` is a slower (4s) watchdog whose
only job is re-enabling the trigger-type objectives (`ts.dl.toggle`, `tschipcraft.menu`) for
players who joined since the last pass — a real, separate concern from the main per-tick logic.
**This is a materially simpler persistence pattern than full continuation capture**: there is no
"resume mid-function" — every invocation runs top-to-bottom and terminates; all cross-tick state
lives in ordinary scoreboards/entity tags, and the "continuation" is just "call this same
function again after N ticks." Worth carrying into Stage 9 planning as a smaller, more tractable
first slice than general suspend/resume: a source-level "repeat this function every N ticks"
construct with no live-value materialization needed at all, since nothing survives *within* a
single invocation across the yield point — only externally-visible state does.

### Per-source-entity light placement: a marker entity is the light's "handle"

For every entity that should currently emit light, dispatch goes
`internal/main_exec(_pass)` → `internal/sources/core` → `internal/sources/entity` →
`predicate dynamiclights:entity/should_emit_light_level/<N>` → `api/place_light/<N>`, for each of
the five light levels (3, 6, 9, 12, 15) as a fixed, closed set — the same "5 near-identical
templated variants, differing only in a numeric constant" shape VeinMiner's tool categories used,
and instantly recognizable as a place a *generic* MDL construct (parameterized by an `Int32`
light level) would collapse 5 duplicated function trees into one.

The actual placement (`internal/place_light/<N>/summon.mcfunction`,
`prev_it/*.mcfunction`, `find_place/*.mcfunction`) is a real, reusable pattern:

1. **Check for an existing light at this exact position** (`execute align xyz if block ~ ~ ~
   minecraft:light run function .../prev_it/find`) — `align xyz` snaps the execution position to
   the block grid first.
2. **`prev_it/find`** looks for a `minecraft:marker` entity tagged `ts.dl.light` within `0.1`
   blocks (i.e., "the marker that owns this exact light block") and, if found, **renews it**
   (`prev_it/check.mcfunction`: bump its stored light level if lower, then `tag @s remove
   ts.dl.remove`).
3. **If no marker owns this position**, `summon_new.mcfunction` summons a fresh
   `minecraft:marker` (invisible, no collision — the standard "pure data handle" entity) via
   `execute align xyz summon minecraft:marker run function .../summon_exec`, which stores the
   light level on it (`scoreboard players set @s ts.dl.l.level <N>`) and places the actual block:

   ```
   fill ~ ~ ~ ~ ~ ~ minecraft:light[waterlogged=true,level=15] replace minecraft:water[level=0] strict
   execute if block ~ ~ ~ minecraft:cave_air run tag @s add ts.dl.cave_air
   execute unless block ~ ~ ~ minecraft:light run fill ~ ~ ~ ~ ~ ~ minecraft:light[waterlogged=false,level=15] replace #dynamiclights:air strict
   ```

   Two block-state variants of the same block (`waterlogged=true`/`false`) depending on what was
   replaced, and a remembered fact (`ts.dl.cave_air` tag on the marker) about what to restore
   later — the restore isn't "put back air," it's "put back *specifically what was here*."
4. **Removal** (`internal/remove_light.mcfunction`, run for every marker still tagged
   `ts.dl.remove` at the end of the tick): restores water/cave_air/plain air depending on what the
   marker remembered, then **teleports the marker to y=320 before killing it** — a deliberate
   trick to avoid the kill itself triggering a game-event vibration (sculk sensors etc.) at the
   light's real position. Every tick starts by tagging *every* current light marker
   `ts.dl.remove`; anything re-validated this tick (step 2/3 above) gets that tag stripped; the
   sweep at the end of the tick only touches what's left untouched — a clean
   "mark-all, unmark-survivors, sweep-the-rest" liveness pattern implemented entirely with entity
   tags.
5. **Multi-layer neighbor fallback** (`find_place/layer_0.mcfunction` → `layer_1` → `layer_2`,
   not read in full here but the pattern is explicit in `layer_0`'s own comment): if the current
   position fails `dynamiclights:world/place_light/valid_pos`, try 1 above, then the current-Y
   ring around the source, then 1 below, then 2 above, etc. — a small fixed search order over
   relative offsets, not a general flood-fill (no recursion needed here, just a fixed sequence of
   `execute positioned ~ ~<dy> ~ ...` attempts).

### Predicates carry real load-bearing logic, not just simple booleans

`valid_pos.json` is a `minecraft:location_check`-based predicate testing fluids/blocks at
**relative offsets** (`offsetX`/`offsetY`/`offsetZ`) against block tags
(`#dynamiclights:avoid`), composed with `minecraft:inverted` and `minecraft:reference` (predicate
files calling other predicate files by resource id) — a full boolean expression tree evaluated by
one `execute if predicate <ns:path>` command, not a chain of separate `execute if block`
commands. `should_emit_light_level/15.json` composes *six* different sub-predicates (mainhand,
offhand, head equipment; container contents; on-fire state; an entity-type tag; a scoreboard
value; two water-state references) into one inverted-OR tree. This is a far heavier, more central
reliance on the predicate system than VeinMiner showed (which used three trivial
`random_chance` predicates) — predicate-tree evaluation should be considered a first-class,
commonly-needed capability, not a minor one.

### Detecting "is this item a light source" uses a real vanilla predicate condition

```json
{
  "condition": "minecraft:entity_properties",
  "entity": "this",
  "predicate": {
    "equipment": { "mainhand": { "items": "#dynamiclights:light_level/15", "count": { "min": 1 } } }
  }
}
```

`equipment.mainhand.items: <item tag>` is a **built-in vanilla predicate field**, not something
built from commands — matching an equipped item against an item tag directly. Item tags per
light level (`tags/item/light_level/<N>.json`) are exactly the closed per-category list pattern
VeinMiner's `blocks.pickaxe`/`tools.pickaxe` storage lists approximated by hand — except here it's
real vanilla data (an item tag), not compiler/datapack-authored state.

### A genuinely clever adapter for entities whose held item isn't in `equipment`

`falling_block`/`block_display`/`ominous_item_spawner` entities store their "held" block/item
under a different NBT shape than the `equipment.mainhand` every other predicate above assumes.
Rather than write parallel predicate logic for them, `internal/sources/parse/main.mcfunction`
**summons a fixed-UUID helper `armor_stand`** (`UUID:[I;-1030365714,...]`, invisible/marker-like
via `Marker:1b,Invisible:1b,...`, but an armor stand specifically because it supports
`equipment`, unlike a marker), copies the odd entity's held-block id into that helper's
`equipment.mainhand.id` (`data modify entity <fixed-uuid> equipment.mainhand.id set from entity
@s BlockState.Name`), runs the *same* item-detection logic **as that helper entity**
(`execute as <fixed-uuid> run function .../parse/main_exec`), then copies the resulting scores
back onto the real entity (`scoreboard players operation @s ts.dl.i.type = .global ts.dl.i.type`,
etc.). A fixed, hardcoded UUID lets the adapter be addressed directly without a selector each
time. This is real, concrete evidence for "reuse logic built for one entity-NBT shape against an
entity with an incompatible shape, by proxying through a real helper entity" as an actual pattern
programs want — not a hypothetical.

## What this means for MDL

Ordered roughly by how load-bearing each requirement is to *this specific* datapack:

1. **Persistent scheduling — confirmed necessary, and confirmed simpler than expected.** Needs
   Stage 9, but the concrete pattern here (`schedule function` self-rescheduling, all state
   external) is a materially smaller slice than full mid-function suspend/resume — worth scoping
   as an earlier, separate Stage 9 sub-capability rather than waiting for the general case.
2. **Predicate-file evaluation (`execute if predicate <ns:path>`), as a full boolean tree
   (`minecraft:inverted`, `minecraft:reference`, `minecraft:location_check` with relative
   offsets, `minecraft:entity_properties` with `equipment.<slot>.items`/`count`).** Flagged as
   missing for VeinMiner too, but this pack shows it's load-bearing and heavily composed, not an
   edge case — likely the single highest-leverage new addition if these two datapacks are
   representative.
3. **A typed "place/replace block at the current frame, conditioned on what's already there"
   operation** (`fill ... replace <specific-prior-block> strict`) — an extension of the
   whole-slot container-write pattern PS-16 already established, but for ordinary world blocks
   instead of a container slot, and needing the *replace-with-match* form, not just an
   unconditional write.
4. **Marker/helper entities as data handles** — summon an invisible, no-collision entity, tag it,
   carry scalar state on it via scoreboard, and later select it by proximity
   (`distance=..0.1,limit=1`) or by a fixed identity (a hardcoded UUID). MDL's entity model today
   has no summon/tag-mutation/scoreboard-on-arbitrary-entity story at all — this is a distinct
   gap from PS-14's player-equipment-read work.
5. **Vanilla item/entity/block tags as MDL-visible values** — `equipment.mainhand.items:
   "#dynamiclights:light_level/15"` matches against a *tag*, and the whole datapack is organized
   around per-category tags (`tags/item/light_level/<N>`, `tags/entity_type/...`,
   `tags/block/...`). No equivalent of "does this item/entity/block match tag T" exists as a
   typed MDL construct.
6. **Relative-offset block/fluid checks** (`minecraft:location_check` with `offsetX/Y/Z`) reads
   as the JSON-predicate-shaped sibling of the "if block at a relative position" gap already
   identified from VeinMiner — same underlying need (test a block/fluid at a frame-relative
   position without holding an absolute position value), expressed here as data instead of a
   command chain.

## What doesn't need anything new

The multi-layer fallback search (`find_place/layer_0/1/2`) is a **fixed, small, compile-time
sequence** of relative-offset attempts, not a flood-fill or recursion — nothing about it needs
runtime positions or unbounded recursion the way VeinMiner's vein-walk did. If MDL had (2) and
(3) above, this specific search pattern would already be expressible as ordinary sequential
frame-relative statements.

## Open questions this note doesn't answer

- Whether a `schedule function`-shaped "repeat every N ticks, no in-flight state" construct is
  small enough to land *before* full Stage 9, or whether the two are entangled more than this
  reading suggests — worth a dedicated look once Stage 9 scoping actually starts.
- Whether vanilla predicate JSON should become a first-class MDL-authored artifact (source syntax
  that compiles to a predicate file) or stay reachable only via an escape hatch that references a
  hand-authored predicate JSON shipped alongside the MDL source — not decided by this note.
- The mod-support integration (`sources/mod_support/curios/*`) reads real per-mod NBT paths
  (Forge/NeoForge `Curios` inventories) — out of scope for MDL entirely (MDL targets vanilla
  Java Edition only), noted here only so a future reader doesn't mistake it for a vanilla gap.
