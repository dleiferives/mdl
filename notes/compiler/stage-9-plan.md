# Stage 9 Persistent Continuations and Static Multi-Tick Scheduling

Status: **planned; not started**

Stage 9 adds one new semantic event — a program can cross a Minecraft tick boundary
— and lowers already-correct synchronous work so it survives that crossing. It does
not add a runtime job queue, dynamic job creation, or a claim that arbitrary programs
finish in one command sequence. It builds three capabilities over one shared model,
in dependency order, and each earlier tier is a usable milestone before the next
begins.

The three capabilities, from cheapest to hardest:

1. an opt-in **one-tick bound contract**: write ordinary bounded code and have the
   compiler prove it fits the target command/fork limits or reject it;
2. **recurring scheduling with no continuation**: a source-level "run this again in
   N ticks" / "run every tick" whose every invocation runs top-to-bottom and
   terminates, with all cross-tick state external — the `dynamic-lights` and
   `spawn-animations` pattern; and
3. **persistent continuations**: a bounded workload the compiler automatically
   partitions across ticks at author-marked structural boundaries, materializing the
   live values that must survive each crossing — the pattern MDL's own Brainfuck
   capstone would need to truly span ticks.

## The one idea: a tick boundary is a third forced cut

The [boundary/region discussion that preceded this plan] established that the emitted
mcfunction partition should be derived from *forced cuts*, not from IR basic blocks.
A macro expansion is a forced cut: the runtime values filling `$(…)` slots must be
marshalled into a storage compound *before* the macro-invoked function runs. A real
conditional is a *negotiable* cut (inline-by-prefix versus outline-by-call is a cost
choice). Stage 9 adds a second forced cut — the tick boundary — and it is the same
operation as the macro cut:

```text
a cut is a value-materialization point:
  externalize the live values that must survive the crossing
  into a persistent store, then re-enter past the cut.
```

The macro crossing engine already does exactly this. It lives in
[`crates/mdl-compiler/src/lower/minecraft/crossings.rs`] as a
`COLLECT → FRAME → BRIDGE → RENDER → CALL` pipeline that walks a command for runtime
operands, bridges them into a macro argument frame, and emits a
`FunctionWithStorage` call. A tick cut adds exactly three things to that same
operation:

- the store must outlive a tick — scoreboard/storage already do, and the macro
  engine already bridges into storage;
- re-entry is dispatched on a **resume discriminant**, a small integer naming which
  region to continue (Esterel's control leaf, PlusCal's `pc`, Rust's async state
  enum — universal across every precedent studied); and
- the execution **context is lost** — `@s`, position, rotation, dimension, and
  anchor do not survive the crossing and must be re-established or reconstructed.

Stage 9's spine is therefore *generalize an existing crossing to a new boundary
kind*, not build a scheduler subsystem. This framing is what keeps the stage small
enough to land in tiers and defensible against the project's standing allergy to new
abstractions: it consolidates macro-crossing and tick-crossing rather than inventing
a third mechanism.

## Six domains that must stay separate

Following the Stage 8 discipline: the reason previous scheduler sketches felt large
is that they fused distinct concerns. These six are separate and must stay separate.

### 1. Cut site

A cut is a legal boundary at which execution may end and later resume. In mode 2 and
mode 1 the only cut sites are whole-function boundaries. In mode 3 a cut site is
either an author-written `yield` or a loop iteration boundary the author opted in by
marking the loop schedulable. A cut site is a semantic marker; it is not a schedule
command, a carrier, or a context.

### 2. Live-state materialization set

The values that must survive a cut, chosen by liveness analysis exactly as Rust and
C# async do — lift only the locals live across the crossing, nothing more.
Protothreads' minimalism (punt every surviving local onto the programmer as a manual
`static`) is explicitly rejected: MDL already has liveness (Stage 5/8), so it does
the Rust thing automatically. The set is externalized to scoreboard (scalars) and
storage (aggregates/lists), reusing the crossing engine's bridge phase.

### 3. Resume discriminant

