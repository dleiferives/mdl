# Rough Compiler Implementation Stages

Status: **Working roadmap**

This roadmap turns the accepted [Stage 0 decisions](stage-0-decisions.md) into
incremental, testable milestones. The ordering is intentional, but the contents are
rough: evidence from the vanilla server may split, merge, or reorder later stages.

Every stage should leave the repository working. A stage is complete only when its
exit criteria are automated; writing the data structures without proving them in an
end-to-end path does not count.

## Milestone overview

| Stage | Outcome |
| ---: | --- |
| 0 | Foundational decisions are explicit |
| 1 | Rust workspace and repeatable vanilla-server harness |
| 2 | Typed, verified SSA IR that can be constructed and printed |
| 3 | Structured Minecraft IR, target profile, and datapack emitter |
| 4 | First end-to-end program compiled and executed without a source parser |
| 5 | Baseline optimization pipeline and measurable lowering alternatives |
| 6 | Minimal typed source language and useful diagnostics |
| 7 | Whole-package modules, then typed Minecraft context/entities/command APIs |
| 7.5 | Ordered execution frames and static spatial semantics |
| 8 | Calling convention and multiple runtime representations |
| 8.5 | Pre-scheduler test oracles, synchronous capabilities, and Brainfuck validation |
| 9 | Persistent continuations and static multi-tick scheduling |
| 10 | Typed compile-time macros and disciplined Minecraft macro lowering |
| 11 | Cost-directed global optimization and optional equality saturation |
| 12 | Stabilization, compatibility, packaging, and ecosystem work |

## Stage 0: Foundational decisions

This stage is complete for the first vertical slice. The decisions are recorded in
[`stage-0-decisions.md`](stage-0-decisions.md).

Exit criteria:

- implementation language, target, IR strategy, runtime boundary, scheduler model,
  raw-command policy, and testing authority are explicit;
- intentionally deferred questions are identified rather than accidentally fixed.

## Stage 1: Workspace and server harness

Status: **Complete for the first vertical slice.** See
[`stage-1-server-harness.md`](stage-1-server-harness.md).

Create the Rust workspace and the infrastructure that will test generated datapacks
against the pinned vanilla 26.2 server.

Likely workspace responsibilities:

```text
compiler data structures and passes
datapack/resource model
command-line driver
vanilla integration-test runner
shared test fixtures
```

The exact crate split should remain small initially. We should split crates only at
real dependency or process boundaries, not create one crate per future IR level.

The server harness must be able to:

1. create a disposable world and datapack;
2. launch the pinned server with an explicit Java executable;
3. wait for readiness without timing guesses;
4. invoke generated test functions;
5. observe scoreboard, storage, success, and failure results;
6. capture logs and useful failure artifacts;
7. terminate the server reliably;
8. run locally and in automation;
9. keep downloaded binaries and generated worlds outside source control.

Unit tests should not launch a server. Server tests need an explicit test category so
the fast test suite remains fast.

Exit criteria:

```text
Rust test -> temporary datapack -> vanilla server -> function call
          -> observed value 42 -> clean shutdown
```

## Stage 2: Typed SSA foundation

Status: **Complete.** See the accepted
[`stage-2-ssa-plan.md`](stage-2-ssa-plan.md) and the
[`Stage 2 implementation record`](stage-2-ssa-implementation.md).

Implement the compiler's semantic control-flow substrate before implementing a
source parser.

Initial concepts:

- shared IR context, Core program, and functions;
- canonical immutable types and explicit constants;
- stable typed IDs for functions, blocks, instructions, and values;
- basic blocks with parameters;
- typed instructions with explicit effect/speculation properties;
- structural `jump`, `branch`, `return`, and `unreachable` terminators;
- source spans that can also represent generated code;
- deterministic traversal and canonical printing.

The verifier must reject malformed IR rather than relying on Rust constructors to
make every invalid state impossible. Passes and future deserialization will still
need a semantic verifier.

Provide both:

- a Rust builder API for tests;
- a deterministic textual dump intended for humans and snapshots.

A parser for the textual IR is useful later, but not required for this stage.

Exit criteria:

