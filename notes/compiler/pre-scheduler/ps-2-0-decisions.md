# PS-2.0 Frozen Decisions

Status: **complete (2026-07-18)**

This file closes the semantic and ownership choices that gate PS-2 implementation.
Syntax remains revisable; the behavior below is the contract that HIR, Core, the
evaluator, libraries, lowering, and vanilla evidence must share.

## Brainfuck dialect

- Cells are wrapping unsigned bytes represented initially as normalized MDL
  `Int32` values in `0..255`. The representation is not a promise that MDL has a
  general `UInt8` machine type.
- The tape is unbounded in both directions at the semantic level and reads a newly
  visited cell as zero. Physical tests and deployments remain bounded by explicit
  list/work limits.
- The eight ASCII characters `><+-.,[]` are instructions. Every other semantic
  string unit is ignored.
- Input and output are byte lists. `,` at end of input writes zero to the current
  cell.
- The complete program is normalized and bracket-validated before any instruction
  executes. Unmatched-open and unmatched-close are distinct typed parse errors with
  normalized instruction positions.
- Every attempted dispatch of one normalized instruction consumes exactly one fuel
  unit. Parsing and bracket validation use a separate input/work budget.
- Fuel exhaustion returns output plus the complete logical tape/cursor, next
  instruction position, remaining input, and zero remaining fuel. It is resumable
  semantic state, not a Minecraft command failure.
- Page order is preserved and one newline is inserted between pages. Newline is not
  an opcode and is therefore ignored by normalization.

## Text and book boundary

The implemented Phase-1 semantic string unit is a Java UTF-16 code unit. String
length and `without_last_unit` use that unit exactly. This revises the initial
Unicode-scalar proposal after pinned target work showed that direct Minecraft string
slicing cannot implement it honestly. Brainfuck opcodes remain single ASCII units;
supplementary non-opcode scalars are traversed as two ignored units. A later
Unicode-scalar abstraction requires an explicit validated conversion.

Phase 1 written-book conversion accepts pages whose Java 26.2 `raw` component is an
NBT string. The narrow intrinsic returns empty for missing/wrong/absent/non-string
content. Styled/nested components and locale/runtime-dependent translation,
selector, score, keybind, and NBT interpolation are outside this first conversion;
the compiler does not guess the text a client would render. This supported subset
is sufficient for the controlled PS-3 Brainfuck book fixture.

The required PS-3 gate uses a controlled exact-one non-player inventory holder on a
clientless vanilla server. A real player's main-hand book remains an opt-in later
boundary; it does not justify implementing a Minecraft network client inside
`mdl-test`.

## Phase-1 limits

All limits are independently configurable downward. Exceeding a host/compiler cap
is a typed resource failure. Exceeding a semantic invocation limit is an ordinary
typed program result where specified. Target hard-limit compatibility is separate.

| Resource | Default | Hard Phase-1 maximum |
| --- | ---: | ---: |
| Written-book pages | 100 | 100 |
| Unicode scalars per page | 1,023 | 1,023 |
| Joined program-input scalars | 8,192 | 102,399 |
| Normalized instructions | 8,192 | 102,300 |
| Input bytes | 4,096 | 65,536 |
| Output bytes | 4,096 | 65,536 |
| Interpreter fuel | 16,384 | 2,147,483,647 |
| Struct fields | 64 | 1,024 |
| Aggregate nesting depth | 16 | 64 |
| List literal elements | 4,096 | 65,536 |
| Runtime list elements | 65,536 | 1,000,000 |
| Runtime string scalars | 102,399 | 1,000,000 |
| Compiler-generated macro templates | 256 | 4,096 |
| Typed fields per macro argument frame | 32 | 256 |
| Evaluator aggregate/list/string nodes | 1,000,000 | 16,000,000 |

The book maxima follow Java's documented 100-page and 1,023-character UI boundary.
The joined maximum includes 99 inserted newlines. The normalized maximum excludes
those delimiters. These are accepted-input caps, not a claim that the corresponding
program fits one Minecraft command sequence. Strict deployment separately requires
conservative proof against the configured sequence and fork limits.

## Arithmetic and loop surface

PS-2A initially exposes explicit wrapping `Int32` addition and subtraction using
Zig-like `+%` and `-%` spellings. Plain `+` and `-` remain reserved until their
checked/trapping contract is designed; negative numeric literals remain valid.
Signed comparisons and Boolean negation retain their current semantics.

The first loop surface is statement-only `while`, `break`, and `continue`:

- conditions are evaluated exactly once per attempted iteration;
- `break` exits and `continue` returns to the condition of the innermost loop;
- labels, loop values, `for`, and suspension are deferred;
- mutable source locals become ordinary loop-carried SSA block arguments; and
- Core retains CFG cycles, not a Minecraft-specific loop operation.

Wrapping byte increment/decrement are ordinary standard-library functions over
explicit wrapping arithmetic and comparisons. Division/remainder are not admitted
merely to normalize bytes; they remain deferred until zero-divisor and signed-edge
semantics receive their own contract.

## Four ownership layers

The model in [`ps-2/standard-library-boundary.md`](ps-2/standard-library-boundary.md)
is frozen:

1. language/Core owns evaluation, types, value semantics, and control;
2. public `std` modules own expressible algorithms and domain abstractions;
3. sealed typed platform intrinsics own semantic operations needing unavailable
   target facilities; and
4. compiler-private runtime helpers own physical ABIs and generated machinery.

The logical root is `std`. Phase 1 has no implicit prelude: every standard-library
module is explicitly imported. Compiler-distributed MDL source is inserted as
canonical virtual modules only when reachable, with a compiler version/content
identity retained in diagnostics and artifacts. It passes through the ordinary
lexer, checker, Core generator, optimizer, and lowerer.

Intrinsic declarations are synthesized only by compiler-owned platform modules and
carry a closed intrinsic ID, exact signature, semantic version, and target
requirements. User source cannot spell or forge that identity. A target-independent
intrinsic requires executable evaluator reference semantics. An inherently
Minecraft-specific intrinsic instead requires a complete context/cardinality/effect/
outcome contract and pinned-vanilla scenarios.

A handwritten target helper is admissible only behind such an intrinsic and must
have typed ABI, cost, reentrancy, cleanup, source-correlation, and direct vanilla
evidence. Scalar-only/no-import fixtures must prove that unreachable `std`, intrinsic,
macro, and private-runtime resources emit nothing.

## Capability ownership

| Capability | Primary owner |
| --- | --- |
| Wrapping arithmetic, comparisons, `while`/`break`/`continue` | Language/Core |
| Nominal struct, list, and string value semantics | Language/Core |
| Primitive list tail and string-consumption operations | Core operations with evaluator semantics and target recipes |
| `Byte`, stack, zipper, parser, fuel/result composition | Public `std` source |
| Brainfuck interpreter | PS-3 application source |
| Exact-one holder/slot/written-book acquisition | Sealed Minecraft intrinsic |
| Page joining and opcode normalization | Public `std` source |
| Score/NBT/macro/activation-frame machinery | Compiler-private runtime |

## Primary references

- [Zig wrapping arithmetic operators](https://ziglang.org/documentation/0.15.2/)
- [Rust loop, break, and continue semantics](https://doc.rust-lang.org/stable/reference/expressions/loop-expr.html)
- [MLIR SCF loop-carried values and lowering to CFG](https://mlir.llvm.org/docs/Dialects/SCFDialect/)
- [Minecraft item-stack and written-book components](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-5)
- [Minecraft book page and character limits](https://www.minecraft.net/en-us/article/book-and-quill)
- [Pinned Java Edition 26.2 target](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
