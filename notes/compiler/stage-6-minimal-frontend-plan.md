# Stage 6: Minimal Typed Frontend Plan

Status: **Complete — implemented, independently reviewed, and gated on Java 26.2**

Execution checklist: [`stage-6-todo.md`](stage-6-todo.md)

Stage 6 adds the first human-written source path without weakening any completed
backend boundary:

```text
immutable source text
    -> bounded lexer and recoverable syntax AST
    -> resolved, fully typed HIR
    -> verified Core SSA
    -> owned Core optimization output
    -> owned Minecraft lowering output
    -> separate target-cost result
    -> owned datapack emission output
    -> official vanilla execution
```

The first implementation is intentionally a single compilation unit with scalar
types, functions, locals, calls, structured conditionals, assignment, and return.
It is a real end-to-end compiler slice, not a grammar showcase. Stage 6I has now
resolved the later module and effect contracts: whole-package modules and exports are
the Stage 7A prerequisite, while typed Minecraft operations and literal unsafe
commands begin in Stage 7B; scheduling consumption and runtime interpolation remain
Stages 9 and 10. Neither extension may bypass Core or silently create a package
system.

## Outcome

The first complete tranche accepts one owned UTF-8 source file and produces:

- deterministic lexer/parser diagnostics with exact source ranges;
- an immutable resolved typed HIR with no recovery nodes;
- verified Core plus a source-function-to-`FunctionId` map;
- the existing concrete optimization, lowering, target-analysis, and emission
  products without flattening their ownership;
- generated entry resources and typed ABI homes discoverable through the retained
  source/Core map and existing `LoweringMap`;
- byte-identical output for repeated identical inputs; and
- one vanilla Java 26.2 proof covering calls, branches, mutable locals, SSA joins,
  returns, and both `None` and `Baseline` reference policies.

No successful frontend result contains an unresolved name, an unknown type, a
parser recovery placeholder, or an invalid Core body.

## Scope boundary

### First complete scalar language

- one source file and one global function namespace;
- every declared function is externally callable in the single-unit reference
  tranche, matching the current Core and lowering contract; Stage 7A replaces this
  temporary rule with explicit `export fn` while retaining the reference adapter;
- `fn`, positional parameters, and explicit result annotations;
- source value types `Bool` and `Int32`;
- omitted or explicit `Void` function results;
- `const` and scalar `var` locals, including uninitialized `var`;
- simple local assignment;
- Boolean and decimal non-negative `Int32` literals;
- local/parameter references and direct function calls;
- `!` and signed `Int32` comparisons (`==`, `!=`, `<`, `<=`, `>`, `>=`);
- statement-form `if` / `else if` / `else` with exact `Bool` conditions;
- explicit `return`, with fallthrough allowed only for `Void`; and
- semicolon-terminated simple statements and optional trailing commas.

Arguments are evaluated left to right. Function signatures are collected before
bodies, so forward calls and direct recursion resolve normally. A call's argument
and result types must match exactly; Stage 6 inserts no implicit conversions.
Recursion is frontend-valid, but the current Minecraft lowering deliberately rejects
recursive Core call graphs. Such a program reaches and returns the existing typed
`LoweringFailure`; Stage 6 does not hide that target limitation in name resolution.

A call returning a value may be used as a statement and explicitly discards that
value after its effects occur. A `Void` call is valid only as a statement; it cannot
be used where an expression value is required. Parameters are immutable local
bindings in this tranche. Caller-visible parameter mutation remains deferred.

### Explicitly deferred from the first tranche

- inferred `:=` declarations;
- ordinary integer arithmetic and its overflow contract;
- negative literals and general unary negation;
- `Never` and termination-producing operations;
- caller-visible `mut` parameters;
- shadowing, overloads, methods, named/default arguments, and function values;
- structs, lists, strings, `switch`, loops, `break`, `continue`, and deletion;
- typed Minecraft/entity/context APIs;
- language or Minecraft function macros;
- scheduling and multi-tick work partitioning;
- a stable package, module, filesystem, or external ABI; and
- aggressive global optimization.

Deferral is not rejection. The syntax work ledger is broader than this stage's
representable Core vocabulary; parser support follows semantic representation, not
the other way around.

## Why this frontend shape

Mature compilers consistently separate source spelling from resolved typed meaning,
but they use different amounts of machinery:

