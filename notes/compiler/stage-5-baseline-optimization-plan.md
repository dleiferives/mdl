# Stage 5: Baseline Optimization and Cost Instrumentation

Status: **Complete — reviewed revision 13; Stages 5A–5H implemented and gated**

Execution checklist: [`stage-5-todo.md`](stage-5-todo.md)

Core pass plans: [`stage-5-passes/README.md`](stage-5-passes/README.md)

Stage 6 boundary: [`stage-5-handoff.md`](stage-5-handoff.md)

Stage 4 established a correct and deterministic path from verified Core SSA to a
vanilla-executed datapack. Stage 5 reduces runtime work and generated structure
without weakening that path's proof boundaries.

The reviewed pipeline is:

```text
verified Core
    -> target-independent Core simplification
    -> Minecraft semantic inventory
    -> target-legality and nonrecursion gate
    -> runtime demand
    -> [Stage 5F] sparse liveness
    -> typed home assignment, conservative copy coalescing, and exact instruction plans
    -> frozen edge transfers
    -> [Stage 5G] target control recipes and placement-aware resources
    -> frozen verified lowering plan
    -> structured Minecraft IR
    -> explicit read-only local-cost/per-root-bound analysis
    -> deterministic datapack + exact artifact footprint
    -> optional vanilla measurements
```

This revision deliberately separates concerns that the first draft conflated:

- Core pass remarks are not Minecraft lowering decisions;
- exact artifact footprint is not dynamic invocation cost;
- physical coalescing is not Core block-parameter deletion;
- branch encoding cannot be selected independently of target function layout;
- expected pattern failure does not require transactional rollback of a whole pass;
- aggregate statistics are not an unbounded stream of per-entity remarks; and
- the absence of a finite whole-invocation bound does not erase exact local costs;
- an incomplete optimistic analysis is not a safe partially complete analysis; and
- target recipe selection does not feed back into storage reasoning during Stage 5.

## Outcome

Stage 5 should produce:

- a verified, semantically equivalent optimized Core program;
- bounded deterministic Core transformation statistics and optional detailed
  correspondence;
- a verified Minecraft plan with fewer unnecessary homes, copies, helpers, calls,
  and functions;
- a deterministic explanation for every Minecraft control-layout choice;
- exact generated footprint metrics and conservative dynamic execution bounds;
- an unchanged always-correct Stage 4 lowering mode for differential testing; and
- target/JVM-specific measurement records that are never mistaken for semantics.

Stage 5 does not add a source language, scheduler, recursive runtime stack, raw Core
commands, generic dialect framework, e-graphs, automatic PGO, or stable external ABI.

## Architectural boundaries

### Core optimizer

Core transformations may use only Core types, effects, SSA, CFG, dominance, use
information, and internal-call semantics. They must not know scoreboard names,
Minecraft resources, command limits, or target encodings.

Optimization remains optional. `lower_to_minecraft` must continue to accept any
verified, Stage-4-legal Core program; lowering correctness must not depend on a
canonicalizer having run. This follows MLIR's rule that canonicalization is
best-effort and must not be required for pipeline correctness.

Use the semantic queries Core already owns rather than duplicating pass-local truth:

- `effects() == Pure` proves absence of observable resource effects;
- `speculation() == Always` proves evaluation is total/terminating and permits
  introduction or hoisting outside the original control dependence;
- the conjunction of those two is the conservative Stage 5
  `is_trivially_discardable()` rule for DCE; and
- `result_equivalence() == Structural`, together with `Pure + Always`, permits the
  conservative Stage 5 optimizer to value-number and erase deterministic equivalent
  evaluations without treating calls as CSE candidates; and
- `operand_symmetry() == CommutativePair` declares exactly which binary operand pair
  canonicalization and CSE may reorder (both additions and `I32Compare(Eq | Ne)`).

These are deliberately separate questions, following MLIR's separation of effects
from conditional speculation. `ResultEquivalence` and `OperandSymmetry` are closed
`ir::core` semantic enums,
not a CSE key: `opt::core::cse` owns `CoreExpressionKey` construction from the
operation payload, canonical operands, and result contract. This preserves the
dependency direction from optimizer policy to IR semantics. A future operation must
update the exhaustive semantic queries and the optimizer's exhaustive key builder
before it can be value-numbered. Stage 5 does not add resource-level alias effects
merely because MLIR supports them; Core currently has only total pure scalar
operations and unknown internal calls.

### Minecraft planner

The private planner owns target-only decisions:

- score homes and ABI pinning;
- remaining parallel copies;
- physical read/write footprints;
- block placement into generated functions;
- helper and resource inventory;
- legal control recipes; and
- target-limit accounting.

Stage 3 remains the selected structured target representation. It does not acquire a
mutable optimization editor or Core semantics.

### Reporting

Stage 5 has five distinct report domains:

```text
CoreOptimizationReport
    fixed per-pipeline-step aggregate outcomes and bounded structural statistics

LoweringDecisionReport
    complete chosen physical mapping and selected recipes

TargetExecutionCostReport
    structured-command local cost and conservative per-root bounds

ArtifactFootprintReport
    exact emitted files, bytes, physical lines, and trace size

MeasurementRecord
    explicitly nondeterministic target/JVM/fixture samples and protocol
```

One final compilation report may aggregate references to these records later, but
their data models must not be collapsed. Deterministic report dumps never contain
wall-clock durations. In particular, a Core pass cannot honestly claim an exact
Minecraft cost delta without lowering both versions, which is too expensive and
would violate the layer boundary.

Detailed optimization feedback is a separate, explicitly requested diagnostic
stream. LLVM independently enables applied, missed, and analysis remarks and permits
pass filters and hotness thresholds; GHC similarly separates compact simplifier
statistics from its very verbose trace/all-considered-inlining dumps. Stage 5 follows
that shape for Core rewrites:

```text
CoreRemarkPolicy {
  kinds: None | Applied | AppliedAndMissed | All
  optional pass/function/reason filters
  deterministic per-compilation record_limit
}

CoreRemarkStream {
  ordered records up to record_limit
  exact-or-saturated StatisticCount by closed kind and reason
  exact-or-saturated omitted count and deterministic truncation marker
}
```

Normal Core compilation disables this stream; tests can request exact filtered
remarks and developer tooling can request matching kinds with an explicit limit. An
expected pattern non-match is not a missed-optimization event. `Missed` means that a
concrete candidate was formed and rejected for a stable reason, or that a budget
prevented known remaining work. This avoids turning every instruction that fails
every canonicalization pattern into diagnostics.

Lowering is deliberately simpler in Stage 5: the closed selector considers one
terminal-call alternative per reachable branch arm, and `LoweringDecisionReport`
records exactly one selected-or-retained decision with a stable reason for each arm.
That report is already linear, so a hypothetical `LoweringRemarkPolicy` would add an
unused abstraction rather than bound real growth. Add an opt-in lowering remark
stream only if a later stage introduces multi-candidate search whose rejected detail
would otherwise outgrow semantic control sites. Timing remains separate ephemeral
instrumentation.

Until Stage 6 owns a top-level compilation façade, reports follow their producer:

- `optimize_core` consumes one verified-or-verifiable `CoreProgram` and returns
  `CoreOptimizationOutput { program, report }` without cloning the program;
- `LoweringOutput` owns the read-only lowering-decision report beside the verified
  Minecraft program and ABI map;
- explicit target-execution analysis borrows a verified target and returns its owned
  `TargetExecutionCostReport` without mutating or taking ownership of that target;
- `EmissionOutput` owns `ArtifactFootprintReport` beside the exact pack and trace; and
- the server/benchmark harness owns `MeasurementRecord`.

The consumed optimizer API makes the existing pass-runner failure rule unambiguous:
on internal failure the possibly invalid Core unit is dropped inside
`optimize_core`, and `CoreOptimizationFailure` exposes only the phase/pass-aware
bundle and deterministic byte-capped, malformed-safe textual snapshots. Each
snapshot records truncation and omitted bytes; normal failure construction cannot
retain a second complete rendering of a large program. It never returns an ambiguous
`&mut CoreProgram` that callers must remember to discard. A private in-place runner
remains useful for pass tests and orchestration; it is not the safe compilation
boundary. Exact omitted-byte accounting streams over the full diagnostic dump, so the
cap bounds the retained UTF-8 text prefix rather than total failure memory or
failure-time traversal. Dump construction uses linear attachment-bit scratch, and
accumulated verifier findings are separately proportional to the invalid input. `None` performs
one verification and returns the same owned program. No partially constructed
Minecraft program is returned by later phases.

## Rust module and ownership shape

Keep representation mechanisms, concrete optimization policy, lowering decisions,
execution analysis, and emission accounting in dependency order:

```text
ir/core/
    representation, verification, reusable semantic queries, FunctionEditor

opt/core/
    owned API, closed runner/pipeline, options/reports, canonicalize, SCCP, DCE,
    fusion, CSE

lower/minecraft/plan/
    inventory/legality/demand, instruction/home assignment, transfers/resources,
    liveness/coalescing, effects/placement/recipes, verifier, chosen report

analysis/minecraft/
    structured-command steps, execution graph/SCCs, root summaries

datapack/footprint.rs
    post-emission ArtifactFootprintReport
```

Concrete optimizers must not live in `ir::core`: otherwise the IR substrate becomes
the policy layer and future pipelines cannot reuse it without depending on Stage 5.
Likewise, byte footprint does not live under lowering, and the target execution
analyzer does not depend on Core or planner internals after it receives the verified
`MinecraftProgram` plus the explicit supported-root map.

The crate is still unpublished, so use this boundary to narrow accidental API:
`FunctionEditor` remains a reusable public checked editor, but the current
`FunctionPass` trait and public `PassRunner` are replaced by the closed
`CorePassKind`/pipeline under `opt::core`. Migrate tests that currently import the
generic runner to the owned optimizer API, editor tests, or crate-local pipeline
tests, then stop re-exporting runner machinery from `ir::core`. `CorePassContext`,
concrete implementation functions, and malformed intermediate bodies are likewise
compiler-internal. Public callers choose `CoreOptimizationLevel` and receive typed
output/failure reports; they do not assemble an unstable internal pipeline.

Within those modules:

- use `EntityVec`/dense vectors for tables indexed by allocated IDs, sorted vectors
  for sparse sets and ranges, and maps only as non-authoritative lookup indexes;
- model closed semantic choices with exhaustive enums rather than pass/recipe trait
  registries;
- return immutable analysis values, then apply edits through `FunctionEditor` or
  produce the next private planner result from explicit borrowed prerequisites;
- keep large worklists and temporary lattices in per-function/per-lowering scratch so
  they are dropped before the returned reports;
- convert narrow internal error enums into ordered diagnostics only at API boundaries;
  and
- use generic helpers only for a demonstrated shared invariant, not to erase the
  different semantics of Core rewrites, physical copies, and Minecraft commands.

This takes the functional-compiler benefit of separating fact computation from
effectful rewriting, but implements it with ordinary Rust ownership rather than a
persistent IR or monad stack. It also takes Zig's compact per-function-table approach
without adopting index arithmetic where the existing typed entity IDs are safer.

Do not perform a cosmetic file move of all Stage 4 code before 5A. Extract a module
when its first new consumer lands, keep reviewable diffs, and preserve the public
module paths unless the Stage 5 API actually requires a change.

## Core pass pipeline

The baseline pipeline should be explicit rather than a registry of arbitrary
patterns:

```text
verify Core program

for each FunctionId in order:
  canonicalize cheap local identities
  sparse conditional constant propagation
  apply constants and fold executable branches
  detach newly unreachable blocks
  eliminate dead discardable instructions
  fuse straight-line jump regions
  local dominance-scoped CSE
  canonicalize and eliminate dead instructions once more if changed

verify complete optimized Core program
```

Run the complete local pipeline on one function before moving to the next stable
`FunctionId`. MLIR documents the cache and future-concurrency advantages of grouping
consecutive function passes this way. These Stage 5 passes may read immutable program
signatures and callee identities but may neither change signatures nor trigger a
whole-program/call-graph analysis from inside a function pass. Input verification and
the final verifier remain whole-program boundaries; verify-each checks only the body
whose local pass just ran. A failure still consumes and drops the entire optimization
input, so already optimized sibling bodies are never exposed as a partial result.

Passes report `changed` directly. Do not compare dump hashes to decide whether a pass
mutated IR. Do not run the complete pipeline to an unbounded fixed point.

Cheap canonicalization uses one frozen reachable-CFG reverse-postorder sweep with a
typed root-scan allowance; the current backward-substitution rules need no dynamic
user requeueing or separate rewrite fuel.
Completed SCCP and DCE runs reach their own algorithmic fixed points; their typed
limit fallbacks are different as specified below. The final cleanup group runs at
most once unless a measured fixture proves another bounded round materially helps.

### Cheap canonicalization

Use a closed match over the small Core vocabulary, not a generic rewrite-rule engine.
Initial rules include identities whose semantics are exact for Core:

- Boolean constant negation and double negation;
- wrapping or overflowing add with zero in either orientation, with overflowing add
  replacing the complete tuple by the other operand and `false`;
- reflexive comparisons using the explicit predicate map (`eq`, `sle`, `sge` true;
  `ne`, `slt`, `sgt` false);
- deterministic total ordering for the Core-owned commutative pairs (both additions
  and `I32Compare(Eq | Ne)`); and
- same-target branches whose complete argument tuples are equal after virtual
  replacement become jumps.

Preflight a typed dense-table limit against allocated blocks, instructions, and values
before building placement, CFG, reachability, and the dense virtual-value table;
stable detached IDs make an attached-only
scratch claim false. On excess, return unchanged before allocation. Otherwise visit only reachable blocks in
deterministic reverse-postorder and instructions in execution order, so valid SSA
definitions precede users even when raw block layout does not. Maintain iterative
path-compressed virtual replacements. Existing-value replacements must be transitive
operands of their root; constants materialize at the root definition. This
backward-substitution invariant makes a frozen single sweep complete and idempotent
for the current table. Double-negation matching inspects the raw producer before
resolving effective operands; all other rules use the virtual map. A future rule that
creates an operation, points forward, or forms a new parent pattern requires a real
dynamic-use worklist redesign rather than quietly adding requeues to stale original
use lists.

