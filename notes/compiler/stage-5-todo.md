# Stage 5 Implementation Checklist

Status: **In progress — Stages 5A–5G complete and gated; Stage 5H next**

Design authority:
[`stage-5-baseline-optimization-plan.md`](stage-5-baseline-optimization-plan.md)

Core pass design index:
[`stage-5-passes/README.md`](stage-5-passes/README.md)

Stage 5 adds auditable baseline optimization and truthful Minecraft cost evidence
while retaining the complete Stage 4 path as a byte-stable reference mode.

## Rules for every work item

Before checking an item off:

- leave the workspace compiling and tested; do not land `todo!`, `unimplemented!`,
  placeholder reports, or APIs that cannot perform their documented contract;
- keep Core optimization, Minecraft planning, target execution analysis, datapack
  footprint, and empirical measurement as separate dependency layers;
- never make successful lowering depend on optional target-execution instrumentation;
  recipe selection may share its closed local step algebra and assumptions, but must
  not invoke the whole-program analyzer per candidate or as a hidden lowering phase;
- preserve `CoreOptimizationLevel::None` and
  `MinecraftOptimizationLevel::None` as verified differential oracles once their
  owning 5B/5E APIs exist; before then, preserve the current Stage 4 entry point and
  artifact directly;
- use authoritative entity/layout order for output, diagnostics, candidate selection,
  and tie-breaking; maps may only accelerate keyed lookup;
- do not make lowering correctness depend on Core canonicalization or any optional
  optimization pass;
- do not expose partially optimized Core, mutable plan state, an unverified target,
  or a partial datapack after failure;
- use the exhaustive `CoreOp` effect, speculation, structural-result-equivalence, and
  operand-symmetry contracts; keep optimizer-owned expression keys out of `ir::core`
  and do not create pass-local semantic folklore;
- make batch edits actual single-analysis/single-application batches, not loops over
  the existing whole-function-scanning editor methods;
- use explicit worklists/stacks for traversals whose depth follows functions, blocks,
  dominator trees, call graphs, or SCCs; valid deep IR must not overflow the Rust call
  stack;
- bound worklists, typed pass-specific fuel, remark retention, liveness segments,
  candidate search, and cost arithmetic with the documented domain fallback;
- keep expected non-matches successful; optional-analysis limit exhaustion may only
  take its typed conservative fallback, while incomplete verification/construction is
  a compilation failure;
- implement each Core pass against its linked pass plan, updating/re-reviewing that
  plan first if implementation evidence changes the algorithm or proof boundary;
- add focused positive, no-transform, negative, corruption, determinism, and scale
  tests with every feature;
- extend the existing official-server conformance artifact when vanilla semantics are
  involved; do not add a Java startup per optimization;
- run formatting, warnings-denied Clippy, fast tests, and rustdoc at every section
  gate; and
- do not add a source language, scheduler, generic dialect/rewrite framework,
  e-graphs, PGO, interprocedural inliner, or stable external ABI in this stage.

## Stage 5A: Truthful baseline accounting

- [x] **5A.1 — Close the Stage 4 exact-output/report baseline.**

  Replace any digest-only or partial lowering assertions with complete deterministic
  goldens for the canonical diamond, calls, joins, countdown loop, and cyclic copy
  fixture. Record exact lowering map/report, target dump, pack bytes, and trace. Prove
  the unchanged Stage 4 entry point reproduces the artifact byte for byte. Later 5B
  and 5E gates must bind their new `None` levels to this same oracle. Do not start
  candidate selection until the underlying oracle is trustworthy.

- [x] **5A.2 — Add post-emission `ArtifactFootprintReport`.**

  Add a private-field report owned by `EmissionOutput`, computed exactly once after
  successful emission. Count files by kind, physical function lines, total/per-file
  UTF-8 bytes, maximum emitted function-line Java UTF-16 units, and trace records.
  Derive it from the completed artifact/trace rather than estimates from lowering.
  Add empty/minimal, Unicode, metadata/tag, maximum-line, repeated-access, and exact
  dump tests. Emission failure still returns no footprint or partial pack.

- [x] **5A.3 — Define typed target-execution cost primitives.**

  Add private smart-constructed `CountBound`/`CountUpper`, stable unknown/no-bound
  reasons, command-step outcomes, function-local summaries, cost-region identities,
  and root summaries. Make inverted finite ranges and inconsistent analysis-cap states
  unrepresentable. Exhaustively classify every Stage 3 `CommandKind`, execute
  modifier, return form, internal function/tag reference, raw command, and external
  call. Keep sequence operations, execute stages, function invocations, score/NBT
  command executions, and individual ordinary fork-checked redirect expansion
  separate.

