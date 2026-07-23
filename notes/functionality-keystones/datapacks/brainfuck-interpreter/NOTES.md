# Brainfuck Interpreter (datapack) — Feature-Scoping Notes

Source: [Modrinth](https://modrinth.com/datapack/brainfuck-interpreter), project `mqQMbsxg`,
version `0.5` (version id `fmmfgmdl`, "First version"), loader `datapack`. Archive:
`brainfuck.zip`, sha1 `49367e69a914c2584e7f3570f6780691ffa1b9d9` (matches the API's recorded
hash), published 2026-04-29, downloaded 2026-07-23.

**This is materially weaker evidence than the other three datapacks in this folder, and that's
worth stating plainly rather than glossing over:** only one version was ever published, its
`game_versions` list is just `["1.21.11"]` (no `26.2` build exists — it predates 26.2 entirely),
its own `pack.mcmeta` declares `max_format: 94` (well below format 107, the format `26.2` and
every other datapack here actually uses), and it has 11 total downloads vs. thousands for
VeinMiner/Dynamic Lights/Spawn Animations. Nothing here was re-verified against a running 26.2
server. Treated as a single-author hobby build, not a maintained, load-bearing reference — still
worth reading because of *what* it implements, not because of how polished it is. The distributed
archive also contained several stray nested `.zip` files and one literal `"... copy"` duplicate
inside the `function/` tree — not valid Minecraft resources under any pack format, clearly
packaging mistakes rather than live content — dropped from `extracted/` for the same reason the
inapplicable overlay/plural trees are dropped elsewhere in this folder (still recoverable from the
top-level `brainfuck.zip` if ever relevant).

## Why this one is different from the other three

The other three datapacks are all continuous, ambient, per-tick *world effects* (a chest menu of
sorts, a light source, a spawn animation) — this one is a genuine **general-purpose interpreter**
for an arbitrary player-submitted program, implemented entirely in `.mcfunction`/NBT/scoreboard.
That makes it the closest real-world artifact yet to MDL's *own* existing PS-3 Brainfuck capstone
— not a source of new gaps so much as an independent, real-world comparison point for design
choices MDL's own roadmap already had to make.

## How it's actually implemented

### Input: a `minecraft:dialog`, not a written book or chat

`data/brainfuck/dialog/input.json` is a real vanilla `confirmation`-type dialog with a multiline
text field (`code`, up to `1145141919` chars(!)), a single-line text field (`input`, the program's
stdin), and a `number_range` slider (`array_length`, 20–1000 step 20) — a genuine in-game GUI
form. Its two buttons both submit via macro-templated commands:

```json
"no":  {"action": {"type": "dynamic/run_command", "template": "function brainfuck:save/ {input:\"$(input)\",code:\"$(code)\",array_length:$(array_length)}"}}
"yes": {"action": {"type": "run_command", "command": "function brainfuck:init/"}}
```

`dynamic/run_command`'s `template` field splices the player's *typed dialog text* directly into a
function-call argument compound — a rich-UI-driven analogue of MDL's macro/crossings engine's own
argument-frame bridging, but originating from a modern GUI widget MDL has zero concept of. This is
the one genuinely new, clean gap this pack surfaces: **no `dialog` support anywhere in MDL**
(confirmed: zero hits for "dialog" anywhere in `crates/mdl-compiler/src/`). MDL's own precedent so
far for getting arbitrary text into a running program is PS-2/PS-3's written book, not a live
form.

### Execution model: one opcode per tick via self-rescheduling — direct evidence for an already-open question in MDL's own roadmap

`init/.mcfunction` (the "run" button's target) sets up state and ends with:

```
schedule function brainfuck:compile/get_char/ 1t
```

Each subsequent step re-schedules itself again for 1 tick later — **the interpreter advances
exactly one (folded/optimized) opcode per tick**, not all at once in a single synchronous burst.
This is real, concrete evidence bearing directly on a tension MDL's own `roadmap.md` already
names and defers: *"Runtime fuel gives deterministic application termination but is not yet a
static Minecraft command bound; that distinction is retained in the Stage 9 input rather than
hidden."* MDL's own PS-3 Brainfuck capstone runs its bounded, fuel-limited program synchronously;
this independent real-world Brainfuck-in-a-datapack instead chose to spread execution across many
ticks specifically to stay under Minecraft's own per-tick command-count ceiling for
nontrivial programs — the same static-command-bound problem, solved the other way. Configurable
`#max_total_command_count`/`#max_single_command_count` scoreboards (default 30000/3000) plus a
dedicated `too_many_executions` error path are this pack's own hand-rolled version of exactly the
"program-length and execution-fuel limits" PS-2 already froze as a decision MDL's own Brainfuck
work had to make. Worth treating as real prior art when Stage 9 actually gets designed, not just
a hypothetical concern.

### A real preprocessing/optimization pass, running at runtime inside the datapack itself

Before interpreting, the pack folds runs of `+`/`-` and `<`/`>` into single accumulated
`add N`/`shift N` pseudo-ops (`preprocess/fold/add.mcfunction`, `fold/shift.mcfunction`) and
precomputes a `[`/`]` jump table via an explicit push/pop stack
(`preprocess/match/stack_push.mcfunction`, `stack_pop/set_jump_table.mcfunction`) *before* running
anything — genuine peephole optimization and ahead-of-execution bracket resolution, not a naive
re-scan-the-string interpreter. The crucial structural difference from MDL's own PS-3: MDL
compiles a *fixed* `.mdl` source program once, ahead of time, in the Rust compiler, producing a
static datapack. This pack's "compile" step runs *inside Minecraft itself, at runtime*, over
whatever text a player just typed into the dialog — it is its own interpreter-generator, not a
build-time compiler. **Replicating that meta-circular capability (compiling/interpreting
arbitrary player-submitted program text at runtime, with no separate build step) is arguably out
of scope for MDL's own architecture as an ahead-of-time compiler, not a gap to fill** — worth
being explicit about this rather than quietly treating it as a missing feature.

### Byte↔character conversion: a 96-line hand-unrolled dispatch table

`compile/convert/score_to_ascii.mcfunction` is a literal `execute if score ... matches N run data
modify ... set value {"text":"<char>"}` chain, one line per byte value 0–127, because vanilla has
no int-to-character primitive at all. This is exactly the class of problem MDL's own PS-2 "typed
books/items and holder access" / "byte input/output model" work already solved with a real typed
mechanism instead of a hand-written enumeration — **this validates that MDL's existing approach
is already better than what a hand-authored datapack has to resort to**, not a gap.

### Runtime-indexed list access via macro substitution — already covered by MDL's existing engine

```
$data modify storage brainfuck:re cur_char set from storage brainfuck:re code.list[$(ir_ip)]
```

A macro-substituted runtime index into a stored list — structurally identical to what PS-11's
generic macro/crossings engine and `NbtPathSegment::Index(Operand<i32>)` already provide inside
MDL. Another confirmation that existing MDL machinery already covers this, not a new requirement.

### Runtime-sized array initialization: a brute-force workaround MDL already avoids

Tape/array setup (`init/array/1.mcfunction` through `.../2048.mcfunction`) is a closed set of
power-of-2-sized literal arrays (`[0ub, 0ub, ..., 0ub]`, e.g. 1024 literal entries written out by
hand) that greedily subtract the largest chunk fitting the requested length and append it,
looping via `init/array/_.mcfunction` until the remaining length reaches zero — a real, crude
workaround for "initialize an NBT list of a runtime-chosen length" with no general repeat-N-times
primitive available at the raw-command level. MDL already has genuine loops and owned lists in
the source language (PS-2), so an MDL program with the same requirement would not need this
hack — another validating point, not a gap.

## What this means for MDL

Only one clean, new addition, and it's UI-shaped rather than execution-shaped:

1. **`minecraft:dialog` support** — reading structured player input (text fields, number
   ranges, buttons) through a real vanilla GUI form, with the submitted values reaching a
   function call the way this pack's `dynamic/run_command` template does. Nothing else in this
   note requires new compiler capability; every other mechanism shown here either has a direct
   MDL equivalent already (byte/char conversion, macro-indexed list access, bounded execution) or
   is closer to a non-goal (meta-circular runtime program interpretation) than a gap.

Independently useful as **prior art for Stage 9**: this pack's own "one opcode per tick via
self-rescheduling, with configurable total/per-invocation command budgets and a dedicated
over-budget error path" is a real, working answer (if a crude one) to the exact static-command-
bound problem `roadmap.md` already flags as deferred to Stage 9 — worth reading again once Stage
9 design actually starts, specifically for how it split "how much work fits in one tick" from
"how much work the whole program is allowed to do."

## Open questions this note doesn't answer

- Whether `minecraft:dialog` is common enough across real datapacks to justify typed MDL support,
  or a one-off worth leaving to a raw/unsafe JSON-authoring escape hatch — only one data point so
  far.
- Whether this pack's per-tick self-rescheduling execution granularity (one folded opcode per
  tick) was a deliberate design choice or a defensive reaction to a specific measured watchdog
  failure — the pack ships no design notes of its own to check against, unlike MDL's own PS-2/PS-3
  documentation trail.
