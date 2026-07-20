# PS-4 Handoff

Status: **complete on 2026-07-19**

PS-4 implements closed fieldless enums, exhaustive Zig-style switch expressions
and statements, and inclusive signed `Int32` range patterns through the complete
source-to-datapack path. The ordinary validation package and independent case table
live in [`../../../tests/programs/enums-switch-ranges/`](../../../tests/programs/enums-switch-ranges/).

## Public language surface

- `const Name = enum { variant, ... };` declares a nonempty nominal enum.
- `Name.variant` is qualified; `.variant` requires an exact enum context.
- Enum values may flow through locals, mutation, structs, private calls, returns,
  branches, same-tick loops, and recursion.
- Same-enum `==` and `!=` are supported. Ordering, arithmetic, implicit integer
  conversion, and comparisons between distinct nominal enums are rejected.
- `switch (value) { patterns => expression, ... }` produces a value; every arm has
  one exact result type.
- `switch (value) { patterns => { statements }, ... }` is a structured statement
  and participates in definite-assignment and continuation analysis.
- Integer labels admit signed exact values, inclusive `min...max` ranges, multiple
  patterns sharing one body, and `else`. Enum labels admit inferred or qualified
  variants. There is no fallthrough.
- Patterns must be disjoint and exhaustive. `else` must be last and is rejected
  when preceding enum/range coverage is already complete.
- Datapack exports recursively reject enum-containing parameters and results because
  the scalar scoreboard ABI cannot uphold closed-enum validity.

The authoritative implemented grammar is
[`../../syntax/grammar.ebnf`](../../syntax/grammar.ebnf). The original
[`../../syntax/ps-4-grammar-delta.ebnf`](../../syntax/ps-4-grammar-delta.ebnf) is
retained only as the reviewed historical delta. Parser coverage maps the new
productions as follows:

| Production area | Accepted evidence | Rejected/recovery evidence |
| --- | --- | --- |
| enum declaration and variants | parser PS-4 positive test and validation fixture | empty/duplicate/reserved semantic tests; missing-name/body parser diagnostics |
| inferred and qualified variants | parser/checker tests and validation fixture | context-free, unknown, and wrong-nominal checker tests |
| expression and statement switches | parser PS-4 positive test and validation fixture | malformed-arrow recovery before the next function |
| exact, signed range, multiple, and `else` labels | parser dump and independent boundary corpus | backwards, overlap, holes, misplaced/unreachable `else` tests |
| `=>`, `...`, and adjacent dot/arrow punctuation | maximal-munch lexer table | malformed punctuation recovery table |

## Compiler representation and lowering

The checked HIR owns dense `SourceEnumId` and per-enum `SourceVariantId` identities,
typed enum literals, normalized scalar patterns, and separate switch expression and
statement nodes. HIR verification independently recomputes identity, type, coverage,
overlap, exhaustiveness, result, flow, and export-ABI invariants. Interval coverage
uses ordered predecessor/successor lookup and never enumerates an admitted range.

HIR-to-Core erases a variant to its private dense `i32` tag. Switches become an
ordinary binary CFG test chain: the scrutinee is evaluated once, multiple patterns
share their destination, expression results join through typed block parameters,
and statement state uses the existing sparse environment merge. The last exhaustive
or `else` arm is the structural fallback.

`core.i32.in_closed_range` carries a validated inclusive `I32ClosedRange`. It is
pure, total, structurally equivalent, ordered, and discardable; the builder,
verifier, evaluator, printer, ambient analysis, canonicalization, CSE, SCCP, DCE,
and operation censuses understand it. Constant operands fold without introducing
general interval analysis.

Minecraft lowering initializes a normalized Boolean score to zero and conditionally
sets it to one through typed `Condition::ScoreMatches` and `ScoreRange`. Consequently
MDL `0...20` is emitted as Minecraft `matches 0..20`; equal bounds retain Minecraft's
canonical exact-value spelling. This is legalization, not dispatch optimization.

## Evidence

`ps4_enums_switch_ranges.rs` compiles the fixture under all four Core/Minecraft
None/Baseline policy products. Every product is evaluated against an independent
JSON table covering `i32::MIN`, negative values and lower bounds, both inclusive
endpoints, holes, adjacent exact/range patterns, `i32::MAX`, and `else`. The test
also requires native `matches 0..20`, successful target analysis with score work,
and nonempty deterministic footprint reporting.

The ignored `mdl-test` scenario installs all four distinct policy datapacks in one
pinned Java 26.2 server lifecycle, invokes their public scalar ABIs for the same
case table, and verifies public scoreboard results. It uses the PS-1 sandbox,
forceload, hash, cleanup, and concise failure-artifact contracts; no client is
required.

## Deliberately deferred

PS-4 does not add payload variants, `List<Enum>`, imported nominal type spelling, a
stable external enum ABI, guards/captures/destructuring patterns, case reordering,
range coalescing, balanced/DAG/macro dispatch, profile feedback, or a datapack-size
failure threshold. Function/line/byte and command/fork/storage evidence remains
available for later benchmarking. Persistent continuations and cross-tick execution
remain Stage 9 work.

PS-5 may treat nominal enums and exhaustive switches as established clients while
adding structural anonymous results and destructuring. After PS-5, the Brainfuck
interpreter can be rewritten using these language features without changing the
frozen PS-3 behavioral oracle.

## Reproduction

Fast target-independent and lowering evidence:

```sh
cargo test -p mdl-compiler --test ps4_enums_switch_ranges
```

Pinned vanilla evidence:

```sh
MDL_SERVER_JAR=/path/to/minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test ps4_enums_switch_ranges \
  enum_switch_range_products_match_vanilla_in_one_lifecycle \
  -- --ignored --nocapture
```