- well-typed branch and join examples verify;
- bad types, missing block arguments, undefined values, and invalid terminators are
  rejected with locations;
- IR snapshots are stable across repeated runs;
- dead entities can detach from executable layout without changing typed IDs;
- controlled mutation cannot silently leave dangling references.

## Stage 3: Structured Minecraft IR and emission

Status: **Complete.** The validated target IR, layered verifier, exact renderer,
deterministic artifact/trace emitter, generic harness boundary, official Java 26.2
conformance pack, and Stage 4 handoff fixtures are complete. See
[`stage-3-minecraft-ir-plan.md`](stage-3-minecraft-ir-plan.md) and
[`stage-3-handoff.md`](stage-3-handoff.md).

Introduce a target-aware IR that describes Minecraft behavior without prematurely
flattening commands into strings.

Initial operations should cover:

- scoreboard reads, writes, comparisons, and arithmetic;
- storage/NBT reads and writes;
- structured conditions;
- execution-context changes;
- function calls and `return run`;
- function tags and load entry points;
- raw commands as opaque effect barriers;
- command result and command success as distinct values.

The target profile owns version-dependent facts:

```text
Minecraft version and pack format
available command/resource features
default or configured hard limits
syntax and emission differences
known backend capability flags
```

Emission proceeds through a laid-out command/function graph. An SSA block does not
automatically become a function.

Exit criteria:

- structured operations emit deterministic, valid datapack resources;
- resource locations, objectives, storage paths, and generated names are validated;
- output includes enough comments, maps, or side artifacts to trace generated
  functions back to IR;
- the emitted pack loads successfully on the pinned server.

## Stage 4: First vertical slice

Status: **Complete for the first vertical slice.** See
[`stage-4-first-lowering-plan.md`](stage-4-first-lowering-plan.md) and
[`stage-4-todo.md`](stage-4-todo.md).

Construct programs directly with the Rust IR builder and compile them all the way to
Minecraft.

The intentionally small feature set is:

```text
Int
Bool
constants
comparison
function call
if/else
return
block parameters / loops
```

This list follows the Core vocabulary that actually exists at the Stage 3 handoff.
Scoreboard-backed source mutation and raw commands need a target-neutral
effect/import design; Stage 4 must not add Minecraft syntax to Core to satisfy an
obsolete checklist. The existing target-level condition-mutation fixture remains
the proof for using the single safe return-dispatch lowering.

Exit criteria:

- one nontrivial CFG compiles and executes correctly on vanilla 26.2;
- the fixed-slot ABI is explicitly single-context and non-reentrant;
- the public contract states that runtime-dependent Stage 4 loops remain the
  caller's responsibility to keep within target command limits until Stage 9;
- recursive calls and entry-reachable unsupported terminators are rejected before
  target construction;
- the compiler can dump every IR level and the final generated datapack;
- failures preserve the disposable test world and logs when requested.

This is the first real compiler milestone. It deliberately has no user-facing source
language yet.

The completed path verifies Core, audits reachable legality and recursion, freezes a
deterministic physical plan, constructs and verifies Stage 3, exposes a typed
read-only ABI map, emits an exact datapack/trace, and runs that Core-built artifact on
the official vanilla 26.2 server. The baseline uses fixed global call/result slots,
resolved simultaneous edge copies, a mandatory return dispatcher, and a
single-context non-reentrant execution contract. Finite CFG loops work in one tick;
partitioning work across ticks remains Stage 9.

## Stage 5: Baseline optimization and cost instrumentation

Status: **Complete — reviewed revision 13; Stages 5A–5H are implemented and gated.** See
[`stage-5-baseline-optimization-plan.md`](stage-5-baseline-optimization-plan.md) and
[`stage-5-todo.md`](stage-5-todo.md), with the public boundary recorded in
[`stage-5-handoff.md`](stage-5-handoff.md).

Add conventional, auditable optimizations before advanced search techniques. The
closed baseline Core pipeline implements items 1–5, Stage 5F implements physical
storage item 6, Stage 5G implements items 7–8, and Stage 5A implements the
deterministic accounting part of item 10, and Stage 5H completes report ownership,
measurements, hardening, and conformance. Item 9 remains deliberately deferred with
the optional stability-dependent recipes:

