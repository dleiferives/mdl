# Stage 8C — Serial Many-Context Activation

Status: **complete for zero-parameter/result synchronous run bodies**

## Purpose

Stage 8C compiles synchronous run scopes whose entity-query modifiers may invoke the
outlined body more than once, without allocating a dynamic frame merely because
invocation multiplicity exceeds one.

The key distinction is:

```text
multiplicity = how many invocations occur
overlap      = whether two invocations' live physical state exists simultaneously
```

Command/fork cost depends on multiplicity. Static-storage safety depends on overlap.
The current compiler conservatively uses multiplicity as a proxy for overlap; Stage
8C replaces that proxy only after Stage 8.0 measures Java's execution order.

## Current repository boundary

`audit_legality` computes a prefix ledger for every Core run scope. It currently
finds the first prefix whose invocation bound is not at most one and emits:

```text
lower.unsupported-run-cardinality
```

The same audit separately computes the minimum required
`minecraft:max_command_forks` and compares it with configured target assumptions.
These are separate facts and must remain separate.

HIR/Core already represent:

- `InvocationBounds` and query cardinality;
- exact ordered run modifiers;
- outlined body function identity;
- context/fork/work/effect summaries;
- lexical executor capture rules; and
- source/Core/target modifier correlations.

Stage 8C does not redesign those layers.

## Required Java contract

Stage 8C may select static serial activation only if Stage 8.0 establishes:

1. one ordinary child context runs its complete nested synchronous function tree
   before the next child begins;
2. no command in a successfully completed child remains queued after that child's
   epilogue;
3. ordinary nested calls inherit the correct child execution frame;
4. empty matches run no body and define no body-local state;
5. fork-limit rejection runs no partial child body; and
6. return/failure forms used by compiler-generated bodies do not interleave later
   work across children.

If any property is unknown, keep the current rejection for the affected recipe.

## Activation overlap analysis

Add an analysis product independent of `InvocationBounds`:

```text
ActivationOverlap =
  NoInvocation
  | Serial
  | NestedDistinct
  | Reentrant
  | Unknown
```

For Stage 8C:

- ordinary function invocation is nested relative to its caller but uses a distinct
  function's static homes in an acyclic call graph;
- repeated child contexts of one verified ordinary execute redirect are `Serial`;
- recursive SCC edges remain `Reentrant` and rejected until 8D;
- unsafe/raw behavior never proves `Serial` for hidden callbacks or scheduling; and
- scheduled roots/continuations are outside this synchronous analysis.

The result should be a per-function/per-call/run-scope summary with provenance of the
edge or modifier that caused its classification.

Do not encode this as another bit inside `InvocationBounds`: a program can be
unbounded in count but serial in lifetime.

## Static reuse proof

For a run-scope body selected `SerialStatic`, prove:

- every physical read in one child is dominated by a definition in that child or by
  an intentionally inherited caller value;
- no body-local semantic value escapes after the body returns;
- function result slots consumed by the body are read before the child completes;
- recipe and edge temporaries are dead at body exit;
- nested acyclic call homes are restored/redefined under their existing fixed ABI;
- an inherited outer caller value that remains live after the entire run scope is not
  stored in a home clobbered by the child without an existing call/region transfer
  proof; and
- child-specific Minecraft context facts do not leak into later source code except
  through source-visible world effects.

The last item is already supported by structural run outlining: Minecraft execution
context is scoped to the execute/function invocation, not restored through synthetic
enter/leave Core instructions.

The implemented proof composes closed existing invariants rather than adding a
second liveness engine. `RunScopeDecl::is_well_formed` requires an internal body with
zero parameters and zero results, so no semantic scalar can enter or escape through
the scope boundary. Core SSA verification proves every body-local use is dominated;
the physical requirement verifier proves every retained read is reached by an exact
realization; call arguments/results and CFG/recipe temporaries have exact
define-before-use occurrence plans. The pinned server matrix proves child
activations are serial. Together these facts permit static-home reuse and reject the
only possible typed escape shape before lowering. Unsafe raw commands remain an
explicit unknown-effect boundary and cannot establish a hidden capture contract.

## Run-scope legality replacement

Replace the current condition:

```text
prefix invocation bound must be <= 1
```

with:

