# Stage 7 Remainder Implementation Plan

Status: **implemented and verified; retained as the dependency/design record**

This document is the dependency-ordered implementation plan for the remainder of
Stage 7. The broader language design remains in
[`stage-7-plan.md`](stage-7-plan.md), and the executable checklist remains in
[`stage-7-todo.md`](stage-7-todo.md).

## Exit boundary

Stage 7 ends after one complete typed Minecraft command slice:

```mdl
export fn announce() {
    run.as(mc.entities(ArmorStand).with_tag("stage7").limit(1)) |speaker| {
        speaker.say("hello");
    }
}
```

The slice must prove all of these relationships without raw-text fallback:

```text
typed receiver + literal attribute
        -> normalized Say meaning
        -> verified Core semantic declaration
        -> Java 26.2 recipe selected during preflight
        -> structured Minecraft Say command
        -> execute-as context supplied by the enclosing run scope
        -> deterministic datapack and vanilla-server observation
```

The implementation first closed two pieces of evidence that were missing from the
original `.as` slice:

1. nested source-to-target context/provenance fixtures; and
2. the derived minimum `minecraft:max_command_forks` deployment requirement.

The typed `Say` slice then added independently recomputed Core ambient-context
requirements, made meaningful by the first exact executor-requiring Core operation.

The following are not Stage 7 exit blockers:

- query-to-`EntityRef` strengthening and a physical entity-reference handle;
- coordinates, relative/local frames, and additional execute modifiers;
- `store`, conditions, `on`, `summon`, and outcome-producing blocks;
- source syntax for caller-supplied ambient executor capabilities;
- `EntityRef.say` and other entity-handle convenience methods;
- a second Minecraft target, cross-version emulation, or broad command coverage.

Those designs remain useful, but implementing them before the first typed command
would make Stage 7 an open-ended command-library project.

## Codebase findings that shape the implementation

The current code already provides the right substrate:

- executor captures are compiler-known lexical capabilities, not ordinary HIR or
  Core values;
- string literals are rejected outside compiler-known typed contexts;
- HIR verification already walks blocks with an explicit execution context;
- Core external operations are program-owned, non-speculatable, and retained by all
  optimizers;
- run scopes already preserve query steps, modifier order, invocation bounds, and
  the outlined-body function edge;
- target legality already computes the strict fork requirement but discards it on
  success;
- source fixtures already cross HIR, Core, lowering, target IR, and emitted-pack
  boundaries under all four optimization-policy combinations; and
- the dedicated-server harness already proves behavior without a connected client.

The remaining design must build on those facts instead of replacing them with a
generic object/value system.

## Frozen architecture

### Three separate layers

Typed Minecraft operations use three deliberately separate layers:

```text
source method rule
    spelling + receiver constraint + normalization

MinecraftSemanticDescriptor
    normalized target-independent meaning

MinecraftRecipeId
    exact-target construction recipe
```

For the first slice:

```text
Executor<T where CommandExecutor>.say(MessageLiteral)
    --CurrentExecutor normalization-->
MinecraftSemanticKey::Say
    --JavaEditionTarget::V26_2-->
MinecraftRecipeId::Java26_2Say
```

`CurrentExecutor` is a source receiver rule, not a second command meaning. A future
`EntityRef.say` rule may normalize through an exact-one `run.as(entity)` scope and
then use the same `Say` key. It must not introduce a duplicate `EntitySay` semantic
operation.

### The descriptor is a source of facts, not a god object

Use a closed Rust enum plus exhaustive functions or small static tables. A semantic
descriptor owns only shared target-independent facts:

```text
MinecraftSemanticDescriptor {
    key
    semantic signature
    ambient-context rule
    world-state effect
    observable effect
    fork behavior
    work behavior
    command outcome
    validator kind
    documentation
}
```

Source lookup, HIR verification, Core verification, behavior inference, target
preflight, construction, and reporting independently consume those facts. They do
not copy the descriptor into every IR node, and the descriptor does not contain
target recipes, command costs, diagnostics, or function pointers that bypass
exhaustive matching. Irregular future families may have handwritten validators and
lowerers behind the same key.

Do not add an external schema, code generator, proc macro, plug-in registry, type
interner, or lowering DSL in this slice.

