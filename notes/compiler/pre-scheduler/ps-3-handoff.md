# PS-3 to Stage 9 Handoff

Status: **PS-3 and Stage 8.5 complete on 2026-07-19; refreshed through Stage 9B on 2026-07-27**

PS-3 is a complete ordinary MDL Brainfuck application. It uses the accepted PS-2
surface without a Brainfuck intrinsic, raw command, forged platform operation,
private runtime name, or compiler-only test hook. The package and machine-readable
corpora live in [`../../../tests/programs/brainfuck/`](../../../tests/programs/brainfuck/).

## Implemented package

The rooted package contains `main`, `interpreter`, `parser`, `opcode`, `byte`,
`tape`, and `io` modules. Stage 7 modules expose functions but do not provide syntax
for naming another module's nominal struct in a required local annotation. PS-3
therefore keeps `RunResult` nominally owned by `interpreter`; parser module calls
cross the module boundary using `String`, `Int32`, and `List<Int32>`. This is an
honest use of the public language, not structural type emulation.

The interpreter implements:

- all eight instructions and ignored non-opcode UTF-16 units;
- wrapping byte cells represented by normalized `Int32`;
- a two-list, two-sided, zero-extending tape;
- reverse-normalized program and bracket cursors;
- parse-before-execute unmatched-open/unmatched-close errors;
- tail-consumed byte input with zero at end of input;
- ordered byte-list output;
- exact one-unit fuel consumption per dispatched opcode; and
- distinct completed, fuel, parse, text, instruction, input/output-limit,
  invalid-byte, and invalid-fuel statuses.

`cases.json` contains 30 target-independent cases. Every policy evaluates the entire
returned state against an independent Rust reference: left/right tape, past/future
program, remaining input, output, remaining fuel, dispatch count, status, and
position. Boundary functions for opcode/byte/tape/parser/I/O are also evaluated
directly.

## Book and vanilla result

The typed adapter selects exactly one tagged armor stand and walks the supported
page indices from 99 down to 0 through one runtime-indexed entity-NBT path. It
accumulates the UTF-16 unit count, normalizes into one opcode list, validates once,
and runs the same interpreter. Reverse page order plus tail traversal preserves
semantic page order; omitting the specified newline is normalization-equivalent
because newline is ignored.

One pinned Java 26.2 lifecycle installs all four policy packs. A controlled literal
book split over two pages produces byte `65` and the reader-attributed marker
`MDL_PS3_BRAINFUCK_A` under every policy. The same fixture separately proves the
wrong-item/no-program marker and malformed-bracket marker. Missing, wrong,
absent-page, and unsupported raw component shapes intentionally collapse to the
typed read's `""` fallback; PS-3 does not pretend to distinguish information the
source surface does not expose. Connected-player automation remains the explicit
nonblocking PS-2.0 deferral.

## Exact emitted evidence

These values are asserted by `capstone_footprint_recipe_and_cost_evidence_is_pinned`:

| Core | Minecraft | files | functions | lines/trace | UTF-8 bytes | max line UTF-16 | command nodes | score | data |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| None | None | 228 | 226 | 1,385 / 1,384 | 101,867 | 160 | 1,824 | 612 | 430 |
| None | Baseline | 220 | 218 | 1,054 / 1,053 | 72,990 | 160 | 1,485 | 455 | 264 |
| Baseline | None | 207 | 205 | 1,312 / 1,311 | 98,615 | 160 | 1,730 | 562 | 428 |
| Baseline | Baseline | 200 | 198 | 991 / 990 | 70,408 | 160 | 1,402 | 412 | 264 |

The same audit pins physical homes/storages, realizations, materializations, and
physical-recipe sequence work. Every product contains exactly one compiler-owned
function-macro line for the runtime page index and no source `unsafe minecraft`
escape. Target census reports zero raw commands and seven typed `say` commands. No
recursive call occurs, so this application emits no recursive activation edge or
spill bridge.

Two public program roots retain `NoFiniteBoundProven(PositiveCycle)` for
command-sequence work because runtime fuel is not currently converted into a static
CFG bound. The book root is conservatively `Unknown(RawCommand)` for sequence and
fork analysis because the runtime entity-NBT index lowers through that compiler-owned
macro line. The other three roots remain `ProvenWithin` for forks, and the load root
is finite. The vanilla run establishes the selected concrete workload, not a
universal synchronous guarantee. Strict deployment rejection and a static
fuel-to-command proof remain the already-recorded deferred breadth; PS-3 did not
invent a strict mode to satisfy a checklist sentence.

## Stage 9 inputs

Stage 9 now has a concrete persistent-work client. Suspending this interpreter must
preserve at least:

- left/right tape lists and past/future program lists;
- remaining input and accumulated output;
- semantic fuel, dispatch count, status, and normalized position;
- any in-progress bracket-search cursor if search itself can yield; and
- a stable reconstruction strategy for the output executor rather than assuming
  `@s` or its spatial frame survives a tick.

Semantic fuel and per-tick Minecraft work are different budgets. Bracket search
does not dispatch a Brainfuck opcode and therefore consumes no semantic fuel, but it
does execute target work. Stage 9 must either make each search an explicit atomic
region with a proven deployment bound or represent its cursor as suspendable state.
Book acquisition can remain an eager pre-execution phase for this client; retaining
a live inventory read across ticks is not required.

Stage 9 must also preserve the existing abnormal-interruption rule. A normal typed
fuel result is resumable application state. A Minecraft command-limit abort is a
poisoned runtime event requiring the generated load/recovery entry; it is not a
Brainfuck result.

## Reproduction

Fast suite:

```sh
cargo test -p mdl-test --test ps3_brainfuck --no-default-features
```

Pinned vanilla suite:

```sh
MDL_SERVER_JAR=/path/to/minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test ps3_brainfuck --no-default-features \
  split_written_book_executes_the_capstone_under_all_four_policies \
  -- --ignored --nocapture
```