Keep that complete virtual map for matching and multi-result proofs, but submit only
the post-projection live-out subset to the editor: after planned terminator/operand
changes, a mapping used solely inside the planned erase set needs no materialized
constant. This avoids permanently growing append-only stable arenas with temporary
constants that immediate DCE can detach but never reclaim.

Exhausting the distinct checked root-scan allowance applies only the already planned prefix,
produces a stable aggregate statistic and optional missed remark, and leaves valid
IR. CSE independently canonicalizes commutative keys; its correctness/effectiveness
cannot depend on this optional pass or on operand order surviving SCCP replacement.

### Sparse conditional constant propagation

Use one typed lattice per SSA value:

```text
Unknown                 // not yet constrained by executable flow
Constant(Bool | I32)
Overdefined             // runtime-dependent or conflicting constants
```

Track executable blocks and exact successor-arm identities separately. Core edge
identity is `(source BlockId, successor_index)`, not a `(source,destination)` pair:
two arms may target the same block with different arguments. Entry parameters begin
`Overdefined`; instruction results are evaluated only when their block is executable;
block parameters join facts from executable arm identities only; calls produce
`Overdefined` results in Stage 5; and all wrapping/overflowing behavior uses Core's
exact `i32` semantics. A changed value requeues executable instruction/branch users
and re-propagates its argument on an already executable edge—activation alone is not
enough when an edge argument later widens.

After ordinary queue quiescence, pessimistically finalize each executable block at
most once by promoting every remaining `Unknown` block parameter and instruction
result in it to `Overdefined`, then resume events. Newly executable blocks begin
unfinalized. A complete solution contains no unknown definition in executable code;
resolving only branch conditions would leave unknown returns and edge arguments
unpublished. Current valid Core should require zero such resolutions, so test the
completion routine synthetically and assert that corpus-wide expectation while
retaining the conservative release behavior. LLVM SCCP performs comparable
unresolved-value resolution before consuming solver facts.

The solver computes facts without mutating Core. Only a completed solver result may
feed the later application step that installs folded terminators with raw selected-arm
arguments, replaces values/constants once, and then detaches dead code. Derive compact decisions, omit identical/already-unused
constant materializations, then drop solver tables before editing. Use separate typed
`SccpTableLimit` and `SccpEventLimit`;
preflight the former before allocating the edge catalog, `UseIndex`, tables, or queue.
If either limit stops the solver, drop all SCCP facts and compact decisions and leave
Core unchanged: an
optimistic `Constant` may still become `Overdefined` when another edge becomes
executable. Representable entity-capacity failure remains a compilation failure.
System allocator exhaustion follows Rust's process allocation policy and is not
misreported as a recoverable pass error.
This analysis-first shape follows MLIR dataflow and LLVM SCCP: reach a monotone
solution, then rewrite from that solution.

### Dead instruction elimination

Start from attached instructions satisfying the exhaustive
`CoreOp::is_trivially_discardable() == Pure + Always` query whose complete result set
has no external attached uses. Populate one dense value-use-count table with a direct
attached-layout scan; DCE does not retain complete `UseSite` records it never consumes.
Seed a FIFO in layout order; when a whole candidate visit is admitted, select
the complete instruction, decrement every operand occurrence, and enqueue newly dead
producers in first-operand order. Charge the typed visit limit before this indivisible
transition, never halfway through duplicate operands. A limit-stopped selected prefix
is closed because counts are decremented only for users already in the set and
terminator uses are never decremented.

Preflight dense DCE scratch against allocated value/instruction cardinality before
allocation; stable detached IDs mean memory is honestly
`O(allocated values + allocated instructions)`, not merely attached
entities. Exceeding this optional scratch limit returns unchanged. Apply one closed
erasure set in layout order. Calls remain. This is simple DCE: it deliberately does
not remove loop-carried dead cycles through block parameters or rewrite parameter
contracts. A complete run is idempotent; a partial limit-stopped run need not be.

### Straight-line fusion

Fuse `A -> B` only when `A` ends in an unconditional jump, that jump is the only
attached incoming edge to reachable non-entry `B`, and both are reachable. Verified
SSA plus that exact-edge property proves `A` dominates `B`; production fusion does
not need a dominator tree or an arbitrary caller-supplied substitution map.

Construct one opaque `PreparedJumpFusion<'editor>` under an exclusive editor borrow.
It preflights a typed dense fact-table limit over allocated history, builds
reachability/incoming-edge/raw-ownership facts once, discovers head-first maximal
eligible components, and applies before those facts can stale. The only region payload
is ordered block IDs. Resolve each complete edge argument list through earlier
directional mappings before inserting destination-parameter replacements, preserving
simultaneous block-argument semantics. An unexplained leftover eligible edge after
complete discovery is an invariant failure; a mid-region visit-limit stop discards
that unfinished region and applies only earlier complete regions.

Globally preflight disjointness, maximality, final successors, raw ownership, and
checked sizes, then reserve every head before mutation. Rewrite attached parameter
uses once, take/drop the head and every intermediate jump terminator, move existing
instruction IDs, move only the exact tail terminator to the head, and retain
`block_order` once. A tail may target its own surviving head but never a consumed
block. Rebuilding facts or applying repeated single-block edits is forbidden. A
fusion-consumed block remains allocated with its parameters but has no instructions
or terminator, so an `InstId` has one block-container owner even when detached blocks
are inspected.
Allocated identities and instruction data remain available to the debug dumper.

Do not physically delete block parameters in Stage 5. Core's stable identity model
requires every allocated `ValueId` to retain a valid bidirectional definition. Once
all attached uses are replaced and the defining block is detached, the old parameter
is harmless. For reachable blocks whose unused parameters cannot be fused away, the
Minecraft planner may omit physical homes and transfers for those unused values.

### Local CSE

Use a dominance-scoped expression table keyed by closed `CoreOp` semantics and
canonical operands. Do not infer CSE eligibility from `EffectClass::Pure` alone:
purity answers whether unused evaluation is observable, while CSE additionally needs
totality and deterministic, substitutable results. Stage 5 requires
`Pure + Always + Structural` because its shared erasure editor has that conservative
gate. `Always` is not a general theorem required by dominated CSE; relaxing it later
needs a purpose-built redundant-evaluation edit rather than weakening DCE. Add the closed IR query
`CoreOp::result_equivalence() -> ResultEquivalence`, initially `Structural` for the
current constant/arithmetic/compare/Boolean operations and `Opaque` for calls. The
optimizer—not `ir::core`—defines `CoreExpressionKey` and exhaustively constructs it
from a structurally equivalent operation's payload, independently symmetry-normalized
resolved operands, and complete result-type contract. Use a pass-private small-key
sequence with allocation-free zero/one/two shapes and a boxed wider fallback rather
than allocating vectors for every current expression. Future nondeterministic or input-sensitive pure operations remain
`Opaque` until their semantics justify structural equivalence. Lookup order must not
depend on hash iteration; a hash table is acceptable as an index if authoritative
block and instruction order controls insertion and replacement.

Preflight a typed analysis-slot limit over allocated blocks, instructions, and values
before constructing the current dense placement/CFG/reachability/dominance facts;
excess returns unchanged. Build fresh facts after fusion. Derive dominator children in
linear time by iterating allocated `BlockId`s in ascending order, then traverse in
preorder with an explicit stack and one scoped hash table plus undo checkpoints. Bound
active entries separately from scanned instructions. Do not substitute reverse-postorder merely
because another analysis already computed it. While discovering candidates, resolve
operands through the already planned representative map before key construction. This
allows one final replacement batch to catch expressions that consume earlier
duplicates without mutating during traversal; GHC's CSE similarly applies its
substitution before reverse-expression lookup.

Limits stop before a whole instruction, so multi-result operations are replaced only
when the complete result contract matches.
Calls are excluded until Core gains a truthful call-effect summary.

## Narrow editing substrate

The first draft proposed a generic batch editor without defining its invariants. That
would quietly become a second mutable IR API. Instead, add consumer-driven atomic
operations to `FunctionEditor`:

```text
ValueReplacement = Existing(ValueId) | Constant(TypedCoreConstant)
replace_values_batch(final old -> ValueReplacement mappings)
rewrite_inst_operands_batch(inst -> complete operand list)
erase_discardable_inst_set(instructions with no uses outside the set)
set_terminators_batch(block -> terminator)
detach_unreachable_blocks()

// Added with the Stage 5D consumer, not the generic 5B substrate:
prepare_jump_fusion(limits) -> PreparedJumpFusion<'editor>
```

Each operation fully preflights its own batch and either applies it or returns without
mutation. A pass may perform multiple successful batches; if a later internal error
occurs, the compilation unit is discarded exactly as Stage 2 specifies.

`replace_values_batch` is a real batch, not a loop around the existing
`replace_value`. Canonicalize/path-compress existing-value chains, reject cycles and
type mismatches, and preflight the exact entity capacity needed for constants before
mutation. Each typed constant is materialized deterministically at the replaced
value's definition point (before an instruction result's defining instruction or at
the start of a block parameter's block), preserving its origin and dominance without
hoisting it globally. Build placement/use/CFG/dominance facts once, validate all final
replacements, then allocate/materialize and rewrite the function in authoritative
layout order.

Operand rewrites require a complete arity/type-correct list and are applied together;
they support commutative canonicalization without exposing unrestricted instruction
mutation. Terminator and operand batches preserve the caller's raw `ValueId`s; a
following value batch resolves those identities before erasure preflight observes
post-replacement uses. Fusion's opaque prepared edit owns the exclusive editor borrow,
its one fact snapshot, minimal block-ID regions, and atomic application; it neither
accepts stale rich records nor repeatedly calls a single-block edit.
The same one-snapshot/one-preflight/one-application rule applies to erasure,
terminators, and fusion. This is required for the scale contract; the current
single-value editor intentionally rebuilds analyses and rescans the function and must
not become the implementation of canonicalization, CSE, SCCP application, or block
fusion.

`erase_discardable_inst_set` treats its proposed set as untrusted. It independently
rechecks attached unique ownership and `CoreOp::is_trivially_discardable`, then scans
current attached instruction operands and terminators once to reject any selected
result use outside the set. It must not trust pass-local virtual counts, allocate a
second `UseIndex`, or scan the body once per result. Successful erasure only filters
attached block instruction vectors; raw `InstData`, stable IDs, origins, and detached
historical containers remain intact.

Analyses borrow the immutable pre-edit body and are dropped before mutation. A pass
may define a small private `FunctionFacts` bundle for the exact analyses it uses, but
there is no cross-pass cache or preservation protocol in Stage 5. LLVM and MLIR show
that analysis managers are valuable at scale but make invalidation and cross-scope
queries complex; this pipeline is small enough to start with explicit pass-local
lifetimes and measure recomputation directly.

Do not clone the whole function before every pass and do not build MLIR-style pattern
rollback. MLIR documents rollback bookkeeping as costly and difficult to debug and
recommends immediate mutation when backtracking is unnecessary. Core transformations
have one chosen rewrite, not competing legalization paths.

The production pipeline is a closed internal algebra, not a trait-object registry:

```text
CorePassKind = Canonicalize | Sccp | Dce | Fuse | Cse
CorePipelineStep = stable step ID + CorePassKind
CorePipeline = ordered fixed slice of CorePipelineStep, with deliberate cleanup steps
```

An exhaustive dispatcher calls pass functions and returns `PassOutcome`. Tests for
the generic editing substrate test the editor directly; runner failure tests use a
crate-private test hook rather than making arbitrary production passes injectable.
This follows GHC's explicit `CoreToDo` shape and preserves the small inspectable
pipeline, while rustc's more extensible internal `MirPass` trait is unnecessary for
five fixed passes.

Stable step IDs distinguish repeated invocations (for example
`canonicalize.initial`, `dce.after-sccp`, `canonicalize.final`, and `dce.final`) in
reports, filters, dumps, timings, and bisection without duplicating pass algorithms.
Conditional final cleanup records a bounded typed `Skipped(NoEnablingChange)` rather
than silently omitting a configured step or pretending a pass ran unchanged. The
runner evolves narrowly:

```text
CorePassContext {
  pass-specific limits derived from the pass kind and input function size
  bounded CoreRemarkSink
  ephemeral instrumentation hooks
}

PassOutcome {
  changed: bool
  completion: Complete | StoppedAtLimit { stable limit kind }
  statistics: closed inline PassStatistics variant matching CorePassKind
}

CoreStepOutcome = Skipped { NoEnablingChange } | Ran(PassOutcome)

CoreOptimizationRemark {
  kind: Applied | Missed | Analysis
  pass
  function / block / instruction / origin when applicable
  stable reason code
  human explanation
}
```

Because the pass set is closed, each statistics variant is a fixed counter struct
(`CanonicalizeStatistics`, `SccpStatistics`, and so on), not a heap-allocated vector
or string-keyed map per tiny function. Passes submit remarks directly to the
runner-owned sink in `CorePassContext`; they do
not first allocate an unbounded per-pass vector. The sink applies filters and the
record cap at submission time while retaining aggregate omitted counts over closed
reason enums. Statistics are small fixed-name counters merged by the runner in
pipeline/function order.

`PassOutcome` and `CoreStepOutcome` are invocation-local. Merge them immediately into
one fixed `CorePipelineStepSummary` per configured step: functions seen/changed/
skipped/completed/stopped plus each fixed statistic. Use
`StatisticCount::Exact(u64) | Saturated` for merges so diagnostic overflow is explicit
but never turns a correct optimization into failure. The normal report does not retain
a record for every `(function, step)` pair. Exact-or-saturated stopped-at-limit counts
remain in the summary; function/site identities appear only through the capped remark
stream or a failure bundle.

Default execution does not render Core before every pass. Complete before/after pass
dumps are explicit developer instrumentation written to a caller-selected sink and
are excluded from deterministic reports and normal latency claims. Failure bundles
retain only a configured byte-capped current/final snapshot and do not promise the
pre-pass body. This replaces the current `capture_before` behavior, which eagerly
renders the whole program even when all passes succeed.

Do not call a Core dump a replayable reproducer. MLIR can replay initial IR plus a
pipeline because its assembly has a parser; this repository's Core dump is
diagnostic-only and deliberately has no stable parser/serializer. Stage 5 does not add
one merely for crash capture. Reproduction comes from the originating Rust builder
fixture, a recorded generated-test seed, or—after Stage 6—the original source input.
Bounded failures identify the function/step, and explicit developer dumps help
bisection, but neither promises round-trip execution.