### Capability receiver and literal are not SSA values

The captured `Executor<ArmorStand>` is proof that the current execution frame has a
particular executor. `MessageLiteral` is a compile-time command argument. Neither
needs a scoreboard, UUID, NBT representation, or ordinary Core value.

A HIR semantic-operation occurrence stores the exact lexical proof used by the
checker together with closed static data and provenance:

```text
HirSayOperation {
    executor_kind: ArmorStand
    executor_proof
    message: MessageLiteral
    call_origin
    member_origin
    receiver_origin
    message_origin
}
```

HIR verification replays the enclosing execution context and requires that exact
proof to be current. HIR-to-Core then erases the lexical proof, just as a compiler
may erase type-checking evidence after it has established a normalized requirement.
The Core declaration retains the instantiated executor kind, message, and origins,
while its ordinary operand/result lists remain empty:

```text
CoreSayOperation {
    receiver_kind: ArmorStand
    message: MessageLiteral
    call_origin
    member_origin
    receiver_origin
    message_origin
}
```

This erasure is required by outlining. In the valid program below, the inner body is
a distinct Core function and cannot point directly at the outer lexical capture:

```mdl
run.as(query) |speaker| {
    run {
        speaker.say("hello");
    }
}
```

Core must prove the resulting ambient executor requirement through its own
call/run-scope analysis; it must not invent an SSA executor value or retain a stale
source-scope identity.

### Source captures are local; Core requirements are symbolic

Stage 7 source permits typed current-executor methods only through a lexical proof
established by a structured run scope. `ExecutionContext::function_entry()` may
therefore continue to mark a source-level lexical executor unavailable.

That does not mean an outlined Core function starts with no Minecraft executor.
Core functions need a recomputed symbolic `required_ambient_context`: a direct `Say`
requires its declared executor kind; an ordinary call propagates the callee's
requirement; a zero-modifier run preserves the outlined body's requirement; and
`.as(kind)` discharges a matching executor requirement while adding the query's frame
requirements. Unsafe text contributes `Unknown`. Compute the summaries over the
Core call graph to a deterministic SCC fixed point.

Own those results in one target-independent dense `CoreAmbientAnalysis`, indexed by
`FunctionId`. Compute a disposable instance for the unoptimized HIR/Core differential,
then recompute the retained instance after Core optimization. The retained analysis
travels beside `TargetPreflight` into the physical plan and supplies each
`LoweredFunction`; it is never copied into Core declarations.

For every unoptimized source function that maps directly to Core, compare the
recomputed Core requirement with the independently inferred HIR requirement. Publish
the optimized Core entry requirement on its `LoweredFunction` and function report.
Keep the pack-wide `ExecutionContract` limited to activation and command-limit
deployment facts. This is both a differential check and the semantic basis for nested
outlines.

Stage 7 still adds no source form that imports an entity-typed executor capture from
an arbitrary datapack caller. A future source feature may expose such a capability,
but the Core analysis cannot wait for that syntax because outlining already needs
symbolic requirements now.

### Effects and outcomes stay orthogonal

`say` is externally observable, but it does not read or mutate ordinary world state.
Do not misclassify it as a world write merely to keep it alive. Add a small closed
observable-effect component to `FunctionBehavior`:

```text
ObservableEffect = None | Observable | Unknown
```

The first facts are:

| Operation | World state | Observable | Fork | Work |
| --- | --- | --- | --- | --- |
| `.as` entity-query evaluation | read | none | query-owned | finite/unbounded from query |
| `Say` | none | observable | none | finite |
| unsafe command | unknown | unknown | unknown | unknown |

The native command result is independent of both the source result and the target
cost solver's control-flow outcome. `say` returns source `Void`; its target control
outcome is `Continue`; and the validated Java 26.2 command succeeds with result `1`.
Represent those as separate facts so a later typed `execute store` does not have to
recover discarded target semantics. Confirm the pinned-server bytecode observation
with a server differential before relying on it.

