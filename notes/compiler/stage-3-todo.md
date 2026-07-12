# Stage 3 Implementation Checklist

Status: **Complete**

Design authority: [`stage-3-minecraft-ir-plan.md`](stage-3-minecraft-ir-plan.md)

This is the execution checklist for Stage 3. The design plan explains why the
architecture exists; this file records the order in which to build it and the proof
required before moving forward.

All Stage 3 work items and the official Java 26.2 conformance gate are complete.

## Rules for every work item

Before checking an item off:

- keep the public surface no larger than its current consumer requires;
- keep target syntax in closed enums and validated private-field types;
- add focused positive and negative tests with the implementation;
- preserve deterministic traversal and diagnostic ordering;
- do not introduce a Stage 3 editor, generic dialect framework, serialization for
  compiler IR, or speculative lowering policy;
- run formatting, Clippy with warnings denied, fast workspace tests, and rustdoc;
- update this checklist and the Stage 3 status without claiming a later gate is done.

The ignored vanilla test is required only where explicitly listed. Ordinary tests
must not start a server.

## Stage 3A: Shared foundation and validated atoms

- [x] **3A.1 — Extract shared diagnostics.**

  Move `Diagnostic` and `Diagnostics` to `crate::diagnostic`, preserve the existing
  `ir::core` re-exports, keep diagnostic text/order unchanged, and prove both public
  paths name the same Rust types.

- [x] **3A.2 — Add the closed Java 26.2 target.**

  Add `JavaEditionTarget::V26_2` and immutable `TargetSpec` facts for version 26.2,
  pack format `[107, 1]`, singular function/tag directories, command sequence/fork
  defaults, the UTF-16 command limit, and Java 25 conformance.

- [x] **3A.3 — Add resource and artifact names.**

  Implement command-level and pack-safe namespace/path types, distinct function,
  tag, storage, and dimension IDs, structured validation errors, normalized
  `PackPath`, and infallible target-specific function/tag path mapping. Prove dot,
  traversal, unusual valid-name, explicit-namespace, and resource-kind boundaries.

- [x] **3A.4 — Add score atoms and ranges.**

  Implement the private shared `ScoreWord` validator plus `ObjectiveName`,
  `FakeScoreHolder`, `NonNegativeI32`, and the four valid `ScoreRange` shapes.
  Constructors must reject empty/invalid words and backwards ranges; equal closed
  bounds canonicalize to `Exact`. Add exhaustive boundary and canonical rendering
  tests. Do not allocate objectives or generated holder names.

- [x] **3A.5 — Add finite numeric wrappers.**

  Implement `FiniteF32` and `FiniteF64` in a focused numeric module. Reject NaN and
  both infinities, preserve signed zero, and expose only unambiguous traits. Test
  bit-preserving construction and shortest target-compatible formatting. Do not add
  `Eq`, `Ord`, or `Hash` without an explicit signed-zero policy.

- [x] **3A.6 — Add the closed selector subset.**

  Implement `AtMostOneSelector`, `UnboundedSelector`, `Selector`, and `Cardinality`.
  Encode `@s`, `@p`, `@r`, `@a`, and `@e` as variants rather than parsed strings.
  Define conservative context-read masks, infallible conversions into `Selector`,
  and no downcast from a general selector. Do not add filters; scalar score-holder
  construction is proved when `ScoreRef` is introduced in 3B.2.

- [x] **3A.7 — Add NBT keys and static paths.**

  Implement `NbtKey`, nonempty `NbtPathKey`, `NbtPathSegment`, nonempty `NbtPath`,
  and `StoragePath`. Keep empty compound keys legal while rejecting empty path keys.
  Support key, signed index, and all-elements path segments. Test escaping and every
  accepted/rejected static path form.

- [x] **3A.8 — Add immutable NBT values and canonical SNBT.**

  Implement every numeric scalar, strings, heterogeneous lists, and compounds.
  Reject duplicate compound keys, sort compounds by key, preserve list order, and
  enforce the initial depth limit of 64 during construction. Write SNBT through an
  injected sink so Stage 3C can reuse it without a temporary per-command string.
  Test suffixes, signed zero, quotes, slashes, controls, Unicode, empty collections,
  heterogeneity, canonical compound order, and depth 64/65.