A small integer, one objective per resumable function, naming which region to
re-enter. Entry to a resumable function is a generated dispatch
(`execute if score $resume <fn> matches N run function …`) — the command-syntax
realization of the state-machine switch every studied language compiles a suspend
point into. This is the only genuinely new persistent datum a continuation needs
beyond its live-state set.

### 4. Execution-context reconstruction

Across a tick, ambient context is gone. A cut is legal only if the post-cut context
is *self-rooting* — reducible to the scheduler's default root (no executor, world
origin) — or reconstructible from stored data. This is checked with the existing
[`AmbientContextRequirements`] (`crates/mdl-compiler/src/ir/semantic/behavior.rs`):
a scheduled or resumed entry's `generated_entry_requirement` must reduce to the
default root. `dynamic-lights` satisfies this by re-selecting entities from scratch
each tick; `ps-3-handoff.md` names the same obligation as "a stable reconstruction
strategy for the output executor rather than assuming `@s` survives a tick."

### 5. Scheduling primitive

The target facilities: `schedule function <id> <time> [append|replace]`,
`schedule clear <id>`, and registration into the `#minecraft:tick` function tag.
Critically, a Minecraft function has **no per-invocation identity** — `schedule` is
keyed by function resource id and (in `replace` mode) holds exactly one pending slot
per id. This is a hard constraint, not a detail: it means scheduled functions cannot
carry per-invocation arguments through the schedule itself (see the mode 1 argument
rule below). Registration into `#minecraft:tick` is a direct parallel of the existing
`#minecraft:load` registration in
[`crates/mdl-compiler/src/lower/minecraft/construct.rs`]
(`FunctionTagResourceId::parse` + `FunctionTagMerge::Append` + tag entry push).

### 6. Cut/region graph

An internal analysis over the structured target IR — the same status as Stage 5's
physical planning: "ordinary Rust dataflow, not a public physical IR or generic
typestate framework." It records where the forced tick cuts fall and verifies each
inter-cut region independently fits the per-tick budget. It is **not** a serialized
IR level with its own verifier/printer, and mode 1 does not need it at all (its only
cut is the whole-function boundary). The *global* cut-minimizing fusion optimizer
that the boundary discussion also motivated stays Stage 11; Stage 9 needs only
correctness (legal cuts, correct state machine, budget verification).

## Lessons applied from concurrency/scheduling languages

Three research threads (synchronous/reactive languages; coroutine lowering; actor and
budget schedulers) converged hard, and the convergence shapes this plan.

### Esterel and the synchronous family: control state is a small integer

Esterel compiles a suspended thread to a bitset naming which leaf holds control, not
a stack snapshot — proof that discriminant-dispatched re-entry scales. Steal it
directly (domain 3). Reject Esterel's default-everything-concurrent model: MDL's real
precedent is explicit arm/disarm (`schedule`/`schedule clear`), not ambient live
processes, and general `||`-composition of concurrently-awaiting threads is Esterel's
own state-explosion source and is out of scope.

### Rust/C# async and Unity coroutines: compiler-lifted live locals

Every mainstream suspend/resume lowering is the same shape: a state discriminant plus
compiler-lifted live locals, dispatched through a switch. Unity's "call `MoveNext()`
once per frame" is the exact scheduling analog of "call the resume function once per
tick." This is the architectural template (domains 2 + 3). Protothreads is the
cautionary outlier — its 2-byte minimalism is bought by refusing to preserve locals;
do not import that limitation.

### PlusCal: liveness across a label must be an explicit named variable

PlusCal introduces an explicit `pc` and forces any value surviving a label boundary
to be a named process variable, because TLA+ has no implicit stack. This is
structurally identical to MDL's constraint and it makes domain 2 a *compiler-checked*
obligation, not a convention.

### React Fiber: automatic partitioning, but only at structural boundaries

