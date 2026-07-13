# Stage 5 Core Pass Plans

Status: **Implemented and integrated through Stage 5D; fast gates complete**

These notes refine the Core-mutating portion of the
[Stage 5 architecture](../stage-5-baseline-optimization-plan.md). They are the
reviewed contracts and implementation records for each pass:

0. [Core optimizer framework](framework.md)
1. [Cheap canonicalization](canonicalize.md)
2. [Sparse conditional constant propagation](sccp.md)
3. [Dead instruction elimination](dce.md)
4. [Straight-line jump-region fusion](fusion.md)
5. [Dominance-scoped local CSE](cse.md)

The production pipeline is a closed internal sequence of stable `CorePipelineStep`
IDs mapped to `CorePassKind` implementations. Repeated cleanup therefore has distinct
report/bisect identities without duplicate algorithms. These files do not define
public plugins, a rewrite DSL, or independently configurable production passes.

## Shared contract

Every pass:

- receives one verified function body and immutable access to program signatures;
- returns a typed `PassOutcome` with `changed`, completion, and fixed-name counters;
- mutates only through the reviewed `FunctionEditor` batch operations;
- uses its documented deterministic semantic order for traversal/worklists and only
  authoritative stable IDs for tie-breaking; raw block layout is not assumed to be
  definition-before-use;
- applies no target, scoreboard, command-cost, or Minecraft knowledge;
- emits detailed remarks only through the runner-owned filtered/capped sink;
- leaves a verified body after every successful batch; and
- never exposes partial Core if an invariant failure aborts the owned optimizer.

Pass limits are not interchangeable. Canonicalization/CSE/fusion/DCE may retain
individually proven edits when their local work limit is reached, but their dense
fact-table preflight returns unchanged before allocation. SCCP must discard an
incomplete solution. Required verification or checked entity/size failure aborts
compilation; system allocator OOM follows Rust's process policy.

## Pipeline ownership

Run all local passes for one stable `FunctionId` before moving to the next function:

```text
canonicalize
SCCP solve -> apply constants/branches/unreachable blocks
DCE
fuse maximal jump regions
local CSE
canonicalize + DCE once if fusion/CSE changed
```

The repeated cleanup is explicit and occurs at most once. Lowering accepts valid Core
without any pass, so none of these transforms is a correctness prerequisite.

## Review gate for a pass

Before a pass or rule is added or materially changed, its note must settle:

- exact semantic eligibility and non-goals;
- analysis tables and authoritative traversal order;
- mutation batch and provenance behavior;
- asymptotic target and typed limit fallback;
- positive, non-match, corruption, determinism, differential, and scale tests; and
- primary sources, with our design inferences clearly separated from upstream facts.
