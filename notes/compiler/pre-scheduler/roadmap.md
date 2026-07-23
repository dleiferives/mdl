# Stage 8.5 Roadmap

Status: **PS-1 through PS-5 complete; PS-11A–C (generic macro engine) landed; PS-12
(composable entity-NBT paths) complete; BE-1 (block-entity NBT reads) complete;
PS-13 (bot-driven test infrastructure) complete; PS-14 (`Player` entity kind)
complete; PS-15 (advancement-triggered events) complete; PS-16, PS-17 (player
interaction / chest-menu capstone) planned at the milestone level**

The macro/reference work has its own architecture of record —
[`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md) — and
its first executable slice, [`ps-11-macro-reference-crossing-plan.md`](ps-11-macro-reference-crossing-plan.md)
(Tier A: generic value-crossing engine and wired runtime bridge), landed in commit
`7654bc0`. PS-12, with its own architecture note
[`../entity-nbt-path-composability.md`](../entity-nbt-path-composability.md), plan
[`ps-12-entity-nbt-paths-plan.md`](ps-12-entity-nbt-paths-plan.md), and handoff
[`ps-12-handoff.md`](ps-12-handoff.md), fixed two bugs found in the landed engine and
replaced the one hardcoded intrinsic client
(`main_hand_written_book_literal_page_or_empty`, now fully deleted) with a general,
schema-typed, composable entity-NBT path expression. Tier B (first-class references /
`DataRef<T>`) and Tier C (all-command generalization) remain later PS milestones.

Block-entity NBT reads (BE-1, chest contents) landed as the entity-NBT model's second
receiver kind. [`ps-13-17-player-interaction-roadmap.md`](ps-13-17-player-interaction-roadmap.md)
sequences the next five milestones — bot-driven test infrastructure, `Player`, real
push-model events (advancements), block-entity writes, and a chest-menu capstone that
composes all of them — at the milestone level only; each gets its own plan/todo/handoff
when its turn comes.

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
PS-5 anonymous structs, multiple results, and destructuring
  |
  +-> structural multiple-result types over the existing struct ABI
  +-> inferred `:=` declarations and context-inferred `.{}` literals
  +-> pipe destructuring and compile-time positional indexing
  |
PS-13 through PS-17 player interaction / chest-menu capstone
  |
  +-> Azalea-backed bot test infrastructure (PS-13, complete)
  +-> Player entity kind (PS-14, complete)
  +-> advancement-triggered events, the push model (PS-15, complete)
  +-> block-entity NBT writes (PS-16)
  +-> chest-menu capstone, composition only (PS-17)
  |
Stage 9 persistent continuations and scheduling
```

## Completed PS-4 language slice

PS-4 adds closed fieldless enums, Zig-style exhaustive `switch`, and inclusive
integer range patterns through a complete source/HIR/Core/evaluator/Minecraft test
path. It is scheduled before Stage 9 by explicit work order, not because suspension
depends on enums. Its plan and checklist are:

- [`ps-4-enums-switch-ranges-plan.md`](ps-4-enums-switch-ranges-plan.md)
- [`ps-4-enums-switch-ranges-todo.md`](ps-4-enums-switch-ranges-todo.md)
- [`ps-4-handoff.md`](ps-4-handoff.md)

PS-4 deliberately uses one mechanical range/switch lowering, permits large
generated datapacks, and records rather than optimizes footprint. Dispatch trees,
macros, profile-guided selection, and `List<Enum>` remain measured follow-ups.

## Accepted PS-5 language slice

PS-5 adds structural anonymous struct types (named and positional), context-
inferred `.{}` literals, compile-time positional indexing, the `|targets| <= value;`
destructuring statement, and `:=` inferred declarations implementing S-003. It is
scheduled after PS-4 and before Stage 9 resumes by explicit work order. Its plan
and checklist are:

- [`ps-5-anon-structs-destructuring-plan.md`](ps-5-anon-structs-destructuring-plan.md)
- [`ps-5-anon-structs-destructuring-todo.md`](ps-5-anon-structs-destructuring-todo.md)

PS-5 deliberately adds no Core operation and no Minecraft recipe: anonymous structs
ride the existing scalarized struct ABI, which is also the substrate the deferred
S-004 `mut` parameter sugar will lower through. By-name destructuring, aggregate
equality, export-ABI anonymous structs, and `List<T>` generalization remain
explicit follow-ups.

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