- [x] **5A.4 — Build one outcome-sensitive execution-cost graph.**

  Analyze only verified `MinecraftProgram`. Preserve command order, failed guards,
  zero-context execution, early return, no-result continuation, and internal-call
  outcome semantics. Retain exact typed call sites and deduplicated structural graph
  edges separately from aggregate runtime bounds; do not publish syntactic occurrence
  counts as dynamic per-edge multiplicities. Condense the retained structural call
  graph once. For each structural cyclic region, converge fixed outcome-feasibility
  cells, refine only that region into ephemeral feasible-edge SCCs, solve the refined
  component DAG callee-first, and publish one conservative structural-region summary.
  This local refinement is not a second retained graph and is never repeated per root.
  Memoize shared summaries using iterative graph traversals. Produce a linear
  `TargetExecutionCostReport`; drop graph/worklist scratch. Raw/external behavior
  becomes a stable `Unknown`, not a lowering failure.
  Charge solver work in explicit units: one function transfer evaluation, one newly
  discovered function-exit feasibility cell, or one final weighted function
  evaluation. Preflight each region's final weighted evaluations; if its budget is
  exhausted, discard the whole unfinished region while keeping earlier completed
  regions exact. The unfinished region and every subsequently unvisited region/root
  metric (therefore every upstream caller) become `Unknown(AnalysisLimit)`; never
  publish an in-progress finite upper bound.
  Add explicit read-only checked target-analysis and `LoweringOutput` convenience
  entries. Both return an owned report; neither mutates, consumes, caches, or silently
  reruns analysis. Checked construction-size/arithmetic or invariant failure returns
  `TargetExecutionAnalysisFailure` with bounded target context and no partial report;
  it never turns a verified lowering into `LoweringFailure`. Do not claim process
  allocator OOM is recoverable without pervasive fallible allocation.

  Prove acyclic path minima/maxima, feasible and infeasible recursive edges, feasible
  sub-SCC splitting inside one structural SCC, per-metric positive/zero-weight cycles,
  unknown cycle propagation, maximum-chain non-additivity, tail calls, nested
  execute/return, internal tags, unknown barriers, arithmetic caps, exact graph/solver
  budget boundaries, and root-invocation accounting. Add a deep many-function fixture
  where every Core function is a root and assert construction/retained records are
  linear rather than `roots * graph`.

- [x] **5A.5 — Add configurable hard-limit assumptions.**

  Add one smart-constructed `CommandLimitAssumptions` type and store it in
  `LoweringOptions`, defaulting exactly to `TargetSpec`, with explicit validated
  override builders. Pass that same value to target analysis; do not duplicate the
  fields in a lookalike analysis-options type. Carry both assumptions and target
  defaults into `ExecutionContract` and the cost report. Compare sequence
  bounds against the sequence assumption and maximum individual ordinary
  fork-checked redirect expansion against the fork assumption, including Java 26.2's
  configured-zero behavior and signed-integer maximum. Make the deployment
  precondition explicit.
  Never emit gamerule mutations. Keep soft per-tick budgets and scheduling out of
  Stage 5.

- [x] **5A.6 — Close the Stage 5A gate.**

  Reconcile pre-emission structured census with the chosen verified target and
  post-emission footprint without conflating them. Extend existing vanilla boundary
  fixtures to confirm root invocation, execute-stage, function-result, and per-chain
  fork accounting. Run fast gates and inspect exact charged construction/retained-
  record, solver-work, and command-transfer-visit counters on wide, deep, cyclic, and
  many-root programs. Keep actual allocator internals and wall-clock thresholds out of
  the deterministic report; retain a threshold-free ignored release timing probe.

## Stage 5B: Owned optimizer API and bounded editing substrate

Framework design:
[`stage-5-passes/framework.md`](stage-5-passes/framework.md)

- [x] **5B.1 — Add the owned Core optimizer boundary.**

  Add `opt/core/` and a public `optimize_core(CoreProgram, &SourceContext,
  &CoreOptimizationOptions)` that consumes the program and returns
  `CoreOptimizationOutput { program, report }`. On failure, drop the possibly invalid
  program and expose only typed phase/pass context plus deterministic byte-capped,
  malformed-safe snapshots with truncation metadata. `None` verifies and returns the
  same program after one whole-program verification. Do not clone or retain a complete
  rendering of the program to simulate transactional failure. Exact omitted-byte
  metadata may stream across the full dump, so promise a bounded retained UTF-8 text
  prefix rather than bounded total failure memory or construction time. Dump traversal
  uses linear dense attachment bits, and verifier diagnostics are a separate
  finding-proportional payload. Expose only `None` until a real Baseline
  pipeline exists rather than publishing an empty optimization level.

- [x] **5B.2 — Replace the generic runner with a closed bounded pipeline.**

  **Sequencing dependency:** finish the editor substrate and the concrete 5C/5D pass
  consumers before closing this item. An exhaustive dispatcher with fabricated no-op
  pass outcomes would violate the pass/report contracts below.

  Replace production `FunctionPass`/`PassRunner` injection with compiler-private
  `CorePassKind`, stable `CorePipelineStep` invocation IDs, and an exhaustive ordered
  dispatcher. Repeated cleanup steps must remain distinguishable in reports/filters/
  bisection; conditional cleanup records `Skipped(NoEnablingChange)`. Add typed
  per-pass `Derived | Explicit(Newtype)` limits. Each concrete pass derives its exact
  default from the stable metadata it already preflights; the runner must not duplicate
  body scans or use one coarse formula for unlike units. Add ordered
  closed inline per-kind statistics structs, completion status, separate ephemeral timing,
  domain-specific `CoreRemarkPolicy`, and a runner-owned remark sink that filters/caps
  at submission time. Prove expected misses allocate no records and truncation retains
  exact-or-saturated aggregate omitted counts. Merge invocation outcomes immediately
  into one fixed `CorePipelineStepSummary` per step; the default report must not retain
  a `(function, step)` record matrix. Aggregate with explicit
  `StatisticCount::Exact | Saturated`; statistics overflow never fails compilation.
  Prove a default invocation on a tiny no-change function allocates no statistics or
  remark container.

  Migrate external tests that directly import `FunctionPass`/`PassRunner`; stop
  re-exporting unstable runner machinery from `ir::core`. Keep the checked public
  `FunctionEditor`. Replace eager `capture_before`: full pass dumps require an
  explicit developer sink. Mark them diagnostic-only; do not add a fake replayable
  reproducer without a Core parser. Generated tests must print/store a deterministic
  seed, while handwritten failures remain Rust builder fixtures.