1. cheap closed Core canonicalization;
2. sparse conditional constant propagation and branch folding;
3. unreachable-block and dead-pure-instruction elimination;
4. straight-line Core block fusion;
5. local dominance-scoped common-subexpression elimination;
6. dead physical-home pruning and conservative block-argument coalescing;
7. narrow target block placement and control recipes;
8. costed Minecraft lowering selection with the Stage 4 dispatcher as fallback;
9. consumer-driven physical condition-stability analysis when a repeated-condition
   recipe demonstrates a complete win; and
10. exact footprint, exact command-step/local-path cost, conservative per-root bound,
    and empirical measurement reports.

Core optimization has an owned boundary: it consumes a `CoreProgram` and returns
only a verified optimized program plus its report. A closed compiler-internal pass
algebra replaces public pass injection; callers choose a level rather than constructing
a pipeline. Batch editing prepares one atomic fact set, performs each required
ownership/closure/dominance validation once, and commits with one application scan;
it never falls back to repeated scalar edits. Effects, speculation, structural result
equivalence, and operand symmetry are separate exhaustive Core questions, while
expression keys remain optimizer-owned. Each pass has a reviewed
algorithm/proof plan in [`stage-5-passes/`](stage-5-passes/README.md).

Minecraft planning proceeds one way through private immutable semantic inventory,
runtime demand, Baseline sparse liveness, home/instruction assignment, edge-transfer,
and resource results. (`None` bypasses liveness and preserves the Stage 4 assignment.)
Each phase borrows explicit prerequisites and the final plan consumes and independently
verifies compatible parts. Stage 5G adds recipes and placement; physical effects are
added only when a stability-dependent recipe becomes their first real consumer.
Recipes never feed back into Stage 5 coalescing. This
is ordinary Rust dataflow, not a public physical IR or generic typestate framework.
Physical homes remain typed and only same-type block-argument copies are coalesced; a
separate symbolic home-content dataflow checker verifies the frozen assignment without
trusting chooser demand or liveness.

Every optional analysis has a typed completion state and conservative fallback:
incomplete SCCP applies nothing, incomplete liveness uses distinct homes, incomplete
stability rejects dependent recipes, and incomplete cost bounds become unknown.
Required verification and construction never degrade to best effort.

Every pipeline verifies input and final output. Tests/CI and debug builds verify after
each pass; optimized compiler builds may use boundary verification, with an internal
verify-all override and scale benchmarks. Validity checking is instrumentation, not
an optimization level.

Cost instrumentation keeps three cost-evidence domains separate:

- exact post-emission artifact footprint, including files, physical lines, bytes, and
  trace records;
- exact per-generated-function local work plus finite, cap-exceeding,
  no-finite-bound-proven, or unknown summaries for every supported external entry,
  including execute stages, function calls, selector forks, and scoreboard/NBT
  command executions; and
- target/JVM/fixture-specific tick, wall-time, and reload measurements.

Target execution cost is an explicit read-only analysis over verified Stage 3, not a
mandatory lowering phase. A cost-analysis failure cannot discard an otherwise valid
`LoweringOutput`; the Stage 6 façade will invoke and aggregate it once.

Ordinary reports contain bounded aggregate statistics and the complete chosen
lowering. Core per-rewrite remarks are filtered, capped, and opt-in. Lowering records
one stable selected-or-retained reason per reachable branch arm, so its complete
decision report remains linear without a second remark framework.

Stage 5 accepts typed assumed `max_command_sequence_length` and `max_command_forks`
values for compatibility assessment, defaulting to the selected target. It reports
but never mutates the server gamerules. Java 26.2 accepts configured values from zero
through `2_147_483_647`; sequence zero has an effective quota of one, while fork zero
requires special treatment for checked redirects. An override is a deployment
precondition: lower actual values invalidate a retained `ProvenWithin` result. Soft
per-tick budgets and work partitioning remain Stage 9.

Small-function inlining, specialization, cold outlining, region duplication, and
profile-guided layout remain Stage 11. Stage 5 first builds the cost and decision
records needed to implement those transformations without a definition-size guess.

