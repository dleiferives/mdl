# Stage 8 Completion Audit

Status: **complete for the frozen scalar synchronous scope**

Stage 8 is complete for `Bool`/`Int32`, synchronous many-context run bodies with no
scalar boundary captures/results, the direct-score function ABI, and recursive-SCC
spill frames. This audit records what the implementation proves and what it does not
claim.

## Implemented products

The lowering pipeline now retains and independently verifies:

- semantic value facts separate from mutable physical storage;
- zero, one, or several realization occurrences per `ValueId`;
- exact sparse live segments and non-overlap for reused score storage;
- per-use accepted storage classes and selected realization identities;
- explicit constant-to-score materializations;
- indexed function/call ABI modes and activation disciplines;
- recursive call occurrences with exact clobber-intersect-live spill sets;
- typed `Bool` byte and `Int32` int activation fields;
- type-split physical recipe inventories and exact local sequence/fork counts;
- a caller-owned append/store/call/restore/result/pop protocol; and
- read-only dumps, statistics, ABI maps, activation/depth contracts, artifact traces,
  and final structured-target verification.

The mature fixed-score `InstructionPlan`, call, edge-transfer, and construction
tables remain a compatibility projection. They are not a second representation
selector: the physical verifier reconstructs their storage/use/ABI relationships,
and post-construction reconciliation checks the emitted target commands. Replacing
those score-shaped Rust types becomes necessary only when a later ABI is not
score-shaped.

## Correctness evidence

Fast evidence includes:

- 20,000-value sparse realization and liveness scale tests;
- exact multi-realization ownership and live-segment retention tests;
- corruption tests for facts, storage declarations, realization ownership/lifetime,
  use requirements, materializations, ABI modes, call transfers/activation, frame
  inventories, resources, plans, and constructed-command correlations;
- direct multiple-result recursion under both Minecraft lowering policies;
- direct and mutual source recursion, branches, early returns, dead-result demand,
  serial contexts, and recursion nested inside a run body under all four Core/target
  policy combinations;
- repeated full-product determinism comparisons; and
- structural proof that nonrecursive output contains no frame storage, reset, bridge,
  or pop support.

The data-driven source corpus compiles every valid and invalid fixture twice under
all four `Core None|Baseline` × `Minecraft None|Baseline` combinations. It compares
requested HIR/Core/lowering/target/pack observations, whole emitted packs, and
failure diagnostics.

Pinned clientless Java 26.2 evidence proves:

- zero, one, several, and nested selector contexts (including a 3×3 fork);
- complete child unwind before the next selected context;
- ordinary `return` stops only the current child/function;
- ordinary command failure does not abort the following function command;
- typed command-storage append, `[-1]`/`[-2]`, score/NBT bridges, and tail removal;
- direct and mutual compiler-generated recursion under all four policies;
- recursion inside every child of a compiler-generated many-context run body;
- caller-live spill restoration and multiple returned values; and
- command-limit interruption residue followed by explicit load/recovery cleanup.

No Minecraft client connection is required.

## Final gate record

The 2026-07-16 completion run passed:

- `cargo fmt --all --check` and `git diff --check`;
- `cargo test --workspace`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`;
- `cargo check --workspace --all-targets` on the installed Rust 1.85.0 toolchain;
- both Stage 8 focused structural/determinism suites; and
- the pinned official Java 26.2 activation and compiler-generated four-policy server
  suites using Java 25.

The Stage 4 `None` golden changed only to include reviewed physical activation
reasons and exact physical recipe totals, then passed cleanly after explicit
blessing.

## Selected contracts

- Repeated synchronous run children are `SerialStatic`; invocation multiplicity and
  fork cost remain separately reported.
- Only reachable recursive SCC members use `RecursiveStack`.
- Parameters/results remain in pinned score homes. Frames contain only exact caller
  values live across and clobbered by a recursive subtree.
- Recursion depth is `CallerBounded`. The compiler invents neither a silent bound nor
  a trap.
- Normal calls are well bracketed. Command-limit interruption is not transactional;
  callers must invoke `<namespace>:__mdl/load` or reload before another export after
  known abnormal termination.
- Typed scheduling/yield does not exist yet and therefore cannot suspend a live
  synchronous frame.

## Deliberate deferrals

The following are not missing scalar Stage 8 work:

- frame-field parameters/results, `[-2]` frame-to-frame forwarding, and generalized
  frame-source parallel copies;
- persistent continuation frames or scheduling across ticks;
- scalar captures/results across many-context run bodies and their reduction order;
- structs, lists, strings/text, floats/fixed point, runtime spatial values, persistent
  entity references, maps, and arbitrary references;
- tail-recursion elimination, macros/dynamic paths, global layout search, and a
  stable nested external ABI.

Those features change source semantics or activation lifetime and have their own
follow-on boundaries.
