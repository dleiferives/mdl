# mdl-test

`mdl-test` runs generated datapacks on a real vanilla Minecraft dedicated server.
It is intentionally separate from fast compiler unit tests.

The Stage 1 smoke test creates a disposable flat world, installs a data-pack format
107.1 pack, waits for the server's readiness event, invokes a function over server
stdin, observes a score-backed success marker, stops the server, and removes the
sandbox. Server stdout and stderr are drained concurrently into `harness.log` so the
Java process cannot block on a full output pipe.

## Run the smoke test

Pass the official Minecraft 26.2 server JAR explicitly:

```sh
cargo run -p mdl-test -- smoke \
  --server-jar /path/to/server.jar \
  --java /path/to/java
```

On this development machine, Java is currently:

```text
/opt/homebrew/opt/openjdk@25/bin/java
```

The CLI also accepts environment variables:

```sh
MDL_SERVER_JAR=/path/to/server.jar \
MDL_JAVA=/path/to/java \
cargo run -p mdl-test -- smoke
```

Use `--keep` or set `MDL_KEEP_TEST_DIR=1` to retain a successful sandbox. Failed
server runs are always retained, and the error prints the sandbox path. Every
retained directory contains `harness.log`, the generated world, and the exact pack
that ran.

## Test layers

Fast tests do not start Java:

```sh
cargo test --workspace
```

The pre-scheduler semantic foundation can be exercised more narrowly:

```sh
cargo test -p mdl-compiler --lib ir::core::eval
cargo test -p mdl-compiler --test core_semantics
cargo test -p mdl-compiler --test ps2_arithmetic_control_flow
cargo test -p mdl-compiler --test ps2_fixed_aggregates
cargo test -p mdl-compiler --test ps2_owned_lists
cargo test -p mdl-compiler --test ps2_runtime_strings
cargo test -p mdl-compiler --test ps2_composition_rehearsal
cargo test -p mdl-test --lib scenario
cargo test -p mdl-test --test pre_scheduler_semantics
cargo test -p mdl-test --test outcome_channels
cargo test -p mdl-test --test exact_limit_scenario
cargo test -p mdl-test --test ps1_calibration_suite
cargo test -p mdl-test --test ps2_composition_semantics
```

The PS-2 rehearsal composes runtime string parsing, `List<Int32>` stacks and tape,
wrapping bytes, structs, and returned dispatch fuel. Its ignored companion compares
the same Core oracle with all four generated packs. The book suite separately pins
the Java 26.2 item path and empty-result fallback:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test \
  --test ps2_book_semantics \
  --test ps2_composition_semantics \
  -- --ignored --nocapture
```

The `pre_scheduler_semantics` command compiles the scalar calibration source under all four Core/Minecraft
policy products, evaluates each optimized Core program, and validates every published
ABI adapter without starting Java. Its ignored companion installs those four packs
plus one generated driver before a single server startup:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test pre_scheduler_semantics \
  scalar_core_result_matches_all_four_policies_in_one_vanilla_server \
  -- --ignored --nocapture
```

The runner retains a failed sandbox and writes `scenario-failure.txt` beside
`harness.log`; it includes the normalized contract and bounded state difference.
Set `MDL_KEEP_TEST_DIR=1` to retain a successful run too.

The outcome calibration uses the same runner and one server startup for all four
policies. It keeps missing success/result channels distinct from numeric zero and
checks ordinary failure, successful zero, `return 0`, nonzero return, zero child
contexts, and outer continuation:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test outcome_channels \
  synchronous_command_outcome_classes_are_distinct_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

Command-sequence interruption remains in the bare exact-limit layer below; it is not
faked by inserting a `return` into the semantic wrapper.

The typed exact-limit calibration invokes policy entries directly. The sequence
case uses a limit of two and proves its terminal completion write is interrupted.
The fork case creates three controlled entities, uses a fork limit of three, and
proves the rejected redirect runs no bodies while its containing function continues.
All harness setup and observation commands are separate roots:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test exact_limit_scenario \
  exact_sequence_and_fork_boundaries_run_without_semantic_wrappers_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

The batched calibration exercises bounded exact-type SNBT plus zero/one/many and
nested entity contexts for all four policies in one server lifecycle:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test ps1_calibration_suite \
  batched_nbt_and_nested_entity_calibration_runs_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

Scenario cases execute in caller order; use Cargo's test-name filter for focused
local runs. A suite is capped at 64 cases. Observations, SNBT size/depth/node count,
entity-query results, diff rendering, and attributed in-memory logs are bounded.
The complete transcript still streams to `harness.log`.

Failed scenario sandboxes additionally retain `artifacts/<policy-pack>/` compiler
IR/debug material when supplied by the deployment bridge. The official 26.2 bundle
hash accepted by these gates is
`cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`;
the extracted server hash is
`183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8`.

