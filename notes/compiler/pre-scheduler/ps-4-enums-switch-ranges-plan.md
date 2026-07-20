# PS-4: Enums, Switch, and Inclusive Range Patterns

Status: **complete on 2026-07-19**

## Identity

- **Capability:** closed fieldless enums, Zig-style `switch`, and inclusive integer
  range patterns;
- **Substage:** PS-4;
- **Owner documents:** this plan and
  [`ps-4-enums-switch-ranges-todo.md`](ps-4-enums-switch-ranges-todo.md);
- **Required before resuming Stage 9:** yes by the accepted work order, but not
  because scheduling semantically depends on enums; and
- **Optimization posture:** implement one mechanical lowering, permit large
  generated datapacks, retain measurements, and defer dispatch selection.

PS-4 is an accepted post-capstone language-capability milestone. It does not alter
the already-achieved PS-1 through PS-3 exit or pretend to be a new Brainfuck
requirement.

## Why PS-4 exists

The Brainfuck package and other current MDL programs encode closed states, opcodes,
and result kinds as unrelated `Int32` constants. That loses nominal type checking,
makes invalid states expressible, and produces long `if` chains whose intent is
harder to inspect than the generated Minecraft range tests.

Minecraft already provides a useful physical predicate:

```mcfunction
execute if score fungi88 booger matches 0..20 run say hi
```

The bounds are inclusive. The existing target IR already models all canonical score
range shapes with `ScoreRange` and renders them through
`Condition::ScoreMatches`. What is missing is a target-independent source path for
closed choices and integer interval tests.

PS-4 adds that path without making Minecraft syntax the language semantics and
without solving the later dispatch-optimization problem.

## Frozen source surface

The implemented grammar authority is [`../../syntax/grammar.ebnf`](../../syntax/grammar.ebnf).
PS-4's implemented productions are in that authority. The original accepted delta
is retained as the historical
[`../../syntax/ps-4-grammar-delta.ebnf`](../../syntax/ps-4-grammar-delta.ebnf).

### Closed fieldless enums

The initial declaration and value syntax follows the already selected Zig-style
surface:

```mdl
const State = enum {
    idle,
    running,
    complete,
};

fn initial() -> State {
    return .idle;
}

fn is_terminal(state: State) -> Bool {
    return state == State.complete;
}
```

The first slice has these rules:

- an enum is nominal: two declarations with identical variants are different types;
- every enum has at least one unit variant;
- variant names are unique within the declaration;
- tags are assigned densely in declaration order starting at zero for lowering, but
  tags are not source-visible semantics;
- variants may be written as `Type.variant` or as `.variant` when an exact expected
  enum type is available;
- equality and inequality are permitted only between the same enum type;
- ordered comparison, wrapping arithmetic, and implicit enum/integer conversion are
  rejected;
- enum values may be locals, mutable bindings, parameters, private/public function
  results, and struct fields; and
- an enum may cross an internal recursive call because its erased Core value is an
  ordinary `i32` leaf.

Explicit tag types, explicit discriminants, integer casts, methods declared inside
an enum, non-exhaustive enums, zero-variant enums, and payload variants are deferred.
Reordering variants may change generated discriminants but cannot change correct
source behavior.

An inferred literal receives context from an explicitly typed declaration or
assignment, a function parameter/result, a struct field, an enum switch scrutinee,
or the typed counterpart of `==`/`!=`. Two context-free literals such as
`.ready == .ready`, or expression arms containing only inferred literals when the
whole switch has no expected result type, are rejected rather than guessed.

### Export and module boundary

A datapack caller can place any `i32` in an exported scoreboard parameter. Such a
caller cannot uphold the closed-enum invariant. PS-4 therefore rejects an enum, or a
struct recursively containing one, in a `DatapackExport` parameter or result. A root
export may accept/return existing ABI types and use enums internally.

Ordinary package-public functions may carry enum values. The existing Phase-1
module system still cannot spell another module's nominal type in a local type
annotation. Inferred argument/result contexts can remain well typed, but qualified
type paths and explicit type visibility are a separate module-system repair and are
not hidden inside PS-4.

### Switch expressions and statements

PS-4 supports the two useful structured forms:

```mdl
fn weight(state: State) -> Int32 {
    return switch (state) {
        .idle => 0,
        .running => 10,
        .complete => 20,
    };
}

fn classify(value: Int32) -> State {
    switch (value) {
        -2147483648...-1 => { return .idle; },
        0, 2...20 => { return .running; },
        else => { return .complete; },
    }
}
```

The exact grammar uses:

- `switch (scrutinee) { prongs }`;
- `=>` between patterns and a body;
- commas between prongs, including after block bodies;
- `a...b` for a closed range inclusive at both ends;
- comma-separated exact/range patterns sharing one body;
- `.variant` or `Type.variant` enum patterns; and
- `else` as the sole catch-all spelling.

A switch expression prong initially contains one expression. A switch statement
prong contains one ordinary block. Labeled blocks, prong captures, guards, pattern
bindings, nested destructuring, fallthrough, and labeled switch continuation are
deferred.

The source `...` spelling is intentionally distinct from Minecraft's `..` spelling:

```text
MDL 0...20  ->  Minecraft matches 0..20
```

This follows Zig's inclusive switch-range convention and avoids implying the
half-open meaning that `..` has in many source languages.

## Semantic contract

1. Evaluate the scrutinee exactly once.
2. Patterns are compile-time constants and have no effects.
3. Exactly one prong is selected for every accepted scrutinee value.
4. Evaluate/execute only the selected prong body.
5. Prongs never fall through.
6. All expression prongs have one exact common result type; no implicit numeric or
   enum coercion is introduced.
7. A statement switch merges definite assignment and continuation state across only
   the arms that continue, using the same rule as `if`.
8. `return`, `break`, and `continue` inside statement-prong blocks retain their
   ordinary enclosing function/loop meanings.

PS-4 rejects any overlap, including partial overlap between integer ranges. This is
stricter than languages where an overlapping but partially useful later arm is
legal. The restriction makes source order semantically irrelevant, keeps accidental
shadowing out of Phase 1, and preserves freedom for later target reordering.

Coverage is checked as follows:

- an enum switch without `else` must mention every variant exactly once across its
  prongs;
- an integer switch without `else` must cover the entire signed `Int32` domain with
  nonoverlapping exact values/ranges;
- otherwise an `else` prong is required;
- `else` appears at most once and must be last; and
- an `else` after already complete coverage is unreachable and rejected.

Range endpoints are signed decimal `Int32` literals in PS-4. The lower endpoint may
equal the upper endpoint and canonicalizes to an exact pattern. A lower endpoint
greater than the upper endpoint is an error. Open-ended ranges, range values, runtime
bounds, chars, strings, and floating-point patterns are deferred.

## Coverage and usefulness implementation

PS-4 does not need a general Maranget pattern matrix. Its admitted patterns have one
scalar column and no guards, payloads, or nesting.

Use two small, explicit domains:

- enum coverage is a dense bitset indexed by source variant ID; and
- integer coverage is a sorted set of disjoint inclusive `i32` intervals.

Checking each integer pattern performs predecessor/successor overlap queries and an
ordered insertion. Final exhaustiveness checks adjacency from `i32::MIN` through
`i32::MAX` without enumerating values. This is deterministic `O(p log p)` time and
`O(p)` memory for `p` patterns, avoiding both quadratic pairwise checks and expansion
of a large range.

Diagnostics retain the source span of the earlier covering pattern. They distinguish:

- duplicate enum declaration/type name;
- empty enum;
- duplicate or unknown variant;
- context-free inferred enum literal;
- wrong nominal enum type;
- unsupported enum operation or exported ABI occurrence;
- invalid switch scrutinee;
- wrong pattern kind/type;
- out-of-range or backwards integer endpoint;
- overlapping pattern, with the earlier pattern as support;
- duplicate/misplaced/unreachable `else`;
- non-exhaustive enum switch listing missing variants;
- non-exhaustive integer switch requesting `else`; and
- inconsistent expression-arm result types.

Compiler resource bounds continue to derive from the existing package token,
syntax-depth, diagnostic, and Core join-edge budgets. Do not add a magic variant or
prong cap unless a scale test demonstrates a distinct allocation risk those budgets
do not cover.

## Representation and IR ownership

### AST and checked HIR

AST retains declaration order, exact pattern/body spans, recovery nodes, and whether
the construct occurs as an expression or statement.

Checked HIR owns:

- dense `SourceEnumId` and per-enum dense `SourceVariantId` identities;
- nominal `ValueType::Enum(SourceEnumId)`;
- verified enum declarations and variant inventories;
- typed enum literals;
- normalized exact/range/else patterns with origins; and
- separate typed switch-expression and switch-statement forms.

HIR verification independently recomputes declaration identity, pattern type,
coverage, overlap, arm result types, flow validity, and export-ABI restrictions.
The checker is not the only trusted owner of those invariants.

### HIR to Core