Exit criteria:

- the compiler can explain why it chose a branch lowering;
- optimization preserves differential test results;
- command-limit estimates are checked against boundary tests;
- the byte-stable Stage 4 lowering remains available as a correctness oracle;
- scale tests reject dense quadratic liveness/interference representations; and
- no wall-time coefficient is treated as authoritative without repeatable evidence.

## Stage 6: Minimal typed source language

Status: **Complete — Stages 6A–6I are implemented/reconciled and both Java 26.2
source/CLI server gates pass.**

Detailed design and execution order:
[`stage-6-minimal-frontend-plan.md`](stage-6-minimal-frontend-plan.md) and
[`stage-6-todo.md`](stage-6-todo.md).

Add the smallest frontend that makes the compiler usable by a person. Implementation
builds one typed scalar compilation unit and preserves it as the reference path. It
also resolves the later whole-package and Minecraft-effect contracts without folding
either implementation into this already substantial stage:

- one owned source input and global function namespace;
- functions and local bindings;
- explicit core types;
- expressions, calls, `if`, and return;
- a deliberately small mutation model;
- source spans through every IR level;
- diagnostics with primary and supporting labels.

Stage 6I assigns whole-package inputs/imports/exports to Stage 7A and the
external-operation spine, typed Minecraft APIs, and literal unsafe command to Stage
7B. Stage 9 consumes their scheduling contracts; Stage 10 owns typed runtime
interpolation and the Minecraft function-macro ABI. The detailed decisions are in
[`stage-6-modules-plan.md`](stage-6-modules-plan.md) and
[`stage-6-effects-plan.md`](stage-6-effects-plan.md).

Surface syntax should be judged by how well it exposes types, context, effects, and
lowering when requested—not by how quickly a large grammar can be implemented.

Exit criteria:

```text
source file -> parse -> type check -> lower -> optimize -> datapack
            -> vanilla execution -> expected assertion
```

This is the first minimal user-facing compiler.

## Stage 7: Typed Minecraft programming model

Status: **complete and gated.**

Authoritative design and execution order:
[`stage-7-plan.md`](stage-7-plan.md) and
[`stage-7-todo.md`](stage-7-todo.md). The audited implementation order is retained in
[`stage-7-remainder-plan.md`](stage-7-remainder-plan.md), and the exact boundary for
Stage 7.5 and Stage 8 is in [`stage-7-handoff.md`](stage-7-handoff.md).

Stage 7A implemented the rooted whole-package prerequisite:

- complete owned module inputs independent of diagnostic filenames;
- one root and Zig-like per-module dependency names;
- compile-time namespace values with uniform dotted member/call resolution;
- private, package-visible, and datapack-exported functions; and
- deterministic whole-package resolution and lowering without filesystem discovery,
  dependencies, or a stable cross-package ABI.

Stage 7B implemented the conservative external Core operation and literal-only
`unsafe minecraft("...")` boundary. Stage 7C implemented the first semantic and
structured-context slice:

- semantic identities distinguishing entity references, possibly-many queries, and
  bound executors, plus the first `ArmorStand` kind; the runnable slice currently
  constructs queries and scoped executors rather than physical entity references;
- selectors with known or bounded cardinality;
- executor, position, rotation, dimension, and anchor context;
- ordered `.as(query)` run scopes with exact query/modifier provenance, strict fork
  assumptions, and one outlined body edge; and
- source-owned inferred context/effect/fork/work function summaries.

Additional frame modifiers and static coordinate values are the bounded Stage 7.5
work; Stage 7 does not pretend they exist merely because its context lattice has a
slot for them.

Stage 7D implements the first method-oriented operation and complete vertical proof:

```mdl
export fn announce() {
    run.as(mc.entities(ArmorStand).with_tag("stage7").limit(1)) |speaker| {
        speaker.say("hello");
    }
}
```

