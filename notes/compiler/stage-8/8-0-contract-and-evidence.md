# Stage 8.0 — Activation Contract and Java Evidence

Status: **complete on the pinned Java 26.2 server**

## Purpose

Stage 8.0 determines what Minecraft actually guarantees about synchronous forked
function execution and recursive frame primitives. Later tranches may rely on only
facts pinned here or on explicitly conservative unknowns.

Its output is a measured target contract, a handwritten reference ABI, and decisions
about abnormal termination and external entry nesting. The serial-context and
recursive legality replacements now consume that evidence.

## Current repository boundary

Before Stage 8 the backend published only:

```text
ActivationContract::SingleContextNonReentrant
```

and `audit_legality` rejected:

- a run-scope prefix whose invocation upper bound exceeds one with
  `lower.unsupported-run-cardinality`; and
- every reachable recursive direct-call SCC with `lower.recursive-call-abi`.

The implemented replacement publishes `SynchronousSerial` or
`SynchronousRecursiveStack`, plus `Static` or `CallerBounded` activation depth. It
retains independent fork/sequence legality and uses `<namespace>:__mdl/load` as the
explicit abnormal-termination recovery entry.

Existing evidence already establishes:

- Java 26.2 counts command-sequence operations across nested calls;
- ordinary redirect fork legality is per checked execute expansion rather than a
  cumulative root token bucket;
- multiple root commands run sequentially on the server thread but receive separate
  sequence budgets;
- function calls inherit the Minecraft execution context;
- `return` exits its containing function, and `return run` has special fork/result
  behavior; and
- command-limit abortion can stop a command tree before later cleanup commands.

The completed handwritten matrix additionally establishes:

- an empty selector invokes no child and leaves the child counter at zero;
- `limit=1` invokes exactly one child;
- three outer contexts followed by a fresh three-entity inner `as` produce exactly
  nine completed inner children;
- ordinary `return 7` exits each child function without suppressing later sibling
  children or the parent continuation;
- a failing `execute if entity` command does not abort the remainder of its function;
- all three ordinary children fully unwind before the next child begins; and
- command-sequence abortion remains non-transactional and requires the published
  recovery entry before reuse.

The current clientless harness, exact bundle hash, log barriers, force-loaded chunks,
and sandbox-preservation behavior should be reused.

## Questions that need measurements

### E1 — Are ordinary fork children activation-serial?

For:

```mcfunction
execute as @e[tag=mdl8_child] run function mdl8:outer
```

does `outer(A)` and every function it calls complete before `outer(B)` starts, or can
queued commands from the two children interleave?

The compiler needs the stronger property:

```text
for every child context C:
  enter(C), all nested synchronous work(C), exit(C)
occur contiguously before enter(next child)
```

Selector iteration order itself does not need to be specified. Only activation
non-overlap matters.

### E2 — Do static homes remain isolated by complete redefinition?

A many-context body should write a distinct per-entity input to a shared scratch
score, call nested functions that use the same ABI inventory, then consume the
result. The observation must fail if one child's stale score leaks into another.

### E3 — Are tail-frame operations sufficient without macros?

Measure ordinary structured commands for:

- append an empty compound to a compiler-owned storage list;
- write/read `frames[-1].arguments.x` and `frames[-1].spills.x`;
- after a nested push, read the caller through `frames[-2]`;
- remove `frames[-1]`;
- copy every initial Stage 8 scalar direction: score→NBT int/byte and NBT→score; and
- preserve canonical Boolean `0|1`.

No dynamic NBT path or function macro should be needed.

### E4 — Which return forms reach a wrapper epilogue?

The reference shape is:

```mcfunction
# wrapper
function mdl8:body
function mdl8:epilogue
```

Test body fallthrough, ordinary `return`, branch helper returns, a nested function
return, `return run`, `return fail`, and a command that produces no result. The
compiler must know which forms return control to `wrapper` and which would bypass its
epilogue if selected incorrectly.

### E5 — What residue remains after command-limit abortion?

Lower the sequence gamerule far enough that execution stops:

1. before child push;
2. after push but before the worker;
3. inside recursive work;
4. after results but before pop; and
5. after pop.

Observe the exact stack list, depth sentinel, scratch scores, result scores, and
world writes. This is a failure/recovery experiment, not an attempt to make world
mutation transactional.

## Handwritten activation prototype

Use at least three tagged marker armor stands with distinct typed scoreboard IDs.
The outer function should:

1. increment a global trace ordinal;
2. append one frame;
3. store the current entity's ID into the top frame and shared parameter score;
4. call a nested function that pushes its own frame, reads the caller at `[-2]`, and
   validates the current executor/ID;