Fieldless enum values erase to `CoreType::I32` at the same aggregate-flattening
boundary where source structs already become Core leaf bundles. Variant construction
becomes `CoreOp::I32Constant(tag)`, and enum equality becomes `CoreOp::I32Compare`.
No enum tag is exposed as an MDL integer.

Add one target-independent pure Core operation:

```text
core.i32.in_closed_range(value, min, max) -> bool
```

`min` and `max` are static operation attributes with the invariant `min <= max`.
The operation is total, speculatable, structurally equivalent, constant-foldable,
and trivially discardable. It is not a Minecraft `ScoreRange` embedded in Core.

Switch lowering constructs an ordinary CFG:

```text
evaluate scrutinee once
  -> test pattern 0 --true--> arm A
                    --false-> test pattern 1
  -> ...
  -> else/remaining exhaustive arm
all continuing statement arms -> sparse SSA environment join
all expression arms            -> typed block-parameter result join
```

Multiple patterns in one prong target the same body block; the body is not cloned.
An exhaustive enum's final remaining prong may be the structural fallback after all
other tags are tested. That fallback is valid because no source or admitted export
can manufacture an invalid tag.

Do not add a Core multiway-switch terminator in PS-4. LLVM and MLIR retain switch
operations because their targets have multiple profitable lowering shapes, but the
current MDL Core and optimizer are already built around binary SSA branches. The
new range predicate preserves the target-relevant fact while avoiding an invasive
terminator addition before MDL has measured dispatch policies.

### Core evaluation and optimization

The Core evaluator evaluates the inclusive signed predicate directly and charges
the ordinary operation/value limits. Every exhaustive Core operation match, printer,
verifier, editor/query, effect classifier, SCCP transfer, canonicalizer, CSE key,
DCE rule, and lowering audit must classify the new operation explicitly.

Required baseline folds are limited to existing generic behavior plus:

- a constant operand folds to a Boolean constant; and
- an exact closed range may remain a range operation—rewriting it to equality is not
  required.

There is no interval analysis, switch folding beyond ordinary SCCP, pattern decision
DAG, or range-based specialization in PS-4.

## Mechanical Minecraft lowering

The range predicate has one syntax-directed recipe:

```mcfunction
scoreboard players set <result> <objective> 0
execute if score <value> <objective> matches <min>..<max> run \
  scoreboard players set <result> <objective> 1
```

Equal bounds use the canonical exact `ScoreRange` spelling. The result remains a
normalized Boolean and then flows through the existing safe branch dispatcher.
This may execute more commands than a fused compare-and-branch recipe. That is
accepted baseline behavior, not a correctness gap.

Generated datapack size is not a PS-4 constraint. The lowering may produce many
blocks, functions, and command lines. It must still record deterministic function,
line, byte, local-command-cost, and target-analysis metrics so later benchmarks have
evidence.

## Deferred dispatch optimization

After representative programs exist, benchmark and select among:

- source/static-order linear early-return tests;
- merged adjacent patterns with one destination;
- balanced range trees;
- constant/range-analysis specialization;
- shared decision DAGs; and
- validated Minecraft macro dispatch for dense/repeated domains.

The choice belongs to Minecraft lowering and optimization profiles. It does not
change enum or switch semantics. A future tuning option such as
`max_linear_switch_cases` may influence selection, but PS-4 adds no threshold and no
pack-size rejection.

## Test architecture

### Frontend tests

- lexer coverage for `enum`, `switch`, `=>`, `.variant`, and `...`, including
  decimal/coordinate/member punctuation ambiguity;
- parser AST goldens for declarations, expression/statement switches, multiple
  patterns, negative bounds, nesting, and bounded recovery;
- declaration/name-resolution tests for nominal identity and type-namespace
  collisions;
- expected-type tests for inferred literals in declarations, assignments, calls,
  returns, comparisons, struct fields, and switch patterns;
- negative diagnostics for every frozen error class;
- flow tests for all/some/no continuing arms, return, loop control, mutable joins,
  and unreachable source; and
- HIR corruption tests that independently exercise every verifier invariant.

### Core and optimizer tests

- builder/verifier/printer round trips for the inclusive range operation;
- `i32::{MIN,MAX}`, negative, zero, equal, and crossing-zero bounds;
- evaluator truth tables and resource-limit behavior;
- SCCP constant folding, CSE, DCE, block fusion, and no-optimization equivalence;
- enum/struct flattening through parameters, results, joins, and recursion; and
- scale tests with many enum variants and many sparse ranges, asserting bounded
  complexity rather than an optimized target shape.

