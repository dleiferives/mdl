# Stage 7 to Stage 8 Handoff

Status: **Stage 7 implementation boundary complete**

Stage 7 establishes one honest typed Minecraft vertical slice. It is intentionally
not a broad command library and does not choose runtime representations for future
values. Stage 7.5 and Stage 8 should build on the verified contracts below rather
than bypassing them with raw command templates or implicit executor assumptions.

## What Stage 7 now guarantees

The canonical program is:

```mdl
export fn announce() {
    run.as(mc.entities(ArmorStand).with_tag("stage7").limit(1)) |speaker| {
        speaker.say("hello");
    }
}
```

Its path is identity-preserving and independently checked:

```text
SourceExternalOpId
  -> ExternalOpId + Core FunctionId/InstId
  -> selected Java26_2Say recipe
  -> structured target Say
  -> target McFunctionId/CommandId
```

- The lexical capture proves the current `Executor<ArmorStand>` only within its
  exact run context. There is no implicit `self` and no fabricated executor SSA
  value.
- HIR behavior and Core ambient requirements are inferred independently. An `.as`
  scope discharges its body's executor requirement while retaining the frame
  requirements introduced by the query.
- Possibly-many execution requires an explicit run fork. Successful lowering
  publishes configured command-limit assumptions, target defaults, and the derived
  minimum `minecraft:max_command_forks` separately.
- `Say` is an observable, non-world-writing, nonforking, finite operation with
  source `Void`. Its Java 26.2 target command reads the executor, produces `OUTPUT`,
  has native result `Exact(1)`, and locally continues. These are distinct domains.
- Target preflight owns the validated recipe selection before resource allocation.
  Typed `Say` emits directly; unsafe text remains isolated in a dedicated helper and
  keeps unknown effects, context, outcome, forks, and transitive work.
- Recipe reconciliation compares the instantiated semantic descriptor, selected
  recipe projection, structured command contract, local cost, exact message,
  occurrence origin, and post-construction command identity.
- The supported source-method/target matrix is generated from the closed method
  registry and recipe table rather than maintained as a second list.

## Public inspection boundaries

Use the owned producer products instead of reconstructing relationships from dumps:

- `CheckedFrontendOutput` exposes source function/external identities and inferred
  source behavior.
- `SourceToCoreMap` exposes functions, general external declarations, and exact
  typed semantic occurrences.
- `CompilationOutput::source_semantic_command` composes a typed source occurrence to
  its generated `LoweredCommand` after defensively rechecking optimized Core.
- `LoweringMap` exposes generated function ABIs, per-function optimized ambient
  requirements, execution/deployment limits, and Core-instruction command
  correlations.
- `LoweringDecisionReport` retains recipe, placement, semantic/target contracts,
  physical cost, and exact call/member/receiver/message provenance. Successful
  output appends the actual target function/command correlation; preconstruction
  failure cannot claim command IDs that were never built.

Text dumps remain deterministic evidence and debugging tools, not hidden compiler
inputs.

## Conformance boundary

Fast tests cover all four Core/Minecraft optimization combinations, invalid source
forms, target-preflight failures, verifier mutations, deterministic rebuilding, and
20,000-function ambient-analysis chain/fanout/cycle shapes.

The opt-in Java 26.2 gates use the official distribution bundle and Java 25:

- Mojang's generated `commands.json` must expose
  `say -> message: minecraft:message`;
- a clientless dedicated server force-loads the test chunk and summons a uniquely
  named/tagged armor stand;
- all four compiled policies produce exactly one executor-attributed message;
- doubled internal spaces, quotes, a backslash, and Unicode are preserved;
- removing the entity proves empty-query skip; and
- a separate handwritten `execute store result ... run say ...` probe measures
  native result `1` without confusing it with the source function's `Void` result.

Local artifact paths and extracted libraries are not repository source. The pinned
bundle and extracted-payload hashes are recorded in
[`../../versions/README.md`](../../versions/README.md).

## Stage 7.5 completed bridge

Stage 7.5 completed the bounded bridge between this one-command proof and Stage 8. It adds
ordered execution-frame transforms, exact static spatial attributes, and distinct
frame-relative teleport/receiver-relative movement while keeping every new argument
compiler-known and non-first-class. Its design and checklist are:

- [`stage-7-5-plan.md`](stage-7-5-plan.md)
- [`stage-7-5-todo.md`](stage-7-5-todo.md)
- [`stage-7-5-handoff.md`](stage-7-5-handoff.md)

Conditions, stores, summon, persistent entity handles, runtime positions, and
reentrant/fork-safe call frames remain outside that bridge.

## Stage 8 starting boundary

Stage 8 owns the scalar calling-convention and physical-realization foundation. Its
first work is measured serial-fork and recursive-frame behavior, not a universal
value system. Aggregate layouts and persistent identities are separately planned
clients after values can cross ordinary calls and outlined run scopes safely.

Candidate questions for Stage 8 and its separately planned follow-on clients include:

- when a scalar stays compile-time, occupies a scoreboard home, or lives in NBT;
- how exact-one entity identity can be represented across context changes, absence,
  multiplicity, calls, and ticks;
- which aggregates split into scalar homes and which require structured NBT;
- how strings, text, lists, locations, and enums expose typed operations without
  assuming one physical carrier;
- whether a call is reentrant and what storage is caller-owned versus callee-owned;
  and
- which representation conversions are explicit source operations versus compiler
  recipes proven by cost and lifetime analysis.

Do not turn `EntityRef.say` into an implicit possibly-many fork. It remains future
sugar for an exact-one receiver establishing the same ambient `Say` operation.
Scheduling remains Stage 9, macros/interpolation Stage 10, aggressive semantic
optimization Stage 11, and stable package/distribution policy Stage 12.

## References

- [Stage 7 authoritative design](stage-7-plan.md)
- [Stage 7 audited remainder plan](stage-7-remainder-plan.md)
- [Stage 7 completed checklist](stage-7-todo.md)
- [Stage 7.5 execution-frame and spatial-semantics plan](stage-7-5-plan.md)
- [Stage 7.5 implementation checklist](stage-7-5-todo.md)
- [Stage 7.5 to Stage 8 handoff](stage-7-5-handoff.md)
- [Semantic ambiguity ledger](semantic-ambiguities.md)
- [Testing harness](testing-harness.md)
- [Typed execute-chain research](../mcfunction/execute-chains.md)
- [Minecraft Java Edition 26.2](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [MLIR operation interfaces](https://mlir.llvm.org/docs/Interfaces/)
- [Rust compiler query-system overview](https://rustc-dev-guide.rust-lang.org/query.html)
- [Zig language reference](https://ziglang.org/documentation/master/)