- [x] **5B.3 — Make verification policy effective.**

  Keep `FunctionEditor::new` as a checked standalone constructor. Add a private safe
  trusted-open path used only after the optimizer verifies the program. Tests/CI and
  debug builds verify after each pass; optimized builds may verify pipeline
  boundaries, with an internal verify-all override. New batch operations must not
  invoke a redundant whole-function verifier. Count verifier calls in tests and
  benchmark both policies on the scale corpus.

  The checked/trusted editor split, hidden-verifier counters, release/debug pipeline
  policy, and comparative scale evidence are implemented against the real closed
  dispatcher.

- [x] **5B.4 — Implement atomic value, constant, and operand batches.**

  Implement `replace_values_batch(ValueId -> Existing(ValueId) | Constant)` with
  final-map path compression, cycle/type/placement/dominance/entity-capacity
  preflight, deterministic definition-site constant placement, and one body rewrite
  scan. Add `rewrite_inst_operands_batch` with complete arity/type/dominance checks.
  Retain allocated identities/origins. Prove no mutation on every rejected
  precondition and no repeated `replace_value` fallback.
  Reject duplicate sources, self/cyclic mappings, detached nontrivial sources, and
  detached final existing endpoints. Define constant origin and insertion order
  deterministically; detached historical uses remain untouched.

- [x] **5B.5 — Implement atomic erasure and CFG batches.**

  Implement `erase_discardable_inst_set`, `set_terminators_batch`, and
  `detach_unreachable_blocks` with consumer-specific preflights. Add the single
  derived `CoreOp::is_trivially_discardable() == Pure + Always` helper used by both
  editor and later passes. Erasure independently rechecks unique attached ownership and the
  exhaustive `Pure + Always` helper, scans current attached operands/terminators once
  for closure, mutates only attached instruction vectors, and retains raw instruction
  data/origins. Leave `PreparedJumpFusion` to its 5D consumer rather than embedding
  fusion policy in the generic editor tranche.
  Preserve stable allocated IDs, bidirectional definitions, instruction/block order,
  origins, and reachable-use dominance. Test cross-set uses, terminator dependencies,
  simultaneous projected-CFG dominance changes, entity-limit atomicity, and
  rollback-free failure. Move overlapping-region, entry/self/ambiguous-edge, and
  parameter-substitution acceptance tests to 5D.1 with `PreparedJumpFusion`. Extend the
  Core verifier with a
  separate raw-container invariant over all allocated blocks: detached membership is
  not executable placement, but every listed ID must be valid and no `InstId` may
  occur more than once across or within block instruction lists. Zero raw owners is
  legal for erased history.

- [x] **5B.6 — Close the Stage 5B gate.**

  Add instrumentation assertions showing one analysis construction/application scan
  per batch on 20,000 replacements/erasures. Prove optimizer failure consumes the
  unit, failure snapshots truncate deterministically, successful default execution
  performs no dump rendering, public output is verified, deterministic dumps contain
  no timing, and the public API exposes levels rather than an internal pass registry.
  Run all fast gates.

## Stage 5C: Canonicalization, SCCP, and dead-code cleanup

- [x] **5C.1 — Implement the closed cheap canonicalizer.**

  Add the Core-owned exhaustive
  `OperandSymmetry::{Ordered, CommutativePair}` query, then follow
  [`stage-5-passes/canonicalize.md`](stage-5-passes/canonicalize.md). Add only its
  reviewed exact rules, reachable-CFG reverse-postorder frozen planning,
  iterative path-compressed virtual replacements, raw-producer-first double-not
  matching, backward-substitution invariant, one checked root-scan allowance,
  effective identical-arm folding, and the fixed terminator/operand/value/erasure
  batch order. Preserve raw IDs through the first two batches for the value batch to
  resolve. Keep the complete virtual map for reasoning, but perform one projected-use
  scan and materialize only mappings with a use outside the planned erase set so
  temporary constants do not permanently bloat stable arenas. Limit exhaustion returns only a verified planned prefix. Prove non-layout
  definition order, skipped attached-unreachable code, triple/quad negation,
  virtual-equal branch arguments, complete overflowing-add result replacement,
  constant-allocation preflight, dense-table fallback with large detached history,
  root-cap prefixes, and complete-run idempotence.

- [x] **5C.2 — Implement typed sparse conditional constant propagation.**

  Follow [`stage-5-passes/sccp.md`](stage-5-passes/sccp.md). Add the
  typed flat `Unknown | Constant(Bool/I32) | Overdefined` join lattice, an exact
  semantic-edge catalog keyed by source plus successor index, and a stable
  nonrecursive event queue for blocks/instructions/terminators/edges/edge arguments.
  Re-propagate arguments when facts on already executable edges widen. Add exhaustive
  `CoreOp` transfers with independent multi-result facts, overdefined inputs/calls,
  and per-block pessimistic completion of every executable unknown definition. Use
  separate preflighted table and event limits. Construct `CompletedSccpSolution` only
  after all completion invariants hold; discard every fact/decision if incomplete.

- [x] **5C.3 — Apply SCCP decisions once.**

  Materialize typed constants, batch-replace uses, fold branches with known
  conditions, and detach newly unreachable blocks using the 5B editor. Preserve the
  solver's original-graph authority; do not interleave mutation with lattice solving.
  Derive compact decisions and drop solver scratch before editing. Do not rematerialize
  an identical constant definition or a proven constant with no surviving executable
  use. Assert the application API cannot accept incomplete solver state. Preserve the
  chosen same-target arm's exact arguments and terminator origin. Apply folded
  terminators before the value batch, then detach, so raw selected-edge arguments are
  resolved once rather than reintroduced after replacement. Add a structurally
  independent dense round-robin oracle; edge-argument
  `Unknown -> Constant -> Overdefined`; same-target arms; multi-result overflow;
  zero/exact-boundary limit discard; large detached-history table preflight; call
  barriers; synthetic completion-state tests; and a current-valid-corpus assertion
  that pessimistic resolution count remains zero.

