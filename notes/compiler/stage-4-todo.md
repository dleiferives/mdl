# Stage 4 Implementation Checklist

Status: **Complete**

Design authority: [`stage-4-first-lowering-plan.md`](stage-4-first-lowering-plan.md)

All Stage 4 work items and completion gates are closed.

## Rules for every work item

Before checking an item off:

- leave the workspace compiling and tested; do not land `todo!`, `unimplemented!`,
  placeholder output, or a public API that cannot perform its documented contract;
- keep Core target-independent and Stage 3 serialization-near;
- reuse `ControlFlowGraph`, `Reachability`, `EntityVec`, shared diagnostics, Stage 3
  builders/verifiers, and the existing server harness before adding infrastructure;
- derive generated resources/homes only from stable IDs and explicit options;
- preserve source origins on every generated command;
- verify Core before planning and Stage 3 before returning output;
- accumulate independent legality findings, but stop a dependent construction phase
  after its prerequisite fails;
- return no partial runnable program or builder state;
- use closed exhaustive matches for Core and Minecraft vocabularies;
- add focused positive, negative, corruption, and determinism tests with the code;
- avoid whole-program clones, duplicate cycle analysis, and repeated full-CFG scans;
- run formatting, warnings-denied Clippy, fast tests, and rustdoc at each section gate;
- do not add a source language, generic dialect framework, recursive runtime stack,
  optimizer, scheduler, macro ABI, or e-graph machinery.

Ordinary tests remain server-free. Extend an existing ignored server conformance run
when it already owns the relevant target boundary; do not add another Java startup
for each primitive.

## Stage 4A: Target prerequisite and lowering foundation

- [x] **4A.1 — Add the typed internal-function execute condition.**

  Add the narrow Stage 3 `Condition` variant needed to represent
  `execute if function` with an internal `McFunctionId`. Thread
  `&MinecraftProgram` through execute-modifier/condition rendering, reuse internal
  function resource resolution, and extend reference verification to inspect
  modifier conditions as well as nested command nodes. Keep its effect, context,
  and fork contract conservatively unknown like `FunctionCall`; do not add an
  interprocedural contract solver.

  Update every closed match in dump, render, contract, and verification code. Add
  exact rendering/dump tests, valid forward-reference coverage, and a corrupted
  private fixture proving an absent ID reports
  `minecraft.invalid-internal-reference`. Extend
  `crates/mdl-compiler/tests/stage3_handoff.rs` so the public Stage 3 boundary proves
  this shape without raw text.

- [x] **4A.2 — Pin all required target semantics in the existing vanilla run.**

  Extend `mdl-test/tests/minecraft_ir.rs` rather than launching a separate server.
  Pin `execute if function` for positive, zero, and failed `return run` outcomes;
  pin `return run data modify`; and pin scoreboard assignment, `+=`, `-=`, and swap
  at `i32::MIN/MAX`, signed comparisons at both boundaries, and absent source/target
  holder behavior for every sequence Stage 4 will emit.

  Query the current namespaced gamerules
  `minecraft:max_command_sequence_length` and `minecraft:max_command_forks` and
  assert the Java 26.2 `TargetSpec` defaults. Preserve the existing one-startup,
  byte-identical artifact/trace, and log-cleanliness checks. If any measurement
  differs, revise the design before implementing lowering.

- [x] **4A.3 — Add the private lowering module, options, and generated names.**

  Add `src/lower/mod.rs` and `src/lower/minecraft/` with only code used by this
  slice. Define `LoweringOptions` with target, non-`minecraft` pack namespace, and
  exclusive register objective. Implement deterministic validated constructors for
  load/probe/block/edge/storage resources and value/result/temp holders. Use typed
  then/else identity and lower it to numeric edge ordinals only in the name
  constructor.

  Test reserved namespace rejection, duplicate name hints having no effect,
  objective-byte hex reversibility, path safety, uniqueness, and repeated
  construction identity. Do not expose `lower_to_minecraft` yet: the public entry
  point must not exist until it can return a complete verified output.

- [x] **4A.4 — Close the Stage 4A gate.**

  Audit the Stage 3 extension for exhaustive reference traversal and the lowering
  foundation for API width and target mutation. Run all fast gates plus the single
  extended ignored Stage 3 conformance test.

## Stage 4B: Complete immutable planning