The real-server integration test is ignored by default:

```sh
MDL_SERVER_JAR=/path/to/server.jar \
MDL_JAVA=/path/to/java \
cargo test -p mdl-test --test vanilla_smoke -- --ignored --nocapture
```

The Stage 3 direct-Minecraft-IR conformance pack uses the same pinned server and
Java 25 boundary:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test minecraft_ir -- --ignored --nocapture
```

The focused command-limit differential test checks the exact sequence and fork
boundaries, including the accepted zero-valued gamerules:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test command_limits -- --ignored --nocapture
```

## Stage 5 measurements

Run the ignored timing suite with the release profile. Give it a fresh JSONL path
when the output will be consumed by another tool; the harness appends one validated
record for each of the tiny, normal, and scale fixtures:

```sh
MDL_MEASUREMENT_JSONL=/tmp/mdl-stage5-measurements.jsonl \
MDL_MEASUREMENT_SAMPLES=8 \
cargo test --release -p mdl-test --test stage5_measurements \
  records_raw_interleaved_stage5_measurements_without_timing_thresholds \
  -- --ignored --exact --nocapture
```

`MDL_MEASUREMENT_SAMPLES` accepts any nonzero `u32` and defaults to `8`. Every
iteration visits the declared subject order and rotates the four configuration
orders, so repeated samples are counterbalanced instead of running one configuration
in a single contiguous batch. Warm-ups use the same ordering rule. Each raw sample
keeps its execution ordinal; record construction rejects a missing, extra, reordered,
or mislabeled sample.

The compiler-only suite records the git revision and dirty state, build-time
`rustc -vV`, Cargo release profile, Rust target triple, host, and distinct Minecraft
target. It does not launch Java and therefore never records JVM, server JAR, or heap
metadata. Core-program cloning is outside the optimizer timer, and complete
compilation measures only optimize, lower, and emit; target-cost analysis and
deterministic generated-code evidence remain separate. Every measured result stays
alive until its elapsed time is captured, so output destruction is outside the phase
and complete-compilation timing windows. When
`MDL_MEASUREMENT_JSONL` is absent, records and the human-readable generated-code
evidence are both written to stderr.

The Stage 5H Core conformance gate uses one server startup for the complete
differential bundle. It first runs the fixture through Core optimization at
`None` and `Baseline`, pairs those outputs with Minecraft lowering at `None` and
`Baseline`, and installs the two terminal-call-recipe packs in the same world.
Every pack has its own namespace, register objective, and initialization
sentinel:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test core_lowering \
  optimized_core_lowering_conformance_runs_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

Before Java starts, the gate proves repeated Core optimization, lowering, and
emission are deterministic and confirms that the Baseline physical plan performs
a real home-coalescing merge. On the server it checks exact function success and
result values, result homes, both branch paths, calls, loops, parallel-copy
behavior, `i32::MIN`/`i32::MAX` transfer, terminal-call effects, reload cleanliness,
and collision-safe pack initialization. It preserves the world, all four generated
packs, logs, optimized Core dumps, optimization/lowering reports, target dumps, and
compiler traces on failure. No client connection is required.

The command-limit differential remains a separate gate and the authority for
sequence and fork boundaries; the Core conformance test does not duplicate those
limit probes.

## Stage 6 source compiler

The Stage 6 source gate compiles one checked scalar fixture under all four Core and
Minecraft `None|Baseline` policy combinations. Fast tests compare HIR, maps, Core,
reports, target IR, analysis, artifacts, traces, ABI data, failures, and provenance.
The ignored test installs all four packs in one server startup and exercises calls,
branches, mutable joins, Boolean operations, signed `Int32` extremes, `Void`, reload,
and reinvocation through generated ABI mappings:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test source_compilation \
  source_compilation_runs_all_four_policies_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

The separate =mdl= crate owns the command-line/filesystem boundary. Its ignored
server test spawns the real binary, installs the safely materialized directory, and
invokes the ABI printed by that process.

## Stage 7A package compiler

The Stage 7A server gate compiles a rooted two-module package under all four Core and
Minecraft `None|Baseline` policy combinations. The root exports one function that
calls a `pub` helper through a driver-provided local dependency name. One server
startup installs all four packs and verifies both branch outcomes through the
exported ABI:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test source_compilation \
  stage7_cross_module_export_runs_all_four_policies_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

The successful 2026-07-14 run used Homebrew OpenJDK 25.0.3 and the bundled official
26.2 server artifact with SHA-256
`cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`.
The runnable artifact is the Mojang bundler JAR (`Main-Class:
net.minecraft.bundler.Main`), not its extracted internal server JAR; the latter does
not carry required libraries such as `joptsimple`. No client connection was used.