- [x] **5C.4 — Implement simple discardable-instruction elimination.**

  Follow [`stage-5-passes/dce.md`](stage-5-passes/dce.md). Require `Pure + Always`,
  populate one dense use-count table with a direct attached-layout scan, seed a
  layout-order FIFO, count every operand
  occurrence, charge limits before whole-instruction transitions, walk producers
  backward, and erase one independently rechecked closed set. Preflight dense scratch
  from allocated value/instruction cardinality; a configured scratch excess returns
  unchanged before allocation. Calls remain. Do not claim ADCE or block-parameter
  cycle deletion. Cover duplicate operands, every terminator/edge root, partial
  multi-results, detached raw data/ownership corruption, complete idempotence, a
  naive-rescan generated oracle, candidate-limit boundaries, many detached IDs, and
  20,000-chain one-count-scan/one-batch/no-recursion counters.

- [x] **5C.5 — Assemble and prove the first baseline Core pipeline.**

  Run cheap canonicalization, SCCP/application including reachability cleanup, and DCE
  in the documented order. Do not add the post-fusion/CSE cleanup early. Add exact
  standalone pass fixtures, subset-pipeline snapshots, and a small bounded test-only
  Core evaluator comparing result plus ordered call-effect traces between `None` and
  baseline on terminating fixtures. Treat lowering as legality/integration evidence,
  not semantic equivalence. Add generated small typed CFG cases, determinism,
  typed-limit fallbacks, and scale counters.

- [x] **5C.6 — Close the Stage 5C gate.**

  Confirm lowering accepts verified uncanonicalized Core, all transforms preserve
  Core verification, expected no-transform cases are quiet by default, and no pass
  performs accidental repeated whole-function scans. Run all fast gates.

## Stage 5D: CFG fusion and dominance-scoped CSE

- [x] **5D.1 — Implement straight-line jump fusion.**

  Follow [`stage-5-passes/fusion.md`](stage-5-passes/fusion.md). Discover maximal
  regions head-first from unconditional jumps whose reachable non-entry destination
  has exactly one attached incoming edge. Use `PreparedJumpFusion<'editor>` so one
  exclusive fact snapshot cannot stale; accept only minimal ordered block IDs and do
  not build a production dominator tree. Preflight allocated-ID fact tables before
  allocation, and use a separate block-visit limit. Compose each complete edge tuple
  directionally before inserting parameter mappings. Globally preflight regions/final
  successors/raw ownership, reserve heads, rewrite attached uses once, take/drop head
  and intermediate jumps, move only the tail terminator, and retain block order once.
  Consumed blocks remain allocated but empty/unterminated. Cover adversarial layout,
  same-target arms, attached-unreachable predecessors, empty/value-carrying chains,
  self-loop results, joins, substitution swaps, consumed-tail targets, dense detached
  history, visit boundaries, exact scan/reserve counters, raw ownership, and provenance.

- [x] **5D.2 — Add closed deterministic CSE eligibility/keying.**

  Add IR-owned exhaustive `ResultEquivalence::{Structural, Opaque}` separate from
  effects/speculation, and reuse the `OperandSymmetry` fact already required by 5C.
  Canonicalization and CSE share only that semantic symmetry fact. Keep the exhaustive
  `CoreExpressionKey` builder private to `opt::core::cse`; use allocation-free
  zero/one/two `SmallKeySeq` shapes with a boxed wider fallback, and include payload,
  independently symmetry-normalized resolved operands, and the complete result-type
  contract. Eligibility requires the editor-compatible
  `Pure + Always + Structural`; `Always` is a conservative editing restriction, not a
  general dominated-CSE theorem. Calls remain opaque. Make every new `CoreOp` update
  result-equivalence decision, existing symmetry decision, and key tests or fail to
  compile/tests.

- [x] **5D.3 — Implement local dominance-scoped CSE.**

  Follow [`stage-5-passes/cse.md`](stage-5-passes/cse.md). Preflight allocated
  block/instruction/value slots before dense post-fusion facts; oversize analysis
  returns unchanged. Build dominator children linearly in `BlockId` order and traverse
  with an explicit stack, one scoped table, and undo checkpoints. Resolve operands
  through planned substitutions before lookup; bound active entries and whole-root
  scans separately. Choose the earliest dominating surviving representative, plan
  complete multi-result instructions atomically, replace in one batch, and retain
  eliminated-site correspondence only when requested. Prove equality/hash agreement
  with forced collisions; map iteration cannot choose output.

- [x] **5D.4 — Add the single bounded cleanup round.**

  Run canonicalization/DCE at most once after fusion/CSE when changed. Record whether
  it helped; do not iterate the whole pipeline to a fixed point. Add fixtures where
  cleanup is useful. Under the current closed rules, complete canonicalization already
  composes its virtual replacements and DCE only deletes instructions; deletion cannot
  expose a new operand or CFG rewrite. Prove this by running a shadow second baseline
  in tests and requiring every step to be unchanged. If a future pass makes a second
  cleanup round useful, retain the one-round production contract until measurements
  justify changing it and add the newly deferred opportunity as an explicit fixture.

- [x] **5D.5 — Close the Stage 5D gate.**

  Prove exact optimized Core, differential target results, deterministic
  representative choice, stable detached identities, and approximately linear
  behavior on repeated expressions, deep dominator trees, and wide sibling trees. Add
  a small reachable body with large detached block/instruction/value history and prove
  analysis limits and scratch accounting use allocated slots. Run all fast gates.

## Stage 5E: Functional Minecraft planning and runtime demand

