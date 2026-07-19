# Pre-Scheduler Evidence Inventory

Status: **PS-1 baseline plus accepted PS-2 capability evidence**

This inventory records which existing layer owns each kind of claim. PS-1 extends
these suites; it does not make one oracle impersonate another.

## Fast compiler suites

| Suite | Existing authority |
| --- | --- |
| Compiler module tests | Smart-constructor, verifier, corruption, analysis, lowering, emission, and scale invariants |
| `core_proofs.rs` | Typed SSA formation, dominance, recursive-call validity, verifier diagnostics |
| `core_optimization.rs` | Pass-local structural contracts and reports |
| `core_semantics.rs` | Executable Core results and ordered internal-call observations across `None`/`Baseline`, including generated CFG families |
| `ps2_arithmetic_control_flow.rs` | Wrapping arithmetic, mutable loops, and exact evaluator results across Core policies |
| `ps2_fixed_aggregates.rs` | Nested nominal-value scalarization across calls, branches, and target lowering |
| `ps2_owned_lists.rs` | `List<Int32>` copy semantics and tail operations across calls and target storage |
| `ps2_runtime_strings.rs` | UTF-16-unit traversal and static suffix/slice lowering without command interpolation |
| `ps2_composition_rehearsal.rs` | Parser, stack/zipper, byte, fuel, evaluator-limit, and four-policy composition |
| `source_fixtures.rs` | Source-to-HIR/Core/lowering/target/pack phase checks |
| `stage3_handoff.rs` | Core construction and handoff invariants |
| `stage4_lowering.rs` | Compatibility lowering and exact reviewed Stage 4 goldens |
| `stage5_baseline_lowering.rs` | Baseline target transformations, physical homes, and deterministic output |
| `stage8_lowering.rs` | Recursive activation-frame and physical-ABI structure |

The exact files in `crates/mdl-compiler/tests/golden/` remain compatibility oracles,
not general semantic snapshots. New semantic cases should prefer Core evaluation,
typed queries, and narrow phase checks.

## Fast harness suites

`mdl-test` library tests cover disposable sandbox safety, datapack installation,
attributed logs, pinned artifact hashing, measurement-record validation, typed
scenario validation, normalized observation comparison, driver materialization, and
score-feedback parsing. They never start Java.

`pre_scheduler_semantics.rs` compiles the first scalar case under all four policy
products in the default fast suite. It evaluates each optimized Core program and
constructs every policy deployment from the compiler-published export ABI. Its
ignored companion is the vanilla execution gate.

`outcome_channels.rs` constructs the typed synchronous outcome calibration under
all four policies. Its ignored companion distinguishes absent result channels,
ordinary failure, successful zero, `return 0`, nonzero return, zero child contexts,
and outer continuation on vanilla. Abnormal command-sequence interruption remains
an exact-limit calibration rather than being simulated by the semantic wrapper.

`exact_limit_scenario.rs` constructs typed sequence and fork bare-root boundary cases. Their
terminal completion write is part of the measured function, while setup, barriers,
publication, and queries are independent console roots. The fast test proves the
semantic and exact runner surfaces cannot be interchanged.

`ps1_calibration_suite.rs` runs exact-type storage NBT plus zero/one/many and nested
entity-context cases across all four policies in one server lifecycle. Entity setup
uses a scheduled condition probe rather than a fixed sleep or tick count.

## Ignored pinned-vanilla suites

| Suite | Runtime evidence |
| --- | --- |
| `vanilla_smoke.rs` | Server startup, pack loading, function execution |
| `official_command_report.rs` | Bundled Brigadier command-report compatibility |
| `minecraft_ir.rs` | Direct Minecraft IR emission and execution |
| `core_lowering.rs` | Generated scalar Core lowering on vanilla |
| `source_compilation.rs` | Source compiler policies, reload behavior, exports, typed `say`, unsafe boundary |
| `spatial_command_semantics.rs` / `stage75_server.rs` | Execution-frame and spatial semantics |
| `stage8_activation_contract.rs` | Serial contexts, completion, frames, abort residue/recovery |
| `stage8_compiler_server.rs` | Generated recursion and serial context behavior under four policies |
| `command_limits.rs` | Exact sequence/fork boundaries and runtime selector multiplicity |
| `pre_scheduler_semantics.rs` | One-startup normalized scalar differential for all four policies |
| `outcome_channels.rs` | Optional success/result channels and synchronous continuation across four policies |
| `exact_limit_scenario.rs` | Bare-root sequence interruption plus exact fork rejection/continuation |
| `ps1_calibration_suite.rs` | Batched exact-type NBT and entity multiplicity/context behavior |
| `ps2_book_semantics.rs` | Java 26.2 written-book path, empty fallback, UTF-16 slices, and compiled typed intrinsic |
| `ps2_composition_semantics.rs` | Parser/tape/fuel rehearsal result across all four compiler policy products |

These tests require the pinned official Java 26.2 bundle or extracted server JAR and
Java 25. They are clientless and operate in controlled force-loaded chunks where
world entities or blocks matter.

## Measurement suite

`stage5_measurements.rs` and the measurement record types own warmup, rotation,
environment metadata, sample status, and raw timing evidence. Timing is not an
ordinary semantic assertion and does not set a correctness threshold.

## Frozen authorities

1. The bounded Core evaluator owns target-independent execution for its supported
   closed subset.
2. Typed compiler queries, reports, phase patterns, and deliberately reviewed exact
   goldens own compiler structure and selected recipe facts.
3. The pinned vanilla server owns command parsing, world transitions, execution
   context, completion behavior, and hard-limit boundaries.

The normalized scenario projection compares only declared test-owned scores,
storage NBT, blocks, projected entity sets/multisets, and public log effects. Missing
values remain distinct from zero/empty values. UUIDs, timestamps, selector order,
and compiler-private runtime paths are excluded unless a separate target contract
explicitly makes one observable.

## Scenario modes

- **Semantic:** generous hard limits, typed observation wrapper, explicit expected
  state, and policy differential.
- **Exact limit:** bare measured root followed by a new console observation root;
  semantic wrappers are unrepresentable in this mode.
- **Measurement:** named measurement protocol with environment metadata and no
  ordinary pass/fail timing threshold.

The supported declarative surface remains deliberately smaller than arbitrary
commands. Compiler ABI adaptation is a validated single-line Rust escape boundary;
unusual target research remains a focused Rust test. This prevents fixture data from
becoming a second unsafe command language.

Runtime-dependent and target-version facts continue in
[`../semantic-ambiguities.md`](../semantic-ambiguities.md), including selector
multiplicity/visibility, completion analysis, and the two Java 26.2 command limits.
