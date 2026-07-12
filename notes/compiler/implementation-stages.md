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
| 7 | Typed Minecraft context, entities, selectors, and command APIs |
| 8 | Calling convention and multiple runtime representations |
| 9 | Loops, bounded work analysis, and static multi-tick scheduling |
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

Construct programs directly with the Rust IR builder and compile them all the way to
Minecraft.

The intentionally small feature set is:

```text
Int
Bool
constants
scoreboard-backed mutable state
comparison
function call
if/else
return
raw command
```

At least one test must reproduce the condition-mutation trap from the branch
research and prove that the safe dispatcher runs exactly one arm.

The same semantic branch should be lowerable through explicitly selected policies:

```text
return dispatcher
snapshotted Boolean
dual guards when stability is proven
```

Exit criteria:

- one nontrivial CFG compiles and executes correctly on vanilla 26.2;
- alternative legal lowerings produce equivalent observable results;
- invalid unsafe dual evaluation is rejected or not selected;
- the compiler can dump every IR level and the final generated datapack;
- failures preserve the disposable test world and logs when requested.

This is the first real compiler milestone. It deliberately has no user-facing source
language yet.

## Stage 5: Baseline optimization and cost instrumentation

Add conventional, auditable optimizations before advanced search techniques:

1. constant folding and propagation;
2. unreachable-block elimination;
3. dead-instruction elimination using effects;
4. Boolean and branch simplification;
5. straight-line block fusion;
6. condition stability analysis;
7. phi/block-argument coalescing;
8. local common-subexpression elimination for pure operations;
9. small-function inlining and cold outlining;
10. Minecraft lowering selection.

Each pass should have verification before/after in debug and test configurations,
standalone snapshots, and semantic integration tests.

Cost instrumentation records multiple dimensions rather than only line count:

- command contexts and execute stages;
- function invocations;
- selector forks and cardinality bounds;
- scoreboard and NBT operations;
- macro invocations and argument locality;
- generated functions, lines, bytes, and reload time;
- measured tick/wall time where the harness can obtain reliable samples.

Exit criteria:

- the compiler can explain why it chose a branch lowering;
- optimization preserves differential test results;
- command-limit estimates are checked against boundary tests;
- no wall-time coefficient is treated as authoritative without repeatable evidence.

## Stage 6: Minimal typed source language

Add the smallest frontend that makes the compiler usable by a person:

- files, modules, imports, and names;
- functions and local bindings;
- explicit core types;
- expressions, calls, `if`, and return;
- a deliberately small mutation model;
- raw-command escape hatch with typed interpolation;
- source spans through every IR level;
- diagnostics with primary and supporting labels.

Surface syntax should be judged by how well it exposes types, context, effects, and
lowering when requested—not by how quickly a large grammar can be implemented.

Exit criteria:

```text
source file -> parse -> type check -> lower -> optimize -> datapack
            -> vanilla execution -> expected assertion
```

This is the first minimal user-facing compiler.

## Stage 7: Typed Minecraft programming model

Introduce the types and APIs that make Minecraft commands pleasant and safe:

- `Entity<T>` and `Player`;
- selectors with known or bounded cardinality;
- executor, position, rotation, dimension, and anchor context;
- locations and coordinate spaces;
- structured `Text` distinct from runtime `String`;
- typed scoreboard, storage, NBT, block, item, and predicate access;
- method/context forms such as `player.say(text)`;
- command success, command result, absence, and failure as explicit semantics.

The compiler should be able to type-check an expression in the spirit of:

```text
prison.cells[input.id].user.say("hello")
```

Dynamic indexing may still use a simple initial lowering. Optimization comes after
the semantics are stable.

Exit criteria:

- context-invalid operations fail during type checking;
- cardinality-changing operations are visible to analysis;
- nested context operations lower correctly and can share/fuse prefixes;
- typed APIs and raw commands interoperate through explicit boundaries.

## Stage 8: Calling convention and representation selection

Define how values cross function boundaries and how one source type may use several
Minecraft representations.

Research and implement candidates for:

- constants, scoreboard values, and NBT numeric values;
- deferred conditions and normalized Boolean scores;
- entity values as active context, selectors, UUIDs, or handles;
- structs split across scores versus stored as NBT;
- fixed and dynamic lists;
- enums and interned strings;
- locations and structured text;
- arguments, returns, temporaries, recursion, and reentrancy.

Representation choice belongs to an analysis/lowering pass. The frontend should not
commit every `Int`, struct, or list to one physical form.

Exit criteria:

- calling conventions are documented and verified;
- alternative representations can be compared without changing source semantics;
- conversion/bridging costs are explicit;
- repeated dynamic access can be cached or specialized when legal;
- unused runtime support is eliminated.

## Stage 9: Loops and static multi-tick scheduling

Add structured loops and lower them according to bounds and target budgets:

- constant folding and complete unrolling for tiny known loops;
- partial unrolling;
- recursive same-tick functions;
- selector/native bulk transformations;
- bounded dynamic iteration;
- static continuation phases across ticks;
- explicit live-state preservation at yield points.

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

Language macros should operate on typed or type-checkable structures, preserve
source locations, participate in hygiene/name resolution, and produce normal IR
that all verification and optimization passes can inspect.

Minecraft macros remain a backend choice for runtime-to-syntax substitution. Their
use requires typed serialization, escaping, range/path validation, and a cost model
that accounts for parsing and caching.

Exit criteria:

- useful abstraction is possible without raw textual code generation;
- macro-generated code receives ordinary diagnostics and verification;
- a language macro can lower without emitting any Minecraft macro;
- backend macro use can be replaced by specialization or static dispatch.

## Stage 11: Cost-directed global optimization

Once semantics, effects, representations, and measurements are credible, add more
aggressive search:

- costed trace and basic-block layout;
- representation selection across regions;
- loop specialization and bulk-operation recognition;
- context-prefix fusion;
- linear, balanced, and macro dispatch selection;
- profile-guided hot/cold decisions;
- interprocedural specialization;
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

Prebuilt compiler binaries are an optional release convenience, not an architectural
requirement. Building the compiler must not require LLVM or a custom Minecraft
installation.

## Immediate implementation tranche

The next engineering tranche is Stages 1 through 4:

```text
workspace + server harness
        -> verified SSA substrate
        -> Minecraft IR and emitter
        -> one real end-to-end branch program
```

We should resist expanding the surface language until this path is reliable. Once it
exists, every backend research result can become an alternative lowering plus a
real-server regression test.

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
- optimization passes must be individually testable and disableable;
- deferred decisions should stay deferred until a milestone actually needs them.
