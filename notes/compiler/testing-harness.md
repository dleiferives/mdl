# Compiler Test Harness

Status: **source-fixture, explicit-golden, generated-report, and clientless-server
layers implemented**

## Problem

The compiler needs exact tests, but not every test should compare an entire textual
dump. Full-string assertions made small source-language work unnecessarily iterative:
an irrelevant space or trailing newline could fail beside real verifier, lowering,
or target defects. Running `cargo test -p mdl-compiler <filter>` also starts every
integration-test binary even when the filter names only a library test.

The test strategy now distinguishes three contracts:

1. Typed unit tests inspect AST/HIR/IR structure and verifier failures directly.
2. Source fixtures state only the semantically relevant observations at each phase.
3. Exact goldens remain for outputs whose complete byte-stability is itself public
   evidence.

Real-server conformance remains a separate opt-in layer. A source fixture proves
compiler transformations; the server proves Mojang accepts and executes the emitted
datapack.

Stage 7 also separates generated syntax evidence from runtime semantics. The ignored
`official_command_report` test hashes the pinned 26.2 distribution bundle, runs
Mojang's data generator, and checks the exact `say -> message: minecraft:message`
leaf. The typed-`say` server test separately proves executor attribution, empty-query
skipping, literal preservation, and native result. Neither test is used to make
claims outside the evidence it observes.

## Data-driven source fixtures

Fixtures live below `crates/mdl-compiler/tests/source-fixtures/`. They are ordinary
MDL files, and the MDL lexer already treats their harness directives as comments:

```mdl
// MDL: success
// HIR: capture speaker: Executor<ArmorStand>
// CORE: minecraft.run_scope @run0
// PACK-COUNT-1: execute as @e[type=minecraft:armor_stand
export fn announce() {
    // source under test
}
```

The supported checks are `HIR`, `CORE`, `LOWERING`, `TARGET`, and `PACK`. A bare
check means “present at least once”; `-NOT` means absent; `-COUNT-N` requires exactly
`N` non-overlapping occurrences. Malformed harness-like directives fail instead of
being silently ignored.

A negative fixture names both its owned failure boundary and diagnostic:

```mdl
// MDL: failure minecraft-lowering lower.unbounded-command-forks
```

Every fixture automatically runs the four `None|Baseline` Core/Minecraft policy
combinations. Each combination compiles twice and compares all inspected artifacts,
the emitted pack, and failure rendering for determinism. On a failed artifact check,
the runner writes complete HIR, Core, lowering, target, and pack text beneath
`target/mdl-fixture-failures/<fixture>/<policy>/`.

Run all source fixtures with:

```sh
cargo test -p mdl-compiler --test source_fixtures
```

Run one named fixture without launching unrelated test binaries with:

```sh
MDL_FIXTURE_FILTER=run_as_at_most_one \
  cargo test -p mdl-compiler --test source_fixtures -- --nocapture
```

For a focused unit test, specify the library target explicitly:

```sh
cargo test -p mdl-compiler --lib module::tests::name -- --exact
```

Run the two Stage 7 Java gates with the exact official distribution bundle:

```sh
MDL_SERVER_JAR=/path/to/bundled-minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test official_command_report -- --ignored --nocapture

MDL_SERVER_JAR=/path/to/bundled-minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test source_compilation \
  stage7_typed_say_runs_as_captured_executor_on_vanilla_26_2 \
  -- --ignored --exact --nocapture
```

Stage 8 adds two focused clientless gates. The activation fixture measures zero,
one, many, nested, return, failure, tail-frame, and limit-abort/recovery behavior.
The compiler fixture builds direct/mutual recursion plus recursion inside serial
run children under all four policy combinations:

```sh
MDL_SERVER_JAR=/path/to/bundled-minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test \
  --test stage8_activation_contract \
  --test stage8_compiler_server \
  -- --ignored --nocapture
```

Fast Stage 8 gates remain in the ordinary workspace suite; the dedicated files are
`stage8_lowering`, physical-plan unit corruption/scale tests, the four-policy source
fixture runner, and the non-ignored structural/determinism portion of
`stage8_compiler_server`.

## Exact goldens

Stage 4's whole-output compatibility evidence still uses exact golden files. A
mismatch now writes the new output below `target/mdl-golden-failures/` and prints a
ready-to-run `diff -u` command. Intentional changes can be accepted explicitly:

```sh
MDL_BLESS=1 cargo test -p mdl-compiler --test stage4_lowering
```

Blessing is never automatic. The generated diff must still be reviewed, and a clean
run after blessing confirms that the embedded expected file and producer agree.

## Rules for new tests

- Assert typed structure when a public query exists; do not parse a dump merely to
  recover structure.
- Use phase patterns when the rendered semantic fact is the contract but unrelated
  lines are not.
- Use a whole-output golden only for a deliberately frozen serialization or
  compatibility oracle.
- Keep compiler rejection, target-legality rejection, datapack reload, and runtime
  behavior as separate cases so a later layer cannot hide an earlier regression.
- Reproduce any discovered bug with the narrowest unit test and retain one vertical
  fixture only when the cross-phase relationship matters.

## Primary references

- [rustc UI tests and explicit `--bless` workflow](https://rustc-dev-guide.rust-lang.org/tests/ui.html)
- [LLVM `lit` test discovery, filtering, and update hooks](https://llvm.org/docs/CommandGuide/lit.html)
- [LLVM FileCheck partial and exact-pattern assertions](https://llvm.org/docs/CommandGuide/FileCheck.html)
- [Cranelift file tests with test directives beside CLIF input](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/filetests/README.md)
- [MLIR testing guide](https://mlir.llvm.org/getting_started/TestingGuide/)