```text
every reachable prefix must have:
  valid target fork evidence
  and an activation plan compatible with its body/ABI
```

For a many-context serial scope:

- retain exact `InvocationBounds` in semantic/cost reports;
- retain minimum fork gamerule evidence;
- select `ActivationDiscipline::SerialStatic`;
- reuse ordinary score realizations and fixed call ABI; and
- add no activation NBT storage or push/pop commands.

The compiler should still reject a prefix whose fork bound violates configured hard
assumptions even if storage reuse is safe.

## Interaction with values and results

Stage 7/7.5 run bodies have zero semantic parameters/results at their structural
boundary and cannot capture ordinary outer scalar locals. This is valuable: each
child's scalar locals are internal to the outlined body.

Stage 8C should keep that restriction. Allowing arbitrary scalar capture or reduction
across many contexts requires explicit aggregation semantics:

- last child, first child, sum, list collection, or Minecraft native result are not
  interchangeable;
- selector iteration order is not a language-level ordering contract; and
- concurrent-looking source mutation through shared scores is not automatically
  deterministic.

Scalar run captures/results need a later language feature and must not sneak in as
part of removing the physical gate.

## Context and effect interaction

Each child inherits the exact ordered frame produced by the run modifiers. Static
score reuse does not mean Minecraft context is global. Existing HIR/Core context
replay and target modifier correlations remain authoritative.

World writes and output from several children are repeated source-visible effects.
The behavior/cost report must continue to show possible multiplicity; activation
seriality is not permission to mark work/effects `ExactlyOnce`.

## Diagnostics

Replace the blanket cardinality diagnostic with specific causes:

- unknown/unmeasured activation ordering for the selected target recipe;
- recursive/reentrant body requires Stage 8D frame planning;
- escaping scalar state across children has no aggregation contract;
- target fork assumption too small; or
- activation analysis resource limit.

Diagnostics should label both the multiplicity-producing modifier and the body/call
site whose storage would overlap.

## Public reporting

For every run scope publish separately:

- semantic invocation lower/upper bound;
- prefix fork expansion and required gamerule;
- activation overlap classification;
- selected activation discipline;
- body ABI/storage class;
- repeated effects/work; and
- target execute/function placement.

This prevents “serial” from being misread as “one execution” or “cheap.”

## Compiler-generated server fixture

Create source functions where several tagged armor stands each have a distinct score
input. The run body should:

1. capture the current executor;
2. read/copy its ID into an ordinary scalar function call;
3. execute nested branches and a second call with multiple results;
4. perform typed `say` or a distinguishable world/score effect;
5. consume a result after the nested call; and
6. leave an entity-specific final score.

Assertions:

- every matched entity gets its own expected result;
- no entity receives a previous child's value;
- empty selection produces no changes;
- exact-many selection respects the configured fork boundary;
- target contains no frame-stack storage commands; and
- all four Core/Minecraft optimization policies agree.

Use a handwritten parallel oracle in the same server lifecycle where practical.

## Fast tests

- Zero, at-most-one, bounded-many, and unbounded query cardinalities.
- Repeated/nested `as`/`at` prefixes and their exact fork products.
- Acyclic nested calls with parameters/results.
- Rejected recursive body before 8D.
- Rejected scalar escape/capture forms.
- Corrupted overlap classification, wrong modifier provenance, and detached body ABI.
- 20,000 run-scope/call edges with iterative overlap propagation.
- Deterministic reports under module/source allocation permutations.

## Gate

Stage 8C completes when many-context source bodies run correctly with the unchanged
static-score ABI, no NBT frame commands are emitted for multiplicity alone, fork and
work accounting remain conservative, and recursion is still rejected pending 8D.

## References

- [Mojang function/fork semantics](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
- [`execute-chains.md`](../../mcfunction/execute-chains.md)
- [`command-limits-and-multi-tick.md`](../../mcfunction/command-limits-and-multi-tick.md)
- [`../stage-7-5-plan.md`](../stage-7-5-plan.md)
- [`crates/mdl-compiler/src/lower/minecraft/audit.rs`](../../../crates/mdl-compiler/src/lower/minecraft/audit.rs)
- [`crates/mdl-compiler/src/ir/core/run_scope.rs`](../../../crates/mdl-compiler/src/ir/core/run_scope.rs)
