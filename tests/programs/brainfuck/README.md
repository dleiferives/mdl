# MDL Brainfuck capstone

This package is the Stage 8.5 PS-3 composition proof. Every `.mdl` file is ordinary
source supplied through the public in-memory package API. It contains no raw
Minecraft command, compiler-private storage/objective name, synthetic intrinsic, or
test-only compiler hook.

The frozen dialect uses wrapping byte cells represented by normalized `Int32`, a
two-sided zero-extending tape, byte-list input/output, ignored non-opcode UTF-16
units, parse-before-execute bracket validation, and one unit of fuel per dispatched
Brainfuck instruction.

`cases.json` is the target-independent corpus. Its input arrays use the public MDL
list representation, where the list tail is the next byte; output arrays are in
emission order. `book-cases.json` records the separate pinned-vanilla adapter cases.
The adapter walks the supported Java page indices from 99 down to 0 through one
runtime-indexed entity-NBT path, so tail parsing preserves page order without
unrolling 100 reads or needing string concatenation. The omitted newline delimiter
is normalization-equivalent because newline is not an opcode.

The implementation deliberately exercises the current language surface: inferred
declarations, switch statements and expressions, anonymous tuple returns,
destructuring assignment, string-literal NBT path segments, and runtime list-path
indexing.

Status codes are:

- `0`: completed;
- `1`: fuel exhausted;
- `2`: unmatched opening bracket;
- `3`: unmatched closing bracket;
- `4`: input text unit limit exceeded;
- `5`: normalized instruction limit exceeded;
- `6`: output byte limit exceeded;
- `7`: input byte limit exceeded;
- `8`: an input element is outside `0..=255`; and
- `9`: fuel is negative.

The required book gate uses an exact-one tagged armor stand holding a controlled
plain-literal written book. Connected-player automation is explicitly deferred by
the accepted PS-2.0 decision.

Run the fast four-policy semantic and inspection suite with:

```sh
cargo test -p mdl-test --test ps3_brainfuck --no-default-features
```

Run the official-server gate with the pinned Java 26.2 bundle and Java 25:

```sh
MDL_SERVER_JAR=/path/to/minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test ps3_brainfuck --no-default-features \
  split_written_book_executes_the_capstone_under_all_four_policies \
  -- --ignored --nocapture
```