The runner retains named pipelines, failure bundles, optional before/after dumps, and
an internal verification policy:

```text
VerificationPolicy::PipelineBoundaries
VerificationPolicy::AfterEachPass
```

Every mutating `Baseline` mode verifies input and final whole-program output, and every
pass still has the contract of producing valid IR. `None` performs one input
verification and returns the identical owned value; rescanning it as “output” would add
latency without another proof. Tests/CI force `AfterEachPass`; debug builds default
to it; optimized compiler builds default to `PipelineBoundaries` with a developer
override. Benchmark both modes on the scale corpus. This balances MLIR's default
verify-every-pass discipline with Zig's debug-gated checks and Swift's configurable
per-pass verification, while respecting the compiler's sub-second latency goal. The
policy is internal compiler instrumentation, not a public optimization level and not
a semantic difference. Pass timing is separate ephemeral instrumentation and is not
stored in deterministic `CoreOptimizationReport` output.

For the verification policy to be real, avoid the current double-check path.
`FunctionEditor::new` remains the standalone checked constructor. The optimizer first
verifies the program, then `CorePipelineRunner` uses a private safe
`FunctionEditor::from_verified_body` constructor whose precondition is owned by the
runner; no `unsafe` block is involved. New batch operations preserve documented local
invariants without invoking the whole-function verifier internally. The runner alone
performs after-pass verification when policy requests it. Existing legacy editor
methods may retain stronger checks, but baseline passes must use the batch path so
`PipelineBoundaries` actually removes redundant full scans.

## Optimization levels and budgets

Follow the useful distinction in rustc and Swift between required transformations
and optional optimizations, without inventing a mandatory Core phase:

```text
CoreOptimizationLevel::None
CoreOptimizationLevel::Baseline // exposed only once its real closed pipeline exists

MinecraftOptimizationLevel::None      // exact Stage 4 physicalization
MinecraftOptimizationLevel::Baseline
```

`None` verifies once and is the unchanged differential oracle. `Baseline` contains
only proven, bounded passes and is not published while its dispatcher would be empty.
Research, pass bisection, and incomplete future
pipelines use crate-private tooling; do not publish an `Experimental` level with no
defined contract. Add another public level only when a concrete reviewed consumer
needs it.

Do not use one shared decrementing “optimization budget.” Each pass invocation gets
typed `Derived | Explicit(Newtype)` limits. The concrete pass computes its exact
derived default from the stable pre-pass entity/edge/use metadata it already scans;
the runner does not duplicate that work or guess from coarse counts. Checked absolute
caps remain distinct: canonicalization root visits, SCCP propagation events,
DCE whole-instruction visits, candidate visits, and retained records are different
units. A shared counter would make later
passes depend opaquely on how much earlier passes happened to consume. This follows
GHC's size-relative simplifier ticks without pretending unlike algorithms spend the
same currency.

The closed pipeline may be inspected and bisected by crate-local tests and developer
tooling. The public surface exposes levels and diagnostics, not pass assembly or a
permanently stable list of internal pass names.

### Analysis completion and safe fallback

“Stopped at a limit” has a domain-specific safe meaning; it is not automatically a
usable partial answer:

| Computation | Safe limit behavior |
| --- | --- |
| Core pass dense fact/table preflight | Return unchanged before allocation; stable detached IDs are included in the required slot count. |
| Canonicalization, DCE, fusion, CSE local scan/visit limits | Stop before the next indivisible root/region edit; batch preflight and verification retain the already closed partially improved Core. |
| SCCP | Discard the incomplete lattice and executable-edge solution; apply no SCCP-derived constants, branch folds, or reachability edits. Optimistic facts can later become overdefined. |
| Runtime demand | Discard the whole incomplete solution and select the typed all-reachable Stage 4 value/instruction policy. |
| Liveness/coalescing | Discard incomplete liveness or tentative unions, preserve exclusive pinned ABI homes, and assign distinct ordinary homes. |
| Physical condition stability | Classify the affected site as unknown and reject recipes that require stability. |
| Execution-cost analysis | Preserve exact completed local census where available, but classify every affected region/root metric as `Unknown(AnalysisLimit)`; never publish a partial finite upper bound. |
| Candidate enumeration | Retain the already constructed Stage 4 candidate and stop considering optional recipes. |

Required verification, plan construction, target construction, and emission do not
degrade to “best effort”: representable entity/checked-size exhaustion or inability
to complete an invariant proof is a compilation failure, never permission to expose
unverified output. System allocator OOM follows the Rust process policy unless the
repository later adopts fallible allocation pervasively. Every optional analysis
reports `Complete` or a typed incomplete reason, and only its documented fallback may
consume the latter. MLIR's monotone lattice model and LLVM SCCP both distinguish
solving to convergence from rewriting with the solved facts; the same separation is
mandatory here.

Preserve the behavior of the existing `LoweringOptions::new` as the conservative
Stage 4 mode. Add an explicit builder/setter for Minecraft optimization rather than
silently changing existing callers' artifacts. A future top-level compilation façade
may choose `Baseline` by default, but the lowering API keeps compatibility and a
direct differential oracle.

Stage 5 is also the first real consumer of Stage 0's configurable hard target limits:

```text
CommandLimitAssumptions {
  max_command_sequence_length: u32
  max_command_forks: u32
}
```

`LoweringOptions::new` uses the selected target defaults exactly as Stage 4 does; a
typed builder may override the assumed server values used by recipe safety
comparisons. The same immutable `CommandLimitAssumptions` value is passed to explicit
target-execution analysis; do not maintain lookalike lowering and analysis structs
that can disagree. The chosen assumptions and target defaults both appear in
`ExecutionContract` and `TargetExecutionCostReport`. They affect diagnostics and
recipe safety comparisons, not generated commands, and the compiler never changes global gamerules. Soft
per-tick budgets, overflow policy, runtime gamerule checks, and work partitioning stay
in Stage 9 because Stage 5 never schedules across ticks.

The Java 26.2 configured-value domain for both gamerules is
`0..=2_147_483_647`. The smart constructor rejects larger `u32` values, and target
defaults pass through the same invariant. Limit assessment then derives the pinned
runtime semantics rather than treating the raw integers as interchangeable maxima:

- sequence execution uses `max(1, max_command_sequence_length)` and is safe at
  equality;
- positive expansion at one ordinary fork-checked redirect is rejected at equality with
  `max_command_forks`;
- at configured fork zero, a direct non-redirect command still runs, while any
  possible ordinary checked expansion is not proven safe. Exact zero expansion is
  `ProvenWithin` only when the execute-stage bound also proves no execute stage is
  reached.

Root reports retain
`ProvenWithin | MayExceed | ProvenExceeds | NoFiniteBoundProven | Unknown` separately
for sequence work and the maximum individual ordinary checked redirect. They never add fork counts across
commands, roots, or loop iterations. An override is a deployment precondition, not a
server configuration action: a consumer relying on `ProvenWithin` must provide actual
gamerule values at least as large as the retained configured assumptions or re-run
analysis with the actual values.

## Refactoring physical planning

Stage 4's `PlanBuilder` allocates one home and one target function per reachable
semantic entity before all target choices are known. Stage 5E replaces that mutation
order with the smallest set of immutable results that already have consumers:

```text
verified CoreProgram
    -> SemanticInventory
    -> audit_legality(core, inventory) -> Result<(), Diagnostics>
    -> RuntimeDemand
    -> [Baseline only] LivenessResult
    -> HomeAssignment (owns aligned InstructionPlan table)
    -> EdgeTransferPlan
    -> ResourceInventory
    -> LoweringPlan::from_parts(...) -> verified immutable LoweringPlan
    -> construct_program(core, &plan)
```

These are private immutable result records, not public IRs, serializable dialects, or
generic typestate parameters. Later phases borrow the earlier results they need; each
phase may use a short-lived mutable local builder and returns one completed record.
Function signatures enforce order: Baseline home assignment consumes Core,
inventory, demand, and its aligned liveness result; `None` deliberately bypasses
liveness and constructs the exact Stage 4 assignment. Transfer and resource
construction then consume the frozen assignment and their explicit prerequisites.
`LoweringPlan::from_parts` locally flattens the phase-local identities, independently
verifies the candidate, and publishes it only on success. Construction then consumes
the verified plan rather than consulting inventory as a second target authority. This
preserves the useful functional analysis-to-analysis flow seen in GHC without forcing
persistent trees or retaining cosmetic phase tokens.

Every policy-sensitive phase result records its explicit
`MinecraftOptimizationLevel`. Each consuming phase rejects mismatched prerequisite
provenance, and final assembly rejects assignment, transfer, or resource results from
another policy—even when an empty or fully demanded program makes their incidental
contents identical. Roles, names, and scratch presence are consequences of the policy,
not reliable proof of which constructor produced a value.

Stage 5F inserted the first real `LivenessResult` consumer between `RuntimeDemand` and
Baseline `HomeAssignment`, which freezes conservative coalescing before transfers are
derived once. The `None` path never computes liveness or coalescing.
Stage 5G adds `ControlRecipePlan` and `BlockPlacement`, and makes
`ResourceInventory` reflect that placement after homes and transfers freeze. The
required terminal-call contraction does not consume condition stability, so
`PhysicalEffectSummary` remains deferred with the optional repeated-condition recipe
that would become its first real consumer. Stage 5E neither computes placeholder
liveness/effects nor consumes Core blocks: every reachable block remains materialized
until Stage 5G has an actual recipe that can own it.

The verified `CoreProgram` remains the immutable semantic authority throughout these
calls; phase records store IDs and derived facts rather than cloning operations,
operands, terminators, or signatures. More precisely, analysis functions that inspect
semantics take `&CoreProgram` alongside their explicit phase prerequisites (for
example `compute_demand(core, &inventory)`). The abbreviated signatures above describe
phase order, not permission to duplicate the source IR into inventory.

Per function, `SemanticInventory` records authoritative reachable block, instruction,
and value order plus dense membership bits; one ordered semantic-edge array; dense
incoming-edge index lists that refer into that array; ordered call sites; origins; and
checked incidence counts used by bounded analyses. It stores Core IDs and derived
facts, not cloned operations, terminators, or signatures, and decides neither storage
nor names/Stage 3 IDs. Immediately after inventory, preserve Stage 4's
target-vocabulary and recursive-call rejection. `audit_legality` builds the iterative
SCC adjacency it needs from inventory call sites, then drops that scratch. A successful
empty `LegalityAudit` token and a second retained call graph add no proof value. No
physical phase runs after a failed gate.

Stage 5E evolved the former Stage 4 emitted-program analysis into this single
`SemanticInventory`, preserving the `None` path's traversal and order. Do not
reintroduce a second reachable-semantic census or boxed copies of the same IDs and
edge tuples.

This graph is deliberately one-way. Stage 5F liveness is computed on the semantic CFG
and is conservative for every later recipe. A Stage 5G recipe may consume a
materialized block or omit a now-redundant transfer command, but it may not extend
semantic lifetimes, change value-to-home assignments, or request re-coalescing. Stage
5 does not iterate home assignment and recipe selection to chase secondary
opportunities; the final plan verifier proves the selected placement against frozen
assignments and effects. A future global-search stage may own that feedback loop
explicitly.

“Requires runtime storage” comes from a private backward demand analysis, not merely
from `UseIndex::use_count != 0`. For every reachable function, seed each `Return`
operand, each branch condition, and every instruction that is not
`Pure + Always`. Marking an instruction marks all its operands; marking an
instruction result marks its producer; and marking a block parameter marks the
corresponding argument on every reachable incoming edge. Dense mark bits make cycles
terminate. Calls therefore remain and demand their arguments without a second
call-specific seed rule. Only a `Pure + Always` instruction with no demanded result
may be omitted. Stage 5E does not delete CFG control.

Every Core function is currently independently exposed by lowering, so every
reachable return operand is semantically observable. ABI storage is a separate
physical requirement: every entry parameter and callee result slot remains pinned
even when an entry value is undemanded or a caller ignores that result. An unused
caller-side call result needs neither a value home nor a result-copy command, but the
call and the callee's complete result ABI remain. This analysis does not mutate Core
and must run in optimized Minecraft lowering even when Core optimization was skipped.
`MinecraftOptimizationLevel::None` does not run the pruning analysis; it records the
all-reachable policy and reproduces Stage 4 exactly.

Demand has only complete outcomes: precise Baseline facts, an all-reachable
optimization-disabled policy, or an all-reachable conservative fallback with a typed
reason and statistics. Separate checked limits cover dense table slots and
propagation events. Dense capacity is one result record per function plus every
allocated value and instruction slot, including detached history; empty functions
therefore cannot evade the bound. The event counter has one documented increment rule
for seed occurrences, first marks, dequeues, demanded-instruction operand occurrences,
and incoming-edge-argument occurrences. Its derived limit is a checked conservative
maximum, while an explicit event limit applies to actual charged work and may complete
below that maximum. A checked derived bound that overflows is itself a fallback reason.
If Baseline reaches a limit, discard the entire transient solution before using the
all-reachable policy; never prune from partial facts. Failure or capacity exhaustion
in mandatory plan construction or verification is a compilation error, not an
optimization fallback.

The verifier independently recomputes transient Core reachability and minimum runtime
demand, accepts conservative supersets, and rejects missing required computation. It
also checks final type/reference coverage, ABI pinning, instruction/result indexing,
edge-transfer semantics, resource ownership, and the absence of dangling control
targets. This independent walk is not a second retained semantic census.

## Physical homes and copy coalescing

Stage 4's one-home/one-value `HomeRole::Value` is intentionally too strong for
coalescing. Separate physical storage from semantic correlation:

```text
AssignedHome {
  CoreType
  class: FunctionRegister { function }
       | PinnedParameter { function, index }
       | PinnedResult { function, index }
       | RecipeTemporary { function, ordinal }
}

ValueAssignment {
  function
  value
  CoreType
  home: AssignedHomeId
}

EdgeTemporaryDescriptor {
  function
  kind: Typed(CoreType) | LegacyUntyped
}
```

`HomeAssignment` owns function-local `AssignedHomeId`s, the complete pinned ABI,
dense optional semantic value assignments, and a dense instruction-plan table keyed
by allocated `InstId`. `None` in that table means detached or unreachable;
`Some(OmittedPure)` means reachable and deliberately unmaterialized, so construction
cannot confuse absence from Core layout with an optimization decision.
`EdgeTransferPlan` separately owns function-local edge-temporary descriptors because
the parallel-copy resolver discovers their need only after assignments freeze. Final
plan construction deterministically flattens both identity spaces into `HomeId`;
transfer planning never mutates `HomeAssignment` to append scratch.

