# Stage 8E — Hardening, Public Evidence, and Handoff

Status: **complete**

## Purpose

Stage 8E turns the preceding implementation into a trustworthy compiler boundary. It
adds no representation or source feature. It proves that scalar realizations,
serial activation, and recursive frames are complete, deterministic, inspectable,
and absent when unused.

## Required frozen products

Before 8E begins, successful lowering must own and verify:

- semantic inventory and target preflight;
- ambient/context and command-limit evidence;
- runtime demand and liveness;
- activation-overlap and recursive-SCC analysis;
- realization requirements and selected realizations;
- physical storage declarations;
- call ABI and activation plans;
- physical preflight recipes;
- generalized instruction/call/edge transfers;
- resource inventory and immutable lowering plan;
- constructed target program and command correlations; and
- emitted artifact, trace, execution analysis, and deployment contract.

No successful public output may refer to a temporary builder or mutable analysis
state.

## Public conceptual model

Public reports must keep these domains separate:

| Domain | Question answered |
| --- | --- |
| Semantic value | Which typed Core value is this? |
| Fact | Is it compile-time known/rematerializable? |
| Physical storage | Which score/frame location exists and why? |
| Realization | During what region is that value available there? |
| Use requirement | What physical class does this exact consumer accept? |
| Materialization | Which explicit recipe created another realization? |
| Function ABI | How do indexed parameters/results cross this boundary? |
| Activation discipline | May invocations reuse storage serially, or need frames? |
| Cost/deployment | What work, forks, depth, limits, and external discipline apply? |
| Physical correlation | Which target command implements this occurrence? |

Do not collapse this to “value X uses score Y” once values may also have frame
realizations.

## Public API shape

Extend owned read-only maps rather than exposing plan internals. Candidate queries:

```text
LoweringMap::function_activation(FunctionId)
LoweringMap::function_physical_abi(FunctionId)
LoweringMap::call_activation(FunctionId, InstId)
LoweringMap::value_realizations(FunctionId, ValueId)
LoweringMap::use_realization(FunctionId, InstId, operand_index)
LoweringMap::materialization(MaterializationId)
LoweringMap::recursive_frame(RecursiveSccId)
```

Exact names may differ. Requirements:

- typed IDs, no inferred allocation-order lookup;
- fallible foreign-ID access;
- stable semantic/physical distinction;
- exact target function/command placement when construction succeeded;
- no target command identity on preconstruction failure; and
- exports exclude private activation workers/wrappers.

The CLI should summarize public function activation/depth requirements and ordered
external ABI. Detailed internal realization output belongs behind an explicit
`explain`/dump option rather than flooding normal compile output.

## Explanation report

For each selected nontrivial choice show:

```text
value: function=3 value=12 type=Int32
use: call=instruction 17 argument 0
required: ActivationNbtI32
source realization: score home 8
materialization: Java26_2ScoreI32ToFrameInt
reason: recursive SCC 2 re-enters function 3; home 8 live/clobbered
placement: function 21 command 4
cost: one execute-store command; no fork
```

For serial many-context scopes show multiplicity and activation separately:

```text
invocations: 0..unbounded
activation: serial-static (Java 26.2 evidence E1)
fork requirement: target assumption >= N/unknown
frame cost: none
```

For rejected alternatives, report only candidates actually considered by the bounded
algorithm. Do not manufacture a global optimality claim.

## Unused-runtime elimination

Prove absence structurally and in emitted artifacts:

- no recursive SCC reachable → no frame storage schema, reset/recovery wrapper,
  push/pop bridge, or activation-depth contract;
- no many-context scope → no serial-activation report rows beyond defaults;
- constant folded away → no materialization or storage;
- dead call result → no result field/load;
- no live clobbered value → no spill field/store/restore;
- internal worker unreachable → no resource; and
- `None` nonrecursive compatibility fixtures retain current artifact shape.

The load function must not initialize empty runtime support “just in case.”

## Corruption audit

Every retained product needs at least one independent negative mutation. Cover:

### Realizations

- wrong semantic owner/type/storage;
- live region shortened before a use;
- overlapping storage lifetimes;
- missing use link;
- use linked to right-typed wrong value; and
- detached materialization.

### ABI/activation

- parameter/result reordering;
- wrong direct/frame mode;
- static mode on recursive edge;
- stack mode referencing another SCC schema;
- internal call routed through public reset entry; and
- caller-bounded depth falsely marked exact.

### Frames/transfers

- missing spill/result field;
- pop before load;
- restore/result alias conflict;
- unbalanced wrapper path;
- Boolean byte receives noncanonical score;
- wrong `[-1]`/`[-2]` path; and
- unplanned frame command in construction.

