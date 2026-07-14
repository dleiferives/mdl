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

The ordinary server harness is the initial path because compiler tests need direct
function and command invocation. Mojang's dedicated GameTest entry point remains a
useful later layer for tick-sensitive block and entity tests with JUnit-like XML
reports.

## Primary references

- [Minecraft Java 26.2, data-pack format 107.1, and official server JAR](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Mojang's dedicated GameTest entry point and report options](https://www.minecraft.net/en-us/article/minecraft-snapshot-25w03a)
- [MLIR's separation of focused compiler checks from opt-in runtime integration tests](https://mlir.llvm.org/getting_started/TestingGuide/)
- [Wasmtime's differential execution-testing model](https://github.com/bytecodealliance/wasmtime/blob/main/fuzz/README.md)
- [Cargo build-script inputs and generated-output boundary](https://doc.rust-lang.org/cargo/reference/build-scripts.html#inputs-to-the-build-script)
- [Cargo-provided build environment metadata](https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-build-scripts)
- [Rust child-process and piped-I/O behavior](https://doc.rust-lang.org/stable/std/process/index.html)