Fiber yields automatically based on a time budget, but only at pre-existing structural
unit boundaries (between fibers), never mid-unit. This reconciles the roadmap's
"a bounded workload is automatically partitioned across ticks" with the unanimous
research finding that no system infers arbitrary split points: **the loop iteration
boundary is MDL's fiber unit.** The author marks a loop schedulable (opting its
iteration boundaries in as legal cut sites); the compiler automatically decides how
many iterations run per tick to fit the budget. The explicit `yield` statement is the
finer-grained escape hatch for non-loop code. The compiler never invents a cut in the
middle of straight-line code.

### Lingua Franca and BEAM: what Stage 9 is *not*

Lingua Franca's declared trigger→reaction-at-a-logical-tag is the clean model for
event triggers — and MDL already shipped that as PS-15 advancement-triggered events.
Stage 9 adds **no new trigger mechanism**; a trigger handler is an ordinary function
that may itself `schedule` or be `scheduled`. BEAM's automatic reduction-counted
preemption has **no analog** for MDL and this is structural, not a choice: a Minecraft
function runs every command to completion with no hook to interrupt it mid-execution.
"Suspend" therefore only ever means "this function ends and a later, separately
invoked function continues the work." Koka/OCaml effect handlers are rejected for the
same reason they don't fit: they assume a heap and multi-shot continuations, and a
tick boundary is forward-only and single-shot.

## Frozen semantic decisions

These are language/runtime semantics, not emitter choices. 9.0 has measured all six
against the pinned Java 26.2 server
([`stage-9/9-0-contracts-and-evidence.md`](stage-9/9-0-contracts-and-evidence.md),
measurements M1-M18); each entry below now states its measured disposition. Two
sub-questions (dimension reconstruction; player-presence selector leakage) remain
genuinely open and are called out where they apply — 9B/9C must not assume answers
to those.

- **Mode 1 scheduled functions are argument-free global singletons.** Because
  `schedule` is keyed by function id with one pending slot (`replace`), a scheduled
  function takes no per-invocation arguments; state it needs is read from external
  stores (entity scoreboards, storage) that the program writes before scheduling.
  This matches every real datapack and sidesteps reinventing a job queue.
  **Confirmed by grammar** (`schedule function <id> <time> [append|replace]` has no
  argument-passing syntax) and by measurement M5-M8: even under `append`, only
  *different target ticks* of the same function can coexist — Minecraft's scheduled-
  function storage is keyed per function id to a **set of pending target ticks**, not
  a list of independent invocations, so there is no vanilla mechanism that could
  carry per-invocation arguments regardless of mode.
- **`replace` is the default schedule mode; `schedule clear` is exposed.** M5/M12
  confirm `replace` collapses to a single pending entry and survives `/reload`
  mid-chain with zero duplication. M6 additionally established the exact mechanism:
  a second `replace` call **fully replaces** the pending entry including its target
  tick (the most recent call always wins), not merely a same-function dedup that
  preserves the earliest fire time. **The evidence does not support requiring
  `schedule clear` at load time** — M13a (matched-delay `append` re-arm) and the
  attempted M13b (mismatched-delay `append` re-arm, modeled on `spawn-animations`'s
  own defensive pattern) both showed no duplication; M13b's negative result is
  unexplained but does not license a stronger rule than what was already written
  here. `schedule clear` stays an exposed tool, not a mandated one, and MDL's own
  lowering uses `replace` unconditionally regardless.
- **Schedule delays are compile-time constant `Ticks`.** A runtime delay would need a
  macro (`schedule` takes a literal time); runtime delays are deferred. A dedicated
  `Ticks` type prevents "is this a tick count or a raw int" confusion. **Confirmed
  by grammar** — no indirection form exists for `schedule`'s time argument.
- **Self-reschedule is not recursion.** A function that schedules itself gets no
  Stage 8 activation frame; the compiler recognizes scheduled re-entry as distinct
  from a synchronous recursive call (Stage 8's handoff is explicit that Stage 9 must
  not reuse synchronous tail frames across a tick). The recommended lowering is
  **schedule-self-first**: re-arm the next invocation before doing work, so a
  mid-tick abort does not break the chain (the `dynamic-lights` robustness pattern).
  **Confirmed by measurement M17**: a self-rescheduling function whose own
  `schedule ... replace` call is its first command survived three consecutive
  command-sequence-limit aborts (each cutting the function off well before its final
  line) without ever missing a subsequent firing.