The ordinary server harness is the initial path because compiler tests need direct
function and command invocation. Mojang's dedicated GameTest entry point remains a
useful later layer for tick-sensitive block and entity tests with JUnit-like XML
reports.

## Stage 7B unsafe-command boundary

Two opt-in tests separate the compiler's deliberately narrow physical validation
from vanilla Brigadier validation. The first compiles, loads, and executes two
literal raw fragments under all four Core/Minecraft optimization combinations. Its
first fragment is `return 1`, proving that raw control flow remains confined to the
fragment's dedicated helper and cannot skip the following source statement. The
second proves that an unknown but physically valid command remains compilable and is
then rejected by the official server while loading the generated function:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test source_compilation \
  stage7_literal_unsafe_command_runs_all_four_policies_on_vanilla_26_2 \
  -- --ignored --exact --nocapture

MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test source_compilation \
  stage7_brigadier_invalid_unsafe_command_is_rejected_only_by_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

These tests use the bundled server artifact described above and do not require a
client connection.

## Stage 7D typed `say`

The typed-command gate compiles the canonical captured-executor program under all
four Core/Minecraft optimization combinations. Before Java starts, it requires one
structured `say` node and no raw command node in every target program. One server
startup then force-loads an Overworld chunk, summons one uniquely named and tagged
armor stand, and proves that each compiled export logs the message under that
entity's name. The message deliberately includes repeated internal whitespace,
quotes, a backslash, and Unicode so the same run proves their exact preservation.
After removing the entity, it invokes every export again and proves the empty query
emits no message. A separate handwritten
`execute store result ... run say ...` probe records Java 26.2's native result as
`1`; this intentionally does not confuse the nested command result with the
compiled source function's `Void` ABI.

The presence and absence observations use log checkpoints followed by unique
console barriers, not sleeps. The server remains clientless throughout:

```sh
MDL_SERVER_JAR=/path/to/minecraft_server.26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test source_compilation \
  stage7_typed_say_runs_as_captured_executor_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

The separate command-report audit runs the pinned distribution bundle in Mojang's
data-generator mode and checks that the exact syntax leaf used by the typed recipe is
still `say -> message: minecraft:message`. This is syntax evidence only; the server
test above supplies the context, effect, multiplicity, and result evidence:

```sh
MDL_SERVER_JAR=/path/to/bundled-minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test official_command_report -- --ignored --nocapture
```

## Stage 7.5 spatial semantics

The Stage 7.5 gate combines three independent forms of evidence. The official report
test checks the selected `execute` and `teleport` Brigadier trees. A handwritten
clientless probe measures frame changes, modifier order, local coordinates, anchors,
dimension scaling, teleport outcome, and frame immutability. A compiler-generated
probe then compiles the spatial source slice under all four optimization-policy
combinations and executes every result in one server lifecycle:

```sh
MDL_SERVER_JAR=/path/to/bundled-minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test \
  --test official_command_report \
  --test spatial_command_semantics \
  --test stage75_server \
  -- --ignored --nocapture
```

The generated proof verifies frame-relative `teleport`, receiver-relative `move_by`,
sequential teleport frame immutability, and structured output under Core/Minecraft
`none|baseline`. Any behavioral drift preserves the sandbox for inspection.

## Stage 8 activation and recursion

The Stage 8 activation fixture is handwritten so it can measure Mojang behavior
independently of compiler lowering. It covers empty, one, three, and nested 3×3
contexts; complete child unwind; `return`; ordinary failure; typed command-storage
tail frames; and command-limit residue followed by recovery. The compiler fixture
then compiles direct/mutual recursion and recursion nested in a serial many-context
run body under all four Core/Minecraft policy combinations. It checks distinct
per-entity results, caller-live spill restoration, multiple results, balanced frames,
and empty recovery state. Both run on the dedicated server without a client:

```sh
MDL_SERVER_JAR=/path/to/bundled-minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test \
  --test stage8_activation_contract \
  --test stage8_compiler_server \
  -- --ignored --nocapture
```

## Primary references

- [Minecraft Java 26.2, data-pack format 107.1, and official server JAR](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Mojang's dedicated GameTest entry point and report options](https://www.minecraft.net/en-us/article/minecraft-snapshot-25w03a)
- [MLIR's separation of focused compiler checks from opt-in runtime integration tests](https://mlir.llvm.org/getting_started/TestingGuide/)
- [Wasmtime's differential execution-testing model](https://github.com/bytecodealliance/wasmtime/blob/main/fuzz/README.md)
- [Cargo build-script inputs and generated-output boundary](https://doc.rust-lang.org/cargo/reference/build-scripts.html#inputs-to-the-build-script)
- [Cargo-provided build environment metadata](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts)
- [Rust child-process and piped-I/O behavior](https://doc.rust-lang.org/stable/std/process/index.html)