- [x] **3A.9 — Close the Stage 3A gate.**

  Audit all atom constructors for I/O, hidden name allocation, unchecked public
  strings, and duplicated validators. Recheck official report/binary facts, run the
  entire fast suite, and record any deliberately conservative grammar subset.

## Stage 3B: Program, commands, builders, and contracts

- [x] **3B.1 — Add target-program identities and callable references.**

  Define `McFunctionId`, `FunctionTagId`, and `CommandId`; internal/external function
  and tag references; tag merge mode; external required/optional mode; and ordered
  tag entries with origins. IDs remain dense, typed, owner-local, and unserialized.

- [x] **3B.2 — Add typed scoreboard commands.**

  Define `SingleScoreHolder`, `ScoreRef`, multi-holder `ScoreSelection`,
  `ScoreOperation`, and the initial objective/player command variants. Preserve
  scalar versus bulk legality in constructors: `SingleScoreHolder` accepts only a
  fake holder or `AtMostOneSelector`. Document and test that multi-holder operations
  are one native command with target-major sequential semantics, not
  execution-context forks.

- [x] **3B.3 — Add typed storage commands.**

  Define storage get/remove/modify, `DataModifyMode`, and value/from sources over the
  Stage 3A NBT/path types. Keep root access, insert, dynamic paths, block/entity data,
  arrays, and string slicing out of scope.

- [x] **3B.4 — Add recursive command and execute syntax.**

  Define `CommandNode`, closed `CommandKind`, `ExecuteCommand`, opaque nonempty
  `ExecuteModifiers`, modifier origins, conditions, five `ScoreComparison` variants,
  result/success stores, `StorageNumericType`, function calls, return value/fail/run,
  and `UnsafeRawCommand`. Preserve modifier order and enforce structural depth 64.
  Raw construction rejects physical-line hazards and the UTF-16 bound.

- [x] **3B.5 — Add final program/resource containers.**

  Define immutable `FunctionBody`, `McFunction`, `FunctionTag`, and
  `MinecraftProgram` dense stores. Function bodies may be empty. Nested commands own
  their child through `Box`; top-level commands alone receive `CommandId`. Keep
  provenance intrinsic and all derived analysis out of the nodes.

- [x] **3B.6 — Add two-phase checked builders.**

  Implement `MinecraftProgramBuilder`, `FunctionBodyBuilder`, and
  `FunctionTagBuilder`. Declarations return IDs before definitions, scoped builders
  reserve one still-empty slot, their consuming `finish` is infallible, and program
  `finish` accumulates every missing definition. Prove recursion, forward tag
  references, empty definitions, duplicate rejection, drop-without-finish, and move
  conversion without whole-body cloning.

- [x] **3B.7 — Add conservative command contracts.**

  Complete the private-bitset contracts with `EffectCategories`, then add
  `EffectSummary`, `ContextSummary`, and `ForkClass`. Derive contracts exhaustively
  from syntax; never cache them in IR. Pin modifier-order composition, `as`/`at`
  fan-out, `in` position scaling, predicate non-forking, native bulk score behavior,
  and unknown call/raw barriers.

- [x] **3B.8 — Add the malformed-safe debug dumper and size guardrails.**

  Implement one deterministic generic dumper that tolerates invalid IDs, depths,
  references, and origins without target-rendering malformed nodes. Record
  `size_of` for principal nodes, add controlled malformed fixtures, and prove normal
  construction cannot create an empty execute sequence or continued physical line.

- [x] **3B.9 — Close the Stage 3B gate.**

  Confirm every command variant forces exhaustive updates in contracts/dumping,
  final IR contains no pending state, no Core ID appears in Minecraft APIs, and no
  scheduler, macro ABI, generic selector framework, or lowering policy entered the
  representation.

## Stage 3C: Layered verification and exact command rendering

- [x] **3C.1 — Build the verifier scaffold and malformed test surface.**

  Add deterministic `minecraft.*` diagnostics, stable dense traversal, fallible
  lookups, and crate-private malformed constructors used only by verifier unit
  tests. Establish the exact verifier layer order before adding global algorithms.

- [x] **3C.2 — Verify target, stores, atoms, and local command shapes.**

  Check target identity, dense ID ownership/ranges, leaf validity, score cardinality,
  execute non-emptiness/order, raw physical-line rules, and command/NBT depth. Each
  rule receives an isolated negative test and independent failures accumulate.