| Compiler | Useful pattern | Stage 6 consequence |
| --- | --- | --- |
| rustc | Tokens become a syntax-shaped AST, then compiler-friendly HIR/THIR with resolved identities and types. Body checking and CFG-oriented MIR are separate boundaries. | Keep AST and typed HIR distinct, collect signatures before bodies, and use body-local typed IDs. Do not add rustc's query engine. |
| Zig | `Ast` borrows externally owned source and owns compact offset-based tokens/nodes; semantic analysis produces typed per-function AIR from a whole-file untyped form. | Keep source ownership outside tokens and use `u32` offsets. Do not add a ZIR analogue without Zig-like compile-time execution or generic-instantiation needs. |
| OCaml | Location-rich `Parsetree` is separate from `Typedtree`, where spellings have resolved paths and concrete types. | A syntax AST followed directly by resolved typed HIR is the closest proportional model. |
| GHC | Parsed, renamed, and typechecked forms have explicit phase invariants. | Preserve phase distinctions, but use ordinary separate Rust types instead of pass-indexed extensible trees. |
| MLIR | Source-located parsing, attached diagnostic notes, deterministic diagnostic ordering, and explicit legality at conversion boundaries are first-class. | Keep provenance and verification explicit. A dialect/conversion framework would duplicate the existing closed Core builder. |
| Cranelift | Block parameters represent SSA joins; its frontend can construct SSA for arbitrary mutable variables. | Lower this stage's loop-free structured branches directly with deterministic join parameters. General sealed-block SSA construction belongs with loops. |

The proportional internal pipeline is therefore:

```text
SourceContext owns text and provenance
    |
    +-- TokenBuffer { TokenKind, Span }
    |       (temporary; no borrowed lexemes or copied trivia)
    v
AstModule
    |       syntax-shaped; may contain recovery nodes
    v
ProgramIndex
    |       stable source order; all function signatures collected first
    v
HirProgram
    |       resolved IDs; exact types; no error nodes
    v
CoreProgram
```

There is no untyped HIR between the AST and typed HIR, no generic frontend pass
manager, no public AST mutation API, no type interner for three closed types, and no
incremental query system. Those are future responses to measured consumers.

## Lexical and grammar contract for the first tranche

The initial lexer recognizes:

- ASCII identifiers `[A-Za-z_][A-Za-z0-9_]*`;
- reserved words `fn`, `const`, `var`, `if`, `else`, `return`, `true`, `false`,
  `Bool`, `Int32`, and `Void`;
- decimal integer tokens containing only `0` through `9`;
- `(`, `)`, `{`, `}`, `:`, `;`, `,`, `=`, `->`, `!`, `==`, `!=`, `<`, `<=`, `>`,
  and `>=`;
- ASCII spaces, tabs, carriage returns, and newlines as trivia; and
- `//` line comments ending before the newline or at EOF.

The source container is UTF-8, but non-ASCII identifier characters and other unknown
characters receive lexical diagnostics in this tranche. One invalid non-ASCII
character consumes one complete UTF-8 scalar, never one byte. Block comments, numeric
separators, bases, suffixes, floats, strings, and characters are deferred.

The grammar is intentionally closed:

```text
module       := function* EOF
function     := "fn" IDENT "(" parameters? ")" result? block
parameters   := parameter ("," parameter)* ","?
parameter    := IDENT ":" value_type
result       := "->" (value_type | "Void")
value_type   := "Bool" | "Int32"

block        := "{" statement* "}"
statement    := declaration
              | assignment ";"
              | call ";"
              | if_statement
              | return_statement
declaration  := "const" IDENT ":" value_type "=" expression ";"
              | "var" IDENT ":" value_type ("=" expression)? ";"
assignment   := IDENT "=" expression
if_statement := "if" "(" expression ")" block
                ("else" "if" "(" expression ")" block)*
                ("else" block)?
return_statement := "return" expression? ";"

expression   := comparison
comparison   := prefix (comparison_op prefix)?
comparison_op := "==" | "!=" | "<" | "<=" | ">" | ">="
prefix       := "!" prefix | primary
primary      := BOOL | DECIMAL_INT | IDENT call_suffix? | "(" expression ")"
call_suffix  := "(" arguments? ")"
arguments    := expression ("," expression)* ","?
```

Comparison operators do not chain. `==` and `!=` accept two values of the same
source value type; ordered comparisons accept `Int32`. Decimal integer conversion is
checked against `0..=2_147_483_647` during semantic analysis. Ordinary `+` is
deliberately absent until the language selects wrapping, checked, saturating, or
another observable overflow contract.