Stage 5E homes are typed, and every `ValueAssignment` must match its home's
`CoreType`. It gives distinct homes to demanded semantic values and performs no
general liveness-based reuse. ABI parameters/results remain pinned so public
invocation and call boundaries stay simple. A fixed scalar recipe may also require a
typed `RecipeTemporary` for an undemanded physical output; that temporary is not a
semantic `ValueAssignment`. Size a deterministic per-`(function, CoreType)` recipe
pool to the maximum simultaneously required scratch of that type, retaining an
ordinal in the representation for future recipes. Stage 5F then considers only
verified same-type block-argument copy coalescing. Allowing unrelated nonoverlapping
`Bool` and `I32` values to reuse a slot would require a general allocator and would
complicate Boolean-normalization proofs without reducing an edge copy, so cross-type
register reuse remains deferred.

Emission is selected by an instruction-aligned `InstructionPlan`, not by asking the
emitter to infer demand from missing homes:

```text
InstructionPlan =
    OmittedPure
  | Scalar { operands, results: [ScalarResultPlacement] }
  | Call { arguments, result_destinations: [Option<AssignedHomeId>] }

ScalarResultPlacement =
    Semantic { value, home }
  | RecipeTemporary { home }
```

Both result arrays preserve exact Core/callee result arity and indexing. A retained
fixed scalar recipe receives every physical output it requires. For example, if only
one result of `I32AddOverflowing` is demanded, that result receives a semantic home
and the undemanded sibling receives the correctly typed recipe temporary; the sibling
does not become semantically demanded. If neither result is demanded, the discardable
operation is `OmittedPure`. Calls are never omitted, but only `Some` caller
destinations receive result-copy commands. The `None` path gives every scalar result a
semantic placement and every call result a `Some` destination, preserving Stage 4.

Keep the closed exhaustive scalar physical-output/storage contract beside the scalar
emitter rather than adding target facts to `CoreOp`. The verifier independently
interprets every operation's complete result/storage behavior instead of accepting
the planner's query as proof.

Semantic non-interference is necessary but not sufficient for sharing a home. The
current scalar emitter expands one Core instruction into an ordered command recipe,
and some recipes overwrite a result before finishing all operand reads. Wrapping add
normally emits `result = left; result += right`; when `result` already is `left`, it
elides the assignment and emits only the read-modify-write addition. It may therefore
reuse `left`, while reusing a distinct `right` destroys that value before the second
read. Overflowing add reads
both operand signs after writing its sum, and `BoolNot` writes `1` before subtracting
its operand, so neither recipe may reuse an operand home. A loop-carried block copy
can otherwise coalesce an instruction result with exactly such an operand even though
ordinary SSA liveness permits it.

Keep this target fact out of `CoreOp` semantics and out of the generic Core optimizer.
The Minecraft planner owns one private, closed, exhaustive `ScalarAccessContract`
query beside scalar lowering. For every result it states which input positions, if
any, may share the result home under Stage 5's fixed scalar recipe. The initial table is:

| Core operation | Result/input reuse allowed by the current recipe |
| --- | --- |
| constants | no inputs |
| `I32AddWrapping` | the result may reuse input 0 (`left`) only |
| `I32AddOverflowing` | neither result may reuse either input |
| `I32Compare` | none (the result is also a different type) |
| `BoolNot` | none |

If both add operands are the same semantic value, reusing `left` naturally covers
that case; two distinct simultaneous operands may not acquire one home merely because
their bits happen to be equal. New Core operations and new target recipes must extend
this exhaustive table in the same change. This is a permissive alias contract, not a
requirement that a result reuse an input. It mirrors the separation in regalloc2
between liveness and operand timing/`Reuse` constraints, and LLVM's explicit
two-address def/use operands.

Calls are intentionally outside `ScalarAccessContract`. Their separate fixed-slot
rule reads every caller argument before invoking the callee and defines demanded
caller results simultaneously afterward. Consequently any result may reuse a dead
caller argument, while live-through arguments and demanded sibling results remain
distinct.

This does not create a backwards edge from the Stage 5G `ControlRecipePlan` to
`HomeAssignment`: Stage 5G selects only control/placement recipes after homes freeze,
while scalar recipes are fixed inputs to assignment. A future scalar-recipe chooser
must either choose an access contract before assignment or conservatively accept the
frozen homes; it may not silently change operand timing afterward.

Fixed ABI copies do not need a second general allocator in Stage 5. Recursion is
already rejected, caller/callee home families remain disjoint, all call arguments are
copied before invoking the callee, and all caller results are copied afterward. Thus a
call result may safely reuse a now-dead caller argument. Function result slots remain
pinned and distinct, so ordered return copies remain safe. Model every multi-result
instruction as simultaneous definitions at one program point: two distinct demanded
results may not share a home even if their later live segments look disjoint. The
final symbolic checker validates these call/return and definition-boundary facts as
well as CFG-edge transfers.

After assignments freeze, `EdgeTransferPlan` derives only copies whose destination
block parameters have a semantic `ValueAssignment`. ABI-pinned entry parameters need
no exception because verified Core forbids every edge targeting the entry block.
Demand propagation guarantees a home for each corresponding source argument. The
Baseline resolver visits the fixed type order `Bool`, then `I32`, groups remaining
copies by that `CoreType`, and lazily allocates exactly one typed edge temporary per
`(function, CoreType)` only if it actually emits a scratch step for that type.
Verified edge copies are type preserving, so a single cycle cannot mix types, but one
function may have separate Boolean and integer cycles. The conservative `None` path
resolves every Stage 4 copy together and retains its existing at-most-one legacy
untyped temporary and exact name, again only if scratch is used.

Stage 5E `ResourceInventory` materializes every reachable block. It allocates a
conditional branch helper only when that arm's frozen transfer has at least one step;
there is no consumed placement yet. To preserve Stage 4 order, allocate load/init
resources first, then for each function allocate block resources in inventory layout
order followed by required branch helpers in semantic-edge and arm order. Stage 5G
later changes this input from all-reachable materialization to verified placement and
selected recipes.

Generated holder identity must change with that ownership model. The conservative
Stage 4 policy retains its byte-identical `#f{function}v{value}`, result, and temporary
names. Under Baseline, pinned entry parameters retain their value-correlated
`#f{function}v{value}` spelling and result ABI slots retain
`#f{function}r{index}`. Ordinary distinct/coalesced physical registers use stable
function-local `#f{function}h{ordinal}` names; they must not be named after an
arbitrary semantic value. Typed recipe scratch uses
`#f{function}q{b|i}{ordinal}`, and typed edge scratch uses
`#f{function}t{b|i}{ordinal}`. The stable `b`/`i` discriminator prevents Boolean and
integer scratch from colliding; only `None` retains the exact single legacy untyped
`#f{function}t0` name. Dumps correlate every semantic value with its physical holder,
so the shorter physical name does not erase meaning.

Assigned-home naming belongs to `HomeAssignment`, and edge-temporary naming belongs to
`EdgeTransferPlan`; neither is `GeneratedNames` guessing from a `ValueId`. Repeated
optimized planning must produce the same ordinals. During
`LoweringPlan::from_parts`, the `None` path flattens, for each function, all reachable
legacy value homes in their existing order, then ABI result homes, then the optional
legacy edge temporary before moving to the next function. Combined with the resource
order above, toggling back to `None` must reproduce target commands, generated names,
home-role/debug forms, lowering map/execution contract, public lowering dump, and
datapack bytes exactly.

Stage 5F extends the Stage 5E boundary with an analysis-then-assignment structure
analogous to MLIR One-Shot Bufferize:

1. compute complete-or-fallback semantic liveness;
2. enumerate block-copy candidates in authoritative edge/parameter order;
3. choose deterministic in-place/coalesced assignments subject to the exhaustive
   scalar access contract and ABI pins;
4. freeze `ValueId -> HomeId` and stable physical-register ordinals;
5. derive each simultaneous edge assignment once;
6. remove home-equal no-ops and resolve remaining cycles once; and
7. verify actual home contents and transfers independently.

Do not construct a dense value-by-value interference matrix or block-by-value bit
matrix. The existing 20,000-value scale fixture makes quadratic storage unacceptable.
Use sparse live ranges/sets and a stable greedy candidate order. Stage 5E establishes
dead-home pruning with distinct homes; Stage 5F adds conservative block-argument copy
coalescing. General graph coloring, spilling, cross-function reuse, and recursive
frames remain out of scope.

The liveness representation follows regalloc2's useful shape without adopting its
complete allocator. For a block with `n` instructions, entry is point `0`, instruction
`i` reads at `2i + 1` and defines all results at `2i + 2`, terminator/selected-edge
uses occur at `2n + 1`, and `2n + 2` is the exclusive block end. Edge transfer is
edge-sensitive: each live destination parameter substitutes to the argument on that
exact edge occurrence, while direct cross-block live values retain their identity.
Duplicate branch arms targeting one block are not deduplicated.

Sparse sorted live-in/live-out sets drive a stable backward worklist and are discarded
after construction (unit tests retain them only for a dense-oracle comparison).
Production retains one dense optional per-value segment span plus a flat array of
sorted half-open `LiveSegment`s. Candidate interference scans those segment lists.
Propagation/set-element work and retained segment count have checked derived or test
limits. Overflow or exhaustion discards every fact for that function and selects the
distinct-home fallback; no partial analysis is published.

Coalescing similarly charges discovery, ordered deduplication, group searches,
all-member interference/access tests, linear group merges, the final edge audit, and
freeze work. Candidate order is semantic edge order followed by destination parameter
order; normalized `ValueId` pairs only deduplicate later occurrences without changing
the first winner. Candidate discovery precedes target-conflict and DSU construction;
when no block copy can be removed, one dense value-to-distinct-group freeze completes
without scanning scalar conflicts or allocating per-value singleton group vectors.
ABI entry homes are exclusive singletons under A-015. Any budget failure discards all
tentative unions and reports zero retained merges.

Program-point ordering must make the parallel edge boundary explicit: all arguments
on one semantic edge are read from the predecessor state before any destination
parameter receives its new state, and block-parameter definitions begin after that
boundary. Coalescing legality is not inferred from one candidate pair alone: every
member across both proposed groups must satisfy liveness, simultaneous-definition,
type, ABI, and fixed-recipe access constraints. After grouping, audit every complete
edge once for typed sources and unique physical destinations, freeze assignments,
then invoke the existing parallel-copy resolver once. Its mathematical schedule proof
and the final symbolic checker cover swaps, rotations, duplicate-source fan-out,
critical arms, and loop backedges without rescanning every edge for every tentative
union.

Unlike regalloc2, Stage 5 need not split critical Core edges: Stage 4 already has an
edge-specific transfer/helper representation for conditional arms. The invariant is
that every remaining parallel copy belongs to exactly one semantic edge and is never
emitted unconditionally in its predecessor. Add explicit corruption and differential
fixtures for critical-edge diamonds as well as swap/rotation cycles.

The production plan verifier must not accept the planner's `RuntimeDemand`, its scalar
storage/access queries, or Stage 5F's `LivenessResult` as proof that the final plan is
correct. It performs a separate transient Core reachability and minimum-demand walk,
then checks `InstructionPlan` arity, result indices, omitted-operation legality,
references, types, ABI pins, resources, and transfer ownership. A conservative
all-reachable plan is a valid superset; omitting independently required work is not.

Recompute final allocation semantics with a separate symbolic dataflow checker over
the frozen homes and transfers, following regalloc2's checker shape: each home carries
the set of semantic `ValueId` names known to denote its contents; pinned entry
parameters initialize entry state; CFG joins intersect facts; instruction results
define fresh symbols simultaneously; and edge transfers/block-parameter assignments
apply simultaneously. A whole-state top marker means only “block not reached by the
checker yet”; it is not a per-home excuse for an unknown read. Separately validate that
each resolved move sequence implements its mathematical parallel assignment. The
checker interprets every closed scalar recipe independently: late operand reads must
still find their symbols after early result writes, permitted result/input aliases
behave as explicit read-modify-write ties, and a recipe-temporary output creates no
semantic symbol. Calls read every argument before the invocation and simultaneously
define only their indexed `Some` caller destinations afterward; callee result ABI and
return copies remain complete. Before every semantic operand use, its assigned home
must contain that value's symbol.

This checker validates the produced plan rather than reconstructing the greedy
coalescer. It is required verification: failure or inability to complete rejects the
plan instead of falling back after an optimized assignment has already been chosen.
Literal segment non-overlap remains a chooser invariant tested against exact liveness;
the verifier's independent obligation is observable home-content correctness. A
fabricated recipe-legal alias based on false liveness must therefore fail on the later
semantic read even though the verifier never consumes `LivenessResult`.
Production uses fallible sparse symbolic states and a stable worklist; it never
allocates a dense `blocks * homes` matrix and drops all proof state after verification.
Today it copies sparse per-block/per-edge fact vectors when propagation needs an owned
state. A genuinely dense live-through CFG may therefore require proportionally dense
proof data, and the report must not falsely call that linear in entities alone.
Copy-on-write/interning and future-use pruning are deferred until measurement shows
that this simpler representation is a real bottleneck. Compare production checking
against a deliberately simple dense oracle on generated small CFGs, and instrument
the one-block 20,000-value case to prove linear retained state there.

## Target control recipes and block placement

The first draft incorrectly treated branch encoding as independent from target
layout. With one generated function per Core block, Stage 4's return dispatcher is
usually already cheaper than dual guards or a snapshot. A meaningful selector must
also decide whether a narrow successor region is materialized or consumed inline.

Stage 5G introduces a closed private recipe enum, not a general target CFG:

```text
ControlRecipe =
    TailJump { target }
  | ReturnDispatcher { then_target, else_target }
  | InlineTerminalArm { arm, command, other_target }
```

The exact variants may be reduced after fixture construction. Every recipe must have
a concrete emitter and verifier consumer before it is added.

The first recipe experiment should be narrower than the enum: retain the dispatcher
and allow a uniquely reached terminal block to be consumed only when its operations
and terminator combine into one legal nested target command—for example, a zero-ABI
call followed by return becoming a tail call. Add stable-dual-guard or snapshot
construction only after a fixture demonstrates the complete return/completion
semantics and a cost advantage over that dispatcher. Merely proving stability is not
evidence that dual guards are profitable.