- [x] **3C.3 — Verify resources, references, and artifact paths.**

  Detect same-kind resource duplicates, same-kind external aliases of owned
  resources, broken internal function/tag IDs, reserved paths, and final path
  collisions. Preserve the legality of equal function/tag resource text.

- [x] **3C.4 — Verify tag dependency semantics.**

  Reject direct duplicate entries, replacement of shared load/tick tags, and every
  owned tag cycle. Use iterative `O(V + E)` traversal with deterministic diagnostics;
  permit ordinary function recursion and treat external tag contents as unknown.
  Stress a long acyclic tag chain.

- [x] **3C.5 — Verify provenance without cascading panics.**

  Validate resource, command, modifier, tag-entry, and nested origins against the
  supplied `SourceContext`. Invalid origins fall back predictably and never make the
  dumper or later verification layers panic.

- [x] **3C.6 — Implement the one-pass command sink.**

  Add a final-buffer `CommandSink` with Java UTF-16 accounting, checked segment
  writes, LF finalization, counter reset, and whole-function discard on failure.
  Test ASCII, BMP, non-BMP, exactly-at-limit, over-limit, and LF exclusion without a
  second target-rendering pass.

- [x] **3C.7 — Render every structured command family.**

  Implement exhaustive private rendering for atoms/SNBT, selectors, score commands,
  data commands, execute modifiers/conditions/stores, calls, returns, and raw lines.
  Use exact target tokens and preserve every semantic order. Add focused golden lines
  for all variants and prove structured valid IR has no renderer error other than
  line-length exhaustion.

- [x] **3C.8 — Close the Stage 3C gate.**

  Ensure no unchecked renderer is public, structural verification always precedes
  target rendering, malformed diagnostics use only the generic dumper, and every
  verifier rule/renderer variant appears in the proof corpus.

## Stage 3D: Deterministic artifact and trace emission

- [x] **3D.1 — Add in-memory artifact and trace models.**

  Define sorted `DatapackArtifact`/`PackFile` and dense `TraceMap`/`FunctionTrace`
  storage with read-only views. Derive physical line from `CommandId`; do not store
  redundant line, target-format, or command identity fields.

- [x] **3D.2 — Add output-only JSON dependencies and DTOs.**

  Add `serde` with derive and `serde_json` only now. Define private `Serialize` DTOs
  for `pack.mcmeta` and function tags. Do not derive `Serialize`/`Deserialize` for IR.
  Pin exact fields, omission rules, escaping, tag target prefixes, and terminal LF.

- [x] **3D.3 — Implement checked `emit_datapack`.**

  Verify first, render each function once into its final byte buffer, build trace
  origins only after successful line finalization, serialize metadata/tags, reject
  duplicate paths, sort files once, and return nothing runnable on failure. Emission
  performs no helper generation, naming, repair, optimization, or filesystem I/O.

- [x] **3D.4 — Add exact artifact/trace proofs.**

  Golden-test every path and byte, empty functions, empty/nested/optional tags,
  nested tag overlap de-duplication, compound canonicalization versus ordered lists,
  result/success distinction, top-level versus nested origins, failure discard, and
  byte-identical repeated emission.

- [x] **3D.5 — Add scale and complexity guardrails.**

  Build and emit 100,000 simple commands server-free; assert file, command, trace,
  and byte counts. Add a separate non-gating geometric benchmark that reports
  scaling without wall-clock pass/fail thresholds. Inspect for repeated scans,
  per-line temporary strings, and whole-program clones before considering packed
  storage.

- [x] **3D.6 — Close the Stage 3D gate.**

  Audit determinism, public API width, serialization boundaries, and logical path
  safety. Confirm the library still has no general filesystem writer and ordinary
  tests still start no server.

## Stage 3E: Vanilla 26.2 conformance

- [x] **3E.1 — Add the generic harness installation boundary.**

  Implement `ServerSandbox::install_datapack` over validated relative path/byte
  entries. Validate the outer pack name, revalidate every component, refuse escape,
  install before startup, and keep `mdl-test` independent from compiler internals.