- [x] **4B.1 — Build the private plan and role tables.**

  Add private `HomeId` and `PlannedFunctionId` identities using the existing
  `entity_id!`/`EntityVec` machinery. A `Home` owns its fake holder and one closed
  value/result/edge-temp role. A `PlannedFunction` owns its resource, origin, and
  one closed load/probe/block/branch-helper role. Store only IDs in value maps,
  `FunctionAbi`, block maps, transfers, and helper references.

  Build through a private mutable `PlanBuilder`; only `finish` may create a
  field-private immutable `LoweringPlan`. Plan every defined Core function as a
  potential external root, but do not allocate executable mappings for detached or
  entry-unreachable entities. Own the small copied name/type metadata required by
  dumps so the eventual report borrows neither Core nor `SourceContext`.

- [x] **4B.2 — Derive one reusable emitted-function analysis.**

  For each function, build the existing `ControlFlowGraph` and `Reachability` once.
  Record reachable blocks in original `body.block_order()` order, reachable values,
  ordered call sites, and outgoing Core edges in one ephemeral analysis record.
  Do not use reverse-postorder as emitted layout merely because reachability uses it.
  Prove detached blocks, attached-but-entry-unreachable blocks, detached
  instructions, and unreachable calls are excluded without mutating Core.

- [x] **4B.3 — Audit legality and the emitted call graph before physicalization.**

  Exhaustively match every reachable Core operation/terminator. Accumulate
  deterministic legality findings, including entry-reachable `unreachable`, before
  creating Stage 3 state. Build dense forward/reverse call adjacency from the
  ordered reachable call sites. Reject direct and mutual recursion once per SCC,
  ordered by lowest `FunctionId` and anchored at the earliest in-SCC call site.

  Use iterative Kosaraju with explicit work frames. Test forward/nested acyclic
  calls, self recursion, mutual recursion, several independent SCCs, a deep chain,
  and a deep cycle without relying on the host stack. CFG loops remain legal.

- [x] **4B.4 — Allocate conservative homes/functions and finalize edge transfers.**

  Traverse the reusable analyses in stable function/block/instruction order.
  Allocate one value home per reachable SSA value, ordered result homes per
  function, all block functions, and the two initialization functions. Let checked
  `EntityVec::push` be the allocation boundary; translate exhaustion by table and
  discard `PlanBuilder`. Do not add a separate whole-program pre-count pass whose
  only purpose is predicting the same checked failure.

  Derive each Core edge's no-op-filtered simultaneous assignments once. Resolve it
  with an internal symbolic scratch location, allocate one function edge temporary
  iff any finalized transfer uses scratch, substitute its `HomeId`, and attach a
  branch helper iff that conditional edge has moves. Do not store a second schedule
  database or rerun cycle detection.

  Unit-test no-op sets, chains, fan-out, mixed acyclic/cyclic sets, two/three cycles,
  stable ready ordering, stable cycle selection, and malformed inputs.

- [x] **4B.5 — Verify, dump, and project the completed plan.**

  `PlanBuilder::finish` runs one linear `verify_plan` boundary covering role and ID
  ranges, reachable coverage, absent detached mappings, `FunctionAbi` agreement,
  forward/reverse role agreement, CFG/transfer shape, helper/temp correspondence,
  and type-correct edge assignments. Corrupted private fixtures diagnose rather
  than panic.

  Verify transfer correctness independently: label input homes with symbolic tokens,
  abstractly execute stored moves, reject uninitialized temp reads, and compare
  destination tokens with mathematical simultaneous assignment. Never call the
  resolver from the checker or require one exact valid sequence. Exhaustively check
  small move graphs and integer states.

  Add a deterministic lowering dump and a private immutable `LoweringReport`
  projection containing only correlation/public-ABI/debug facts. SCC adjacency,
  reachability scratch, `PlanBuilder`, transfer-construction scratch, and finalized
  move steps must not survive the projection.

- [x] **4B.6 — Close the Stage 4B gate.**

  Confirm planning is approximately `O(program + calls + CFG edges + moves)`, with
  `O(R log R)` sorting only for stable presentation of invalid recursive SCCs.
  Assert byte-identical plan/report dumps and audit that no hash-map iteration or
  recursive whole-program DFS controls output or diagnostics.