Every selected recipe must preserve Stage 4's explicit generated-function success
and result of exactly one. Vanilla functions that fall off the end have no result. Therefore a dual-guard
or snapshot recipe needs an explicit completion `return` on the path whose guarded
arm returns normally; that command and result behavior are part of legality and cost,
not an emitter afterthought.

Stage 5G target placement is deliberately narrow:

- straight-line fusion should normally happen in Core;
- a uniquely reached trivial terminal arm may be consumed into its dispatcher;
- shared, cyclic, supported-ABI entry, or non-reducible regions remain functions;
- joins are not duplicated; and
- general trace layout, hot/cold placement, and region duplication remain Stage 11.

`BlockPlacement` records whether each reachable Core block is materialized as a
planned function or consumed by exactly one recipe. Stage 5G then rebuilds
`ResourceInventory` after this decision; Stage 5E's earlier inventory deliberately
materializes every reachable block.

### Deferred physical condition stability

Stable dual guards evaluate the same physical condition twice. Legality requires a
transitive proof that executing the first selected arm cannot change anything read by
the second evaluation.

For the current normalized score condition this means:

- the condition `HomeId` is not written by any block reachable from the first arm
  before semantic return;
- a loop cannot revisit the source and reassign that block parameter home;
- edge transfers, instruction results, and call-result copies are included;
- internal callees cannot alias caller homes because cross-function reuse is forbidden;
- execution context and fork behavior remain unchanged; and
- any future unknown/raw target effect rejects the proof.

Do not compute this until a complete stable-dual-guard or snapshot fixture proves a
benefit over the dispatcher. At that point, compute the smallest private physical
footprint fixed point required by the recipe over the semantic CFG and internal
calls. Do not widen Stage 3's coarse public `CommandContract` into a whole-program
interprocedural solver merely for a future optimization.

`PhysicalEffectSummary` is immutable and tied to the frozen home/transfer assignment.
Its join is monotone set union over a finite physical universe. If its update/space
limit is reached, the affected site is `Unknown(AnalysisLimit)` and no stability-based
recipe may use it. A partially accumulated “no writes seen yet” set is never proof of
stability. Condition-stability analysis consumes the summary, and recipe selection
consumes only its complete proof; neither may mutate homes or ask for a second, more
favorable assignment. The earlier always-legal terminal-arm recipe fabricates neither
summary nor proof.

The always-legal return dispatcher remains the fallback. A snapshot is considered
only when instability blocks a layout that otherwise saves measured static work.

## Cost evidence: three different things

### Exact footprint

`ArtifactFootprintReport` is derived only after successful emission and is exact for
the generated artifact:

```text
artifact files by kind (metadata, functions, and tags)
physical command lines
UTF-8 bytes
maximum emitted function-line Java UTF-16 units
trace records
```

It says how much code exists, not how often it executes.

It cannot be owned by `LoweringOutput`: exact JSON metadata/tag bytes, rendered
function bytes, physical-line lengths, and trace records do not exist until
`emit_datapack` succeeds. `EmissionOutput::new` computes the report once from its
owned `DatapackArtifact` and `TraceMap`; accessors never rescan files. Pre-emission
structured counts needed by recipe selection—functions, tags, commands by kind, and
score/NBT command census—belong to `TargetExecutionCostReport`. Planned homes,
helpers, placements, and resources remain in `LoweringDecisionReport`. These records
are reconciled with the emitted footprint, but none are mislabeled as artifact bytes.

### Local execution cost and whole-root bounds

A single `Bound<T> = ... | Unbounded | Unknown` loses useful information. Once one
reachable loop has no proven trip count, it would hide the exact savings from removing
three operations in that loop body. Instead, derive a read-only weighted
`ExecutionCostGraph` from the verified Stage 3 program:

```text
CommandStepCost {
  direct single-context syntax weights before outcome/context composition
  per-execute-chain context expansion bound/unknown reason
  possible outcome: Continue | Return(value class) | NoResult | Fail
  ordered exact typed internal-call sites (structure, not runtime frequency)
  outcome-partitioned aggregate execution bounds
}

FunctionLocalSummary {
  exact ordered command-step structure
  aggregate local path ranges excluding recursively expanded callee bodies
  outcome-partitioned exits, including early return and guard failure
}

ExecutionCostGraph {
  one node per generated function with FunctionLocalSummary
  unique caller-to-owned-function connectivity projected from reachable typed sites
  deterministic strongly connected regions
}
```

This graph is analysis output, not another actionable IR. It cannot be edited or
emitted. A physical line is not assumed to execute merely because it exists: a prior
`return`, a failed `execute if`, or a zero-context fork can skip its nested or later
commands. The analyzer composes the immutable structured command tree in target order
and keeps those continuation/return outcomes distinct. Local step weights do not
recursively include callee bodies, so they remain exact inside loops and do not
double-count calls. Unsafe raw or external calls produce a stable `Unknown` reason
instead of borrowing the coarse `CommandContract` as a fake execution model.

A call site is structural syntax, not a dynamic frequency. Ordered local summaries
retain its target and ordinary/`return run`/condition role. The SCC graph expands tag
sites and projects reachable occurrences to unique caller-to-owned-function
connectivity because roles and duplicates do not affect condensation; `graph_edges`
counts that projected relation. Outcome-sensitive aggregate invocation bounds live in
the metric vector. Phase 1 deliberately does not publish a
per-edge runtime multiplicity: selectors, mutable world state, guards, recursion,
raw commands, and external datapacks make exact counts unavailable without additional
environment assumptions or profile data. This follows MLIR/LLVM/Cranelift call-graph
practice and GHC's separation of syntax from conservative cardinality analysis. See
[`semantic-ambiguities.md`](semantic-ambiguities.md), entry A-001.

Run this analyzer only over verified Stage 3. It is a read-only explicit analysis,
not a lowering phase and not a prerequisite for obtaining a runnable
`LoweringOutput`. Modeled internal commands yield facts, while valid raw/external behavior
yields `Unknown` rather than failing analysis. An internal analysis limit likewise
yields `Unknown(AnalysisLimit)` for every affected region/root metric; it can never
turn an in-progress upper bound into a finite claim. The graph-entity limit has
concrete retained-record units. Its base charge is one per structured command node,
function, function tag, and declared function-tag entry. Construction then charges
one per insertion-ordered unique expanded entry retained in each tag expansion and
one per deduplicated structural callee edge retained for a caller. The graph and SCC
worklists are scratch and are dropped after producing `TargetExecutionCostReport`;
that report retains the linear structured census, per-function local summaries, one
shared region table, and small supported-root summaries, not every solver intermediate.

The concrete local solver uses fixed outcome cells rather than enumerated paths. A
command transfer distinguishes no result, success-zero, success-nonzero, failure,
and enclosing-function return. Choice joins corresponding cells immediately and
ordered composition carries terminal cells past later commands. This keeps constant
state per command while preserving the cost/outcome correlation required by function
conditions and `return run`. Execute composition separately tracks the closed context
classes zero, one, and many; selector products transform that domain, while body work
is repeated by the surviving class. This follows MLIR's monotone lattice/transfer
separation and GHC's powerset-style `{0,1,n}` cardinality algebra without introducing
a generic dataflow framework into the compiler.

There are deliberately two graph views with different lifetimes. The retained
structural call graph contains every reachable typed internal call edge and is
condensed exactly once. A public `CostRegionId` always names one SCC in that structural
graph. A structural cycle can nevertheless contain an outcome-infeasible back edge,
so treating every structural SCC as one executable cycle would invent false
`PositiveCycle` results. The solver therefore performs the following private
refinement once for each structural cyclic region, never once per root:

1. Converge a four-bit function-exit mask for no result, zero, nonzero, and failure.
   Re-evaluate only a function whose callee mask changed, using a deduplicating queue;
   joins only add bits. Divergence remains a separate conservative path rather than a
   fifth terminating exit.
2. Record the internal calls actually reached by those transfers, compute ephemeral
   SCCs of that feasible-edge subgraph, and solve its component DAG callee-first.
   These feasible components have no public identity and are dropped with the queue,
   masks, and edge sets.

Thus “one fixed point per cyclic region” means one finite feasibility fixed point,
not numerical iteration of weighted costs. A feasible acyclic component is evaluated
exactly once using already solved callees. Within a feasible cyclic component, a call
back into that component uses a zero-body-cost placeholder carrying the feasible exit
mask and conservative divergence; the ordinary invocation cost is still charged.
Private correlation fields retain the additive work before and after that recursive
call. After evaluating every member once, the solver derives one conservative
component envelope and one cycle effect per metric. A positive additive effect becomes
`NoFiniteBoundProven(PositiveCycle)`; an already unbounded or unknown effect propagates
its reason; a zero effect stays finite. Maximum individual ordinary fork-checked
redirect expansion is combined only by maximum, including across recursion, and is
never widened by adding cycle iterations.

The public structural-region row is a conservative alternative join across its
member-entry summaries. It does not mean that all functions in the structural SCC run
sequentially, and roots do not read it as a substitute for their entry function. Its
`cyclic` flag and a root's `entry_region` are structural facts, not claims that a
runtime-feasible cycle exists; such a row may correctly retain finite bounds.
Per-function solved summaries feed root metrics, while the shared region row exists
for linear inspection and stable structural identity. This preserves useful precision
when feasible SCC refinement splits an acyclic subgraph out of a syntactic cycle while
keeping the retained report independent of solver scratch.

Expose a checked standalone entry over `&MinecraftProgram`, `&SourceContext`, explicit
typed roots, `CommandLimitAssumptions`, and analysis limits. A `LoweringOutput` convenience entry
supplies its verified program, public Core-function/load roots, target, and exact
`ExecutionContract` through a private trusted path so it does not redundantly verify
the target. Both return an owned `TargetExecutionCostReport`; neither caches it or
silently reruns it. Stage 6's façade will invoke the analysis once and aggregate the
result.

Configured analysis-limit fallback is a successful report as above. Checked
construction-size/arithmetic invariant failure or an internal contradiction returns
`TargetExecutionAnalysisFailure` with bounded context and no partial report; the
borrowed verified target remains available to the caller. Such an instrumentation
failure never reclassifies correct code generation as `LoweringFailure`. Process-level
allocator OOM is outside this recoverable API contract.

Visit structural regions in deterministic callee-first order. SCC discovery,
condensation, feasible-subgraph refinement, and all deep graph walks use explicit
worklists/stacks rather than host recursion. The solver-update limit counts exactly
three kinds of indivisible work: one function transfer evaluation during feasibility,
one newly discovered function-exit bit, and one final weighted function evaluation.
Reserve the final evaluation for every member before starting a cyclic region and
commit no member of an unfinished region. Exhaustion therefore preserves earlier
completed regions exactly and marks the unfinished region plus every not-yet-visited
region (including its upstream callers) `Unknown(AnalysisLimit)`. Memoize each
completed region and per-function summary; do not traverse or copy the reachable graph
independently for every external root.
Each resolved root sequence references its entry region and stores only its metric
vector. In particular, do not copy a list of every reachable cyclic region into
every root record. A requested function root resolves to one sequence. A requested
function-tag root resolves to one report entry per insertion-ordered unique owned
function because scheduled tags create independent roots; an external tag entry is a
typed unresolved/unknown root entry rather than a fabricated internal region. This is
distinct from an ordinary `function #tag` command, whose resolved functions remain in
the caller's one sequence. Add a many-functions/deep-call-chain scale fixture where
all Core functions are roots to catch accidental `roots * graph` behavior.

Whole-invocation summaries are computed separately for every supported external
entry: each Core function entry in `LoweringMap`, plus this pack's generated load-tag
entry. Private helpers such as init still have local summaries but are not mislabeled
as supported roots. The reports do not claim to model other datapacks appended to the
global `minecraft:load` tag. Each metric uses a capped range rather than overloading
one `Unbounded` case:

```text
CountUpper =
    Finite(u64)
  | AboveAnalysisCap
  | NoFiniteBoundProven { cause: PositiveCycle | SelectorCardinality }
  | Unknown { reason }

CountBound { lower: u64, upper: CountUpper }

RootExecutionSummary {
  resolved_root
  entry_region: Internal(CostRegionId) | ExternalTagEntry
  command-sequence operations: CountBound
  execute stages: CountBound
  internal function invocations: CountBound
  score/NBT command executions: CountBound
  maximum reachable individual ordinary fork-checked redirect expansion: CountBound
}
```

Keep `CountBound` fields private behind smart constructors. `Finite(max)` requires
`lower <= max`. `AboveAnalysisCap` requires either an in-domain finite upper witness
above the retained report cap or checked `u64` overflow proving that the mathematical
finite result crossed it. The lower may remain below the cap when only an optional
expensive path crosses it; once the lower itself crosses, retained lower arithmetic
saturates at `min(cap + 1, u64::MAX)`. The report retains the cap that gives this state
meaning. This avoids duplicated `at_least` fields, represents mixed cheap/high
alternatives, and stops acyclic doubling from failing after the answer is already
known to exceed the hard-limit comparison.

`AboveAnalysisCap` is a known finite result in an analyzed acyclic region whose exact
arithmetic was intentionally stopped once it could no longer affect a target-limit
comparison.

The deterministic arithmetic cap is an analyzer option and must be at least one more
than the corresponding configured hard-limit assumption (computed in `u64`). Thus
`AboveAnalysisCap` can never hide whether a finite root crosses the limit it is being
checked against.

`NoFiniteBoundProven` means either that a reachable positive-weight cycle has no
proved trip bound or that an understood selector has no configured cardinality
bound; it does not pretend the compiler proved mathematical divergence. `Unknown` is
reserved for behavior whose command semantics are not modeled. These states must
never be silently converted into one another by overflow or an analysis budget.

