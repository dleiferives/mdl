# Spawn Animations (Tschipcraft, datapack) — Feature-Scoping Notes

Source: [Modrinth](https://modrinth.com/datapack/spawn-animations), project `zrzYrlm0`, version
`[DP] Release v1.11.5` (version id `kXr2QX8r`), loader `datapack`, `game_versions` includes
`26.2`. Archive: `spawnanimations-v1.11.5-mc1.17-26.2.9-datapack.zip`, sha1
`df5a918a606fe06520b1c201a17386a272967ebf` (matches the API's recorded hash), published
2026-06-24, downloaded 2026-07-23. Same publisher/house style as
[`dynamic-lights`](../dynamic-lights/NOTES.md) (same scoreboard-naming convention,
`tschipcraft:` shared namespace, same overlay/pack-format structure) — read directly, not from
the store page.

**Pack-format note:** identical shape to `dynamic-lights` — `pack_format: 15`,
`supported_formats: [7,107]`, six overlays (`overlay_33/35/39`, `overlay_pre_49/63/92`), none
reaching format 102+, so `26.2` (format 107) runs the base pack's **singular**-named directories
(`function/`, `predicate/`, `tags/function/`). `extracted/` here was trimmed to that tree before
reading anything (plural mirrors and all six overlays dropped, ~300 files → ~140), per the
now-updated convention in `../README.md`.

## How it's actually implemented

### Scheduling: both real mechanisms this project has now seen, used for different purposes

`data/minecraft/tags/function/tick.json` genuinely registers `spawnanimations:main` — this pack
**does** use the real `#minecraft:tick` tag (unlike `dynamic-lights`, which used pure
self-rescheduling for everything). `main.mcfunction` runs every tick unconditionally. Alongside
it, `loop.mcfunction` (slower background work — validation, format migration, cleanup) uses
self-rescheduling exactly like `dynamic-lights` did:

```
schedule function spawnanimations:loop 5t
```

and `install.mcfunction` shows the companion primitive neither prior note surfaced:

```
schedule clear spawnanimations:loop
schedule function spawnanimations:loop 1s
```

`schedule clear <resource>` cancels any already-pending scheduled call before re-scheduling —
necessary specifically to stop duplicate self-rescheduling chains from stacking up across a
datapack reload (without it, every `/reload` would add a second parallel `loop` chain running
forever). Both `#minecraft:tick`-tag-driven functions and `schedule function`/`schedule clear`
are confirmed real, necessary, and distinct primitives — MDL has zero support for either.

### Per-tick work budgeting via selector `limit`/`sort=random` — real evidence for Stage 9's own stated scope

`main.mcfunction` and `run_activation_batch.mcfunction` bound how many entities get touched per
tick everywhere, and sample randomly so coverage still happens over many ticks rather than never:

```
execute as @e[type=#spawnanimations:dig_up_animation,tag=!ts.sa.verify,...,limit=10,sort=arbitrary] run function .../prepare
execute if score $global ts.sa.count matches ..100 as @a[...,sort=random,limit=5] at @s run function .../run_activation_batch
execute if score $activation_dist ts.sa.settings matches 25..100 as @e[...,distance=..100,limit=10,sort=random] at @s run function .../calc_activation
```

This is a real, concrete instance of exactly what `roadmap.md` names as part of Stage 9's scope
("static work partitioning under soft per-tick budgets") — not a hypothetical concern invented
for the roadmap, an actual technique a shipped, popular datapack uses throughout.

### The animation itself: a scoreboard-timer state machine writing directly into the entity's own `Pos[1]`

`internal/animation/dig_up/core.mcfunction` runs once per tick per animating entity. A
scoreboard timer (`ts.sa.timer`, starting at `-300`, incremented by a per-entity speed score
each tick) drives a computed Y-offset, written straight into the entity's live position:

```
scoreboard players operation #temp ts.sa.e.y = @s ts.sa.timer
scoreboard players operation #temp ts.sa.e.y *= @s ts.sa.e.height
scoreboard players operation #temp ts.sa.e.y += $offset ts.sa.e.y
scoreboard players operation #temp ts.sa.e.y += @s ts.sa.e.y
execute store result entity @s Pos[1] double 0.01 run scoreboard players get #temp ts.sa.e.y
```

`execute store result entity <target> <path> double <scale> run <command>` is the vanilla
primitive here — storing a *score* (always an integer) into an arbitrary **live entity's own
NBT field** as a scaled double, not a container slot. This is a structurally different write
target than anything built so far: PS-16/BE-2 only ever wrote a `Chest` block entity's
`Items[slot]` (an `{id, count}`-shaped compound at a fixed schema location); this writes one
scalar field (`Pos[1]`) on an arbitrary selected entity, computed from ordinary scoreboard
arithmetic MDL already has. The arithmetic itself needs nothing new — only the write target does.

The same function also does plain whole-NBT-compound writes on the entity directly:
`data merge entity @s {Invulnerable:1b}` and `data merge entity @s {FallDistance:0f,fall_distance:0f}`
(save/restore a boolean and reset fall damage) — simpler than the `Pos` case (no arithmetic, a
literal value), but still a general-entity write, not a container write.

### Whole-equipment-slot save/restore, disguised inside a fake chest item

`internal/entity/ehs/save_armor.mcfunction` hides a mob's rendered armor/held items by replacing
its entire `equipment` compound in one shot — `data modify entity @s equipment set from storage
spawnanimations:temp equipment_build` — where `equipment_build` is a single fake `minecraft:chest`
item (in the normally-invisible "body" slot) whose own `minecraft:container` item component holds
the six real armor/hand items as nested container slots, each flagged with a custom-data marker
(`TsSaRemove`) so a stray `/give` or inspection doesn't mistake it for a real chest.
`restore_armor.mcfunction` reverses this exactly, pulling each slot back out of the disguised
container and writing the whole `equipment` compound back in one `data modify entity @s
equipment set from storage ...`. Two things worth separating: (1) a genuinely useful technique —
using an item's own `minecraft:container` component as scratch storage *attached to the entity
itself* (survives chunk unload/reload the way abstract `storage` NBT does, but colocated with the
specific entity rather than global), and (2) the underlying requirement — **writing an entity's
entire `equipment` compound at once**, not slot-by-slot, which is a different shape than anything
container-write-related built so far.

### `execute if block <relative position> matches <tag>` — confirmed a third time

Every particle/verification function in this pack tests blocks at frame-relative or
eyes-anchored-local positions against block **tags** (`#minecraft:sand`, `#spawnanimations:dirt`,
`#minecraft:logs`, `#spawnanimations:nonsolid`, ...), e.g. `execute if block ~ ~ ~
#minecraft:sand ...`, `if block ^ ^0.2 ^ #spawnanimations:nonsolid`. Combined with VeinMiner's
macro-templated single-id match and `dynamic-lights`'s `minecraft:location_check`-predicate
relative-offset checks, this is now the **single most recurring confirmed gap across all three
datapacks read so far** — a typed "does the block at this frame-relative position match this
id/tag" test, usable both as a bare command and inside a predicate tree.

### Predicates, again load-bearing (third confirmation)

`spawnanimations:exclude` (glowing/levitation/invisibility/riding/burning exclusion),
`spawnanimations:validate` (save-format integrity check), `spawnanimations:trigger` — same
pattern as `dynamic-lights`'s heavy predicate-tree use, reinforcing predicate-file evaluation as
high-priority, not incidental.

### Minor, likely-stays-`unsafe` items

`particle <resource> <pos> <spread> <count> <mode>` (cosmetic-only, no state, no return value —
plausibly never worth a typed builtin, matching this project's own precedent for `give` staying
`unsafe` rather than being promoted pre-emptively) and `playsound` (same reasoning).

## What this means for MDL — cumulative picture across all three datapacks so far

Ranked by how many of the three datapacks now confirm each gap:

1. **Frame-relative/local block-or-tag matching** (`execute if block ...`) — needed by
   **all three**. Highest-confidence, highest-priority addition of everything found so far.
2. **Predicate-tree evaluation** — needed by **two of three** (`dynamic-lights`,
   `spawn-animations`); VeinMiner only used it trivially. Still likely high-value given how
   central it is in the two packs that do lean on it.
3. **General entity-NBT writes beyond block containers** — new this pass. PS-16/BE-2 covers
   exactly one shape (`Chest.Items[slot] = {id, count}`); this pack needs (a) a single scalar
   field on an arbitrary live entity computed from a score (`Pos[1]`, via `store result entity
   ... double <scale>`), and (b) a whole-compound write (`equipment`, `Invulnerable`,
   `FallDistance`) — both structurally different from, not just a bigger version of, the
   container-slot write PS-16 built.
4. **Persistent scheduling (Stage 9)** — confirmed by **two of three**
   (`dynamic-lights`, `spawn-animations`), and this pass adds the missing half: a genuine
   `#minecraft:tick`-tag-driven "run every tick, no scheduling call at all" mode alongside
   `schedule function`/`schedule clear` self-rescheduling — MDL needs both entry points, not just
   one.
5. **Per-tick work-budget partitioning** (`limit=N`, `sort=random`) — real, concrete confirmation
   of a specific piece of Stage 9's already-stated scope, not a new gap by itself (MDL's
   `mc.entities(...)` selector builder would need `limit`/`sort` support, which is closer to "does
   the existing selector grammar already expose this" than "design a new subsystem").
6. **Native vanilla stat-scoreboard readback** — VeinMiner only; not seen again here.
7. **Loot tables / item modifiers** — seen in neither of the last two either; still not
   confirmed as a real requirement by any of the three so far.

## What doesn't need anything new

The animation math itself (timer increment, multiply-by-height, offset arithmetic) is ordinary
scalar arithmetic MDL already has in full — the entire gap is the *write target*
(`entity Pos[1]`), not the computation. Likewise the armor-hide/restore data shape is a plain
compound copy once whole-entity-equipment writes exist; nothing about it needs new arithmetic or
control flow.

## Open questions this note doesn't answer

- Whether "write a computed value into an arbitrary scalar field of the *current executor's own*
  entity" is meaningfully smaller in scope than "write into any selected entity's arbitrary NBT
  path generally" — this pack only ever writes `@s`'s own fields, never a different entity's. A
  first slice scoped to "the current executor writes its own fields" might be considerably
  smaller than full general entity-NBT writes, similar to how BE-1 scoped its first slice to
  `Chest`-only before generalizing.
- Whether the `double <scale>` conversion in `execute store result entity ... double 0.01 run
  scoreboard players get ...` is a special case MDL's existing Int32/scoreboard model can express
  cleanly, or needs its own explicit fixed-point/float story — not investigated here.