The implementation preserves every modifier and query refinement in semantic order
and carries context, multiplicity, effect, outcome, recipe, placement, and provenance
facts. Typed operations resolve through one compiler-owned semantic registry and
lower through one reconciled Java 26.2 structured recipe. Stage 7.5 owns its selected
static frame modifiers and spatial commands. Conditions, stores, `on`, `summon`, and
multi-version recipes remain deliberately deferred; target dependence still stays
out of semantic HIR/Core.

Users cannot assert purity or effect precision for unsafe text. Runtime interpolation
remains Stage 10.

The compiler should be able to type-check an expression in the spirit of:

```text
prison.cells[input.id].user.say("hello")
```

Dynamic indexing may still use a simple initial lowering. Optimization comes after
the semantics are stable.

Exit criteria:

- module resolution, visibility, cycles, deterministic ordering, and explicit
  datapack exports pass the Stage 7A package/server gates;
- context-invalid operations fail during type checking;
- cardinality-changing operations are visible to analysis;
- nested execute plans lower correctly without leaking context;
- typed APIs and raw commands interoperate through explicit boundaries.

## Stage 7.5: Execution frames and static spatial semantics

Status: **complete and gated.**

Authoritative design and execution order:
[`stage-7-5-plan.md`](stage-7-5-plan.md) and
[`stage-7-5-todo.md`](stage-7-5-todo.md).

Stage 7.5 broadens the typed Minecraft programming model without selecting runtime
representations. It adds exact ordered `at`, `at_executor`, `positioned`, `rotated`,
`in`, `anchored`, and `align` transforms around the existing `as` scope; immutable
absolute/relative/local coordinate and rotation attributes; and two spatial methods
whose difference is source-visible:

```mdl
executor.teleport(~, ~10, ~); // current execution frame
executor.move_by(0, 10, 0);   // receiver's own frame
```

These arguments remain compiler-known operation attributes. They are not ordinary
locals, SSA values, function parameters, or storage carriers. This gives Stage 8
concrete pressure from ambient behavior, ordered frame transformation, frame-relative
commands, and receiver-relative recipes without prematurely designing a universal
value system.

Exit criteria:

- modifier order and exact frame provenance survive source, HIR, Core, preflight,
  structured target IR, and emitted datapacks;
- HIR and Core independently agree on ambient requirements for world/relative/local
  coordinate instances;
- every modifier and spatial operation consumes a retained Java 26.2 recipe;
- teleport and move-by remain semantically distinct and pass clientless vanilla
  differentials; and
- coordinates and entity identities still cannot escape into runtime values.

## Stage 8: Calling convention and representation selection

Status: **complete for the frozen scalar synchronous scope.**

Detailed design and execution order:
[`stage-8-plan.md`](stage-8-plan.md) and
[`stage-8-todo.md`](stage-8-todo.md).

Define how the existing scalar values cross synchronous function boundaries without
committing semantic Core to one Minecraft carrier. Stage 8 separates facts, mutable
storage, zero-or-more realizations of one SSA value, per-use requirements,
materializations, physical ABI modes, and activation lifetime.

The bounded implementation covers:

- compatibility migration of current `Bool`/`Int32` score homes;
- explicit score/frame/score realization transfers;
- measured serial reuse for synchronous many-context run bodies;
- recursive-SCC-only command-storage activation frames; and
- arguments, results, spills, cleanup, abnormal termination, and caller-bounded
  recursion depth.

Deferred conditions, structs, lists, entity references, runtime locations, strings,
and alternative aggregate layouts are follow-on representation clients, not Stage 8
exit requirements. This keeps the framework driven by executable scalar evidence.

Exit criteria:

- physical realization, ABI, and activation contracts are documented and verified;
- many-context run bodies and direct/mutual scalar recursion execute correctly;
- one semantic scalar can cross score -> frame -> score through explicit recipes;
- acyclic functions retain the static-score fast path; and
- unused frames, wrappers, transfers, and storage paths are eliminated.

## Stage 8.5: Pre-scheduler capability validation

Status: **complete through PS-3.** See the
[`Stage 8.5 documentation index`](pre-scheduler/README.md) and
[`roadmap`](pre-scheduler/roadmap.md).

Prove that MDL can express and validate a substantial synchronous program before
adding suspension as a new activation lifetime. The first capstone is a
fuel-bounded Brainfuck interpreter whose program is ultimately obtained from a
written book.