The cycle classification is per metric: a reachable cycle with zero NBT weight does
not make an exact zero NBT count unbounded merely because its sequence-operation
weight is positive. Acyclic structural regions and acyclic feasible subcomponents use
checked, outcome-sensitive compositional path maxima. Cyclic feasible subcomponents
retain exact member/transition local costs before applying the conservative component
envelope and per-metric cycle effect; Stage 5 does not infer a loop trip bound. Mojang
counts single-context command execution, ordinary `execute` stages, and every function
invocation toward the sequence limit. Exact 26.2 server inspection adds two custom
executor exceptions: native `return value`/`return fail` do not increment sequence
cost, and `execute if/unless function` remains an execute stage but its custom
modifier does not independently increment sequence cost beyond the function
invocations it queues. Function-condition custom modifiers also bypass the generic
`BuildContexts` fork guard, so their surviving context is not an ordinary checked
redirect expansion. Ordinary execute modifiers still increment once when the current
context list is empty; only final body work disappears. The fork gamerule is
different: it bounds context expansion at one ordinary checked redirect, not the sum
of contexts produced by every command in a root. Consequently a loop containing a
bounded three-way fork has a three-context fork bound even when its sequence work has
no finite bound. Each ordinary checked-redirect expansion is retained locally, and
the root reports the maximum reachable expansion. The root invocation is included in each public root's sequence and
function-invocation ranges before its body; nested calls likewise count their
invocation before callee work.

The report compares proven finite/capped ranges with the configured hard-limit
assumptions and also displays the selected target defaults. Exceeding an assumption
or lacking a finite caller-controlled-loop bound is reported, not silently rejected:
server configuration may differ and Stage 4 already assigns responsibility to the
caller.

### Empirical measurements

`MeasurementRecord` owns build and Minecraft target identity, fixture, warm-up/sample
schedule, sample count, and raw observations. It can additionally own a server hash,
Java version, and configured heap when—and only when—the recorded subject actually
launches that JVM. Wall time, tick time, reload time, and macro-cache behavior never
become universal weights without a separate reviewed decision.

The harness schema is versioned JSONL and records the git revision/dirty state, exact
build-time `rustc -vV`, Cargo profile, build target triple, host OS/architecture, and
selected Minecraft target. Samples remain in execution order and store integer
nanoseconds; the harness does not average them. Its checked constructor rejects an
ordinal, count, subject, configuration, or order that disagrees with the recorded
protocol. The release suite measures several counterbalanced rounds and interleaves
all four Core `None|Baseline` × Minecraft `None|Baseline` configurations for each
subject after explicit warm-ups on tiny, normal, and scale fixtures. It measures Core
optimization (excluding fixture cloning), complete Minecraft lowering, target-cost
analysis, emission, and the complete optimize-to-lower-to-emit path. Compact exact
target-cost census/root summaries and artifact-footprint totals are printed separately
from these environment-specific timings; complete compiler dumps remain available on
their owning typed reports but are intentionally not repeated for every scale sample.

Compiler-private hooks observe every Core pipeline step and every lowering phase,
including explicit skipped events. The production path instantiates zero-sized no-op
observers; hooks do not enter public options or deterministic reports. This mirrors
MLIR's separation of pass statistics from timing instrumentation, rustc's separate
self-profile/performance tooling, Cranelift's internal timing scopes, and GHC/OCaml's
separate timing or profiling facilities.

### Candidate comparison

Do not sum unrelated dimensions into one magic number, and do not invent execution
frequencies. LLVM's block-frequency analysis explicitly models relative/expected
frequency and requires branch probabilities; Stage 5 has neither profiles nor source
probabilities. A target policy therefore uses strict dominance where possible, an
explicit lexicographic priority for genuinely comparable candidates, and otherwise
retains the Stage 4 recipe. The baseline policy is:

1. legality and target-limit safety;
2. require a typed local graph-contraction certificate proving that no root's
   finite/unproven/unknown limit classification or metric can worsen;
3. prefer a candidate whose exact local cost is no worse in every runtime dimension
   and strictly better in at least one;
4. do not run or approximate whole-root analysis during selection; a future
   root-bound tier requires explicit reviewed inputs and remains outside Stage 5;
5. use smaller exact pre-emission structured size (commands, functions, and helpers)
   only after runtime dimensions do not regress; and
6. use a stable recipe ordinal only to break otherwise semantically/cost-identical
   choices, never to resolve a real unknown trade-off.

Each recipe uses one shared typed accounting algebra, and the chosen recipe's
predicted local delta is reconciled against the commands actually constructed. A
selected recipe must recount to the same command-step/local-path summary; the final
whole-program analyzer then derives root ranges from those constructed commands.
Post-emission byte footprint is evidence about the chosen program, not an input to
this pre-emission recipe decision. This prevents candidate formulas, emitted
structure, and final target accounting from drifting silently.

Do not run whole-program root analysis once per candidate. A baseline candidate must
either prove that it preserves call/control graph classification and compare a closed
local replacement, or fall back to the Stage 4 recipe. Build the full cost graph once
for the selected verified program. Future global search may add incremental cost
maintenance, but Stage 5 does not need that complexity for a few closed local recipes.

## Inlining and outlining

Inlining is not a Stage 5 completion requirement.

Rust separates MIR optimization levels, GHC uses phased size-relative simplifier
budgets, and OCaml Flambda makes call-site decisions using context, removed operations,
code growth, and bounded speculation. A definition-size threshold alone would be a
poor fit here because Minecraft calls also include fixed-slot parameter/result copies
and inlining may expose constant branches.

Stage 5 should measure direct call/copy overhead and produce the report data needed by
a later call-site inliner. Implementing a sound interprocedural editor, cloning
provenance, recursive-SCC policy, speculative cleanup, and growth accounting merely to
check a roadmap box would overwhelm the baseline work. Cost-directed inlining,
specialization, cold outlining, and region duplication belong in Stage 11 unless a
small concrete Stage 5 fixture demonstrates a simpler self-contained transform.

## Determinism, failure, and provenance

- Stable IDs and authoritative layout control every traversal and tie-break.
- Hash maps may serve keyed lookup but never determine output or diagnostic order.
- Expected non-matches and exhausted optional-analysis budgets take only their typed
  conservative fallback; required verification/construction exhaustion fails.
- Internal pass failure stops the pipeline and discards the compilation unit; no
  partial target or rollback promise escapes.
- Stage 5 does not mutate `SourceContext`. Constant replacements inherit the replaced
  instruction's existing origin; straight-line fusion moves instructions with their
  origins; CSE keeps the representative's origin. The optional applied-rewrite record
  correlates every eliminated instruction/value and origin with its representative,
  so explainability does not require changing the surviving entity's provenance.
- A selected target recipe records all contributing semantic origins in its linear
  decision record. Each emitted command retains one truthful primary action origin
  (for example the call origin in a tail call); compiler-only completion scaffolding
  remains `OriginId::UNKNOWN`. Do not change `lower_to_minecraft` to take a mutable
  source context merely to manufacture compound origins.
- The unoptimized Stage 4 mode remains available and byte-stable.
- Optimization reports retain stable reason codes so prose may improve without
  breaking tools.

## Proof strategy

The five Core passes have individual algorithm/proof plans under
[`stage-5-passes/`](stage-5-passes/README.md). A pass implementation may refine its
note when evidence demands it, but the note and checklist must be reviewed together
before the code lands.

Every Core transform needs:

1. exact before/after verified Core fixtures;
2. non-match and editor atomicity tests;
3. generated small-CFG differential tests against unoptimized Core; and
4. scale tests that detect accidental repeated whole-function scans.

Every physical or control optimization needs:

1. an independent plan verifier rather than reuse of the choosing algorithm;
2. exact plan, decision-report, target, artifact, and trace fixtures;
3. optimized-versus-Stage-4 differential execution;
4. corruption tests for placement, aliasing, footprints, and predicted cost; and
5. vanilla tests whenever equivalence depends on Minecraft return, execute, score,
   fork, or command-limit behavior.

Property-style fixtures should cover small typed cyclic and acyclic CFGs, integer
boundaries, executable-edge combinations, copy graphs, liveness overlap, and branch
mutation. The fast Stage 3 executor may accelerate supported subsets, but vanilla is
the target semantic authority.

## Revised implementation slices

### 5A — Close the Stage 4 boundary and record the baseline

- fix the incomplete lowering decision dump and replace digest-only “exact” proofs
  with complete golden fixtures;
- define exact post-emission `ArtifactFootprintReport`, plus pre-emission
  command-step/local-path summaries, the derived cost graph, and per-root execution
  summaries for unchanged Stage 4 output;
- add typed hard-limit assumptions defaulting to `TargetSpec`, without adding soft
  scheduling budgets or changing gamerules;
- add boundary tests against target sequence/fork defaults;
- prove cost analysis shares SCC/region summaries across many supported roots rather
  than traversing the graph once per root;
- assert exact construction, solver-work, and evaluated-command-transfer counts on
  meaningful wide, deep, cyclic, and many-root fixtures, with a separate ignored
  threshold-free release timing probe; and
- preserve byte-identical baseline mode.

### 5B — Owned optimizer boundary and narrow editor batches

- add the consuming `optimize_core` API and return only a verified program plus its
  report; failures retain byte-capped safe snapshots but never expose an intermediate
  body;
- after the real 5C/5D consumers exist, replace the public generic pass trait/runner
  with a closed internal `CorePassKind` pipeline while exposing optimization levels,
  typed outcomes, and the reusable checked `FunctionEditor`; never install no-op pass
  variants merely to satisfy the framework shape;
- add ordered aggregate statistics, runner-owned bounded opt-in remarks,
  pass-specific typed size-relative limits, safe fallback statuses, and separate
  nondeterministic timing instrumentation;
- document that dumps are diagnostic-only and keep reproducibility in Rust fixtures or
  recorded property-test seeds rather than adding a Core serialization format;
- add only the consumer-driven editor operations listed above, implemented as true
  one-snapshot/one-scan batches rather than loops over single-edit methods;
- make boundary-only verification real through a private safe trusted-open editor
  path, while retaining verify-each in tests/debug and an override; and
- prove preflight atomicity, bounded remarks, verifier-call counts, and no per-pass
  whole-function clones.

### 5C — Canonicalization, SCCP, and DCE

- implement the closed cheap canonicalizer;
- implement the typed SCCP lattice and executable-edge solver;
- apply constants and branch folds once only after complete convergence; discard all
  SCCP-derived decisions if its solver stops at a limit;
- detach unreachable blocks and eliminate dead `Pure + Always` instructions by
  worklist;
- add exact, generated, scale, and differential tests.

### 5D — Straight-line fusion and local CSE

- use one opaque prepared-fusion edit to discover/apply head-first unconditional jump
  regions whose consumed blocks have exactly one incoming edge;
- preserve stable detached identities and provenance;
- add IR-owned exhaustive structural-result-equivalence and operand-symmetry queries
  distinct from effects/speculation, an optimizer-owned exhaustive expression-key
  builder, and then dominance-scoped multi-result-aware local CSE;
- run one bounded cleanup group and record its benefit;
- prove the pipeline remains approximately linear for non-pathological input.

### 5E — Refactor planning and prune dead physical state

- add `MinecraftOptimizationLevel::{None, Baseline}` with `None` as the existing
  constructor default, evolve the current emitted analysis into one
  `SemanticInventory`, and make target-vocabulary/nonrecursion checking a fallible
  gate with disposable SCC scratch rather than a retained success token;
- compute bounded backward `RuntimeDemand`, discarding every incomplete solution for
  an explicit all-reachable fallback; omit only undemanded discardable target
  instructions, homes, call-result copies, and edge copies while retaining CFG
  control, calls, and complete pinned ABI;
- freeze distinct typed `HomeAssignment`s together with exact-arity
  `InstructionPlan`s, representing undemanded fixed-recipe outputs as physical recipe
  temporaries and unused caller results as indexed `None` destinations;
- let `EdgeTransferPlan` own lazily discovered typed scratch, then allocate a
  `ResourceInventory` for every reachable block and only the branch helpers whose
  transfers remain nonempty; and
- flatten the parts deterministically, independently verify them, and make the
  emitter consume only the verified plan. Preserve Stage 4 home/resource/name/report
  order and complete output bytes under `None`.

### 5F — Sparse liveness and conservative coalescing

- insert the first real sparse `LivenessResult` between runtime demand and home
  assignment, suitable for the scale fixture;
- pin ABI homes and forbid cross-function reuse;
- greedily coalesce block-argument copies in stable order;
- deterministically reassign the Stage 5E physical-register ordinal naming policy to
  the coalesced homes while preserving all Stage 4 names under the `None` policy;
- resolve remaining transfers once;
- test exact chooser noninterference, then independently verify actual typed home
  contents, fixed-recipe access, and transfer semantics without consuming chooser
  liveness. Prove numeric Boolean normalization separately by composition: normalized
  external inputs, closed normalized-result recipes, and same-type copies.

### 5G — Target recipes and costed branch selection

- add `ControlRecipePlan`, `BlockPlacement`, and only the minimal recipes exercised by
  real fixtures;
- first consume uniquely reached terminal arms that combine into one legal target
  command, including a measured zero-ABI tail-call fixture;
- in 5G.4, keep `PhysicalEffectSummary` and condition-stability analysis deferred
  until a complete profitable stability-dependent recipe becomes their first real
  consumer; that future analysis must use frozen homes/transfers and reject an
  incomplete proof rather than feeding recipes back into coalescing;
- add dual-guard or snapshot recipes only with a complete profitable fixture;
- compare all constructed candidates through shared accounting and explain the result;
- retain the baseline recipe when missing frequency/bound information makes a real
  trade-off incomparable;
- run mutation-trap, differential, exact-output, and vanilla proofs.

### 5H — Completion and roadmap reconciliation

- [x] expose typed, owned lowering/report and harness-measurement boundaries;
- [x] measure every Core step and lowering phase plus complete compilation without
  adding wall-time thresholds or an inliner;
- [x] run generated reproduction, corruption, scale, benchmark, determinism, and
  one-startup optimized-Core official-server gates;
- [x] confirm the revised Stage 6, Stage 9, and Stage 11 ownership in
  [`stage-5-handoff.md`](stage-5-handoff.md); and
- [x] retain stable dual-guard/snapshot recipes, wider condition-stability analysis,
  and global optimization as explicit deferrals rather than completion requirements.

## Decisions now considered settled

1. Core optimization, physical planning, target recipes, and cost reporting remain
   separate layers.
2. The Stage 4 lowering is a permanent correctness/reference mode.
3. Core passes report structural facts; lowering recipes report target costs.
4. Post-emission footprint, pre-emission execution cost/bounds, and empirical
   measurements are separate types with different owners.
5. No generic rewrite registry, analysis manager, transactional rewriter, or new
   public IR is introduced in Stage 5.