- [x] **5E.1 — Split planning into immutable phase results.**

  Add `MinecraftOptimizationLevel::{None, Baseline}` through an explicit
  `LoweringOptions` builder/setter, keeping the existing constructor selecting `None`
  and its generated outputs byte-identical. Replace the monolithic mutation order
  with this private one-way graph:

  ```text
  verified CoreProgram
      -> SemanticInventory
      -> audit_legality(core, inventory) -> Result<(), Diagnostics>
      -> RuntimeDemand
      -> HomeAssignment (owns aligned InstructionPlan table)
      -> EdgeTransferPlan
      -> ResourceInventory
      -> LoweringPlan::from_parts(...) -> verified LoweringPlan
      -> construct_program(core, &plan)
  ```

  Evolve/rename the existing private
  `EmittedProgramAnalysis`/`EmittedFunctionAnalysis` into one inventory. Per function,
  retain authoritative reachable block/instruction/value order and membership bits,
  one ordered semantic-edge array, incoming-edge index lists into that array, ordered
  call sites, origins, and checked incidence counts. Store IDs/derived facts rather
  than cloned Core operations, terminators, or signatures; do not retain a second
  reachable-semantic census.

  Run the existing target-vocabulary and recursive-call legality gate immediately
  after inventory, deriving disposable iterative SCC/call-graph scratch from the
  inventory's call sites. Return diagnostics directly and retain neither that call
  graph nor an empty success token; no physical phase may run after a failed gate.
  Later phases borrow explicit prerequisites; short-lived local builders mutate only
  their result. Avoid generic typestate and phase records without consumers. Stage 5F
  inserts the first real `LivenessResult` between demand and assignment. Stage 5G adds
  `ControlRecipePlan`, `BlockPlacement`, and placement-aware resource rebuilding after
  homes/transfers freeze. A physical condition-stability result remains deferred with
  the optional repeated-condition recipes that would consume it; Stage 5E materializes
  every reachable block.

  Record the explicit `MinecraftOptimizationLevel` in every policy-sensitive phase
  result and reject cross-policy composition, including empty and fully demanded
  programs whose incidental homes/transfers may otherwise look compatible. Do not
  infer phase provenance from roles, names, or scratch presence.

- [x] **5E.2 — Compute backward runtime demand.**

  Give `RuntimeDemand` only complete states: optimization-disabled all-reachable,
  precise Baseline facts, or all-reachable conservative fallback with a typed reason
  and statistics. For every reachable function, seed every `Return` operand, every
  branch condition, and every instruction that is not `Pure + Always`. Marking an
  instruction marks all operands; marking an instruction result marks its producer;
  marking a block parameter marks its indexed argument on every reachable incoming
  edge. Use dense first-mark bits and a deterministic worklist so cycles terminate.
  Calls remain because they are non-discardable and demand their arguments through
  the ordinary operand rule.

  Every Core function is currently independently exposed, so retain every function's
  return semantics. Keep ABI pinning separate from semantic demand: pin all entry
  parameters and result slots even when a caller does not use a result. Omit only
  undemanded `Pure + Always` instructions, their semantic homes, corresponding edge
  copies, and caller-side result copies. Preserve all CFG control. Run Baseline demand
  even when Core optimization is `None`; Minecraft optimization `None` skips pruning
  and retains Stage 4 behavior.

  Use separate checked dense-table and propagation-event limits. Charge table capacity
  by one result record per function plus allocated value and instruction slots,
  including detached history. Document/test the exact event increments for seeds,
  first marks, dequeues, operand attempts, and incoming edge arguments. The derived
  event limit is a checked conservative maximum; an explicit event limit applies to
  actual charged work and may complete below that maximum. Treat checked derived-bound
  overflow as a typed fallback reason. On
  any optional demand limit, discard the entire transient solution before selecting
  all-reachable fallback; never prune from incomplete facts. Mandatory planning or
  verification capacity failure remains a compilation error.

- [x] **5E.3 — Freeze exact instruction requirements and distinct homes.**

  Make `HomeAssignment` own function-local assigned-home identities, the complete
  pinned ABI, dense optional typed `ValueAssignment`s, and instruction-aligned
  `InstructionPlan`s. Use a dense allocated-`InstId` table whose `None` entries are
  detached/unreachable and whose `Some(OmittedPure)` entries are reachable but
  deliberately unmaterialized. Stage 5E uses distinct homes and performs no
  liveness-based coalescing; Stage 5F is the first reuse consumer. Forbid
  cross-function/cross-type reuse and require every semantic assignment to match the
  home's `CoreType`.

  Represent plans as `OmittedPure`, exact-arity scalar operands/results, or exact-arity
  call arguments plus indexed `Option` caller-result destinations. A retained fixed
  scalar recipe must receive all physical result slots it needs. If only one result of
  `I32AddOverflowing` is demanded, place that result in its semantic home and the
  undemanded sibling in a correctly typed recipe temporary without creating a
  semantic assignment. If neither result is demanded, omit the discardable operation.
  A retained call emits no copy for an indexed `None` result while preserving the
  call, argument copies, callee result ABI, and return copies. Under `None`, every
  scalar result is semantic and every call result destination is `Some`.

  Put one private exhaustive scalar physical-output/storage contract beside scalar
  emission, not in `CoreOp`. Size a deterministic scratch pool per
  `(function, CoreType)` for maximum simultaneous fixed-recipe need, with stable
  ordinals identifying entries in that pool. The independent verifier must interpret
  result storage/timing separately rather than accepting that contract as proof. Keep
  all conservative home roles and names unchanged; optimized assigned homes may use
  deterministic function-local physical ordinals.