Names use exact byte spelling and are case-sensitive. Functions cannot share a name.
Statement parsing uses one-token lookahead after an identifier to distinguish `=`
assignment, `(` call, and a bare-name error.

Each function body and conditional arm is a lexical scope. A branch-local binding
leaves scope at its closing brace and cannot participate in the outer SSA join.
Declarations become visible only after their initializer has been checked. A name
cannot duplicate a binding in the same or any active enclosing scope, including a
parameter; the same spelling is allowed in already-ended disjoint sibling scopes.
This defers active shadowing without unnecessarily coupling disjoint branches.

## Bounded lexer and recoverable parser

The lexer returns a token buffer plus diagnostics. Tokens contain a closed kind and
validated `Span`; identifier and literal text is sliced from `SourceContext` only
when needed. EOF has an empty span at the file end. No origin is allocated for trivia
or punctuation.

The parser is hand-written layered recursive descent for comparison and prefix
expressions. It returns a partial AST plus diagnostics so independent syntax
errors can be reported in one run. Recovery synchronizes at:

- `fn` or EOF for top-level items;
- `;`, `}`, or a statement-leading keyword inside blocks; and
- `,` or the closing delimiter inside lists.

Every failed recovery consumes a token unless it is already at a valid follow token.
Required malformed children use explicit internal error nodes; `Option` represents
only grammatically optional syntax. `FrontendLimits`, owned by `CompilationOptions`,
defaults to 1,000,000 tokens including EOF, 256 nested syntax constructs, and 100
ordinary frontend diagnostics. While the Phase 1 parser uses bounded host recursion,
256 is also a reviewed hard maximum rather than merely a configurable default.
Reaching a token/depth limit or attempting an
additional diagnostic emits exactly one final truncation/limit finding and prevents
HIR construction; the final finding is in addition to the 100 ordinary findings.
AST node count is bounded by token count, and successful HIR entity count cannot
exceed its clean AST/token inventory, so the implementation does not maintain
lookalike node limits.

Core generation reuses `FrontendLimits::max_tokens` as a conservative
whole-compilation cap on value operands carried by predecessor edges into
source-level SSA joins. All other Stage 6 Core constructs expand by a constant factor
from the token-bounded HIR, but a many-predecessor join with many differing locals can
otherwise grow quadratically. The lowerer computes the complete sparse join shape,
checks multiplication and the remaining budget before allocating any join parameters
or edge vectors, and returns a typed `CoreGenerationFailure::ResourceLimit` when the
cap would be exceeded. This is a host-resource guard that may reject valid source;
later join topology or IR representation work can replace it without changing source
semantics.

Recovery nodes are retained only for malformed statements/expressions where doing so
helps synchronization inside a body. A malformed top-level function, parameter,
required type, or closing delimiter may instead cause that partial construct to be
discarded at its recovery boundary. Dirty AST is never checked, so a universal
optional/error wrapper around every required token would have no semantic consumer.

The first frontend implementation type-checks only when lexing and parsing are clean.
The partial AST exists for recovery and diagnostic testing, not as permission to
publish a partially typed program.

## Diagnostics and provenance

The shared diagnostic model grows compatibly from one origin to:

```text
Diagnostic {
  code
  message
  primary: DiagnosticLabel { origin, optional message }
  supporting: [DiagnosticLabel]
  notes: [text]
}
```

`Diagnostic::new` continues to create the primary location, and `origin()` continues
to return it. Existing verifier/lowering/emission diagnostics therefore retain their
behavior. Supporting locations and notes belong to their parent finding and never
become independently reordered errors.

A pure renderer borrows `SourceContext`, resolves an origin to a best direct source
span, and returns text without performing I/O. Direct source origins display that
span; call-site origins prefer the caller; fused origins choose the first resolvable
input deterministically. Composite traversal is iterative and bounded by the
context's origin inventory. Canonical storage remains UTF-8 byte offsets. Rendered
locations use one-based line and one-based UTF-8 byte column, matching
`SourcePosition`; no terminal-width behavior is implied for tabs or wide Unicode
scalars.

Required source-level label policy includes:

- duplicate declaration: duplicate is primary; original is supporting;
- unknown name: reference is primary;
- type mismatch: offending expression is primary; expected declaration is
  supporting when useful;
- uninitialized read: read is primary; declaration is supporting; and
- missing return: closing brace is primary; result annotation is supporting.