6. SCCP is analysis-first and typed; calls are overdefined.
7. Stable allocated Core identities are detached, not physically deleted.
8. Physical homes are typed storage locations. Stage 5 coalesces only same-type
   block-argument copies; ABI homes remain pinned and cross-type reuse is deferred.
9. Branch selection owns a narrow block-placement decision and always retains the
   return dispatcher fallback.
10. Inlining and outlining move to Stage 11 unless a concrete bounded transform earns
    its way back into Stage 5.
11. Core optimization consumes its input and returns the verified optimized program
    with its report. Later lowering, execution-cost, footprint, and measurement
    reports remain separately owned; Stage 6 may aggregate them without collapsing
    their determinism or failure boundaries.
12. Aggregate statistics are always bounded; Core per-rewrite remarks are filtered,
    capped, and opt-in, while lowering records one stable decision per branch arm.
13. Exact command-step weights and finite local-path ranges are retained independently
    from per-root bounds, so cyclic code remains optimizable and explainable.
14. Every supported external entry receives its own root summary; private helpers
    retain local summaries, and the compiler does not claim ownership of other
    datapacks' global tag composition.
15. Mutating pipelines unconditionally verify input/final output; `None` verifies its
    unchanged value once. Tests and debug builds verify every executed pass; optimized
    builds may use boundary verification. This remains an internal runner choice, not
    an optimization level.
16. Stage 5 preserves the immutable `SourceContext` boundary; rewrite and recipe
    correspondence lives in bounded reports instead of synthetic provenance records.
17. One typed sequence/fork-assumption value is shared by lowering recipe checks and
    explicit Stage 5 cost analysis; target defaults remain the compatibility default,
    and soft per-tick budgets remain Stage 9.
18. Physical pruning is driven by backward runtime demand: undemanded
    `Pure + Always` producers disappear, calls and public ABI remain, and lowering
    never requires Core DCE.
19. The generic function-pass trait/runner is replaced by compiler-internal closed
    `CorePassKind` algorithms and stable `CorePipelineStep` invocation IDs. Public
    callers select a level; `FunctionEditor` remains the reusable checked mutation
    API.
20. Stage 5 uses pass-local immutable facts and true batch edits. It does not add a
    general analysis manager, invalidation protocol, or repeated single-edit fallback.
21. Effects, speculation, structural result equivalence, and operand symmetry are
    separate exhaustive Core queries; the optimizer owns expression keys, and CSE
    never treats `Pure` as sufficient evidence.
22. Planner ordering is expressed by ordinary immutable phase results and borrowed
    prerequisites, not a public physical IR or generic typestate state machine.
23. Count bounds are smart-constructed as a lower bound plus a finite,
    above-analysis-cap, no-finite-bound-proven, or unknown upper classification; an
    invalid finite interval or cap state cannot be constructed.
24. Normal pass execution never renders whole-program before/after dumps. Failure
    snapshots are deterministic and byte-capped; full dumps are explicit developer
    instrumentation written outside deterministic reports.
25. Limits are typed per computation. Incomplete SCCP facts are discarded, incomplete
    liveness uses distinct homes, incomplete stability rejects the recipe, and
    incomplete cost bounds become `Unknown`; required verification never degrades.
26. Optimized physical planning is a one-way immutable graph. Stage 5E owns inventory,
    legality, demand, instruction/home assignment, transfers, and all-reachable
    resources; Stage 5F adds real liveness before Baseline assignment and freezes
    coalescing before deriving transfers; Stage 5G adds recipes, placement, and
    placement-aware resources. Physical effects land only with a consuming recipe.
    Recipes never iterate back into coalescing.
27. The complete local pass pipeline runs per function in stable order. Local passes
    may read immutable signatures but cannot initiate mutable whole-program analyses.
28. Core dumps are diagnostic-only. Stage 5 adds no parser/serializer or replayable
    crash-reproducer format; Rust fixtures and recorded property seeds reproduce
    failures without retaining every intermediate program.
29. Simple DCE requires `Pure + Always`; Stage 5 CSE requires
    `Pure + Always + Structural`. Purity alone is never a proof of termination or
    deterministic substitutability.
30. Canonicalization and CSE plan substitutions virtually before one batch. Fusion's
    opaque exclusive prepared edit owns its fact snapshot and applies maximal regions
    once. None may hide a repeated full-body editor scan behind a per-match loop.
31. Target cost analysis is an explicit read-only analysis of verified Stage 3, not a
    lowering phase. Configured limits return typed unknowns; true analysis failure
    exposes no partial report and does not discard a valid `LoweringOutput`.
32. Per-invocation pass statistics use closed inline structs, then merge into fixed
    step summaries; compiling many tiny functions does not allocate a statistics map
    or vector for every invocation.
33. Optimized parallel-copy scratch is lazy and typed: at most one temporary per
    `(function, CoreType)`. The `None` path preserves Stage 4's exact single-temp
    representation and names.
34. Semantic liveness does not by itself authorize result/operand home aliasing. A
    private exhaustive Minecraft scalar access contract describes the selected
    recipe's safe reuse positions, and the independent symbolic verifier rechecks
    ordered reads/writes, simultaneous results, and fixed-slot ABI boundaries.
35. Liveness and coalescing are complete-or-fallback per function. Propagation work,
    retained segments, and coalescing work are independently bounded; a fallback
    publishes no segment facts or tentative unions and reports zero retained segments
    or merges.
36. Coalescing candidates follow authoritative semantic-edge and parameter order.
    Normalized value pairs exist only for deterministic first-occurrence deduplication;
    they never reorder the winner. ABI entry homes are exclusive singletons.
37. Production liveness retains only a dense optional value-to-span index and a flat
    sorted segment array. Live-in/live-out sets are disposable solver scratch, and the
    20,000-simultaneously-live-value gate forbids a dense interference matrix.

## Explicitly deferred experiments

Additional uniquely reached terminal shapes, stable dual guards, and condition
snapshots are not checklist blockers. Admit one only when a complete fixture proves
its emitter, verifier, completion-result semantics, mutation safety, accounting
reconciliation, and concrete advantage over the dispatcher together. Otherwise keep
the Stage 4 recipe and record the experiment for a later stage.

## Cross-compiler research applied

The middle column records behavior stated by the primary source. The final column is
our design inference for this compiler; it should not be read as a claim made by the
upstream project.

| Compiler | Relevant finding | Stage 5 consequence |
| --- | --- | --- |
| Rust | The MIR query pipeline takes exclusive ownership of bodies before mutation; the pass manager uses an explicit ordered list, mutates one body, and can validate around every pass. Its closed instruction simplifier is a small ordered sweep; its CFG simplifier counts predecessor edges, takes terminators, and batches statement movement/reservation. `rustc_data_structures::WorkQueue` is a deduplicating queue backed by a deque and dense occupancy bitset. MIR destination propagation separately collects copy candidates, models before/after statement timing, joins only disjoint live ranges, and keeps required argument/return locals fixed. | Make `optimize_core` consuming, use a closed local pipeline, give canonicalization only the worklist machinery its rules need, use move-based prepared fusion, make verify-each configurable, and keep copy preference, exact access timing, interference, and pinned ABI constraints separate. |
| Zig | AIR is dense and per-function. Its liveness result uses compact per-instruction tomb bits for common cases, a sparse side table for control/special cases, and temporary loop-analysis data that is not retained by backends. | Keep Core dense and analyses disposable; retain only consumer facts, use sparse exceptional storage, and drop loop/worklist scratch rather than creating permanent analysis state. |
| GHC (Haskell) | Core passes have an explicit `CoreToDo` algebra and transform an owned `CoreProgram`/`ModGuts` value. Cmm dataflow separates lattice joins/facts from transfer and rewrite functions; occurrence analysis uses dependency SCCs for dead recursive groups; demand analysis represents cardinality as a powerset over zero, one, and many; CSE substitutes before reverse lookup. The native graph allocator computes liveness first, coalesces move-related registers only under those facts, and carries a canonical rename map into a later patch. Simplifier ticks are size-relative, while compact statistics and verbose traces are separate. | Use a closed owned pipeline, analysis-then-application APIs, fixed outcome/cardinality transfer cells, keep leaf DCE distinct from SCC-aware deletion, resolve planned values before CSE keying, use typed limits/bounded cleanup, and separate normal/detail reporting. Freeze deterministic semantic-to-physical names after liveness-qualified grouping. |
| OCaml | Flambda inlining is decided at call sites using exposed simplification benefit, code growth, bounded depth/unrolling, and per-round reports. The native backend constructs hard interference and weighted copy preference separately, excludes interfering/different-class pairs from preferences, and accounts for simultaneous results. | Defer threshold-only inlining; first build truthful call/copy accounting, and keep coalescing candidates distinct from type/interference legality. |
| MLIR | Canonicalization is bounded and best-effort; `PatternRewriter` requires a successful match before mutation and leaves visitation/cost policy to the driver; sparse dataflow separates executable-edge and value subscriptions; dataflow lattice joins must be monotone and report whether state changed; effects, speculation, structural equivalence, and commutativity are distinct contracts; `getSinglePredecessor` counts duplicate edges; consecutive function passes run function-by-function. Analyses are read-only values separate from transformations. One-Shot Bufferize asks operations which operands read/write and results may alias. | Lowering cannot require canonical form; separate legality from recipe preference; do not import a generic scalar pattern-benefit score into Minecraft's multi-dimensional cost policy; incomplete optimistic SCCP cannot rewrite; exact successor occurrences matter; use a finite monotone feasibility lattice and enqueue dependents only on change; group the local pipeline per function; keep final target-cost instrumentation an explicit analysis; and make target recipe alias timing explicit. |
| LLVM | SCCP solves constants/executable blocks before rewriting and resolves unknown executable facts; simple DCE uses a producer worklist; EarlyCSE uses an iterative dominator walk and scoped table; block merge moves structure while updating analyses. Its SCC iterator uses an internal visit stack and emits SCCs callee-first in reverse topological order. GlobalISel gives each lowering phase an explicit completion invariant and uses verification between serializable boundaries; instruction selection may fold through use-def chains but must leave no generic instruction behind. The pass manager distinguishes analyses from transforms and restricts outer-scope computation. Two-address lowering represents tied def/use explicitly; the register coalescer treats copies as a worklist over live intervals and has explicit compile-time cutoffs for repeatedly visited large intervals. | Give SCCP an all-or-nothing event solution, DCE/CSE their own algorithms and fallbacks, use prepared move-based fusion, keep cost analysis separate from lowering mutation, use explicit-stack callee-first SCC traversal for deep target call graphs, and publish each Minecraft physical phase only after its own closed invariant is complete. Represent safe target reuse explicitly and bound coalescing work rather than relying on elapsed time. |
| Cranelift | ISLE uses strongly typed closed lowering terms, permits overlapping legal rules, and separates which rewrites are correct from explicit priority/selection strategy; correctness must not depend on an unspecified specificity tie-break. Dominator-tree construction stores explicit DFS and path-evaluation worklists and computes postorder without host recursion. | Keep Stage 5 recipes closed and typed, prove every candidate legal independently of preference, use stable order only for true equality, and keep deep target-graph traversals iterative with disposable traversal stacks. |
| GCC | SSA-CCP joins PHIs using executable edge objects, simple and aggressive DCE are separate, and CFG maintenance uses dedicated verified hooks. Its pass manager records ordered passes/properties but does not automatically regenerate every needed IR-side structure. | Give Core successor arms exact identity, keep leaf DCE conservative, route fusion through one checked editor owner, keep phase prerequisites explicit, and never imply a generic manager repairs stale facts. |
| Binaryen | General IR and physical Stack IR exist for different consumers; coalescing, local CSE, block merging, deterministic output, fuzzing, and target cleanup are separate concerns. | Add no physical IR without a concrete recipe consumer; keep coalescing and target placement in the private plan and fuzz alternative pipelines. |
| Swift | Mandatory canonical SIL transformations are distinct from optional SIL optimizations and lower-level LLVM work; its pass manager supports verify-all and selected before/after verification. | Keep verified Core legal without optimization, and make expensive per-pass checking explicit compiler instrumentation rather than semantics. |
| regalloc2 | SSA block parameters are treated as edge uses and block-entry defs; dense liveness matrices were rejected because of quadratic space. Operands separately encode early/late effects and def/use/mod kind, while `Reuse` ties a def to a particular use. Its checker symbolically propagates virtual-value equivalence through allocations, moves, and parallel block-parameter assignments, then validates every operand use. | Use sparse liveness for choosing homes, represent Minecraft recipe access/reuse separately, and independently verify the frozen assignment with symbolic home-content dataflow rather than trusting chooser facts. |
| Minecraft Java | Sequence limits count single-context command execution, ordinary `execute` stages, and function invocation; functions without `return` have no result. In 26.2, return custom executors and function-condition custom modifiers have the special accounting described above, function conditions bypass the generic fork guard, ordinary tags run all members, return-mode tags stop at the first return, and an ordinary fork check rejects expansion greater than or equal to the gamerule. | Use an outcome-sensitive multi-dimensional weighted execution graph, model custom accounting and fork checks by command kind rather than a universal formula, preserve ordered tag modes, include root calls, and compare ordinary checked-redirect expansion with a strict `<` boundary. |

## Repository references audited

- Closed owned Core pipeline, ordered pass algebra, reporting, and verification
  instrumentation:
  [`opt/core/mod.rs`](../../crates/mdl-compiler/src/opt/core/mod.rs) and
  [`opt/core/pipeline.rs`](../../crates/mdl-compiler/src/opt/core/pipeline.rs)
- Current checked editor and its atomic prepared batch operations:
  [`ir/core/edit.rs`](../../crates/mdl-compiler/src/ir/core/edit.rs)
- Current Core semantic-query and public re-export boundary:
  [`ir/core/mod.rs`](../../crates/mdl-compiler/src/ir/core/mod.rs)
- Append-only provenance and the immutable `SourceContext` consumer boundary:
  [`source.rs`](../../crates/mdl-compiler/src/source.rs)
- Current lowering API/report ownership:
  [`lower/minecraft/api.rs`](../../crates/mdl-compiler/src/lower/minecraft/api.rs) and
  [`plan/report.rs`](../../crates/mdl-compiler/src/lower/minecraft/plan/report.rs)
