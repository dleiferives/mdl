# PS-2 Runtime Strings and Program Parsing

Status: **implemented for the PS-3 slice; broader text construction remains deferred**

## Goal

Convert runtime book text into a validated opcode sequence without exposing raw SNBT,
JSON text components, or arbitrary command construction to the program.

## Semantic string decisions

The Phase-1 contract is now:

- Java UTF-16 code units for `length` and tail consumption;
- length and slice/index units;
- bounds failure behavior;
- copy/value semantics;
- concatenation and page-joining behavior;
- conversion between display `Text`, plain `String`, and byte streams; and
- limits on source literals, runtime values, and produced output.

Minecraft Java 26.2 NBT string slicing observes Java UTF-16 units. MDL exposes that
exact unit in the current primitive rather than claiming Unicode-scalar indexing.

Brainfuck opcode recognition needs only ASCII characters. The composition rehearsal
ignores both units of a supplementary non-opcode scalar and preserves opcode order.

## Operation algebra

Prefer the smallest operations needed to traverse once:

- empty/length;
- split or consume one semantic unit from a chosen end;
- optional concatenation/page joining;
- unit equality against compile-time ASCII opcodes; and
- construction of output text/bytes through a builder or owned list.

A generic random `string[index]` is not required if destructive bounded consumption
is simpler and preserves order through one reverse. Keep parsing in normal MDL
library code where possible.

## Constant and runtime paths

Compile-time strings should fold and may be parsed entirely during compilation when
the API semantics permit. Runtime strings use an NBT string or another explicit
representation. The same semantic parser cases must agree between constant and
runtime paths.

## Typed Minecraft macro boundary

If the selected runtime operation requires a numeric start/end inside command
syntax, use a closed macro template with typed integer arguments. Required safety:

- only compiler-generated templates;
- canonical argument fields and stable ordering;
- range and syntax validation before invocation;
- no ordinary string-to-command conversion;
- explicit behavior for missing fields or invalid expansion;
- recursive/reentrant frame safety;
- deterministic target dumps and source correlation; and
- cold/hot cache cost measurements separated from semantic tests.

Before selecting a macro, compare tail consumption, fixed specialization, or a
representation that avoids runtime syntax.

## Parser contract

The Brainfuck parser should be an MDL library state machine:

- traverse normalized book text;
- append recognized opcode values;
- maintain a bracket stack or equivalent validation state;
- report unmatched close immediately or as a typed parse result;
- report unmatched opens at end; and
- retain normalized instruction positions for errors/jumps.

Compiler support should be added only for general string/list operations used by
that program.

## Required evidence

- empty, constant, runtime, and maximum-size strings;
- ASCII opcode and ignored-character recognition;
- non-ASCII including supplementary characters at semantic-unit boundaries;
- page delimiter behavior;
- invalid bounds and limit diagnostics/results;
- missing/extra/escaped macro fields if macros are selected;
- constant/runtime parser equivalence;
- unmatched bracket positions; and
- server measurements for every target-defined string operation and macro cost
  assumption.