## Stage 4C: Target construction substrate and scalar lowering

- [x] **4C.1 — Declare the frozen plan into Stage 3 IDs.**

  Create `MinecraftProgramBuilder`, declare the dense planned-function table in
  order, and record only `PlannedFunctionId -> McFunctionId` plus the load-tag ID in
  an ephemeral `DeclarationMap`. Declare `minecraft:load` with
  `FunctionTagMerge::Append`. Prove positional length/identity agreement with the
  plan and define the tag entry through the mapped load ID.

  Translate every `BuildError` atomically. Duplicate/invalid/already-defined errors
  after a verified plan are construction-invariant diagnostics; entity exhaustion
  is a stable capacity diagnostic. Never expose the target builder or declaration
  map.

- [x] **4C.2 — Establish the per-function construction context.**

  Add one short-lived `FunctionLoweringCx` borrowing the Core body, immutable plan,
  declaration map, and one Stage 3 `FunctionBodyBuilder`. Centralize checked
  `HomeId -> ScoreRef` and `PlannedFunctionId -> McFunctionId` resolution. `finish`
  consumes the context. It cannot mutate planning decisions, perform resource-string
  lookups, or escape as public state.

- [x] **4C.3 — Generate collision-safe objective initialization.**

  Define the creation probe as `return run scoreboard objectives add`. Define load
  so an existing compiler sentinel returns success, a nonzero internal-function
  condition commits the sentinel through `return run data modify`, and every failed
  creation/commit returns failure with the sentinel absent. The load tag contains
  only the mapped load function.

  Golden-test exact structured commands and provenance. Server-free tests must prove
  only the configured objective/sentinel are referenced; vanilla collision and
  reload behavior is exercised in 4E.5.

- [x] **4C.4 — Lower constants and wrapping addition.**

  Materialize exact Boolean `0/1` and all `i32` constants. Lower wrapping addition
  only through the primitive sequence proven by 4A.2, preserving instruction origin
  on every generated command. Golden-test exact target IR and boundary values.

- [x] **4C.5 — Lower overflowing addition, comparisons, and Boolean negation.**

  Produce the wrapping sum plus normalized signed-overflow flag for every sign
  combination and boundary crossing. Lower all six signed predicates, using typed
  `unless` for not-equal, and normalized `1 - operand` for Boolean negation. Query
  types through `HomeRole`; do not independently lower ABI/value types.

- [x] **4C.6 — Close the Stage 4C gate.**

  Exhaustively match every non-call Core operation. Confirm scalar construction uses
  no raw commands, strings, selectors, or unplanned names, and translate command-ID
  exhaustion into an atomic construction diagnostic.

## Stage 4D: Calls and structural control flow

- [x] **4D.1 — Implement the fixed-slot internal call ABI.**

  Copy operands into the callee's `FunctionAbi` parameter homes, issue an ordinary
  mapped call, and copy ordered result slots into instruction-result homes. Cover
  zero/multiple arguments/results, forward calls, nested calls, and provenance.
  Prove ordered copies are sufficient because recursion is rejected and the
  caller/callee value/result home families are disjoint.

- [x] **4D.2 — Lower jumps and block arguments.**

  Emit the frozen edge transfer followed by `return run function` to the mapped
  destination block. Prove empty edges, value-producing joins, and a self-loop
  swap. Construction consumes moves; it never invokes the resolver.

- [x] **4D.3 — Lower returns and reject unsupported termination.**

  Copy semantic results into function result homes and return success. Cover zero
  and multiple results. Keep reachable `unreachable` entirely in the legality
  rejection path; do not emit `return fail` as a fake trap.

- [x] **4D.4 — Lower the mandatory single-evaluation branch dispatcher.**

  Emit one conditional `return run` for the true edge and one unconditional false
  tail. Route an edge through its planned helper only when it has moves. Golden-test
  direct/helper combinations, same-destination edges with different arguments,
  joins, and nested branches. Snapshot and dual-guard selection remain Stage 5.