Stage 8.5 has three required substages:

1. PS-1 adds a bounded target-independent Core evaluator and a state-based scenario
   runner around the pinned vanilla server. It keeps compiler structure, pure
   semantics, and Minecraft runtime behavior as separate test authorities.
2. PS-2 adds the general synchronous capabilities required by the capstone:
   arithmetic/mutation, same-tick loops, aggregates, owned lists/stacks/zippers,
   runtime strings/parsing, typed books/items, bounded execution, and any narrowly
   justified typed Minecraft macro recipe.
3. PS-3 writes the interpreter as an ordinary MDL package and validates independent
   interpreter, parser, book, output, limit, cost, and cleanup layers. The actual
   connected-player fixture remains the explicit PS-2.0 deferral.

PS-2 explicitly separates core language semantics, public MDL standard-library
algorithms, sealed typed platform intrinsics, and compiler-private runtime support.
Known optimized mcfunction helpers are permitted only behind an exact intrinsic
contract with pinned-server conformance; opaque handwritten target code does not
become the source semantic authority.

This stage reprioritizes the synchronous portion of the previous loop/aggregate/
macro roadmap. It does not add persistent continuation frames, yields, a runtime job
queue, or transparent context survival across ticks. Dynamic programs use explicit
finite fuel and program-size bounds when a synchronous guarantee is required.

Exit criteria:

- structural fixture failures and vanilla state failures are concise and use the
  correct independent oracle;
- every capstone capability has frozen semantics, verified IR, physical contracts,
  four-policy differential evidence, and applicable pinned-server evidence;
- an ordinary MDL package parses and executes the frozen Brainfuck corpus;
- at least one supported written-book-to-output path runs on vanilla;
- fuel exhaustion, invalid input, deployment rejection, and abnormal Minecraft
  interruption remain distinct; and
- Stage 9 receives concrete live-state/context requirements without Stage 8.5
  pretending that work already survives a tick.

PS-4 and later validation programs are individually accepted using the Stage 8.5
template. They do not silently extend this stage's required exit after PS-3.

Completion evidence and the concrete state that Stage 9 must persist are recorded
in [`pre-scheduler/ps-3-handoff.md`](pre-scheduler/ps-3-handoff.md).

## Stage 9: Persistent continuations and static multi-tick scheduling

Add suspension and lower already-correct synchronous work according to bounds and
target budgets:

- static continuation phases across ticks;
- explicit live-state preservation at yield points.

Stage 8.5 owns ordinary same-tick loop semantics, baseline synchronous lowering,
fuel-bounded dynamic execution, and the first aggregate/list/string clients. Stage 9
may add scheduling-aware loop transformations, but it does not redefine those
semantics.

Scheduling consumes the Stage 7 context/fork/work contract. It cannot yield inside,
duplicate, replay, or prove a hard bound through an unsafe raw operation whose
behavior remains unknown.

The compiler must distinguish:

```text
hard command-sequence compatibility
hard per-expansion fork compatibility
soft compiler-selected work per tick
unbounded work that requires a contract or diagnostic
```

Exit criteria:

- a bounded workload is automatically partitioned across ticks;
- its completion and results are deterministic;
- yielded code does not accidentally assume that `@s` or another command context
  survives scheduling;
- configured bounds are checked using real-server limit tests;
- there is still no requirement for a dynamic runtime job queue.

## Stage 10: Macros and compile-time metaprogramming

Add typed language-level metaprogramming separately from Minecraft function macros.

Stage 8.5 may already contain a closed set of typed Minecraft macro recipes required
by its string/book clients. Stage 10 generalizes the serialization/interpolation
surface and adds language-level metaprogramming; it must preserve those earlier
semantic and target contracts rather than introducing a second incompatible macro
boundary.

Language macros should operate on typed or type-checkable structures, preserve
source locations, participate in hygiene/name resolution, and produce normal IR
that all verification and optimization passes can inspect.

Minecraft macros remain a backend choice for runtime-to-syntax substitution. Their
use requires typed serialization, escaping, range/path validation, and a cost model
that accounts for parsing and caching.