5. restore the shared score from the outer frame;
6. validate it still belongs to the current entity;
7. pop the outer frame; and
8. increment a completion score unique to that entity.

The trace should append compounds marking enter/nested-enter/nested-exit/exit with an
integer entity ID and ordinal. Dynamic values can be stored into newly appended
static-schema fields through `execute store`; no macro string substitution is
required.

Required observations:

- each entity completes exactly once;
- every nested-enter/nested-exit pair belongs to its enclosing entity;
- the stack depth returns to zero between outer child completions;
- no validation-failure marker is produced;
- an empty selector produces no frame activity;
- a fork-limit rejection produces no partial child frame; and
- repeating the root does not observe residue from a successful previous root.

## External entry and recovery alternatives

### Alternative A — Root-only resetting export

An exported root clears/replaces compiler-private activation storage before starting.
Internal calls to the same source function use a separate private worker entry.

Advantages:

- deterministic recovery from stale frames on the next root;
- simple implementation; and
- matches the current notion of a datapack export as an entry resource.

Costs:

- an external caller may not nest that root inside another live MDL activation; and
- the public execution contract must say so explicitly.

### Alternative B — Explicit recovery entry

Normal exports never clear live state. A separately exposed reset/recovery function
is required after a deployment-limit abort.

Advantages:

- does not silently destroy an outer activation; and
- keeps ordinary entries composable in principle.

Costs:

- a stale stack can corrupt later calls if the operator fails to recover; and
- detecting poison without extra state remains necessary.

### Alternative C — Epoch/keyed roots

Allocate or select a root-specific frame collection. Minecraft has no free unique
synchronous root ID, so generating a collision-free dynamic key requires machinery
outside Stage 8 or a caller-provided token. This is not the baseline.

Server evidence selected Alternative B for Stage 8. Successful roots are balanced;
abnormal command-sequence termination can retain a child frame. The generated
`<namespace>:__mdl/load` resource is the explicit recovery entry and resets the
private frame list before ordinary initialization. Operators must invoke it (or
reload the datapack) before another export after a known limit abort. Internal calls
never invoke recovery. Stage 12 can add a stable nestable external ABI and a richer
poison protocol if required.

## Recursion depth contract

The compiler cannot prove a general recursive depth from the current Core. Stage 8
therefore reports:

```text
ActivationDepthContract::CallerBounded
```

This is analogous to the existing caller responsibility for runtime-dependent
command work. It does not authorize:

- silently truncating recursion;
- returning a fabricated default value;
- adding an unspecified trap; or
- claiming the configured command-sequence limit is a semantic recursion bound.

A future source/runtime failure model may add checked depth. Stage 8 only ensures
that each depth has isolated storage until the target stops execution.

## Outputs

Stage 8.0 produces:

- a new ignored server test dedicated to activation order and frame primitives;
- checked-in handwritten datapack fixtures;
- exact observations and bundle hashes in the ambiguity/research notes;
- a selected external-entry recovery contract;
- local structured-command cost measurements for push/load/store/call/pop; and
- negative evidence that prevents unsafe static or stack selection.

## Exact local physical-recipe cost

`PhysicalPreflight` now records typed recipe inventory and publishes this exact
physical-only local sequence count:

```text
constant materializations
+ 2 * recursive spill fields       # score->frame and frame->score
+ 2 * recursive call occurrences   # frame append and pop
+ 1 if recursive runtime is present # recovery/reset initialization
```

Every listed physical recipe has zero local forks. The actual function call,
parameter/result score copies, CFG commands, and source run-scope redirect costs are
accounted by their existing plan/target analyses rather than double-counted here.
For the checked direct multi-result fixture this physical-only count is exactly
seven with zero forks under both Minecraft lowering policies.

## Gate

The substep is complete only when the reference ABI passes on the pinned official
server, every unresolved case is modeled conservatively, and the compiler legality
code remains unchanged.

## References

- [Mojang Java 1.20.3 function, fork, return, and command-limit changes](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
- [`command-limits-and-multi-tick.md`](../../mcfunction/command-limits-and-multi-tick.md)
- [`lists/callbacks-and-frames.md`](../../mcfunction/lists/callbacks-and-frames.md)
- [`macro-composition.md`](../../mcfunction/macro-composition.md)
- [`testing-harness.md`](../testing-harness.md)
- [`crates/mdl-test/tests/command_limits.rs`](../../../crates/mdl-test/tests/command_limits.rs)