Diagnostic order is source input order, phase order, function ID order, then source
traversal. Hash-map iteration never selects IDs, errors, dumps, or output. If checking
becomes parallel later, findings are buffered per preassigned function and merged in
that order.

## Typed HIR contract

Frontend types are not aliases for backend types:

```text
ValueType      = Bool | Int32
FunctionResult = Void | Value(ValueType)
```

The checker may use a private error sentinel to suppress cascades, but it cannot
appear in successful HIR. `Void` is a result contract and is never an SSA value.

HIR uses dense, typed, owner-local identities (the syntax AST itself uses ordinary
syntax structs/enums, not semantic IDs):

- `SourceFunctionId` identifies a function in deterministic declaration order;
- `LocalId` identifies a parameter/local within one function;
- every name use contains its resolved function or local ID;
- every expression contains its exact `ValueType` and `OriginId`; and
- every declaration, statement, and control construct retains an origin.

All signatures are collected before any body is checked. This permits forward calls
and recursion without placeholder types. Body checking resolves names, checks exact
types, and constructs HIR in one operation; it does not mutate the syntax AST with
optional semantic fields.

The HIR has a deterministic dump and an internal verifier. Verification checks ID
ownership/ranges, expression result types, call signatures, assignment mutability,
and that all successful nodes have valid provenance. HIR is compiler-owned and
read-only outside its module in Phase 1.

## Definite assignment and return completeness

Flow checking uses structured state rather than guessing during Core construction.
A dense owner-local assignment table provides constant-time reads, while nested
branches use checkpoint/rollback journals and intersect only sparse false-to-true
deltas; they never clone or scan the whole live-local inventory per arm:

```text
Flow = Continues(assigned local bitset) | Terminates
```

- parameters and initialized declarations begin assigned;
- an uninitialized `var` begins unassigned;
- reading a local requires it to be assigned on every continuing path;
- assignment marks the local assigned after its right-hand side is checked;
- an `if` continuation intersects the assigned sets of branches that continue;
- a missing `else` contributes the incoming state;
- a returning branch contributes no state to the join; and
- a value-returning function must terminate on every reachable path.

`Void` functions may fall through; Core lowering emits an empty return at the closing
brace origin. Unreachable source is legal. The checker still traverses it to report
independent name/type errors, but it cannot change continuation flow and Core lowering
does not attach unreachable statements after an already emitted terminator.

## HIR-to-Core SSA lowering

All Core functions are declared first in source order with the same signatures.
Bodies are then built through the existing `FunctionBuilder` and installed only after
verification.

The loop-free structured mutation model does not require a general sealed-block SSA
builder. Lowering maintains `LocalId -> ValueId` for the current path. For an `if`:

1. checkpoint one mutable incoming environment;
2. lower each arm into its own block;
3. ignore arms that terminated;
4. pop branch-local bindings, retain only each continuing predecessor's sorted net
   overrides, and roll back to the checkpoint;
5. create a continuation block when any path continues;
6. aggregate only overridden locals in `LocalId` order and mark one required block
   parameter for each definitely assigned value that differs between predecessors;
7. determine the complete parameter inventory and predecessor-edge operand count;
8. charge that operand count to the checked shared budget before allocating either
   parameters or edge vectors;
9. append every parameter, then build complete edge-argument lists before installing
   any predecessor terminator;
10. order parameters and edge arguments by `LocalId`; and
11. continue with the merged environment.

An absent `else` is an explicit continuing edge carrying the incoming environment.
Generated control blocks/terminators use the `if` origin; value instructions use the
expression that produced them. Every constructed Core function and the whole program
are verified independent of optimization level.

The source contract permits `Bool == Bool` and `Bool != Bool`, while the completed
Core vocabulary has neither Boolean comparison nor xor/select. Stage 6 therefore
evaluates both operands left to right and lowers the result through a small typed CFG
diamond with a Boolean join parameter. This is semantically exact and keeps call
effects in order, but costs more target control flow than a future reviewed
`core.bool.compare`/xor primitive. That vocabulary/optimization follow-up must be
measured and added explicitly; it is not silently represented as one existing Core
operation.

The frontend retains `SourceFunctionId -> Core FunctionId`. Dense IDs are ephemeral
and are never serialized as a stable ABI; the source spelling and, later, logical
module path are the future stable-key candidates.

## Compilation façade and ownership

The public library entry accepts owned in-memory source, leaving file discovery and
filesystem writes outside semantic compilation:

```text
compile_source(SourceInput, &CompilationOptions)
    -> Result<CompilationOutput, CompilationFailure>
```

`CompilationOptions` composes `FrontendLimits` and the existing concrete backend
option types. It does not invent a parallel set of optimization or target limits.

On success, `CompilationOutput` owns these products beside one another:

```text
SourceContext
CheckedFrontendOutput          // typed HIR
SourceToCoreMap                 // correlation retained after Core is consumed
CoreOptimizationOutput
LoweringOutput
Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure>
EmissionOutput
```

Target-cost failure is nonfatal: it remains an owned `Result` and never discards a
valid lowered program or emitted datapack. Optimization, lowering, and emission
failures preserve their concrete producer error types rather than becoming strings
or generic frontend diagnostics. Frontend failures retain `SourceContext`, because
their diagnostics contain `OriginId`s.

The façade may offer convenience lookups from a source function to its lowered entry
resource and homes, but ownership remains with the checked frontend, correlation
map, and `LoweringOutput`.
It runs each producer once and does not cache or recompute reports behind accessors.

Failure ownership is likewise explicit:

```text
SourceInput(SourceError)
Syntax { sources, diagnostics }
Semantic { sources, diagnostics }
FrontendInfrastructure { sources, failure }
CoreGeneration { sources, checked_frontend, failure }
CoreOptimization { sources, checked_frontend, source_to_core, failure }
MinecraftLowering { sources, checked_frontend, source_to_core,
                    core_optimization, failure }
DatapackEmission { sources, checked_frontend, source_to_core,
                   core_optimization, lowering, diagnostics }
```

`FrontendInfrastructureFailure` is a public, stage-aware owned translation of
failures that cannot be expressed as ordinary source diagnostics: lexer/parser/checker
source-table or provenance failures, semantic identity-space exhaustion, and an
invalid-HIR invariant failure. It does not expose the private lexer, parser, or checker
implementation types, but it preserves their concrete phase and structured
`SourceError`/`OriginError` payloads. These failures are distinct from `SourceInput`,
which means the owned input could not be installed in the source table at all.

Exact Rust layout may use boxes to keep the enum compact, but successful earlier
products remain available and downstream failures retain the original concrete
producer error. Target-cost analysis is absent from failure variants because it is
nonfatal and remains a value in successful `CompilationOutput`.

`lower_hir` temporarily returns
`CoreGenerationOutput { program, source_to_core }`. The façade separates those parts,
passes the owned `program` into consuming `optimize_core`, and retains only the map
beside `CoreOptimizationOutput`. It does not clone or keep an unoptimized Core merely
to preserve the temporary wrapper.

## Modules, imports, and Minecraft effects

Stage 6I selected two separate contracts:

- [`stage-6-modules-plan.md`](stage-6-modules-plan.md) owns the Stage 7A
  whole-package implementation: explicit logical `ModulePath` values independent of
  diagnostic filenames, module-only imports and aliases, private/package/exported
  functions, valid function-only import cycles, canonical module-order IDs, complete
  in-memory compiler input, and CLI-owned filesystem discovery. `mdl-compiler` never
  opens a path named by an import.
- [`stage-6-effects-plan.md`](stage-6-effects-plan.md) defines a closed typed external
  Core operation and immutable, target-independently shape-checked fragment boundary.
  Compiler-owned Minecraft APIs lower through structured target IR. Literal unsafe
  commands remain fixed `Unknown`/non-speculatable/opaque barriers; runtime typed
  interpolation belongs to Stage 10's separately verified Minecraft function-macro
  ABI.

Modules are implemented after the single-unit reference path as Stage 7A, before
Minecraft APIs widen name and method resolution. Effects implementation begins in
Stage 7B with the external-operation spine, typed APIs, and literal-only unsafe
statement; Stage 9 consumes its scheduling contracts; Stage 10 adds typed runtime
templates. This avoids inventing command interpolation before the language has
entity, context, text, outcome, and serializer types.

## Test and review gates

Each tranche closes with formatting, warnings-denied Clippy, workspace tests,
warnings-denied rustdoc, and the pinned MSRV check. Feature work additionally proves:

- exact token/span tables including UTF-8 failures and EOF;
- parser recovery termination, nesting bounds, and diagnostic caps;
- deterministic AST/HIR/Core dumps;
- duplicate, unknown-name, type, mutability, uninitialized-read, and missing-return
  diagnostics with primary/supporting ranges;