Typed raw-command interpolation is implemented here as a parsed sequence of literal
and typed placeholder segments with compiler-owned serializers. It is never ordinary
runtime string concatenation, and its validated target macro-line representation is
separate from the non-macro raw physical-line type.

Exit criteria:

- useful abstraction is possible without raw textual code generation;
- macro-generated code receives ordinary diagnostics and verification;
- a language macro can lower without emitting any Minecraft macro;
- backend macro use can be replaced by specialization or static dispatch.

## Stage 11: Cost-directed global optimization

Once semantics, effects, representations, and measurements are credible, add more
aggressive search:

- costed trace and basic-block layout;
- call-site inlining with bounded speculative cleanup;
- function specialization and cold outlining;
- region duplication under explicit growth budgets;
- representation selection across regions;
- loop specialization and bulk-operation recognition;
- context-prefix fusion;
- linear, balanced, and macro dispatch selection;
- profile-guided hot/cold decisions;
- equality saturation/e-graphs for pure, bounded rewrite domains.

E-graphs should initially operate on regions where equivalence and effects are easy
to prove, such as pure arithmetic, Boolean expressions, structured data construction,
or backend representation conversions. They should not be the first mechanism used
to optimize arbitrary effectful command CFGs.

Exit criteria:

- every chosen rewrite has a semantic proof obligation and a cost explanation;
- compiler resource limits prevent optimization search from exploding;
- benchmark improvements reproduce across multiple runs;
- optimization profiles such as `speed`, `size`, and `balanced` remain semantically
  equivalent.

## Stage 12: Stabilization and ecosystem

Only after the core pipeline is useful should we stabilize public boundaries:

- package/module conventions;
- dependency and resource namespacing;
- interoperability contracts with handwritten datapacks;
- versioned target profiles;
- incremental compilation and caching;
- language-server/editor features;
- documentation and standard library organization;
- compiler distribution and release automation;
- compatibility and migration policy;
- possible stable ABI for separately compiled packages.

Stage 7A already provides one complete in-memory package with logical module paths,
imports, and explicit datapack exports. Stage 12 owns the filesystem/package
conventions, dependency graph, serialized interfaces, stable resource spelling, and
cross-package compatibility layered over that whole-package compiler API.

Prebuilt compiler binaries are an optional release convenience, not an architectural
requirement. Building the compiler must not require LLVM or a custom Minecraft
installation.

## Immediate implementation tranche

Stages 1 through 8 are complete for their frozen scopes. Stage 8.5 is the immediate
implementation tranche. Its PS-1 test oracles precede PS-2 synchronous capability
expansion and the PS-3 Brainfuck capstone; see
[`pre-scheduler/README.md`](pre-scheduler/README.md). Stage 8's fixed-score
compatibility path, zero-or-more scalar realizations, exact sparse lifetimes, serial
many-context reuse, recursive-SCC-only typed spill frames, recovery contract,
physical recipe accounting, corruption/determinism gates, and clientless Java 26.2
proofs are complete. See [`stage-8-handoff.md`](stage-8-handoff.md). Stage 8.5 must
preserve those synchronous contracts while adding their first aggregate/list/string
clients. Stage 9 must
preserve Stage 7.5's context boundaries and must not suspend Stage 8's synchronous
tail frames across ticks.
Module ordering remains in-memory and logical; filesystem discovery, package
distribution, and stable cross-package ABI are still Stage 12 work. See
[`stage-8-plan.md`](stage-8-plan.md).

## Cross-cutting rules

These apply throughout the roadmap:

- semantics and diagnostics come before clever lowering;
- every IR level has a verifier and readable deterministic dump;
- raw commands are explicit conservative effect barriers by default;
- target-specific facts belong in a target profile;
- generated line count is never the sole performance metric;
- new runtime support is demand-driven and removable;
- compiler output must remain inspectable;
- measured vanilla behavior outranks assumptions about command performance;
- optimization passes must be individually testable; production exposes deliberate
  stable optimization levels rather than a pass registry, and every optimized layer
  remains wholly disableable through its `None` reference mode;
- deferred decisions should stay deferred until a milestone actually needs them.