### Resources/correlations

- duplicate/path-colliding resource;
- missing worker/reset wrapper;
- target command moved/replaced;
- recipe/cost/effect mismatch; and
- public export accidentally names a private helper.

Each mutation should be rejected by the earliest layer that owns the invariant.

## Determinism matrix

Repeat builds while varying only nonsemantic factors:

- source module insertion order before canonical package validation;
- hash seeds/forced hash collisions where supported;
- repeated compiler invocation in one process;
- Core `None|Baseline`;
- Minecraft `None|Baseline`; and
- diagnostic/report rendering calls.

Compare:

- HIR and optimized/unoptimized Core dumps;
- semantic and physical preflights;
- activation/realization/ABI plans;
- assignment/transfers/resources/lowering plan;
- target IR/rendered functions;
- artifact bytes and trace;
- public maps and reports; and
- execution/deployment cost contracts.

Policy changes may alter legal physical optimization, but repeated builds of one
policy must be identical and all policies must be semantically equivalent.

## Scale gates

Retain existing 20,000-node tests and add Stage 8-specific shapes:

- 20,000 scalar values/uses with one realization each;
- one value with many uses without copying its realization list per use;
- 20,000-function acyclic call chain;
- wide call fanout;
- large recursive SCC and many small SCCs;
- many run scopes with bounded/unbounded multiplicity; and
- sparse spill sets over large function-local home inventories.

Track table/work counts, not wall-clock thresholds. Required structural bounds:

- no value×storage matrix;
- no function×function dense call matrix;
- no recursive host traversal;
- no plan cloning per verifier; and
- no quadratic report construction from repeated global scans.

## Fast test gate

Run:

```sh
cargo fmt --all --check
git diff --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
PATH="$HOME/.rustup/toolchains/1.85.0-aarch64-apple-darwin/bin:$PATH" \
  cargo check --workspace --all-targets
```

Also run focused generated-oracle, corruption, scale, exact golden, CLI, and
artifact-determinism suites.

## Official Java 26.2 gate

Use the pinned distribution bundle/hash and Java 25 without a client. Prefer a small
number of server lifecycles with independently attributable markers:

1. command-report audit for every new structured data/frame command parser;
2. handwritten activation-order/frame primitive suite from 8.0;
3. compiler-generated serial many-context four-policy differential;
4. compiler-generated direct/mutual recursion four-policy differential; and
5. deliberate limit-abort plus selected recovery/root-contract suite.

Assertions use world/score/storage state and log barriers, not sleeps or the mere
absence of a server exception. Preserve the sandbox on drift.

Retain Stage 7/7.5 official regressions in the combined gate.

## Documentation completion

Update:

- root README and compiler roadmap;
- Stage 8 overview/checklist/dossiers;
- representation, command-limit, frame, and execute-chain research;
- semantic ambiguity ledger;
- compiler/test harness README;
- CLI and public Rust API docs;
- official artifact hashes/reproduction commands; and
- Stage 8 handoff.

The handoff must state concrete evidence about:

- static serial reuse under forks;
- recursive frame costs and limitations;
- external root nesting/recovery;
- caller-bounded recursion depth;
- score/frame realization bridges; and
- which aggregate/value-client questions remain genuinely open.

## Follow-on plan boundary

Do not automatically begin structs or lists after the Stage 8 gate. First write a
separate client plan that decides source semantics before layout:

1. semantic immutable aggregate value versus writable place;
2. ordinary assignment/call copy semantics;
3. explicit `mut` copy-in/copy-out semantics;
4. ownership/escape/alias rules;
5. Core aggregate operations/types;
6. per-use scalarized/NBT realization requirements; and
7. observation/coherence barriers.

Deferred Boolean computation forms should receive a separate effect/context-
stability plan rather than being disguised as a storage representation.

## Gate

Stage 8 is complete only when every fast and official-server gate passes, public
reports distinguish all physical domains, unused runtime support is absent, and the
handoff contains no unresolved issue that invalidates scalar call correctness.

## References

- [Zig backend liveness/MIR generation](https://ziglang.org/devlog/2025/)
- [MLIR bufferization analysis/rewrite split](https://mlir.llvm.org/docs/Bufferization/)
- [Swift ownership SSA verification](https://forums.swift.org/t/sil-ownership-model-proposal-refreshed/16872)
- [`../testing-harness.md`](../testing-harness.md)
- [`../../../crates/mdl-test/README.md`](../../../crates/mdl-test/README.md)
- [`../stage-5-handoff.md`](../stage-5-handoff.md)
- [`../stage-7-5-handoff.md`](../stage-7-5-handoff.md)