- [x] **3E.2 — Add an attributable log checkpoint.**

  Record the log position before startup/reload and fail on new attributable pack
  loading or function parsing warnings/errors. Preserve the world, pack, trace, and
  logs on failure while keeping successful cleanup automatic.

- [x] **3E.3 — Build the direct-Minecraft-IR conformance pack.**

  Construct IR without Core/parser involvement. Cover fresh-world dummy objective
  initialization, every score/data/execute/call/return/raw variant, internal and
  external-style tag shapes, load/tick policy, empty functions, result versus
  success, trace line correspondence, and deterministic double emission.

- [x] **3E.4 — Run the pinned official server test.**

  Execute the ignored test with the official 26.2 server JAR and Java 25. Require no
  attributable parse/load error and exact observable scores/storage/results. Failure
  output identifies resource, command ID, physical line, origin, and preserved
  sandbox paths.

- [x] **3E.5 — Close the Stage 3E gate.**

  Record the exact server/JVM inputs and successful command-family coverage. Rerun
  fast tests without server environment variables and confirm they remain isolated.

## Stage 3F: Stage 4 handoff audit

- [x] **3F.1 — Hand-construct the branch/call shapes Stage 4 needs.**

  Using only public Stage 3 APIs, build: a one-command conditional gate, a proven
  stable dual-guard branch, and a return-based single-evaluation dispatcher. Also
  build an ordinary call that resumes and a `return run function` tail call. Emit
  exact golden artifacts for each without emitter-created helpers.

- [x] **3F.2 — Audit the legalization boundary.**

  Confirm Stage 4 can choose function boundaries, branch policy, physical
  score/storage homes, generated names, initialization, and call/return convention
  without changing Stage 3 serialization. Record any missing target operation as a
  typed Stage 3 vocabulary gap, not as a generic escape.

- [x] **3F.3 — Freeze the Stage 3 scope and public surface.**

  Verify no Core value/block/terminator appears in target APIs, no pending builder
  state reaches final IR, and raw commands remain the explicit unknown barrier.
  Remove unused exports/helpers discovered by the handoff fixtures rather than
  keeping speculative API.

- [x] **3F.4 — Run the final Stage 3 completion gate.**

  Run formatting, Clippy, all fast tests, rustdoc, the ignored vanilla conformance
  test, deterministic/scale proofs, and final diff review. Mark Stage 3 complete only
  when all required proofs below and the official-server test pass.

## Required proof ownership

| Proof requirement | Owning work item(s) |
| --- | --- |
| 1. Target facts and exact metadata | 3A.2, 3D.2, 3D.4 |
| 2. Resource IDs and traversal | 3A.3, 3C.3 |
| 3. Score/selector/range boundaries | 3A.4, 3A.6, 3B.2 |
| 4. NBT values, paths, escaping, depth | 3A.7, 3A.8, 3C.2 |
| 5. Dummy objective and score commands | 3B.2, 3E.3 |
| 6. Scalar/bulk score semantics | 3B.2, 3E.3 |
| 7. Storage commands and paths | 3B.3, 3C.7, 3E.3 |
| 8. Execute order/context/forks | 3B.4, 3B.7, 3C.7 |
| 9. Comparisons and result/success | 3B.4, 3C.7, 3E.3 |
| 10. Calls, recursion, references | 3B.1, 3B.6, 3C.3 |
| 11. Return and nested execute | 3B.4, 3C.7, 3E.3 |
| 12. Ordered/optional/nested tags and cycles | 3B.1, 3C.4, 3D.4 |
| 13. Empty functions/tags | 3B.5, 3D.4, 3E.3 |
| 14. Raw physical-line rejection | 3B.4, 3C.2 |
| 15. Unknown barriers and no reordering | 3B.7, 3B.9 |
| 16. Structural-depth rejection | 3A.8, 3B.4, 3C.2 |
| 17. Invalid origins/IDs without panic | 3B.8, 3C.1, 3C.5 |
| 18. Checked deterministic emission | 3D.3, 3D.4 |
| 19. Trace correspondence | 3D.1, 3D.4, 3E.3 |
| 20. Complete real vanilla execution | 3E.3, 3E.4 |
| 21. Scale and non-quadratic behavior | 3D.5 |

## Stage 3 completion commands

Fast gate:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Pinned vanilla gate:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test minecraft_ir -- --ignored --nocapture
```
