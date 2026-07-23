# Stage 9.0 — Contracts and Pinned-Server Evidence

Status: **measured on the pinned Java 26.2 server (2026-07-23); harness primitive
implemented; 17 of 18 planned measurements resolved, one (M16) deferred, one (M15)
invalidated by a flawed test method and left genuinely open**

## Problem and non-goals

Every later Stage 9 tranche depends on facts about vanilla mechanisms the compiler
has never touched: `schedule function <id> <time> [append|replace]`,
`schedule clear <id>`, and `#minecraft:tick` registration. 9.0 has two jobs:

1. measure those mechanisms directly against the pinned Java 26.2 server, closing
   the specific gaps left open by the project's own prior research
   ([`../../mcfunction/command-limits-and-multi-tick.md`](../../mcfunction/command-limits-and-multi-tick.md),
   measured 2026-07-11 — see below), and freeze or record uncertainty for each of
   the six semantic decisions in [`../stage-9-plan.md`](../stage-9-plan.md#frozen-semantic-decisions);
2. give the harness a deterministic way to advance ticks and observe state across
   them, since every later tranche's pinned-server gate needs this and none of it
   exists today.

9.0 changes no compiler behavior and adds no language surface. Explicit non-goals
(deferred to the tranche named):

- source syntax for `Ticks`/`schedule`/`yield`, HIR/Core representation for a
  deferred entry, or any lowering — 9B/9C;
- promoting the existing bound analysis to a hard error — 9A;
- a runtime job queue, dynamic job creation, non-constant delays, or any new
  trigger mechanism — out of scope for all of Stage 9 per the plan;
- re-deriving facts `command-limits-and-multi-tick.md` already measured (sequence/
  fork accounting, the 65,536 defaults, per-root sequence budgets) — 9.0 cites and
  extends that note, it does not repeat it.

## Current repository boundary

Verified 2026-07-23 against current source; re-check before building on any line,
per the plan's standing instruction.

### Crossing engine — `crates/mdl-compiler/src/lower/minecraft/crossings.rs` (692 lines)

The shipped `COLLECT → FRAME → BRIDGE → RENDER → CALL` pipeline is smaller and more
concrete than the aspirational Tier A/B/C design in
[`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md):
`collect_runtime_operands` (line 39) currently has match arms for exactly two
`CommandKind` variants (`DataCommand::Modify` with an entity/block NBT source, and
`ItemReplaceBlock`, PS-16/BE-2's whole-slot write); `render_as_macro` (line 152)
mirrors the same two. `build_frame` (line 101) deduplicates by `ValueId` and assigns
stable `i0, i1, …` keys into the fixed `mdl:__mdl/macro args` compound;
`emit_bridges` (line 454) reads each operand's planned home via
`plan.value_home`/`plan.home_type` and emits either a storage-to-storage copy
(`String`) or a score-read-into-storage bridge (everything else);
`seed_macro_frame`/`push_macro_call` (lines 526, 550) seed the empty compound and
push `FunctionWithStorage`. **Stage 9C generalizes this concrete engine**, not the
aspirational `SyntaxSlot`/`Operand<T>` vocabulary in the crossing-model note — that
note's vocabulary is the right target shape to converge toward eventually, but is
Stage 11/12 scoped and not a 9.0 or 9C dependency.

### Ambient context model — `ir/semantic/behavior.rs` (690 lines) + `lower/minecraft/api.rs`

`AmbientContextRequirements` (`behavior.rs:74`) is a five-component struct
(`executor: ContextRequirement<EntityKind>`, `position`/`rotation`/`dimension`/
`anchor: ContextRequirement<()>`), each component an independent three-point lattice
(`None` / `Required(T)` / `Unknown`) that joins pointwise (`behavior.rs:56`,
`:186`). `AmbientContextRequirements::NONE` (`:84`) is exactly the "self-rooting"
predicate Stage 9's domain 4 needs: zero components required. `ForkBound`
(`behavior.rs:300`) is a separate four-point lattice (`None` / `Finite(NonZeroU64)`
/ `NoFiniteUpperBound` / `Unknown`) for redirect expansion, unrelated to context.

`generated_entry_requirement()` is not on `behavior.rs` itself — it is a method on
`LoweredFunction` (`lower/minecraft/api.rs:354`), returning that function's
`AmbientContextRequirements`, **independently recomputed after Core optimization**
(the doc comment at `api.rs:348` is explicit that this is deliberately distinct from
the frontend's source-level published behavior). This is the actual mechanism 9B
must query per candidate scheduled/tick-tag entry.

One gap worth flagging precisely: grepping every non-test call site of
`AmbientContextRequirements::NONE` and `generated_entry_requirement()` in
`crates/mdl-compiler/src` today turns up construction sites (functions whose
requirement legitimately *is* `NONE`) but **no enforcement site** — nothing today
asserts that `#minecraft:load`'s entry requirement reduces to `NONE`. Load is
self-rooting today only because no source path reachable from it happens to consume
ambient context, not because a check would reject it if one did. 9B is the first
tranche to add that enforcement (for scheduled/tick entries); 9.0 does not add it,
but the dossier should not imply the check already exists somewhere for load.

### Bound/cost analysis — `crates/mdl-compiler/src/analysis/minecraft/` (`cost.rs`, `analyze.rs`, `report.rs`, `local.rs`, `graph.rs`, `solve.rs`, `syntax.rs`)

`CountUpperKind` (`cost.rs:47`: `Finite(u64)` / `AboveAnalysisCap` /
`NoFiniteBoundProven(NoFiniteBoundReason)` / `Unknown(UnknownCostReason)`) and
`CommandLimitStatus` (`cost.rs:812`: `ProvenWithin` / `MayExceed` / `ProvenExceeds`
/ `NoFiniteBoundProven` / `Unknown`) are exactly as the plan describes.
`RootExecutionSummary` (`cost.rs:862`) carries `sequence_limit_status` and
`fork_limit_status` per root — this is what 9A promotes from reported fact to hard
error. The load-bearing discovery for Stage 9's registration story:
`TargetExecutionRoot` (`cost.rs:777`) is already `Function(McFunctionId) |
FunctionTag(FunctionTagId)` — the analysis is **already generic over tag-rooted
entries**, not hand-coded to `#minecraft:load`. `ResolvedTargetExecutionRoot`
(`cost.rs:784`) already has a `FunctionTagFunction { tag, entry_index, function }`
variant. This means `#minecraft:tick` tag entries (and, once representable,
scheduled-function entries) can become new analysis roots with **zero changes to
the bound-analysis core** — only the enumeration of what counts as a root needs to
grow, in 9B/9A, not in 9.0.

### Tag registration — `crates/mdl-compiler/src/lower/minecraft/construct.rs`

The exact `#minecraft:load` pattern (`construct.rs:77-98`):
`FunctionTagResourceId::parse("minecraft:load")`, `builder.declare_function_tag(...,
FunctionTagMerge::Append)`, `builder.begin_function_tag(load_tag)`,
`tag.push(FunctionTagEntry::internal(InternalCallableRef::Function(load_function),
origin))`, `tag.finish()`. This is the direct, mechanical parallel for
`#minecraft:tick` registration that 9B will add. Independent confirmation that the
structured IR needs no new surface for tag *identity* itself: an existing IR-level
test, `crates/mdl-test/tests/minecraft_ir.rs:302`, already declares a
`"minecraft:tick"` tag through the same generic `declare_tag`/
`MinecraftProgramBuilder` path used for `"minecraft:load"` a few lines earlier
(`:301`) — the physical builder is resource-id-generic, not load-specific.

### Test harness — `crates/mdl-test/src/`

`TestServer` (`lib.rs:381`) exposes raw console command send (`command`, `:418`,
writes to the server's stdin — this is console/RCON-equivalent input, not a game
action) and log-substring waits (`wait_for_log`/`wait_for_command_log`, `:434`,
`:485`) plus checkpointed log diffing (`log_checkpoint`/`check_datapack_logs_since`,
`:491`, `:505`). `ServerSandbox` (`:95`) writes raw datapack files
(`write_datapack_file`, `:164`) into a sandboxed world before `start` (`:262`)
launches the pinned jar. **No tick-stepping primitive exists anywhere in this
crate** — confirmed by grep, zero hits for `/tick` (freeze/step/query/sprint) in
`crates/mdl-test` or `crates/mdl-cli`.

`crates/mdl-test/src/scenario/observation.rs` already has everything Stage 9 needs
to *read* state: `ObservationKind`/`ObservationPath`/`ObservationValue`
(`:383`-`:560`) cover `Score`, `Storage` (arbitrary NBT path), `Block`, `Entities`
(tag-scoped, multiset or cardinality-only), and `LogEffects`. This machinery is
reused unchanged; 9.0 does not need a new observation type, only a way to pin *when*
an observation is taken relative to a tick boundary.

Three existing test files already depend, implicitly and without measurement, on
`schedule function <id> 1t replace` behaving as a single-pending-slot barrier:
`crates/mdl-test/tests/stage8_activation_contract.rs:26`,
`crates/mdl-test/tests/command_limits.rs:24`, and
`crates/mdl-test/src/scenario/runner.rs:776-791` (`settle_prelude`) +
`:793-839` (`prelude_barrier_contents`) — a self-rescheduling poll loop
(`execute unless score #prelude_ready ... run schedule function ... 1t replace`)
used as scenario-runner synchronization infrastructure, already running in CI. This
is real, already-exercised evidence that `replace` at least does not obviously
misbehave under repeated re-issue in this specific "poll until condition, else
reschedule" shape — but it has never been pinned as a measured contract, and it
never tests `/reload` mid-chain, `append`, or firing order. 9.0 makes explicit what
this existing infrastructure already assumes implicitly.

### PS-13 bot infrastructure — `crates/mdl-test-bot/src/lib.rs`

`BotHandle` (`:106`) is a real Azalea client connected to a `TestServer`. Relevant
to 9.0 only for the one measurement (M16 below) that needs to distinguish "no
player is online" from "a player is online but a scheduled/tick function still
receives no ambient selection of them" — everything else in 9.0 needs no connected
client.

### Prior empirical evidence already on record

[`../../mcfunction/command-limits-and-multi-tick.md`](../../mcfunction/command-limits-and-multi-tick.md)
predates the Stage 9 plan (measured 2026-07-11, during the boundary/region
discussion that produced it) but is already partial 9.0 evidence, measured against
the same pinned Java 26.2 server family (bundle SHA-256
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`, OpenJDK 25.0.3).
It already establishes, with an explicit "Status: Measured" marker on each claim:

- grammar: `schedule function <function> <time> [append|replace]`,
  `schedule clear <function>`; `schedule ... 0t` is rejected outright ("Can't
  schedule for current tick");
- **one scheduled function tag fans out into N separate roots** the following tick,
  each with its own independent sequence budget (demonstrated by lowering
  `max_command_sequence_length` to 10 and observing both tag members stop at
  exactly 9 operations, independently); functions listed separately in
  `minecraft:tick` were "also observed to receive separate roots" — the same
  independence, not yet characterized for firing *order*, only for budget
  isolation;
- **execution context is lost across a `schedule` boundary** — direct measurement,
  scheduling `as` a tagged armor stand: the entry function observed `@s`, the
  1-tick-later scheduled callback did not
  (`{start_has_self:1b,resume_has_self:0b,resumed:1b}`). This is today's only
  measured context-loss data point and it only covers the executor component from
  one `schedule` call shape;
- scheduling a continuation begins a genuinely new command sequence with a fresh
  budget (three functions chained by 1-tick schedules jointly completed 14
  increments that a single sequence capped at 10 could not).

That note's own "Remaining tests" section already names, unprompted, most of what
9.0 needs to close: *multiple `append` schedules of the same function*, *scheduled
callback ordering within a tick*, *persistence across reload and server restart*,
and *rehydrating players, entities, positions, rotations, and dimensions*. The
measurement plan below is organized to close exactly that list, plus the harness
capability gap and two robustness checks stage-9-plan.md's frozen decisions need
(schedule-self-first under abort, `schedule clear` cleanliness) that the prior note
never posed.

## Research applied

No new research threads beyond what `stage-9-plan.md` already synthesized (Esterel,
Rust/C# async, PlusCal, React Fiber, Lingua Franca, BEAM) — 9.0 is the empirical
gate that synthesis was conditioned on, not a source of new design ideas. The
dossier shape (problem/non-goals → current boundary → open questions → proposed
prototype → gate) follows
[`../stage-8/8-0-contract-and-evidence.md`](../stage-8/8-0-contract-and-evidence.md),
the closest prior "tranche 0" precedent in this project.

## Measured evidence

All measurements below were run against `versions/26.2/server-26.2.jar` (extracted
payload SHA-256 `183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`,
matching `command-limits-and-multi-tick.md`'s pinned jar) launched via the
self-extracting bundle `tmp/minecraft-server-26.2.jar` (Bundler-Format 1.0,
`net.minecraft.bundler.Main`; **`MDL_SERVER_JAR` must point at this bundle, not the
already-extracted `versions/26.2/server-26.2.jar` directly** — the extracted jar has
no embedded classpath and fails with `NoClassDefFoundError` when launched bare) and
OpenJDK 25.0.3 (`/usr/lib/jvm/java-25-openjdk-amd64`). Fixtures and reproduction:
[`crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs`](../../../crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs)
(`recon_dimension_selector_scoping` plus 8 `group*` tests, one per group below, all
`#[ignore]`-gated). Every group was verified both individually and as one combined
sequential run (`--test-threads=1`, all 8 tests, 125.69s total) with identical
results. Reproduce with:

```sh
MDL_SERVER_JAR=/path/to/tmp/minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test stage9_0_schedule_tick_evidence -- --ignored --nocapture
```

### Harness primitive: deterministic tick control (M1-M4)

Vanilla's `/tick` command family (`freeze`, `step [<N>]`, `unfreeze`, `sprint`,
`query`, `rate`) exists on 26.2 exactly as hypothesized, with clean, unambiguous log
responses (`The game is frozen`, `Stepping N tick(s)`, `The game is running
normally`, `The game time is N tick(s)` for `time query gametime`). Confirmed:

- **M1 — confirmed.** `tick freeze` stops both wall-clock-driven `#minecraft:tick`
  firing and `schedule` firing: a `#minecraft:tick`-registered counter and the
  native `time query gametime` clock both stayed exactly flat across 1.2s of real
  time while frozen. Ordinary console commands (`scoreboard`, `data`, `function`)
  keep working normally while frozen — proven by every subsequent measurement,
  which all issue console commands while frozen throughout.
- **M2/M3 — confirmed, but with a load-bearing caveat.** `tick step` and
  `tick step <N>` (checked at N=1, 2, 3, 5, 20) advance *exactly* that many ticks —
  cross-checked against both a `#minecraft:tick`-tag counter and native
  `time query gametime`, which always agreed exactly. **The caveat: the
  `Stepping N tick(s)` log line is an acknowledgment, not a completion signal.** A
  counter read immediately after that line raced the actual stepping and observed
  only +1 of a requested +5 in initial testing. The reliable completion signal is
  polling `time query gametime` until two consecutive reads agree (see
  `wait_for_gametime_settled` in the fixture) — frozen ticks appear to process much
  faster than real time (consistent with `tick sprint 1`'s observed ~1650
  ticks/second, ~0.6ms/tick) but not instantaneously relative to console command
  round-trip latency.
- **M4 — confirmed, negative result.** `tick query` reliably reports frozen/running
  state, and while *unfrozen* additionally reports target tick rate and
  P50/P95/P99 performance percentiles — but it does **not** report any incrementing
  tick counter. It cannot serve as the poll-free "wait for tick N" barrier the plan
  hypothesized; `time query gametime` is the correct primitive for that instead,
  and is what the implemented harness primitive uses.

**Harness primitive implemented as measured**, not as originally proposed:
`step_and_settle`/`wait_for_gametime_settled` in the fixture file is the reference
implementation of the polling contract a future `TestServer::step_ticks` should
follow: issue `tick step <n>`, wait for the `Stepping n tick(s)` acknowledgment,
then poll `time query gametime` every 50ms until two consecutive reads agree (5s
timeout). The originally proposed bare `pub fn step_ticks(&mut self, count: u32)`
signature is still correct; its *implementation* must poll internally rather than
trust the acknowledgment line, and its doc comment must say so explicitly (per the
Diagnostics section below). The barrier-poll fallback (`settle_prelude`-shaped) was
**not needed** — deterministic tick-count stepping works and is what every
subsequent group in this dossier used throughout.

### `replace` vs `append` pending-slot semantics (M5-M8)

- **M5 — confirmed.** Three consecutive `replace` calls to the same function at the
  same delay, issued back-to-back while frozen, collapse to exactly one firing.
- **M6 — confirmed, resolves an open question.** `replace` at 5t immediately
  followed by `replace` at 100t (same starting tick) fired **only at +100**, not
  +5: `mark_count` was 0 at tick+5 and 1 at tick+100. **`replace` does not merely
  dedup by function id while preserving the earliest pending fire time — it fully
  replaces the pending entry, including its target tick. The most recent `replace`
  call always wins.**
- **M7 — confirmed, surprising, resolves an open question the opposite way from the
  plan's working hypothesis.** Three consecutive `append` calls to the same
  function **at the identical target tick** collapsed to exactly **one** firing,
  not three. Server log evidence: three separate `Scheduled function 'mdl_probe:mark'
  in 5 tick(s) at gametime 111` acknowledgments (each `append` call is
  individually acknowledged), yet only one actual firing.
- **M8 — confirmed.** `append` at two *different* target ticks (3t then 7t)
  produces two independent firings, one at each target tick, not coalesced.

**Combined model (new, not previously stated anywhere):** Minecraft's scheduled-
function storage is keyed per function id to a **set** of pending target ticks, not
a list of independent invocations. `append` inserts a target tick into that set
(a no-op if that exact tick is already present — this is why M7 collapsed); `replace`
clears the set and inserts just the one new target tick (this is why M6's second
call fully superseded the first). There is no way, even under `append`, to get two
independent pending firings of the same function at the same tick — the only way to
get more than one pending firing is multiple *different* target ticks.

### Same-tick firing order (M9-M11)

- **M9 — confirmed.** Two functions both due the same tick fire in **schedule-call
  order** — verified in both directions (calling A then B fires A then B; calling B
  then A fires B then A).
- **M10 — confirmed.** `#minecraft:tick` handlers fire in the tag file's **declared
  order**, not alphabetical or hash order (a deliberately non-alphabetical `[C, A,
  B]` declaration fired as C, A, B). Stable across `/reload` (re-checked with a
  fresh log-storage reset after `reload`; identical order both times). Not
  independently re-checked across a full process restart in this pass, but a
  `/reload` re-parses the tag from scratch the same way a fresh boot does, which is
  reasonable (if not literally identical) evidence for restart-stability too.
- **M11 — confirmed (bonus finding, not originally required by any frozen
  decision).** `#minecraft:tick` handlers run **before** that same tick's due
  `schedule` firings — consistently observed in every Group 3 trial (`C, A, B`
  always preceded the schedule-fired entries in the combined ordered log).

### `/reload` re-arm hazard (M12-M13)

- **M12 — confirmed.** A self-rescheduling chain (`replace`-based, matching delay
  for both the load-time install call and the chain's own internal reschedule)
  survives `/reload` mid-chain with **zero duplication**: exactly one firing per
  tick, both before and after reload.
- **M13a — confirmed, extends M12.** The same test with the load-time install call
  using `append` instead of `replace` (still matched 1t/1t delay) also showed zero
  duplication — consistent with M7: `append` at the same already-pending target
  tick is a no-op.
- **M13b — attempted, negative result, genuinely open.** The hypothesized
  duplication hazard motivated by M7/M8 (an `append`-based load-time re-arm at a
  delay that does **not** match the chain's own internal reschedule delay — 5t
  chain, 3t install, modeling `spawn-animations`'s defensive `schedule clear`
  pattern) was tested and **did not reproduce**: `loop_count` advanced by exactly
  the single-chain expected amount (4 over 20 ticks) with no excess. This
  contradicts the M7/M8-motivated hypothesis and is recorded honestly as an open
  question rather than forced to a conclusion — see "Unresolved questions" below.

**Net finding:** every reload scenario actually tested showed `replace` is
unconditionally safe (matches the plan's already-frozen default), and no tested
scenario demonstrated an `append`-based load-time duplication hazard, though the one
scenario designed to trigger it did not reproduce it for reasons not yet understood.
This does not license "`append` is safe at load time" as a conclusion — `replace`
remains the required default per the plan, independent of this result.

### Execution context loss/reconstruction (M14-M16)

- **M14 — confirmed, extends the prior note's single data point.** Scheduling
  `as`/`at` a real entity positioned at `[5.0, 200.0, 5.0]` in the overworld, the
  fired callback showed `has_self=0b` (executor lost — independently
  re-confirming `command-limits-and-multi-tick.md`'s executor finding under a
  cleaner, purpose-built setup) **and**, new evidence: the fired callback's
  *ambient position* is not the scheduling entity's position. A marker summoned at
  `~ ~ ~` inside the fired callback materialized at **`[0.0, -60.0, 0.0]`** — this
  flat test world's spawn point / floor level, not the scheduling call's position
  and not literal world coordinate origin `y=0`. **The "default root" position is
  the world spawn point, not the scheduling site's position and not raw `(0,0,0)`.**
- **M15 — attempted, methodologically invalidated, NOT a settled fact.** The
  intended test (schedule from `execute in minecraft:the_nether`, then check which
  dimension the fired callback's ambient context materializes a marker in) produced
  an uninterpretable result: **both** an overworld-scoped and a nether-scoped
  presence check matched the same, definitely-single, definitely-overworld-resident
  test entity. A dedicated diagnostic
  (`recon_dimension_selector_scoping`) traced this to `execute in <dim> as/if
  entity @e[...]` not behaving as assumed in this test harness's conditions:
  - `as @e[...] in <dim>` evaluates the selector **before** `in` changes dimension,
    so it never filters by the target dimension (a real bug in the first attempt,
    fixed);
  - even with the corrected `in <dim> as/if entity @e[...]` order, a definitely
    overworld-only entity matched checks for **both** `minecraft:overworld` and
    `minecraft:the_nether`;
  - a further attempt to force a genuine cross-dimension relocation via
    `execute as @e[...] in minecraft:the_nether run tp @s ~ ~ ~` then made **both**
    checks report absence instead (most likely because the destination nether
    chunk was never `forceload`ed, so the relocated entity became untracked —
    `forceload` only affects the dimension it's issued in).

  **M15 is left open.** It needs a redesigned method (`forceload` the relevant
  chunk in *every* candidate dimension before testing, and independently verify
  `execute in <dim> as @e[...]`'s exact filtering semantics as its own isolated
  measurement before reusing it as a probe for anything else) rather than a forced
  conclusion. No claim about dimension reconstruction should be frozen from this
  pass.
- **M16 — not run this pass.** Deferred: requires standing up a real Azalea client
  via `crates/mdl-test-bot` (`BotHandle`) alongside the frozen-tick harness, which
  was out of scope for this measurement pass's time budget. Executor loss (M14) is
  already independently confirmed twice (this note and the prior one) without a
  connected player; M16 specifically tests whether a *present* player changes
  anything about a fired callback's starting selector state, which remains
  unmeasured.

### Schedule-self-first robustness and `schedule clear` cleanliness (M17-M18)

- **M17 — confirmed.** A self-rescheduling function that issues its own
  `schedule ... replace` as its first command, under a `max_command_sequence_length`
  gamerule of 3 (low enough that the function is cut off well before its final
  line, confirmed each time by checking a marker on that final line never got
  reached), still fired on every subsequent tick across 3 consecutive trials — the
  reschedule call, already executed before the abort, is never un-armed by the
  abort. This is now independently measured evidence for why "schedule-self-first"
  is the correct lowering, not just an inference from `dynamic-lights`'s own stated
  rationale.
- **M18 — confirmed.** After `schedule clear <self>` with no further schedule call,
  zero further firings occurred across 5 subsequently stepped ticks, and no
  exception/error-shaped log output appeared.

### Freezing the six semantic decisions

| Decision (from `stage-9-plan.md`) | Resolved by | Disposition |
| --- | --- | --- |
| Mode 1 scheduled functions are argument-free global singletons | grammar fact (confirmed: `schedule function <id> <time> [append\|replace]` has no argument-passing syntax) | **frozen** by structural necessity |
| `replace` is default; `schedule clear` is exposed | M5, M6, M12, M13a, M13b | **frozen as stated in the plan, not strengthened.** M5/M12/M13a confirm `replace` (and even matched-delay `append`) collapse safely across `/reload`. M13b's negative result means the evidence does **not** support requiring `schedule clear` at load time as a hard rule — the plan's existing wording ("`replace` is the default; `schedule clear` is exposed" as an available tool, not a mandate) stands unchanged. `spawn-animations`'s own use of `schedule clear` remains unexplained by this pass's evidence and is noted as an open question, not adopted as a new requirement. |
| Schedule delays are compile-time constant `Ticks` | grammar fact (`schedule` takes a literal time argument, no indirection form) | **frozen** by grammar; no measurement blocks it |
| Self-reschedule is not recursion; schedule-self-first lowering | M17 | **frozen.** M17 directly confirms mid-abort robustness. |
| Deterministic tick-tag ordering | M10, M11 | **frozen for declared order within one server lifetime and across `/reload`.** Not independently re-verified across a full process restart (reload's from-scratch tag re-parse is treated as reasonable, not conclusive, evidence for restart-stability too) — worded as strong-but-not-restart-proven. |
| Cut-legality rule shared by mode 1 and mode 3 (context self-rooting/reconstructible) | M14 (confirmed), M15 (open), M16 (deferred) | **partially frozen.** Executor loss and the "default root = world spawn point, not literal origin, not the scheduling site" position fact are confirmed (M14). Dimension reconstruction (M15) and player-presence selector leakage (M16) are **not** frozen — see "Unresolved questions." |
| Regions are atomic and replayable | not measurable at 9.0 — no persistent continuation state exists yet | deferred to 9C, unchanged |
| Completion is a stored result plus a signal, never a synchronous await | pure design decision, not a target fact | deferred to 9C, unchanged |

## Invariants and failure behavior

- 9.0 changes no compiler behavior. Invariant: no file under `crates/mdl-compiler/`
  is touched by this tranche; every change lands in `crates/mdl-test/` (harness) and
  `notes/` (evidence, frozen decisions).
- If any measurement contradicts a decision `stage-9-plan.md` currently states as
  frozen, the plan must be updated before 9A/9B/9C may treat that decision as
  settled — per the cross-tranche document rule, this dossier records the evidence
  but the plan is the place a changed cross-tranche invariant is recorded.
- Deterministic tick-stepping (M1-M4) **was** established reliably; the barrier-poll
  fallback was not needed. If a future tranche's pinned-server work finds
  `tick step` behaving differently under conditions this pass did not exercise
  (e.g. an actual player connected, a heavily loaded world), that is new evidence
  requiring its own dossier update, not a silent reversion to barrier-polling.

## Diagnostics and public inspection

9.0 adds no compiler diagnostics — no compiler code is touched. The public surface
this tranche adds is the fixture file's `step_and_settle`/`wait_for_gametime_settled`
helper functions
(`crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs`), the reference
implementation for a future `TestServer::step_ticks(&mut self, count: u32)`. Its doc
comment, when promoted into `crates/mdl-test/src/lib.rs`, must state precisely what
M2/M3 measured: *"advances the world clock by exactly `count` ticks; internally
polls `time query gametime` to completion rather than trusting the immediate
`Stepping N tick(s)` acknowledgment, which precedes actual completion"* — matching
the existing precision of `wait_for_log`'s doc comment (`lib.rs:428-433`). This
promotion (fixture helper → `TestServer` method) is 9B/9C implementation work, not
done in this pass, since 9.0 itself needed no persistent harness change to complete
its own measurements — the helpers living in the test file were sufficient.

## Tests, scale/corruption work, and gate

- Ignored pinned-server test file,
  [`crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs`](../../../crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs),
  one `#[test]` per group (`group1_deterministic_tick_control` through
  `group7_schedule_clear_cleanliness`, plus the standalone diagnostic
  `recon_dimension_selector_scoping`), `#[ignore]`-gated identically to the existing
  Stage 8/PS suites (`MDL_SERVER_JAR` + `MDL_JAVA` + `-- --ignored`). All 8 tests
  pass individually and together (125.69s combined, `--test-threads=1`).
- No fast-suite/unit tests apply — there is no new compiler logic to unit test at
  this tranche. `ObservationSet`'s existing unit tests already cover its own
  correctness; 9.0 exercises it, it does not extend it.
- Scale/corruption work is not applicable yet — no persistent continuation state
  format exists until 9C.
- Gate (from [`../stage-9-todo.md`](../stage-9-todo.md)): every M1-M18 measurement
  recorded with its result; the harness can step ticks and observe state
  deterministically, or the fallback fact is recorded plainly; the six frozen
  decisions table above has no open cell.
  **Reached, with two named exceptions.** 16 of 18 measurements (M1-M14, M17-M18)
  are confirmed with a clean result; M13b is confirmed as a run *and* recorded as a
  genuine negative/open result (not a gate failure — the gate asks for a recorded
  result, not a positive one); M15 is invalidated by a flawed test method and left
  open; M16 was not run. The two frozen-decision table cells touching M15/M16
  (dimension and player-presence reconstruction, inside the shared cut-legality row)
  remain explicitly open rather than falsely closed — see below.

## Unresolved questions that forbid premature implementation

Two questions from the original plan remain genuinely open after measurement, plus
one new question this pass's diagnostics surfaced:

- **Dimension reconstruction after a `schedule`/tick-tag boundary (M15) is
  unresolved**, not merely unmeasured — the attempted method produced
  self-contradictory results (see the M15 writeup above) rather than a clean
  negative. 9B must not assume "default root dimension = overworld" (though that is
  the most likely answer given M14's position result landed at this world's spawn
  point) without a redesigned, cleanly `forceload`-controlled measurement in both
  candidate dimensions.
- **Player-presence selector leakage (M16) was not run** and remains exactly as
  open as before this pass. Needs `crates/mdl-test-bot`'s `BotHandle` alongside the
  frozen-tick harness.
- **New: `execute in <dim> as/if entity @e[...]`'s exact dimension-filtering
  semantics are not understood** and must not be reused as a probe for anything
  else (including future 9B/9C dimension-context work) until measured in isolation
  as its own question, independent of the tick/schedule mechanisms this dossier
  otherwise resolved cleanly. This is now the concrete blocker for redoing M15.
- **M13b's negative result (append + mismatched delay does not visibly duplicate a
  chain) is unexplained.** This does not block 9B (which uses `replace`
  unconditionally per the frozen decision, sidestepping the question entirely), but
  it does mean `spawn-animations`'s own `schedule clear` pattern should not be cited
  as *proof* of a real hazard in any future MDL documentation — the real datapack's
  defensive caution and this pass's inability to reproduce a hazard are both true
  simultaneously and neither explains the other yet.

Every other question the original plan posed (M1-M4's harness feasibility, M6's
exact `replace` mechanics, M10's ordering determinism, M12/M13a's reload safety) is
now closed with a measured answer recorded above. 9A and 9B's dossiers may build on
those directly; 9C's dossier (and any 9B work touching dimension self-rooting)
must still treat dimension reconstruction as open.

## References

Internal:

- [`../stage-9-plan.md`](../stage-9-plan.md) — architecture and the six frozen
  decisions this tranche settles
- [`../stage-9-todo.md`](../stage-9-todo.md) — execution checklist and gate
- [`README.md`](README.md) — required dossier shape
- [`../pre-scheduler/ps-3-handoff.md`](../pre-scheduler/ps-3-handoff.md) — the
  concrete continuation state 9C must eventually preserve
- [`../stage-8-handoff.md`](../stage-8-handoff.md),
  [`../stage-8/8-d-recursive-frames.md`](../stage-8/8-d-recursive-frames.md) — why
  synchronous frames cannot cross a tick
- [`../stage-8/8-0-contract-and-evidence.md`](../stage-8/8-0-contract-and-evidence.md)
  — structural precedent for this dossier
- [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md) —
  the value-crossing architecture Stage 9C generalizes
- [`../../mcfunction/command-limits-and-multi-tick.md`](../../mcfunction/command-limits-and-multi-tick.md)
  — prior measured evidence this tranche extends, not repeats
- [`../../mcfunction/ranges/range-domains.md`](../../mcfunction/ranges/range-domains.md)
  — confirms `tick rate`'s Brigadier argument domain exists in this target's grammar
- [`../testing-harness.md`](../testing-harness.md) — harness layer conventions this
  tranche's new test file must follow
- [`../semantic-ambiguities.md`](../semantic-ambiguities.md) — ledger for any
  residual uncertainty from the "Unresolved questions" section above; new entries
  start at A-028
- [`../../functionality-keystones/datapacks/dynamic-lights/NOTES.md`](../../functionality-keystones/datapacks/dynamic-lights/NOTES.md),
  [`.../spawn-animations/NOTES.md`](../../functionality-keystones/datapacks/spawn-animations/NOTES.md),
  [`.../brainfuck-interpreter/NOTES.md`](../../functionality-keystones/datapacks/brainfuck-interpreter/NOTES.md)
  — real-datapack prior art motivating M6/M13/M17

Codebase anchors (verified 2026-07-23; re-check before building on any line):

- `crates/mdl-compiler/src/lower/minecraft/crossings.rs` — crossing engine,
  currently 2 `CommandKind` variants supported
- `crates/mdl-compiler/src/ir/semantic/behavior.rs:74` — `AmbientContextRequirements`
- `crates/mdl-compiler/src/lower/minecraft/api.rs:354` — `generated_entry_requirement`
- `crates/mdl-compiler/src/analysis/minecraft/cost.rs:777,784,862` —
  `TargetExecutionRoot`, `ResolvedTargetExecutionRoot`, `RootExecutionSummary`
  (already generic over `FunctionTag` roots)
- `crates/mdl-compiler/src/lower/minecraft/construct.rs:77-98` — `#minecraft:load`
  tag registration, the direct parallel for `#minecraft:tick`
- `crates/mdl-test/src/lib.rs:381` — `TestServer`; still no `step_ticks` method on
  the shared type itself (the reference implementation lives in the fixture file
  below, not yet promoted)
- `crates/mdl-test/src/scenario/observation.rs:383-560` — `ObservationSet` reused
  unchanged (not exercised directly by 9.0's own fixtures, which read state via
  plain `scoreboard`/`data get` console round-trips instead — sufficient for this
  tranche's own needs, still the right choice for 9B/9C's richer observations)
- `crates/mdl-test/src/scenario/runner.rs:776-839` — `settle_prelude`; still an
  unpromoted but now independently-validated pattern (M12/M13a confirm `replace`
  behaves exactly as this existing infrastructure has always implicitly assumed)
- `crates/mdl-test/tests/minecraft_ir.rs:301-302` — structural proof the physical IR
  already declares `#minecraft:tick` generically
- `crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs` — this tranche's own
  measurement fixtures and the `step_and_settle`/`wait_for_gametime_settled`
  reference implementation of the tick-stepping primitive
- `tests/programs/brainfuck/` — the 9C client (unaffected by 9.0)

External:

- [Mojang: Minecraft Java Edition 1.20.3 (fork/sequence accounting)](https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3)
- The `/tick` command family (`freeze`/`step`/`sprint`/`query`/`rate`/`unfreeze`)
  shipped in vanilla 1.20.2. No specific patch-note URL is cited here — confirm its
  exact 26.2 grammar and behavior directly against the pinned server (M1-M4), not
  against secondhand documentation, per this project's standing rule.
