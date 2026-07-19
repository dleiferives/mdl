# Stage 8.5 Roadmap

Status: **PS-1 through PS-3 complete; PS-4 planned before Stage 9 resumes**

## Why the roadmap changes here

The previous roadmap placed loops together with static multi-tick scheduling in
Stage 9, general Minecraft macro work in Stage 10, and aggregate values in an
unassigned post-Stage-8 client. The Brainfuck capstone needs portions of all three
areas before it needs persistent scheduling.

Stage 8.5 therefore reprioritizes only the synchronous clients:

- same-tick control flow is separated from yielding control flow;
- aggregates and lists become immediate clients of Stage 8's realization model;
- strings and books receive typed semantics before backend representation choices;
- a narrow typed Minecraft macro facility may be pulled forward when a runtime
  value must enter command syntax; and
- scheduling retains ownership of persistence across ticks.

This is a dependency correction, not permission to implement later stages wholesale.

## Dependency spine

```text
PS-1 test oracles
  |
  +-> Core evaluator for the existing scalar subset
  +-> state-based vanilla scenario runner
  +-> four-policy semantic differential
  |
PS-2 capability expansion
  |
  +-> freeze Brainfuck and bounded-execution contracts
  +-> scalar arithmetic, mutation, and same-tick loops
  +-> fixed aggregates and source-visible state
  +-> owned lists, stacks, and two-list zippers
  +-> runtime strings and opcode parsing
  +-> typed books/items and holder access
  +-> finite-fuel synchronous execution
  |
PS-3 Brainfuck capstone
  |
  +-> pre-parsed opcode execution
  +-> runtime string parsing
  +-> written-book input
  +-> controlled exact-one holder proof; connected player remains deferred
  +-> output, failure, cost, and cleanup evidence
  |
PS-4 enums, switch, and inclusive range patterns
  |
  +-> authoritative EBNF and grammar-conformance cases
  +-> nominal fieldless enums and exhaustive scalar switches
  +-> one mechanical native score-range lowering
  |
Stage 9 persistent continuations and scheduling
```

## Accepted PS-4 language slice

PS-4 adds closed fieldless enums, Zig-style exhaustive `switch`, and inclusive
integer range patterns through a complete source/HIR/Core/evaluator/Minecraft test
path. It is scheduled before Stage 9 by explicit work order, not because suspension
depends on enums. Its plan and checklist are:

- [`ps-4-enums-switch-ranges-plan.md`](ps-4-enums-switch-ranges-plan.md)
- [`ps-4-enums-switch-ranges-todo.md`](ps-4-enums-switch-ranges-todo.md)

PS-4 deliberately uses one mechanical range/switch lowering, permits large
generated datapacks, and records rather than optimizes footprint. Dispatch trees,
macros, profile-guided selection, and `List<Enum>` remain measured follow-ups.

PS-2 is developed as vertical capability slices. It must not implement every
frontend feature first, then every Core feature, and only later discover that none
of them lower correctly. A slice reaches its applicable server or evaluator gate
before the next dependent slice treats it as established.

## Frozen PS-2 decisions

PS-2 froze:

- Brainfuck cell width and wrapping behavior;
- tape direction and growth behavior;
- byte input/output model;
- treatment of non-opcode book text;
- bracket-error behavior;
- book page concatenation semantics;
- program-length and execution-fuel limits;
- source value/copy/ownership semantics for aggregates and lists; and
- which compile modes reject a workload without a proven synchronous bound.

These are language/runtime semantics. They may influence syntax, but they cannot be
left for the emitter to decide.

## Stage 8.5 exit — achieved 2026-07-19

Stage 8.5 is complete after PS-3 when:

- the testing infrastructure gives concise structural and state-based failures;
- every required general capability has an owned semantic and physical contract;
- an ordinary MDL package implements the frozen Brainfuck contract without unsafe
  raw commands or compiler-only access to private storage;
- representative programs run equivalently through all four optimization policies;
- the pinned vanilla server executes the supported book-to-output path;
- synchronous completion is honestly bounded by declared program/fuel/target limits;
- limit exhaustion and invalid input have deterministic observable behavior; and
- the handoff identifies which live values and contexts Stage 9 must eventually
  persist across ticks.

Additional validation programs can be completed before or after the scheduling
design begins, but they do not retroactively make Stage 8.5 unfinishable.

The exact closure audit is [`ps-3-handoff.md`](ps-3-handoff.md). Runtime fuel gives
deterministic application termination but is not yet a static Minecraft command
bound; that distinction is retained in the Stage 9 input rather than hidden.

## Stage 9 after the split

Stage 9 should be restated around one new semantic event: suspension. It owns:

- legal yield points and atomic regions;
- persistent continuation representation;
- live-value materialization at suspension;
- explicit loss/reconstruction of Minecraft execution context;
- static work partitioning under soft per-tick budgets;
- completion, cancellation, and recovery across ticks; and
- eventual dynamic scheduling only if separately justified.

It may reuse Stage 8.5 loops and collections, but it must not reuse Stage 8's
synchronous recursive tail frames across a tick boundary.