- [x] **4D.5 — Prove finite loops, including a genuinely cyclic backedge.**

  Lower the existing `sum_down` fixture for ordinary loop-carried copies. Add a
  separate finite countdown loop whose backedge swaps two block parameters so the
  integration path genuinely uses `EdgeTemporary`; `sum_down` alone is not cyclic.
  Assert exact results and generated helper/temp presence.

  Stage 4 performs no general trip-count or command-sequence proof. Expose the
  caller-bounded command-limit contract in 4E.1 and use small measured integration
  inputs. Configurable hard-limit assumptions enter with Stage 5 cost analysis;
  static partitioning and soft scheduling budgets remain Stage 9.

- [x] **4D.6 — Close the Stage 4D gate.**

  Audit ordinary versus tail calls, one-command edge fast paths, result-slot
  semantics, recursive-call rejection, the single-context/non-reentrant restriction,
  caller-bounded command limits, and the absence of accidental selector forks.

## Stage 4E: Public orchestration, proofs, and vanilla execution

- [x] **4E.1 — Expose the complete checked lowering façade.**

  Only now expose `lower_to_minecraft`, `LoweringOutput`, `LoweringFailure`,
  `LoweringMap`, `LoweredFunction`, `RegisterSlot`, and the execution contract.
  Public fields stay private behind narrow accessors. The contract records both
  `SingleContextNonReentrant` activation and that Stage 4 callers must remain within
  the target command-sequence/fork limits; it does not imply scheduling support.

  Run phases in this order: Core verification, option validation, legality/planning,
  declaration/construction, target-builder finish, Stage 3 verification, report/map
  projection, return. A failure has a structured phase and shared `Diagnostics`;
  failures after plan freeze may expose deterministic `dump_lowering()` text through
  a non-runnable report, but never a partial `MinecraftProgram` or construction
  handle. Derive the public map only from the completed report.

- [x] **4E.2 — Add public API, failure, and exact-output proofs.**

  In a public compiler integration test, lower the canonical diamond, a
  call-containing CFG, `sum_down`, and the swap loop. Assert exact lowering dump,
  Minecraft dump, resources, commands, tags, artifact paths/bytes, trace origins,
  ABI slots, execution contract, and repeated byte identity.

  Exercise every failure phase: malformed Core, reserved namespace, recursion,
  reachable `unreachable`, corrupted private plan fixtures, synthetic checked
  allocation/build-error translation, and target verification. Prove report absence
  before plan freeze, report presence when appropriate afterward, and no target
  output on any failure. Do not attempt to allocate billions of entities merely to
  reach the physical `u32` boundary in a test.

- [x] **4E.3 — Add scale and complexity guardrails.**

  Lower a large server-free program and assert planned-home/function, command, file,
  and trace counts. Add a separate ignored geometric benchmark with no wall-clock
  threshold. Measurements may motivate storage changes later; do not add an arena,
  interning, or a generic pass manager preemptively.

- [x] **4E.4 — Build the Core-to-vanilla integration artifact.**

  Construct Core directly, lower and emit twice, and persist Core/lowering/target/
  trace dumps plus the datapack. Use typed `LoweringMap` accessors to set normalized
  entry homes and observe result homes. Include arithmetic, a nested call, diamond,
  join, mandatory dispatcher, `sum_down`, cyclic swap loop, and returns in one pack.

- [x] **4E.5 — Run the pinned official server proof.**

  Execute the generated pack on official Java 26.2 under Java 25. Prove fresh-world
  initialization, a quiet state-preserving reload, exact results, and no attributable
  parse/load errors. Use inputs whose measured operation count is safely below the
  queried target limit.

  At the end of the same server run, remove only the compiler sentinel while leaving
  its objective in place. Invoke load twice while storing function success to test
  storage; both calls must fail and the sentinel must remain absent. This exercises
  the foreign-objective state without a second Java startup. Preserve every artifact,
  world, and server log on failure.

- [x] **4E.6 — Reconcile documentation and the discovered Core vocabulary gap.**

  Update the Stage 3 handoff to record the concrete internal-function-condition
  extension and the fixed Stage 4 dispatcher. Record that source mutable state/raw
  commands still require target-neutral effect/import design. Do not add a Minecraft
  raw variant to Core.

- [x] **4E.7 — Run the final Stage 4 completion gate.**

  Run formatting, warnings-denied Clippy, every fast test, rustdoc, the extended
  Stage 3 target conformance, full Core lowering conformance, benchmark inspection,
  and final diff review. Stage 4 is complete only when a Core-built program—not a
  hand-authored Stage 3 substitute—passes vanilla.

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