- [x] **5E.4 — Freeze edge transfers before allocating resources.**

  Make `EdgeTransferPlan`, not `HomeAssignment`, own function-local edge-temporary
  descriptors because scratch need is discovered by resolving frozen parallel copies.
  In Baseline, retain copies only for destination parameters that have a semantic
  `ValueAssignment`; Core forbids edges to the ABI-pinned entry block. Group remaining
  copies in the fixed `Bool`, then `I32` type order, and lazily allocate exactly one
  temporary per `(function, CoreType)` only when the resolver emits a scratch step for
  that type. Demand propagation must guarantee each retained source argument has a
  home. In `None`, resolve every Stage 4 copy together and preserve its at-most-one
  legacy untyped temporary, allocation point, and name. Give each Baseline typed edge
  temporary a stable `CoreType` discriminator in its holder name so Boolean and integer
  scratch cannot collide. Use `q{b|i}` for recipe-scratch families and `t{b|i}` for
  edge-scratch families; `None` alone retains the legacy untyped `t0` spelling.

  Build `ResourceInventory` only after transfers freeze. Stage 5E allocates a resource
  for every reachable block and a conditional branch helper only when that arm has a
  nonempty transfer; consumed placement and recipe ownership begin in Stage 5G. Keep
  exact Stage 4 allocation order under `None`: load/init first, then for each function
  all blocks in inventory layout order followed by required helpers in semantic-edge
  and arm order. Verify resource ownership, uniqueness, and transfer/helper references.

- [x] **5E.5 — Expand independent plan verification and reports.**

  Have `LoweringPlan::from_parts` locally flatten per-function assigned-home and edge
  temporary identities into final `HomeId`s, construct a candidate, independently
  verify it, and publish only on success. Under `None`, preserve the exact per-function
  order of reachable legacy value homes, ABI result homes, and optional legacy edge
  temporary, plus the resource order from 5E.4. Preserve target commands, generated
  names, home-role/debug forms, lowering map/execution contract, public lowering dump,
  and datapack bytes. Store final instruction plans in `LoweringPlan`; construction
  and emission consume the verified plan rather than inventory/demand as another
  target authority.

  Independently recompute transient Core reachability and minimum backward demand
  without calling the inventory/demand chooser. Accept conservative supersets, but
  reject omitted required instructions/results. Verify dense mappings, types, ABI
  pins, exact instruction/call result arity and index correlation, semantic versus
  recipe-temporary destinations, resource owners, materialization of every reachable
  Stage 5E block, and all transfer/helper references.

  Add a sparse symbolic home-content checker over frozen assignments/transfers:
  initialize pinned parameters, intersect facts at joins, check every read, model
  instruction definitions and block-parameter assignments simultaneously, and
  separately prove each resolved move sequence implements its parallel assignment.
  Interpret scalar early-write/late-read timing and recipe-temporary outputs
  independently. Model calls as reading all arguments before simultaneously defining
  only indexed `Some` caller destinations; separately verify complete callee result
  slots and return copies. Retain sparse block-entry state and recompute local transfer;
  never allocate a dense block-by-home matrix. Compare with a simple dense oracle on
  generated small CFGs. Required checker failure or inability to complete rejects the
  plan rather than taking an optimization fallback. Keep selected reports linear and
  rejected detail bounded/opt-in.

- [x] **5E.6 — Close the Stage 5E gate.**

  Freeze the complete Stage 4 oracle before refactoring and diff `None` target,
  lowering map/execution contract, public dump, and datapack bytes. For Baseline, cover
  unused pure chains; all four result-usage cases for `I32AddOverflowing`; an
  effectful call whose results are all unused; a multi-result call using only a later
  result index; unused and demanded block-parameter cycles; and Core optimization
  `None` with Minecraft Baseline.

  Test exact demand-limit boundaries and one-less fallback, allocated detached entity
  slots, whole-analysis fallback, separate Bool/I32 edge cycles, scratch needed versus
  not needed, and the legacy untyped `None` cycle byte oracle. Add repeated deterministic
  Baseline runs, 20,000-value and many-function scale fixtures, generated small-CFG
  Core/target differential execution, and corruption cases for omitted required work,
  wrong call-result indices, wrong temporary types, missing helpers, and dangling
  resources. Include cross-policy phase-wiring corruptions for an empty function and
  an all-demanded function. Run formatting, warnings-denied Clippy, fast tests, and
  rustdoc.

  Completed on 2026-07-12. The production path now follows the immutable phase graph
  above and the mutable Stage 4 planner remains test-only as the independent `None`
  oracle. Structural and sparse symbolic verification reject the required corrupted
  plans. Versioned generated straight-line, branch/join, terminating-loop, and
  multi-result-call CFGs execute through the target-command interpreter under both
  physical policies and match independent semantic models. The public Baseline,
  deterministic, 20,000-value/many-function, exact-output, and corruption fixtures
  pass; formatting, workspace warnings-denied Clippy, all fast tests, and
  warnings-denied rustdoc pass; and both `None` and `Baseline` Core-lowering packs
  pass the official Java 26.2 dedicated server under Java 25.

## Stage 5F: Sparse liveness and conservative coalescing

- [x] **5F.1 — Define exact liveness program points and edge semantics.**

  Use `0` for block entry, `2i + 1` for instruction-`i` operand uses, `2i + 2`
  for its simultaneous result definitions, `2n + 1` for terminator/selected-edge
  uses, and `2n + 2` for the exclusive block end. Edge liveness substitutes a
  live destination parameter with that exact edge occurrence's argument while
  retaining direct cross-block values; duplicate successor arms remain separate.
  Keep sorted live-in/live-out sets as analysis scratch (and test-only oracle data),
  then retain flat, half-open per-value segments with a dense optional range index.
  Treat multiple results and block parameters as simultaneous definitions, and call
  arguments as uses immediately before simultaneous call results. Test every boundary.