- **Deterministic tick-tag ordering.** Multiple `#minecraft:tick` handlers run in
  source/module declaration order, consistent with MDL's existing deterministic
  whole-package resolution. **Confirmed by measurement M10** for declared order
  (a deliberately non-alphabetical tag-file order fired in exactly that order) and
  stability across `/reload`. Not independently re-verified across a full process
  restart; `/reload`'s from-scratch tag re-parse is treated as reasonable, not
  conclusive, evidence for restart-stability too. M11 additionally found (not
  previously stated, not required by this decision, but worth recording) that
  `#minecraft:tick` handlers run **before** that same tick's due `schedule`
  firings.
- **The cut-legality rule is shared by mode 1 and mode 3.** The post-cut execution
  context must be self-rooting or reconstructible (domain 4). Mode 1 applies it at the
  whole-function boundary; mode 3 applies it independently at every interior cut site.
  **Partially confirmed by measurement M14**: executor context is lost (`@s` does not
  survive a `schedule` boundary, independently re-confirming
  `mcfunction/command-limits-and-multi-tick.md`'s prior finding), and the ambient
  position a fired callback receives is the **world spawn point**, not the
  scheduling call's position and not literal coordinate origin. **Dimension
  reconstruction (M15) and player-presence selector leakage (M16) remain open** —
  M15's attempted measurement was invalidated by a flawed test method (see the
  dossier) rather than answered either way, and M16 was not run. Self-rooting for
  dimension specifically must not be assumed to reduce to "always overworld" until
  a redesigned measurement confirms it.
- **Regions are atomic and replayable.** The persistent continuation frame advances
  only at a *successful* cut; an aborted tick leaves the previous committed frame
  intact and the interrupted region re-runs from it. Regions must therefore be
  replayable from their entry frame (side effects deferred until after the frame
  commit, or idempotent). This is the roadmap's "atomic regions," and it is how the
  abnormal-interruption rule is preserved: a command-limit abort is a poisoned runtime
  event handled by the generated recovery entry, never a semantic result (from
  `ps-3-handoff.md`). Not measurable at 9.0 (no persistent continuation state exists
  yet); unchanged design commitment for 9C.
- **Completion is a stored result plus a signal, never a synchronous await.** A
  partitioned job writes its typed result to a designated storage destination and sets
  a completion signal; the launcher polls it or runs a scheduled completion handler.
  There is no returned value across ticks because the caller frame is gone.
  Cancellation is `schedule clear` plus resetting the discriminant to a terminal
  state. **Confirmed clean by measurement M18**: `schedule clear` with no further
  schedule call produced zero further firings and no error/exception log noise, so
  it is safe to use exactly as this cancellation model requires. Not otherwise
  measurable at 9.0 (no persistent completion/result state exists yet); unchanged
  design commitment for 9C.

## Implementation tranches

Each tranche gets a detailed dossier in [`stage-9/`](stage-9/README.md) when its turn
comes; the dossiers are the implementation source of truth and this overview owns only
the cross-tranche architecture.

### 9.0 — Contracts and pinned-server evidence

Status: **measured on the pinned Java 26.2 server (2026-07-23)**; see
[`stage-9/9-0-contracts-and-evidence.md`](stage-9/9-0-contracts-and-evidence.md) for
the full M1-M18 record.

- Measured `schedule function`/`schedule clear`/`#minecraft:tick` behavior on the
  pinned Java 26.2 server directly: `replace` versus `append`, same-tick firing
  order, the `/reload` re-arm hazard, and what execution context a scheduled function
  actually receives. All resolved except dimension reconstruction (open —
  methodologically invalidated, needs redesign) and player-presence selector
  leakage (deferred — not run).