- Immutable physical phase boundary and final independent publication:
  [`assignment.rs`](../../crates/mdl-compiler/src/lower/minecraft/assignment.rs),
  [`edge_transfer.rs`](../../crates/mdl-compiler/src/lower/minecraft/edge_transfer.rs),
  [`resources.rs`](../../crates/mdl-compiler/src/lower/minecraft/resources.rs), and
  [`plan/assemble.rs`](../../crates/mdl-compiler/src/lower/minecraft/plan/assemble.rs)
- Stage 5G recipe accounting, explicit placement, and constructed-command
  reconciliation:
  [`recipe.rs`](../../crates/mdl-compiler/src/lower/minecraft/recipe.rs),
  [`placement.rs`](../../crates/mdl-compiler/src/lower/minecraft/placement.rs), and
  [`plan/reconcile.rs`](../../crates/mdl-compiler/src/lower/minecraft/plan/reconcile.rs)
- Stage 5F liveness, coalescing, and independent symbolic validation:
  [`liveness.rs`](../../crates/mdl-compiler/src/lower/minecraft/liveness.rs),
  [`coalescing.rs`](../../crates/mdl-compiler/src/lower/minecraft/coalescing.rs), and
  [`plan/symbolic.rs`](../../crates/mdl-compiler/src/lower/minecraft/plan/symbolic.rs)
- Current ordered scalar and fixed-slot ABI recipes whose access timing constrains
  optimized home reuse:
  [`scalar.rs`](../../crates/mdl-compiler/src/lower/minecraft/scalar.rs),
  [`call.rs`](../../crates/mdl-compiler/src/lower/minecraft/call.rs), and
  [`control.rs`](../../crates/mdl-compiler/src/lower/minecraft/control.rs)
- Current authoritative reachable semantic inventory:
  [`analysis.rs`](../../crates/mdl-compiler/src/lower/minecraft/analysis.rs)
- Current target-vocabulary and iterative recursive-SCC legality gate:
  [`audit.rs`](../../crates/mdl-compiler/src/lower/minecraft/audit.rs)
- Stage 3 command, execute, and coarse effect/fork contracts:
  [`command.rs`](../../crates/mdl-compiler/src/ir/minecraft/command.rs),
  [`execute.rs`](../../crates/mdl-compiler/src/ir/minecraft/execute.rs), and
  [`contract.rs`](../../crates/mdl-compiler/src/ir/minecraft/contract.rs)
- The actual post-lowering emission boundary and owners of artifact bytes/trace:
  [`datapack/emit.rs`](../../crates/mdl-compiler/src/datapack/emit.rs) and
  [`datapack/mod.rs`](../../crates/mdl-compiler/src/datapack/mod.rs)
- Harness-owned measurement schema, interleaved release suite, and optimized-Core
  vanilla differential:
  [`measurement.rs`](../../crates/mdl-test/src/measurement.rs),
  [`stage5_measurements.rs`](../../crates/mdl-test/tests/stage5_measurements.rs), and
  [`core_lowering.rs`](../../crates/mdl-test/tests/core_lowering.rs)
- Selected 26.2 defaults:
  [`target/java_26_2.rs`](../../crates/mdl-compiler/src/target/java_26_2.rs)
- Project decisions and measured vanilla limit behavior:
  [`stage-0-decisions.md`](stage-0-decisions.md),
  [`stage-4-first-lowering-plan.md`](stage-4-first-lowering-plan.md), and
  [`command-limits-and-multi-tick.md`](../mcfunction/command-limits-and-multi-tick.md)

## Primary references

- Rust MIR optimization guide: <https://rustc-dev-guide.rust-lang.org/mir/optimizations.html>
- Rust MIR pass pipeline and optimized-body queries: <https://rustc-dev-guide.rust-lang.org/mir/passes.html>
- Rust MIR pass-manager source: <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/pass_manager.rs.html>
- Rust MIR query implementation and exclusive body transfer: <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/lib.rs.html>
- Rust MIR CFG simplification: <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/simplify.rs.html>
- Rust MIR closed instruction simplification: <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/instsimplify.rs.html>
- Rust MIR dead-store elimination: <https://doc.rust-lang.org/nightly/nightly-rustc/src/rustc_mir_transform/dead_store_elimination.rs.html>
- Rust MIR destination propagation and conservative place merging: <https://github.com/rust-lang/rust/blob/main/compiler/rustc_mir_transform/src/dest_prop.rs>
- Rust MIR structure: <https://rustc-dev-guide.rust-lang.org/mir/index.html>
- Rust MIR validator API: <https://doc.rust-lang.org/nightly/nightly-rustc/rustc_mir_transform/validate/index.html>
- Rust compiler deduplicating work queue: <https://doc.rust-lang.org/nightly/nightly-rustc/rustc_data_structures/work_queue/struct.WorkQueue.html>
- Rust MIR dataflow framework and block-entry result model: <https://doc.rust-lang.org/nightly/nightly-rustc/rustc_mir_dataflow/index.html>
- Rust MIR dataflow design guide: <https://rustc-dev-guide.rust-lang.org/mir/dataflow.html>
- Rust compiler testing layers and package-test guidance: <https://rustc-dev-guide.rust-lang.org/tests/intro.html>
- Rust compiler profiling and self-profile tooling: <https://rustc-dev-guide.rust-lang.org/profiling.html>
- rustc-perf collector protocol: <https://github.com/rust-lang/rustc-perf/blob/main/collector/README.md>
- Zig reproducible release-build and compiler-performance evidence: <https://ziglang.org/download/0.11.0/release-notes.html>
- Zig AIR: <https://codeberg.org/ziglang/zig/src/branch/master/src/Air.zig>
- Zig AIR liveness: <https://codeberg.org/ziglang/zig/src/branch/master/src/Air/Liveness.zig>
- Zig AIR liveness verifier: <https://github.com/ziglang/zig/blob/738d2be9d6b6ef3ff3559130c05159ef53336224/src/Air/Liveness/Verify.zig>
- Zig AIR legalization: <https://codeberg.org/ziglang/zig/src/branch/master/src/Air/Legalize.zig>
- Zig AIR verifier: <https://codeberg.org/ziglang/zig/src/branch/master/src/Air/Verify.zig>
- GHC Core optimization pipeline: <https://gitlab.haskell.org/ghc/ghc/-/blob/master/compiler/GHC/Core/Opt/Pipeline.hs>
- GHC Core pipeline types and explicit pass algebra: <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/GHC-Core-Opt-Pipeline-Types.html>
- GHC Cmm dataflow lattice and rewrite API: <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/GHC-Cmm-Dataflow.html>
- GHC Core demand-analysis implementation: <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Core.Opt.DmdAnal.html>
- GHC demand cardinality algebra: <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Types.Demand.html#L381>
- GHC Core CSE implementation and design notes: <https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/src/GHC.Core.Opt.CSE.html>
- GHC optimization and simplifier budgets: <https://ghc.gitlab.haskell.org/ghc/doc/users_guide/using-optimisation.html>
- GHC compiler dumps, simplifier statistics, and verbose traces: <https://ghc.gitlab.haskell.org/ghc/doc/users_guide/debugging.html>
- GHC native-code register liveness: <https://gitlab.haskell.org/ghc/ghc/-/blob/master/compiler/GHC/CmmToAsm/Reg/Liveness.hs>
- GHC graph-register coalescing and canonical renaming: <https://gitlab.haskell.org/ghc/ghc/-/blob/master/compiler/GHC/CmmToAsm/Reg/Graph/Coalesce.hs>
- OCaml Flambda optimization and inlining: <https://ocaml.org/manual/5.5/flambda.html>
- OCaml Flambda call-site inlining decisions and typed rejection outcomes: <https://github.com/ocaml/ocaml/blob/trunk/middle_end/flambda/inlining_decision.ml>
- OCaml native-code liveness: <https://github.com/ocaml/ocaml/blob/trunk/asmcomp/liveness.ml>
- OCaml native-code interference construction: <https://github.com/ocaml/ocaml/blob/trunk/asmcomp/interf.ml>
- OCaml compiler profiling scopes: <https://github.com/ocaml/ocaml/blob/trunk/utils/profile.ml>
- OCaml Flambda inlining statistics: <https://github.com/ocaml/ocaml/blob/trunk/middle_end/flambda/inlining_stats.ml>
- MLIR pass infrastructure: <https://mlir.llvm.org/docs/PassManagement/>
- MLIR pattern rewriting, legality-before-mutation, and driver-owned cost policy: <https://mlir.llvm.org/docs/PatternRewriter/>
- MLIR side effects and speculation: <https://mlir.llvm.org/docs/Rationale/SideEffectsAndSpeculation/>
- MLIR pass validity/verifier convention: <https://mlir.llvm.org/getting_started/DeveloperGuide/>
- MLIR testing guide: <https://mlir.llvm.org/getting_started/TestingGuide/>
- MLIR canonicalization: <https://mlir.llvm.org/docs/Canonicalization/>
- MLIR dataflow tutorial: <https://mlir.llvm.org/docs/Tutorials/DataFlowAnalysis/>
- MLIR CSE implementation: <https://github.com/llvm/llvm-project/blob/main/mlir/lib/Transforms/Utils/CSE.cpp>
- MLIR commutative operation trait: <https://mlir.llvm.org/docs/Traits/#commutative>
- MLIR block predecessor semantics: <https://mlir.llvm.org/doxygen/classmlir_1_1Block.html>
- MLIR One-Shot Bufferize: <https://mlir.llvm.org/docs/Bufferization/>
- MLIR dialect conversion and rollback: <https://mlir.llvm.org/docs/DialectConversion/>
- LLVM pass manager and invalidation: <https://llvm.org/docs/NewPassManager.html>
- LLVM target-independent code generator and two-address instructions: <https://llvm.org/docs/CodeGenerator.html#handling-two-address-instructions>
- LLVM register coalescer implementation: <https://github.com/llvm/llvm-project/blob/main/llvm/lib/CodeGen/RegisterCoalescer.cpp>
- LLVM two-address instruction pass implementation: <https://github.com/llvm/llvm-project/blob/main/llvm/lib/CodeGen/TwoAddressInstructionPass.cpp>
- LLVM GlobalISel core pipeline and per-phase completion invariants: <https://llvm.org/docs/GlobalISel/Pipeline.html>
- LLVM GlobalISel instruction selection and use-def folding: <https://llvm.org/docs/GlobalISel/InstructionSelect.html>
- LLVM Machine IR operand timing, tied definitions, and early-clobber flags: <https://llvm.org/docs/MIRLangRef.html#register-operands>
- LLVM testing guide: <https://llvm.org/docs/TestingGuide.html>
- LLVM SCCP, DCE, CFG simplification, and instruction combining: <https://llvm.org/docs/Passes.html>
- LLVM SCCP implementation: <https://llvm.org/doxygen/Scalar_2SCCP_8cpp_source.html>
- LLVM SCCP solver interface: <https://llvm.org/doxygen/SCCPSolver_8h_source.html>
- LLVM aggressive dead-code elimination roots and backward worklist: <https://github.com/llvm/llvm-project/blob/6e8a4ce8343f2d489f5ecdca5cd443a584f77e71/llvm/lib/Transforms/Scalar/ADCE.cpp>
- LLVM iterative SCC iterator: <https://llvm.org/doxygen/SCCIterator_8h_source.html>
- Wegman and Zadeck, *Constant Propagation with Conditional Branches*: <https://research.ibm.com/publications/constant-propagation-with-conditional-branches--1>
- LLVM simple DCE implementation: <https://llvm.org/doxygen/DCE_8cpp_source.html>
- LLVM EarlyCSE implementation: <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Scalar/EarlyCSE.cpp>
- LLVM instruction commutativity query: <https://llvm.org/docs/doxygen/classllvm_1_1Instruction.html>
- LLVM block merge implementation: <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Utils/BasicBlockUtils.cpp>
- LLVM SimplifyCFG implementation: <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Transforms/Utils/SimplifyCFG.cpp>
- LLVM optimization remarks: <https://llvm.org/docs/Remarks.html>
- LLVM block-frequency terminology: <https://llvm.org/docs/BlockFrequencyTerminology.html>
- Binaryen optimizer and design: <https://github.com/WebAssembly/binaryen>
- Swift compiler pipeline: <https://www.swift.org/documentation/swift-compiler/>
- Swift SIL pass-manager verification controls: <https://github.com/swiftlang/swift/blob/main/lib/SILOptimizer/PassManager/PassManager.cpp>
- regalloc2 SSA and liveness design: <https://docs.rs/crate/regalloc2/0.13.2/source/doc/ION.md>
- regalloc2 operand effects and positions: <https://docs.rs/regalloc2/latest/regalloc2/struct.Operand.html>
- regalloc2 operand constraints and result/input reuse: <https://docs.rs/regalloc2/latest/regalloc2/enum.OperandConstraint.html>
- regalloc2 symbolic allocation checker: <https://github.com/bytecodealliance/regalloc2/blob/aa9680bce32f2ce9cad48ffe02ce03edc3c07e56/src/checker.rs>
- regalloc2 current ION live-range construction: <https://github.com/bytecodealliance/regalloc2/blob/main/src/ion/liveranges.rs>
- regalloc2 current ION range merging: <https://github.com/bytecodealliance/regalloc2/blob/main/src/ion/merge.rs>
- Cranelift block-parameter SSA reference: <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md>
- Cranelift ISLE typed lowering, overlap, specificity, and explicit priority: <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/isle/docs/language-reference.md>
- Cranelift iterative dominator-tree construction: <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/dominator_tree.rs>
- Cranelift internal timing scopes: <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/timing.rs>
- Wasmtime generated and differential fuzzing/reproduction protocol: <https://github.com/bytecodealliance/wasmtime/blob/main/fuzz/README.md>
- Weighted pushdown systems for interprocedural path problems: <https://minds.wisconsin.edu/handle/1793/60338>
- GCC pass-manager internals: <https://gcc.gnu.org/onlinedocs/gccint/Pass-manager.html>
- GCC SSA-CCP implementation: <https://gcc.gnu.org/git/?p=gcc.git;a=blob;f=gcc/tree-ssa-ccp.cc;hb=HEAD>
- GCC CFG maintenance: <https://gcc.gnu.org/onlinedocs/gccint/Maintaining-the-CFG.html>
- Mojang function results and command-sequence operation accounting: <https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3>
- Mojang namespaced command-limit gamerules: <https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-11>
- Mojang Java Edition 26.2 release and official server artifact: <https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2>