- [x] **5F.2 — Implement bounded deterministic liveness.**

  Use a stable iterative worklist, sparse sorted set operations, checked propagation/
  set-event work, and checked retained-segment limits. On any overflow or exhaustion,
  discard the whole function solution and use distinct homes with a stable fallback
  reason/statistic; never expose partial facts. Compare small irreducible liveness
  against an independent dense oracle and cover duplicate arms, wide joins, deep
  chains, loops, and 20,000 simultaneously live values in one block.

- [x] **5F.3 — Implement stable conservative copy coalescing.**

  Start only with block-argument copies, exclusive ABI pins, and function-local homes.
  Visit candidates in semantic-edge order and destination-parameter order, normalize
  only for first-occurrence deduplication, and retain the first legal winner. Require
  equal types; reject simultaneous-definition, all-member liveness, and all-member
  target-access conflicts with sorted segment intersection instead of a dense matrix.
  Use bounded function-local DSU groups and discard every tentative union if any
  charged discovery/search/merge/freeze work exhausts its limit. Audit complete edge
  destination uniqueness once after grouping, then freeze register ordinals by first
  reachable-value occurrence.

  Keep one private exhaustive Minecraft `ScalarAccessContract` beside scalar lowering:
  wrapping add may reuse only its semantic left input (including `x + x`), while
  overflowing add, compare, and `BoolNot` reuse no operand. Calls use a separate rule:
  all caller arguments are read before simultaneous result definitions, so a result
  may reuse a dead argument but demanded sibling results remain distinct. Do not put
  target access timing in `CoreOp` semantics or expose a public machine IR.

- [x] **5F.4 — Rebuild and verify edge transfers once.**

  Freeze value-to-home assignments, derive transfers once, remove home-equal moves,
  resolve each remaining parallel copy once, and allocate at most one lazy temporary
  per `(function, CoreType)` when required. Verify the resolved schedule against the
  mathematical simultaneous assignment. Keep the `None` path's Stage 4 temp/naming
  exact. Cover swaps, three-way rotations, duplicate-source fan-out, separate Bool/I32
  cycles, loop backedges, and a Baseline conditional arm whose nonempty transfer owns
  exactly its branch helper.

- [x] **5F.5 — Close the Stage 5F gate.**

  Prove the chooser never groups overlapping ranges, every assignment matches its
  home type, ABI/cross-function restrictions hold, fallback is
  deterministic/all-or-nothing, and retained storage is sparse at scale. Prove
  Boolean normalization compositionally from the explicit normalized-entry ABI
  precondition, closed Boolean-producing recipes, and same-type transfers; the
  symbolic home-content domain proves semantic identity/read timing rather than
  numeric `0/1` facts. The independent symbolic checker need not reconstruct chooser
  liveness; it must instead prove actual semantic home contents across instructions,
  calls, joins, and mathematical edge assignments. Fabricate a recipe-legal alias
  based on false liveness and independently fabricate forbidden recipe aliases to
  prove the final verifier trusts neither chooser fact.

  Include loop-carried wrapping, overflowing-add, and `BoolNot` cases; `x + x`;
  call-result/dead-argument reuse; ABI singleton rejection; transitive interference;
  late budget exhaustion after a tentative union; and two demanded results that stay
  distinct. Exercise allowed aliases through the emitted-command interpreter, assert
  at least one generated differential fixture really merges, preserve exact `None`
  output, and run every fast and official-server gate.

  Completion evidence (2026-07-13): Baseline now computes exact sparse half-open
  liveness, performs bounded first-occurrence copy coalescing under exhaustive target
  access rules, freezes assignments before deriving transfers once, and publishes only
  independently verified plans. Exact-limit/one-less and late-fallback tests prove that
  discarded segment prefixes and tentative unions publish zero retained facts.
  Candidate-free functions bypass scalar-conflict and DSU setup and freeze one direct
  all-distinct mapping at their exact charged work bound. Dense oracles,
  irreducible/duplicate-edge/deep/wide/20,000-live-value fixtures, exact pair-alias
  interpreter tests, corruption tests, generated differentials, typed cycles, and
  final critical-arm helper publication pass. `cargo test --workspace`, formatting,
  workspace warnings-denied Clippy, and warnings-denied rustdoc pass. The official Java
  26.2 server under Java 25 passes the literal selector-to-function multiplicity and
  command-limit boundary pack, both `None` and `Baseline` Core-lowering packs, the
  direct structured-Minecraft-IR pack, and the generated smoke pack.
  Compile-time/runtime unknowns encountered here are retained in
  [`semantic-ambiguities.md`](semantic-ambiguities.md), including A-001, A-015, and
  A-016.

## Stage 5G: Closed target recipes and conservative selection

- [x] **5G.1 — Add local recipe accounting and comparison.**

  Define closed recipe-local command fragments and cost summaries using the same
  command-step algebra as final target analysis. Compare legality first and require a
  typed local graph-contraction certificate proving that every whole-root metric and
  bound classification is non-increasing. Prefer strict multi-dimensional local
  dominance, then structured size only when runtime dimensions do not regress. Retain
  Stage 4 when a real trade-off is incomparable, arithmetic is incomplete, or no such
  certificate exists. Whole-target cost analysis remains an explicit post-lowering
  consumer; do not run it or invent root frequencies/work during candidate selection.

- [x] **5G.2 — Add explicit block placement.**

  Represent every reachable block as materialized or consumed by exactly one recipe.
  Shared, cyclic, supported-ABI entry, and regions not reducible to one closed recipe
  command stay materialized. Minecraft has no symbol visibility: mapped Core entries
  are the supported external ABI, while generated non-entry block/helper names are
  unstable implementation details under `Baseline`. General trace layout,
  duplication, and hot/cold placement remain Stage 11.