- Built the cross-tick test capability: deterministic tick stepping (`tick freeze`/
  `tick step <N>`/`tick unfreeze`, with completion detected by polling
  `time query gametime` rather than trusting the immediate acknowledgment log line)
  in
  [`crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs`](../../crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs).
  Not yet promoted into a shared `TestServer` method — the reference implementation
  lives in the fixture file pending 9B/9C's actual need for it. PS-13 bot
  infrastructure was not needed for any measurement actually run this pass (M16,
  the one measurement that would have needed it, was deferred).
- Froze five of six semantic decisions above without qualification and the sixth
  (cut-legality) partially, with the two open sub-questions named explicitly rather
  than hidden; see [`semantic-ambiguities.md`](semantic-ambiguities.md) A-028
  through A-030 for the residual uncertainty ledger entries.

### 9A — One-tick bound contract (capability 1)

- Add an opt-in source marker asserting a function fits one tick.
- Promote the existing target bound analysis
  ([`crates/mdl-compiler/src/analysis/minecraft/`]: `CountUpperKind`,
  `CommandLimitStatus::ProvenWithin`, `ForkBound`) from a reported fact into a hard
  compile error when the assertion is present and the bound is not `ProvenWithin`
  against the configured limits.
- No new IR carrier; this is a checking discipline that also underpins 9B (anything
  running every tick must fit a tick).

### 9B — Recurring scheduling without continuation (capability 2)

- Add the `Ticks` type and the `schedule`/`schedule clear`/tick-registration source
  surface.
- Enforce argument-free self-rooting scheduled functions via `AmbientContextRequirements`.
- Recognize self-reschedule as scheduled re-entry, not recursion; emit the
  schedule-self-first lowering; register tick handlers into `#minecraft:tick` in
  deterministic order via the `construct.rs` tag mechanism.
- Full vertical slice to the pinned server: a self-rescheduling job and a tick-tag
  job both observed across multiple ticks under all four optimization policies.

### 9C — Persistent continuations (capability 3)

- Generalize the crossing engine to the tick boundary: resume discriminant, liveness-
  lifted live-state set, cut/region graph, per-region budget verification.
- Add the explicit `yield` statement and the schedulable-loop iteration-boundary
  auto-partition (Fiber unit); reject any inter-cut region that exceeds the budget
  with an "add a yield / mark this loop schedulable" diagnostic rather than silently
  splitting straight-line code.
- Implement atomic replayable regions, recovery, and the stored-result/completion/
  cancellation model.
- Concrete client: the existing Brainfuck package (`tests/programs/brainfuck/`) run
  as a genuinely tick-spanning job, preserving the live state `ps-3-handoff.md`
  enumerated (tape lists, program cursors, input/output, fuel, dispatch count,
  status, position, in-progress bracket cursor).

### 9D — Hardening and handoff

- Public reports explaining every cut, materialized value, resume dispatch, and
  budget decision.
- Four-policy differential proving partitioning changes *when* work completes, never
  *what* it computes; corruption/determinism/scale gates; pinned-server proofs of
  every capability; and the handoff naming what Stage 10/11 inherit (general macro
  interpolation; the global cut-minimizing fusion optimizer).

## Explicitly deferred

- **Triggers** — done in PS-15; Stage 9 adds no new trigger mechanism, only composes
  with the existing one.
- **A runtime job queue and dynamic job creation/cancellation** beyond
  `schedule`/`schedule clear` — explicitly out of scope per the roadmap.
- **Runtime (non-constant) schedule delays** — need a macro; later.
- **Automatic yield insertion in straight-line, non-loop code** — Stage 9 partitions
  only at author-opted structural boundaries; the compiler never invents a cut.
- **The global cut-minimizing fusion optimizer** (fuse to minimize function/call
  count across the whole program) — Stage 11 (`costed trace/block layout`,
  `context-prefix fusion`).
- **Multi-shot continuations and concurrent `||`-composition within one job** — no.
- **`minecraft:dialog` input** (surfaced by the brainfuck-interpreter keystone) — a
  UI capability unrelated to scheduling; its own future milestone if justified.
- **Suspending a live Stage 8 synchronous recursive frame across a tick** — forbidden;
  continuations use persistent state, not synchronous tail frames.

## Exit criteria