### Source and target integration

Add a small ordinary MDL fixture, not a compiler intrinsic. It should:

- classify `Int32` inputs at negative, exact, closed-range, and default boundaries;
- produce and consume a closed enum internally;
- use both switch expressions and switch statements;
- store an enum in a struct and carry it through a private call;
- expose only `Int32`/`Bool` at the datapack boundary; and
- return a compact checksum suitable for evaluator/server comparison.

Compile it under all four Core/Minecraft policy combinations. The fast structural
test requires at least one rendered `execute if score ... matches 0..20` line but
does not pin a globally optimal function layout.

One ignored pinned Java 26.2 server test should run boundary inputs in a single
server lifecycle and compare public scoreboard results across all four policy
products. The server remains the authority for target parsing/runtime; the Core
evaluator remains the authority for target-independent enum/switch semantics.

### Regression and quality gates

- existing PS-1 through PS-3 suites remain unchanged and passing;
- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace --all-targets` at tranche gates and completion;
- targeted tests during local iteration; and
- exact official-server command only at the target tranche and final audit.

## Non-goals

- payload/tagged-union variants or destructuring;
- generic algebraic data types;
- `List<Enum>` or generic list representation work;
- enum/int casts, explicit tag values, or a stable external enum ABI;
- qualified imported nominal type paths or a general visibility redesign;
- string, float, coordinate, selector, or runtime-bound patterns;
- guards, captures, nested patterns, fallthrough, or labeled switch continuation;
- open-ended source range syntax;
- a general Maranget pattern matrix before the pattern language needs one;
- a Core multiway terminator;
- dispatch trees, macros, PGO, e-graphs, or pack-size optimization;
- scheduler/yield integration; and
- rewriting the PS-3 Brainfuck opcode list as `List<Opcode>`.

`List<Enum>` is the most likely immediate follow-up if a real client justifies
generalizing the current intentionally monomorphic `List<Int32>` API. It must not be
implemented by making enum and integer types implicitly interchangeable.

## Exit criteria

- closed fieldless enums are nominal and usable through every admitted scalar/
  aggregate/internal-call position;
- invalid enum operations and external ABI escape are rejected precisely;
- Zig-style expression and statement switches implement the frozen single-
  evaluation, no-fallthrough semantics;
- duplicate, overlap, result-type, and exhaustiveness checks are bounded and have
  source-supported diagnostics;
- HIR verification independently owns the semantic invariants;
- enum tags erase before Core without becoming source-visible integers;
- inclusive range tests survive to the target-neutral Core operation and render as
  typed Minecraft `matches` conditions;
- the simple lowering is equivalent under all four policy combinations;
- the pinned server executes boundary cases correctly;
- large generated packs are accepted while their footprint/cost is reported;
- dispatch optimization remains an explicit measured TODO; and
- the handoff records all remaining enum/list/module/ABI breadth without silently
  expanding Stage 9.

## Research basis

- [Zig language reference: enums, inferred enum literals, exhaustive switch,
  multiple cases, and inclusive `...` switch ranges](https://ziglang.org/documentation/master/)
- [Rust Reference: nominal enum constructors and discriminants](https://doc.rust-lang.org/reference/items/enumerations.html)
- [Rust Reference: range-pattern and exhaustiveness rules](https://doc.rust-lang.org/stable/reference/patterns.html)
- [rustc pattern analysis: usefulness, redundancy, witnesses, and constructor
  splitting](https://doc.rust-lang.org/nightly/nightly-rustc/rustc_pattern_analysis/usefulness/index.html)
- [Luc Maranget, *Warnings for Pattern Matching*](https://www.cambridge.org/core/journals/journal-of-functional-programming/article/warnings-for-pattern-matching/3165B75113781E2431E3856972940347)
- [GHC User's Guide: incomplete and overlapping pattern diagnostics](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/using-warnings.html)
- [LLVM language reference: target-dependent switch lowering](https://llvm.org/docs/LangRef.html#switch-instruction)
- [MLIR control-flow dialect: `cf.switch` and typed successor operands](https://mlir.llvm.org/docs/Dialects/ControlFlowDialect/#cfswitch-cfswitchop)
- [Mojang Java 1.20.3 notes: `return run` and function-return behavior](https://feedback.minecraft.net/hc/en-us/articles/21968446892173-Minecraft-Java-Edition-1-20-3)
- [MDL target research: branches and control flow](../../mcfunction/branches-and-control-flow.md)
- [MDL target range representation](../stage-3-minecraft-ir-plan.md#score-ranges-and-finite-numbers)