- [x] **5G.3 — Implement the first profitable terminal-arm recipe.**

  Consume a uniquely reached terminal arm only when its operations and terminator
  combine into one legal structured target command. Prove the zero-ABI call+return
  tail-call case first, including exact return success/result semantics, contributing
  origins, resource removal, predicted/recounted cost, and baseline fallback.

- [x] **5G.4 — Scope physical condition-stability analysis to a real consumer.**

  Research and the implemented phase graph found that the required terminal-call
  contraction never consumes condition stability, while 5G.5 makes every repeated-
  condition recipe optional. Do not run or retain an unused whole-program fixed point.
  Defer the result type with those recipes. When one has a complete winning fixture,
  compute transitive physical read/write/context/fork footprints over CFG and internal
  calls using frozen assigned-home and edge-temporary identities. Include instruction
  writes, edge transfers, call-result copies, loops that revisit the source, and an
  exhaustive future-operation unknown barrier. Limit exhaustion becomes
  `Unknown(AnalysisLimit)` and rejects every stability-dependent recipe; recipe
  selection cannot request re-coalescing or use partial negative facts.

- [x] **5G.5 — Admit further branch recipes only with a complete winning fixture.**

  Stable dual guard and snapshot recipes are optional experiments, not completion
  requirements. Add one only if emitter, verifier, explicit exact-one completion,
  condition-mutation trap, local/final cost reconciliation, and a concrete advantage
  over the return dispatcher all land together. That experiment also owns the first
  narrow condition-stability analysis described in 5G.4. Otherwise retain the
  dispatcher and record both recipe and analysis as deferred.

- [x] **5G.6 — Close the Stage 5G gate.**

  Add exact selected-decision/rejection-reason reports, mutation-trap tests, optimized
  versus Stage 4 differential execution, resource/trace goldens, unknown/fallback
  cases, and the existing official-server return/execute proof. Run all fast gates.

  Completion evidence (2026-07-13): the closed `InlineZeroAbiTerminalCall` recipe
  consumes only a uniquely reached, non-entry, empty-transfer, zero-ABI call-plus-
  empty-return block. Selection first proves legality and a non-increasing graph
  contraction, then uses exact path-sensitive command-step Pareto comparison. The
  frozen plan owns placement, resources, decisions, costs, and contributing origins;
  independent verification rederives every selected and rejected decision, resource
  ownership, exact-one completion, and control statistic. Post-construction
  reconciliation classifies the actual final commands with the shared target-cost
  algebra and checks both Core-derived targets, origins, structure, and predicted
  costs. Stable dual-guard/snapshot recipes and physical condition-stability analysis
  remain explicitly deferred because no complete winning fixture consumes them.

  Focused integration covers then, else, both arms, shared destinations, physical
  transfers, ABI rejection, scalar prefixes, mutation/corruption traps, deterministic
  reports, resources, artifacts, traces, and whole-target non-regression. One official
  Java 26.2 startup installs distinct `None` and `Baseline` packs and proves both
  Boolean paths preserve command success `1`, exact result `1`, and the nested
  callee's observable result-slot effect. Formatting, workspace warnings-denied
  Clippy, all fast tests, warnings-denied rustdoc, and diff checks pass.

## Stage 5H: Completion, measurements, and handoff

- [ ] **5H.1 — Finalize report ownership and deterministic dumps.**

  Expose narrow accessors for owned Core optimization output, lowering decisions,
  target execution cost, artifact footprint, and harness measurement records. Keep
  wall-clock time out of deterministic compiler reports. Prove default reports are
  linear and detailed remarks remain filtered/capped.

- [ ] **5H.2 — Record compile-time and generated-code measurements.**

  Measure verification modes, every pass, liveness, planning, target cost analysis,
  emission, and complete compilation on tiny/normal/scale fixtures. Record raw samples
  with build/target/JVM metadata. Use visit/allocation counters for CI complexity
  assertions; do not add brittle wall-clock thresholds or universal runtime weights.

- [ ] **5H.3 — Add generated/property and corruption coverage.**

  Generate small typed acyclic/cyclic Core CFGs, constants at integer boundaries,
  executable-edge combinations, copy graphs, and branch mutations. Compare optimized
  and reference evaluation/lowering. Every failure prints the generator version and
  deterministic seed; promote minimized failures into checked-in Rust builder
  fixtures because Core has no round-trippable text format. Add corrupted editor
  batches, plans, cost graphs, bound constructors, placement, assignments, symbolic
  home states, and predicted-cost fixtures.

- [ ] **5H.4 — Run the complete official-server proof.**

  Extend the existing Core lowering conformance artifact with optimized and reference
  forms. Prove exact observable results, reload cleanliness, return semantics,
  sequence/fork boundary accounting, allowed two-address/result-argument home reuse,
  and no new parse/load errors in one pinned Java 26.2 startup. Preserve
  world/artifacts/logs on failure.

- [ ] **5H.5 — Reconcile roadmap and Stage 6/11 handoff.**

  Update plan/checklist/status/index documentation. Confirm Stage 6 may consume the
  owned optimizer/lowering/emission outputs without collapsing them, Stage 9 owns
  soft scheduling, and Stage 11 owns inlining/specialization/outlining/global search.
  Record deferred recipe experiments explicitly.

- [ ] **5H.6 — Run the final Stage 5 completion gate.**

  Run formatting, warnings-denied Clippy, all fast tests, rustdoc, both existing
  official-server conformance tests, benchmark inspection, deterministic repeated
  builds, link/reference validation, and final diff/API review. Mark Stage 5 complete
  only when optimized Core—not a hand-authored target substitute—passes vanilla and
  the Stage 4 byte-stable reference path remains available.

## Completion commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test minecraft_ir -- --ignored --nocapture

MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test core_lowering -- --ignored --nocapture
```
