# PS-2 to PS-3 Handoff

Status: **PS-3-ready capability slice complete on 2026-07-18**

PS-2 now has enough public, ordinary MDL behavior to write the Brainfuck capstone.
The composition rehearsal already performs the same kinds of parsing, tape movement,
byte updates, output accumulation, and fuel accounting that PS-3 needs. It compiled
under all four Core/Minecraft policy products and produced the same scalar checksum
on the pinned vanilla Java 26.2 server.

This handoff lists the actual implemented surface. It does not turn broader ideas in
the PS-2 dossiers into claims about the compiler.

## Public language and value surface

- Explicit wrapping `Int32` addition/subtraction: `+%` and `-%`.
- Signed comparisons, Boolean negation, `if`/`else`, `while`, `break`, `continue`,
  calls, recursion, modules, mutable locals, and returns.
- Nominal immutable-value structs with nested construction/projection and complete
  scalarized call/return/branch ABI lowering.
- `List<Int32>` with `List.empty()`, `length()`, `push(value)`,
  `last_or_zero()`, and `without_last()`.
- `String` with literals, `length()`, `ends_with_ascii("x")`, and
  `without_last_unit()`.
- Struct/list/string values have source-level copy semantics. Backend destructive
  reuse is permitted only when it preserves those values.

`String` units are Java UTF-16 code units in Phase 1. Brainfuck opcodes are ASCII,
so supplementary non-opcode text is safely ignored as two units. This is not a
Unicode-scalar API.

## Public Minecraft surface used by PS-3

Inside a current inventory-capable executor scope:

```mdl
const page: String = reader.main_hand_written_book_literal_page_or_empty(0);
```

The argument is a compile-time page index in `0..99`. Java 26.2 reads the current
executor's
`equipment.mainhand.components."minecraft:written_book_content".pages[i].raw`.
Missing equipment, a wrong item, an absent page, or non-string raw content returns
`""`. A controlled plain-literal written book is the required PS-3 fixture.
Lowering realizes that partial conversion as an adjacent two-command fragment:
initialize the result to `""`, then attempt the entity read. Reconciliation verifies
both commands and their shared result home; source correlation names the primary
read command, while whole-target analysis counts both.

Page joining needs no new primitive for Brainfuck. PS-3 may parse static pages from
99 down to 0, carrying one opcode list through each call. Tail-oriented parsing then
leaves page 0's first opcode at the final list tail. The specified newline delimiter
is not an opcode and can be omitted by this normalization-equivalent path.

## Ordinary-library patterns already rehearsed

The PS-2 composition source demonstrates:

- byte increment/decrement over normalized `Int32` values in `0..255`;
- a stack over list tail operations;
- a two-list tape zipper with the current cell at the right-list tail;
- zero extension on movement beyond visited cells;
- reverse tail parsing that preserves source execution order;
- all eight opcode recognition and ignored Unicode/non-opcode units;
- distinct unmatched-open and unmatched-close parser statuses;
- typed aggregate parse/run states; and
- exactly one fuel decrement per attempted normalized dispatch.

These are ordinary MDL functions. PS-3 should move reusable pieces into its explicit
package modules; it must not add a hidden Brainfuck Core operation.

## Resource and deployment boundary

- The Core evaluator independently caps steps, active call depth, and created
  aggregate/list/string value nodes. The default value-node cap is 1,000,000.
- Runtime fuel exhaustion is ordinary returned program state. It is not implemented
  by deliberately hitting Minecraft's command-sequence limit.
- Minecraft target analysis remains conservative for data-dependent CFG cycles. It
  may report `NoFiniteBoundProven` even when the MDL program carries runtime fuel.
  PS-3's small synchronous corpus is therefore validated by the evaluator and pinned
  vanilla runs; no compiler claim of a general static loop bound is permitted.
- Persistent continuation, automatic splitting, and tick scheduling remain Stage 9.
- Abnormal Minecraft command-limit termination may leave private frame residue;
  the existing generated load/recovery entry remains authoritative.

## Deliberately deferred breadth

These are not blockers for the controlled PS-3 capstone:

- generic `List<T>` beyond `List<Int32>`, nested lists, and random indexing;
- stable compiler-injected `std` distribution/authentication. PS-3 uses explicit
  ordinary package modules over the public primitives;
- rich `Text` flattening, styled/nested page components, and dynamic text kinds;
- a connected-player automation fixture;
- general string concatenation or Unicode-scalar traversal;
- compile-time parsing/inlining optimizations and static fuel-to-command proofs; and
- dynamic NBT-path macros.

If PS-3 discovers that one of these is actually required for semantic correctness,
it returns to PS-2 as a named gap. It must not be hidden in raw commands or private
storage.

## Evidence gate

- Core/four-policy composition:
  [`../../../crates/mdl-compiler/tests/ps2_composition_rehearsal.rs`](../../../crates/mdl-compiler/tests/ps2_composition_rehearsal.rs)
- Ordinary rehearsal source:
  [`../../../crates/mdl-compiler/tests/source-fixtures/pre-scheduler/ps2_composition_rehearsal.mdl`](../../../crates/mdl-compiler/tests/source-fixtures/pre-scheduler/ps2_composition_rehearsal.mdl)
- Four-policy vanilla differential:
  [`../../../crates/mdl-test/tests/ps2_composition_semantics.rs`](../../../crates/mdl-test/tests/ps2_composition_semantics.rs)
- Pinned book path, UTF-16 slices, fallback, and compiled intrinsic:
  [`../../../crates/mdl-test/tests/ps2_book_semantics.rs`](../../../crates/mdl-test/tests/ps2_book_semantics.rs)
- Primitive vertical tests: `ps2_arithmetic_control_flow.rs`,
  `ps2_fixed_aggregates.rs`, `ps2_owned_lists.rs`, and
  `ps2_runtime_strings.rs` in `crates/mdl-compiler/tests`.

PS-3 may now begin at PS-3.0 by freezing its explicit module layout and case corpus
against this surface.