The planning baseline is already grounded in the pinned official artifact:
`versions/26.2/server-26.2.jar` has SHA-256
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`.
Bytecode inspection shows `MessageArgument.Message.parseText` compares Java
`StringReader.getRemainingLength()` against `256`, and `SayCommand` returns constant
`1` after resolving/broadcasting the message. Gate 2 converts those observations
into repository tests and Gate 9 confirms the behavior on the running server.

### Target selection is instance-aware and happens before construction

Recipe lookup accepts a verified semantic operation instance, not only its key:

```text
select_recipe(target, verified_operation) -> MinecraftRecipeId
```

Future legality can therefore depend on types and attributes without changing the
boundary. Stage 7 has one key, one target, and one supported pair, so the lookup is a
total exhaustive match. Do not add a fake target or fake unsupported key solely to
exercise an unreachable diagnostic. A typed unsupported-pair result becomes useful
when a real second key or target makes the product partial.

Target-dependent validation that can already fail, such as complete rendered
command length, still fails during preflight with exact occurrence provenance and
before any partial target program is exposed.

### Successful legality evidence is retained

First refactor the current legality audit into retained command-limit evidence:

```text
CommandLimitEvidence {
    command_limit_assumptions
    target_defaults
    minimum_max_command_forks
}
```

When semantic recipes exist, assemble the complete immutable preflight product:

```text
TargetPreflight {
    target
    command_limits: CommandLimitEvidence
    selected_semantic_recipes: EntityVec<ExternalOpId, Option<SelectedSemanticRecipe>>
}
```

Thread `CommandLimitEvidence` through the Stage 7C plan and public contract. Gate 6
then combines that evidence with recipe selections and threads `TargetPreflight`
through planning, verification, reporting, and construction. Plan verification
recomputes or structurally validates it; construction never silently reselects a
different recipe.
The dense recipe vector has exactly one slot per external declaration. Unreachable
and nonsemantic declarations hold `None`; every reachable typed semantic declaration
must hold one retained selection.

`TargetPreflight` has one owner. Gate 6 builds and independently verifies it before
resource allocation. Gate 7 borrows it while resources are allocated with the correct
direct/helper placement, then moves it into the immutable `LoweringPlan` together with
the retained `CoreAmbientAnalysis`. Construction and plan verification borrow the
plan-owned values; reports contain bounded projections rather than independent copies
that can drift.

Configured assumptions, target defaults, and derived minimums are distinct facts.
For an ordinary redirect prefix with proven upper bound `n`, Java 26.2 requires
`max_command_forks >= n + 1` because the guard is strict. Aggregate the maximum over
reachable scopes only. A pack with no ordinary redirect has derived minimum zero.

### Typed operations use structured direct placement

Unsafe raw text keeps its dedicated isolation helper because an opaque line may
contain `return`. A verified `Say` command has known continuation behavior and must
not inherit that raw-command tax.

Extend external instruction planning only as far as the first typed command needs:

```text
IsolatedUnsafe { helper }
OutlinedRunScope { helper }
DirectMinecraftRecipe { semantic_key, recipe_id, ... }
```

`Say` is emitted directly as structured target IR in the containing generated
function. The existing verified run-scope helper/outlined-body scheme may remain in
Stage 7; changing its placement is unrelated to proving the typed command boundary.
Inlining a one-command outlined body under `execute ... run` is a later optimization.

## Dependency-ordered implementation gates

Each gate ends in a green repository. Do not begin the next gate with verifier,
determinism, or public-contract failures left behind.

### Gate 1 — Close Stage 7C evidence

1. Add nested source fixtures covering:
   - a zero-modifier inner run inheriting the outer context;
   - an inner `.as(limit(1))` replacing the current executor;
   - exact ordering of two run scopes and their query refinements through HIR, Core,
     target IR, and pack output; and
   - verifier corruption of nested scope identities and context facts.
2. Make legality return `CommandLimitEvidence` rather than `()`.
3. Aggregate and retain `minimum_max_command_forks` across reachable run scopes.
4. Thread it through `LoweringPlan`, `LoweringDecisionReport`, `LoweringMap`, and
   `CommandLimitContract`.
5. Report configured assumptions, target defaults, and the derived minimum with
   unambiguous names in dumps and the CLI.
6. Add zero-modifier, one-prefix, repeated-prefix, nested-command, unreachable-scope,
   insufficient-assumption, determinism, corruption, and scale tests.
7. Print each exported source function's already-inferred HIR behavior in the CLI ABI
   report; the current lookup API alone is not a published report.

Gate: 7C is marked complete only after nested provenance and the successful
deployment requirement are externally inspectable.

### Gate 2 — Freeze the `Say` semantic contract

1. Add `MessageLiteral` as an owned validated compile-time attribute. For the first
   safe slice, reject an empty value, CR/LF and other control characters, and `@`.
   The last restriction deliberately excludes Minecraft selector interpolation until
   its world reads, permissions, failures, and rendered components are modeled.
2. Measure and test whitespace, quote, backslash, and Unicode preservation against
   the pinned server before freezing their accepted spelling. Rendering must be data,
   never command concatenation with a raw fallback. Until an escaping recipe is
   proven, the Java recipe must reject a rendered terminal backslash because the
   mcfunction loader treats it as physical line continuation.
3. Keep target-independent literal shape separate from the Java 26.2 recipe limit of
   256 Java UTF-16 code units and the complete rendered-command length check.
4. Add `ObservableEffect` to semantic function behavior and update joins, unsafe
   summaries, dumps, APIs, corruption tests, and scale tests.
5. Keep native success/result facts distinct from the existing target-cost
   `CommandOutcome::Continue` classification.
6. Record the measured Java 26.2 `say` success/result contract in
   [`semantic-ambiguities.md`](semantic-ambiguities.md).

Gate: the first operation's signature, context, effects, fork/work, outcome, and
literal safety are fully specified before method resolution exists.

### Gate 3 — Add the closed registry

1. Add `MinecraftSemanticKey::Say`.
2. Add a descriptor function/table with no default for any safety-relevant field.
3. Add a small source-method table mapping `Executor<T: CommandExecutor>.say` to
   `Say` with `CurrentExecutor` normalization.
4. Represent the executor-kind relation as a closed receiver/context rule rather
   than cloning one descriptor per entity kind.
5. Add duplicate source-key, missing descriptor, signature, behavior, and
   documentation completeness tests.

Gate: adding a semantic key or receiver rule forces exhaustive consumer/test updates.

### Gate 4 — Resolve the captured method into verified HIR

1. Classify semantic method calls before generic namespace-function resolution.
2. Require the receiver to be the active current executor capture with the exact
   proof and a kind satisfying `CommandExecutor`.
3. Type-check exactly one literal argument as `MessageLiteral`; do not add a runtime
   string type or general compile-time evaluator.
4. Store one HIR semantic operation occurrence with key, closed attributes,
   executor proof/kind, and member/receiver/argument/call origins.
5. Have HIR verification re-resolve the descriptor and replay the current context.
6. Have behavior inference consume the instantiated descriptor facts rather than a
   handwritten `Say` branch.
7. Diagnose unknown methods, wrong arity, nonliteral arguments, expression use of
   `Void`, stale/outer captures, and ordinary values used as executor receivers.

Gate: valid and invalid method programs are decided at the source boundary with no
target recipe knowledge.

### Gate 5 — Preserve normalized operations in Core

1. Add a program-owned semantic-operation identity and immutable declaration.
2. Add `ExternalSemanticBinding::MinecraftOperation(id)` with empty SSA operands and
   results for the first slice.
3. Lower HIR attributes and the instantiated receiver kind into Core, erasing the
   already-verified lexical executor proof.
4. Verify descriptor/attribute shape and every origin, then have the independent Core
   ambient-context analysis require the declared executor kind at the operation.
5. Implement that analysis over entry-reachable instructions using the Core CFG;
   dead operations and the generic function-edge hook are not sufficient semantic
   transfer. Ordinary calls propagate requirements, zero-modifier scopes preserve
   them, `.as` reverse-transfer discharges the matching executor requirement, and
   unsafe operations contribute `Unknown`. Use a deterministic iterative SCC solver.
6. Extend Core reverse-transfer and SCC tests for direct `Say`, ordinary calls,
   zero-modifier nested scopes, `.as` discharge, recursion, unsafe unknowns, and
   malformed receiver metadata. Cross-kind conflict tests wait for a second real
   `EntityKind`. Compare unoptimized HIR/Core requirements.
7. Add 20,000-function chain/fanout/cycle scale tests without recursive host traversal.
   The context solver accepts recursive Core even though the later fixed-slot legality
   gate rejects recursive lowering.
8. Represent the retained dense result as `CoreAmbientAnalysis` and independently
   recompute/verify it. Gate 7 moves it into the first correct `Say`-capable plan and
   projects it into `LoweredFunction`/function reports.
9. Update canonical printing, cloning/editing, function-reference enumeration,
   reachability, all optimizer matches, and corruption tests.
10. Keep the Core instruction `Unknown/Never/Opaque` until a later Core effect system
   explicitly exploits the richer semantic descriptor.

Gate: all Core optimization policies preserve the normalized operation, while Core
independently proves the ambient executor relationship required by outlining.

### Gate 6 — Select and retain the Java 26.2 recipe

1. Add `MinecraftRecipeId::Java26_2Say` in the target-lowering layer, not the
   semantic descriptor.
2. Combine the retained `CommandLimitEvidence` with a recipe selected for every
   reachable semantic operation to form `TargetPreflight`.
3. Retain selections by stable external-operation identity in the standalone
   `TargetPreflight`.
4. Add an independent preflight verifier that re-checks target, operation instance,
   reachability, dense-slot completeness, and selected recipe.
5. Reject all invalid target instances before resource allocation/construction can
   expose partial output.
6. Keep the one-key/one-target lookup total; defer unsupported-pair machinery until
   a real pair is unsupported.
7. Keep exact target/compiler facts, including the required conformance Java runtime
   version, in `TargetSpec`. Keep the resolved Java executable, pinned server JAR/hash,
   and measured evidence in the test harness rather than turning test-artifact
   identity into a compile-target field.

Gate: standalone preflight retains and independently verifies every target decision.
Physical planning and construction remain explicitly unsupported for `Say` until
Gate 7 consumes that preflight.

### Gate 7 — Add structured `Say` target IR and direct lowering

1. Add a typed `SayCommand`/`CommandKind::Say` carrying the validated message.
2. Update builder, verifier, depth walk, dump, renderer, command contract, effect
   census, syntax/local cost, global outcome solver, and every exhaustive match.
3. Model executor context read, a new known target `OUTPUT`/communication effect
   category, no local fork, one command of local work, control-flow continuation,
   and native success/result `1`.
4. Extend the target-local `CommandContract` with a closed native
   success/result/continuation fact, or add one equivalent authoritative local query
   consumed by both recipe reconciliation and the global solver. Do not duplicate a
   handwritten `Say` outcome assertion across layers.
   Recipe reconciliation checks native `Exact(1)`; the existing coarse solver consumes
   its explicit `Exact(1) -> NonZero` projection where a command result is observed,
   while ordinary control flow remains `CommandOutcome::Continue`. Do not widen the
   solver's domain merely for this command.
5. Extend assignment, resource allocation, plan assembly, the frozen
   `InstructionPlan`, plan verification/reporting/symbolic checks, and emission so
   `Say` is emitted directly and raw text remains isolated. Preflight must reach
   resource allocation so only unsafe and run-scope externals receive helpers.
6. Borrow the standalone `TargetPreflight` and `CoreAmbientAnalysis` during resource
   allocation, then move both into the first correct `Say`-capable `LoweringPlan`.
   Plan verification recomputes/validates them; reports expose projections only.
7. Publish optimized Core ambient requirements per `LoweredFunction` and function
   report, not in the pack-wide `ExecutionContract`.
8. Retain the current verified run-scope helper and its one outlined-body invocation;
   direct run-scope placement is not part of this slice.
9. Reconcile predicted semantic facts and recipe cost against the constructed
   command node and target cost analysis.
10. Assert typed `Say` never constructs `UnsafeRawCommand` or `TargetFragment`.

Gate: the emitted pack contains structured `say` under the existing `.as` scope and
no semantic command has crossed the unsafe boundary.

### Gate 8 — Reports and provenance

1. Extend HIR/Core/lowering/target dumps with semantic key and closed attributes.
2. Report recipe ID, receiver/context rule, local fork/work/outcome, direct/helper
   placement, generated command/function IDs, exact local cost, and occurrence
   provenance.
   Label HIR `source semantic requirement` separately from optimized-Core
   `generated entry requirement`; neither silently substitutes for the other.
3. Add post-construction evidence mapping each `(Core FunctionId, InstId)` semantic
   occurrence to its `(target function, CommandId)` outputs. Capture the IDs returned
   during construction; origins are not unique enough to reconstruct this relation.
4. Keep the preconstruction decision report limited to recipe, placement, and planned
   resource identities. Put actual target IDs in the successful post-construction
   map/report, without embedding them back into semantic HIR/Core.
5. Keep normal reports bounded and deterministic; no whole-program cross-product
   matrices.
6. Emit the supported source API/target matrix from registry and recipe data.

Gate: a user can explain exactly why `speaker.say` became its target commands.

### Gate 9 — Vertical, differential, and server proof

Add focused unit/corruption tests plus source fixtures for:

- valid captured `Executor<ArmorStand>.say`;
- omitted capture with no method use;
- stale outer capture under an inner `.as`;
- unknown method, wrong receiver, wrong arity, and nonliteral message;
- target-independent message rejection and target-length rejection;
- empty query skipping the body;
- unbounded query retaining its explicit fork and failing the fixed-slot gate;
- insufficient fork assumption pointing at the responsible modifier;
- no typed operation rendered as raw text;
- deterministic repeated builds and all four optimization-policy combinations;
- verifier mutations of keys, attributes, origins, semantic receiver/scope data,
  recipes, placement, and constructed target commands, plus corruption of retained
  `CoreAmbientAnalysis` that plan verification must reject; and
- an automated pinned `commands.json` assertion that Java 26.2 exposes
  `say -> message: minecraft:message`; this audits syntax coverage but is not semantic
  or outcome evidence.

The pinned Java 26.2 server gate uses one startup and no client:

1. summon one uniquely tagged and named armor stand in a force-loaded chunk;
2. invoke the compiled export;
3. assert the `say` log is attributed to that entity;
4. remove the entity, invoke again, and prove no second typed message occurs;
5. use a separate handwritten `execute store result ... run say ...` conformance
   command to assert the native result used by the descriptor—the compiled `Void`
   wrapper exposes its function outcome, not the nested `say` result; and
6. retain the existing unsafe-command execution/rejection regression.

Gate: source, HIR, Core, target IR, pack bytes, target-cost analysis, and the real
server agree on context, multiplicity, effects, outcome, and observable behavior.

### Gate 10 — Completion audit

Run and review:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Also run the pinned MSRV gate, ignored scale/differential suites, source and CLI
server gates, repeat-build determinism checks, and the official-server suite. Review
every new exhaustive match and corruption test independently. Update the compiler
README, roadmap, Stage 7 status, checklist, testing notes, and ambiguity ledger in
one final documentation pass.

Stage 7 is complete only when every gate above is green and no deferred extension is
still described as an exit blocker.

## Primary references

- [MLIR operation definition specification](https://mlir.llvm.org/docs/DefiningDialects/Operations/)
- [MLIR dialect conversion and legality](https://mlir.llvm.org/docs/DialectConversion/)
- [MLIR interfaces](https://mlir.llvm.org/docs/Interfaces/)
- [MLIR diagnostics](https://mlir.llvm.org/docs/Diagnostics/)
- [MLIR SPIR-V target environments](https://mlir.llvm.org/docs/Dialects/SPIR-V/)
- [Cranelift declarative instruction definitions](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/meta/src/shared/instructions.rs)
- [Cranelift ISLE language reference](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/isle/docs/language-reference.md)
- [GHC primitive-operation registry](https://gitlab.haskell.org/ghc/ghc/-/raw/master/compiler/GHC/Builtin/primops.txt.pp)
- [OCaml primitive effect/coeffect semantics](https://github.com/ocaml/ocaml/blob/trunk/middle_end/semantics_of_primitives.ml)
- [Zig builtin-function registry](https://github.com/ziglang/zig/blob/master/lib/std/zig/BuiltinFn.zig)
- [Zig AIR](https://github.com/ziglang/zig/blob/master/src/Air.zig)
- [Rust THIR](https://rustc-dev-guide.rust-lang.org/thir.html)
- [Rust MIR visitors](https://rustc-dev-guide.rust-lang.org/mir/visitor.html)
- [Mojang Brigadier command nodes](https://github.com/Mojang/brigadier/blob/master/src/main/java/com/mojang/brigadier/tree/CommandNode.java)
- [Minecraft Java Edition 26.2](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