- HIR verifier corruption rejection;
- source-to-Core results under `Core None` before testing Baseline;
- repeated compilation byte equality across dumps, pack, trace, maps, and reports;
- all four Core/Minecraft `None|Baseline` combinations;
- source-origin continuity through an emitted physical command line; and
- one official Java 26.2 startup with parameters, calls, branches, mutable locals,
  joins, both scalar types, `Void`, reload, and reinvocation.

Malformed source never reaches Core. Every valid, target-supported source program
must compile under both reference and optimized modes, and no source construct may
depend on a cleanup pass for legality. Semantic equivalence across policies is proven
by executing all four Core/Minecraft policy combinations on the official server;
artifact comparison alone is only a determinism/structure oracle.

## Implementation order

1. **6A — Contract and shared diagnostics.** Freeze this first grammar, extend
   labels/rendering compatibly, and add exact source helpers.
2. **6B — Lexer and recoverable AST parser.** Build the bounded single-file syntax
   boundary with deterministic dumps/tests.
3. **6C — Signature index and typed HIR.** Resolve all functions/locals, type-check
   bodies, verify HIR, and expose deterministic dumps.
4. **6D — Flow checking and structured mutation.** Implement definite assignment,
   return completeness, and negative diagnostics.
5. **6E — HIR-to-Core lowering.** Predeclare functions, lower branches/joins/calls,
   preserve origins, and verify independently under `None`.
6. **6F — Owned compilation façade.** Compose the existing APIs once without
   collapsing outputs or failures.
7. **6G — Differential and vanilla proof.** Close deterministic, four-policy, trace,
   and Java execution gates.
8. **6H — Minimal CLI process boundary.** Read one file, render diagnostics, write the
   in-memory artifact, and print source-entry mappings without moving filesystem
   policy into the compiler library.
9. **6I — Multi-file and Minecraft-effect design.** Research and write separate
   reviewed plans for the missing contracts and explicitly reconcile roadmap
   ownership.

The first usable source compiler exists after 6G/6H. Stage 6 closes when those gates
and the two reviewed 6I contracts are complete. Whole-package modules and Minecraft
effects no longer block Stage 6 because their exact Stage 7A/7B/9/10 ownership is now
a reviewed roadmap decision.

## Primary references

- [rustc compiler overview and IR layers](https://rustc-dev-guide.rust-lang.org/overview.html)
- [rustc parser architecture](https://rustc-dev-guide.rust-lang.org/the-parser.html)
- [rustc HIR](https://rustc-dev-guide.rust-lang.org/hir.html)
- [rustc AST-to-HIR lowering](https://rustc-dev-guide.rust-lang.org/hir/lowering.html)
- [rustc name resolution](https://rustc-dev-guide.rust-lang.org/name-resolution.html)
- [rustc HIR type checking](https://rustc-dev-guide.rust-lang.org/hir-typeck/summary.html)
- [rustc diagnostics](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- [Zig `Ast`](https://github.com/ziglang/zig/blob/master/lib/std/zig/Ast.zig)
- [Zig `Zir`](https://github.com/ziglang/zig/blob/master/lib/std/zig/Zir.zig)
- [Zig `Air`](https://github.com/ziglang/zig/blob/master/src/Air.zig)
- [OCaml `Parsetree`](https://ocaml.org/manual/5.4/api/compilerlibref/Parsetree.html)
- [OCaml `Typedtree`](https://ocaml.org/p/ocaml-compiler/latest/doc/compiler-libs.common/Typedtree/index.html)
- [GHC parsed/renamed/typechecked phase types](https://ghc.gitlab.haskell.org/ghc/doc/libraries/ghc-9.15-inplace/GHC-Hs-Extension.html)
- [GHC compiler determinism checks](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/debugging.html#checking-for-determinism)
- [MLIR diagnostics](https://mlir.llvm.org/docs/Diagnostics/)
- [MLIR dialect conversion and legality](https://mlir.llvm.org/docs/DialectConversion/)
- [LLVM recursive-descent and precedence parser tutorial](https://llvm.org/docs/tutorial/MyFirstLanguageFrontend/LangImpl02.html)
- [Cranelift frontend SSA builder](https://docs.rs/cranelift-frontend/latest/cranelift_frontend/struct.FunctionBuilder.html)
- [Stage 7A whole-package module research and implementation plan](stage-6-modules-plan.md)
- [Stage 7B typed Minecraft effect and unsafe raw-command plan](stage-6-effects-plan.md)
