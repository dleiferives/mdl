# PS-2 Brainfuck Semantic Contract

Status: **frozen by [`../ps-2-0-decisions.md`](../ps-2-0-decisions.md)**

## Purpose

Freeze one dialect so compiler capabilities and the PS-3 capstone have a stable
client. These choices belong to the program/library contract unless MDL exposes a
more general type operation.

## Recommended initial dialect

| Question | Recommendation | Reason |
| --- | --- | --- |
| Cell | wrapping 8-bit unsigned | Conventional and exercises explicit normalization |
| Physical scalar | MDL `Int32` normalized to `0..255` | Avoids requiring a full integer-width system initially |
| Tape | two-sided, zero-extending | Exercises zipper movement without an arbitrary left-edge trap |
| Program characters | eight ASCII opcodes; ignore all others | Allows comments and book formatting |
| Input/output | bytes | Keeps Brainfuck I/O distinct from Unicode text |
| `,` at end of input | configurable choice; recommend zero | Deterministic and simple, but must be explicit |
| Bad brackets | parse error before execution | Prevents partial execution of structurally invalid source |
| Execution | mandatory finite fuel | Gives deterministic pre-scheduler termination |
| Fuel unit | one dispatched Brainfuck instruction | Stable source-level accounting independent of mcfunction lines |

## Tape model

Define the logical tape independently from the two-list representation:

```text
Tape {
  cells: integer-indexed finite nonzero/visited region
  cursor: integer
}
```

Reading an unvisited cell returns zero. Moving in either direction establishes a
zero cell when needed. The public semantics do not expose list orientation,
rebalancing, NBT paths, or whether zero cells are retained physically.

## Program model

Book/page text is normalized into the eight instructions:

```text
> < + - . , [ ]
```

All other semantic string units are ignored. Bracket validation produces a typed
error identifying at least unmatched-open versus unmatched-close and a normalized
instruction position. Source book offsets may be retained as optional provenance,
but they must not be reconstructed from a lowered list.

The representation may be a two-list program zipper. Jump-table construction is an
optional optimization, not a semantic requirement.

## I/O model

Keep byte streams separate from display text:

```text
Input  = List<ByteValue>
Output = List<ByteValue>
```

The first implementation may encode `ByteValue` as checked/normalized `Int32`. A
later text adapter may decode bytes explicitly. `.` appends the current cell byte;
`,` consumes one input byte or applies the frozen end-of-input policy.

## Fuel and results

Recommended result algebra:

```text
InterpretResult =
  Completed { output, final_tape }
  | StepLimitExceeded { output, partial_tape, next_instruction }
  | InvalidProgram(ParseError)
```

Whether this is expressed initially as an enum, tagged struct, or equivalent closed
sum encoding depends on the general aggregate capability design. It must not be
represented by Minecraft command failure.

Parsing has a separate maximum input/program-unit bound. Fuel applies only after a
valid normalized program exists. Minecraft command-sequence abortion is an abnormal
deployment failure and must remain distinct from `StepLimitExceeded`.

## Book boundary

Recommended normalization:

- preserve page order;
- insert one non-opcode newline between pages;
- apply the same ignore-non-opcode rule afterward; and
- reject books whose inspected semantic text exceeds the configured program-input
  limit before or during bounded parsing with no partial interpreter execution.

Formatted text/component behavior must be pinned against the Java 26.2 item model.
The compiler API should return semantic text, not raw SNBT or JSON component source.

## Decisions to confirm

- [x] Accept wrapping 8-bit cells represented initially through normalized `Int32`.
- [x] Accept a two-sided zero-extending tape.
- [x] Use zero at end of input for `,`.
- [x] Accept ignoring all non-opcode semantic characters.
- [x] Accept parse-before-execute bracket validation.
- [x] Accept one dispatched instruction as one fuel unit.
- [x] Return complete logical interpreter state on fuel exhaustion.
- [x] Join pages with newline and accept only deterministic literal component text initially.
- [x] Freeze configurable defaults and hard maxima in the PS-2.0 decision record.

## Required semantic cases

- empty and comment-only program;
- wrap at zero and 255;
- repeated movement across both sides of the origin;
- zero, one, and exhausted input;
- byte output including zero and 255;
- simple and nested loops;
- unmatched open/close brackets;
- fuel zero, exact completion, one short, and surplus;
- multi-page and non-ASCII ignored text; and
- program/input limit boundaries.