Stage 9 is complete when:

- an opt-in one-tick contract proves-or-rejects against configured, server-checked
  limits;
- a self-rescheduling job and a `#minecraft:tick` job run correctly across many ticks,
  with reload behavior verified on the pinned server and no duplicate chains;
- scheduled and resumed entries provably do not assume `@s` or any ambient frame
  survives, enforced through `AmbientContextRequirements`;
- a bounded workload is automatically partitioned across ticks at author-marked
  structural boundaries, and its completion and results are deterministic and
  policy-invariant;
- an interrupted tick re-runs its region from the last committed continuation frame,
  and a command-limit abort remains a distinct poisoned-runtime event with a recovery
  entry, never a semantic result;
- configured command/fork bounds are checked using real pinned-server limit tests;
- there is still no runtime job queue; and
- the Brainfuck capstone runs as a genuinely tick-spanning job preserving exactly the
  state `ps-3-handoff.md` enumerated.

## Primary references

Internal:

- [`implementation-stages.md`](implementation-stages.md) — Stage 9 roadmap entry
- [`pre-scheduler/roadmap.md`](pre-scheduler/roadmap.md) — "Stage 9 after the split"
- [`pre-scheduler/ps-3-handoff.md`](pre-scheduler/ps-3-handoff.md) — the concrete
  continuation state Stage 9 must preserve
- [`stage-8-handoff.md`](stage-8-handoff.md) and
  [`stage-8/8-d-recursive-frames.md`](stage-8/8-d-recursive-frames.md) — why
  synchronous frames must not be reused across a tick
- [`macro-reference-crossing-model.md`](macro-reference-crossing-model.md) — the
  value-crossing architecture Stage 9 generalizes
- [`../functionality-keystones/datapacks/dynamic-lights/NOTES.md`](../functionality-keystones/datapacks/dynamic-lights/NOTES.md)
  — pure `schedule function` self-rescheduling, all state external (capability 2)
- [`../functionality-keystones/datapacks/spawn-animations/NOTES.md`](../functionality-keystones/datapacks/spawn-animations/NOTES.md)
  — `#minecraft:tick` tag, `schedule clear`, per-tick work-budget selectors
- [`../functionality-keystones/datapacks/brainfuck-interpreter/NOTES.md`](../functionality-keystones/datapacks/brainfuck-interpreter/NOTES.md)
  — one-opcode-per-tick self-reschedule; hand-rolled prior art for capability 3

Codebase anchors (verified 2026-07-23; re-check before building on any line):

- `crates/mdl-compiler/src/lower/minecraft/crossings.rs` — the crossing engine to
  generalize
- `crates/mdl-compiler/src/ir/semantic/behavior.rs` — `AmbientContextRequirements`,
  `ForkBound`
- `crates/mdl-compiler/src/analysis/minecraft/` — bound analysis for the one-tick
  contract
- `crates/mdl-compiler/src/lower/minecraft/construct.rs` — `#minecraft:load` tag
  registration, the parallel for `#minecraft:tick`
- `tests/programs/brainfuck/` — the 9C client

External (from the Stage 9 research threads):

- [The Esterel v5 Language Primer, Gérard Berry](https://www.college-de-france.fr/media/gerard-berry/UPL8106359781114103786_Esterelv5_primer.pdf)
- [A PlusCal User's Manual, Leslie Lamport](https://lamport.azurewebsites.net/tla/p-manual.pdf)
- [Rust async state-machine lowering](https://google.github.io/comprehensive-rust/concurrency/async/state-machine.html)
- [Protothreads (Adam Dunkels)](https://dunkels.com/adam/pt/about.html)
- [Unity coroutines](https://docs.unity3d.com/Manual/Coroutines.html)
- [React Fiber architecture](https://github.com/acdlite/react-fiber-architecture)
- [Lingua Franca actions](https://www.lf-lang.org/docs/writing-reactors/actions/)
- [BEAM scheduler and reduction counting](https://blog.appsignal.com/2024/04/23/deep-diving-into-the-erlang-scheduler.html)
