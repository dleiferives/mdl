# PS-5: Anonymous Structs, Multiple Results, and Destructuring

Status: **planned; implementation not started**

## Identity

- **Capability:** structural anonymous struct types (named and positional),
  context-inferred struct literals, compile-time positional indexing, the pipe
  destructuring statement, and `:=` inferred declarations;
- **Substage:** PS-5;
- **Owner documents:** this plan and
  [`ps-5-anon-structs-destructuring-todo.md`](ps-5-anon-structs-destructuring-todo.md);
- **Required before resuming Stage 9:** yes by the accepted work order, after PS-4,
  but not because scheduling semantically depends on multiple results; and
- **Optimization posture:** reuse the existing scalarized aggregate ABI unchanged;
  PS-5 adds no new Core operation and no new Minecraft recipe.

PS-5 is an accepted post-capstone language-capability milestone. It does not alter
the achieved PS-1 through PS-3 exit and does not retroactively become a Brainfuck
requirement.

## Why PS-5 exists

The Brainfuck capstone documents the cost of having no multiple-result surface:

- `parser.bracket_validation` smuggles a status and a position through an encoded
  `List<Int32>`, and every caller decodes it with `last_or_zero()` /
  `without_last().last_or_zero()` plus a comment explaining the encoding;
- every tape zipper move spends four statements and two `old_*` temporaries because
  two bindings cannot be updated from one call;
- `io.read_or_zero` and `io.after_read` are one logical operation split into two
  functions because a function cannot return a value and the remaining input
  together; and
- nearly every local declaration repeats a type the initializer already proves.

These are language gaps, not application style. PS-5 closes them with structural
types over machinery the compiler already has: nominal structs are fully scalarized
through call/return/branch ABI lowering, so anonymous structs are a frontend/HIR
feature riding an existing physical contract.

## Frozen source surface

The implemented grammar authority is [`../../syntax/grammar.ebnf`](../../syntax/grammar.ebnf).
PS-5's exact proposed productions are isolated in
[`../../syntax/ps-5-grammar-delta.ebnf`](../../syntax/ps-5-grammar-delta.ebnf), and
the selected syntax decisions are recorded as
[S-041](../../syntax/anonymous-structs-and-destructuring.md). Implementation must
merge the delta into the authoritative grammar in the same change as the
lexer/parser and grammar-conformance tests.

### Anonymous struct types

Two forms occupy ordinary type position:

```mdl
pub fn get() -> { left: List<Int32>, right: List<Int32> } { ... }   // named
pub fn bracket_validation(program: List<Int32>) -> { Int32, Int32 } // positional
```

The first slice has these rules:

- identity is structural: an anonymous struct type is identified by its ordered
  component sequence — `(name, type)` pairs for the named form, types alone for the
  positional form. The same spelling in any module names the same type;
- field/component order is part of the type. `{ a: Int32, b: Int32 }` and
  `{ b: Int32, a: Int32 }` are different types; a diagnostic may cite the reordered
  near-miss;
- the named and positional forms never interconvert, and neither converts to or
  from a nominal struct, even with identical components;
- at least one component is required; named field names are unique;
- component types may be `Bool`, `Int32`, `String`, `List<Int32>`, nominal structs,
  PS-4 enums, and other anonymous structs; and
- admitted positions are parameter types, result types, local annotations, nominal
  struct fields, and nested anonymous components. `List` element types remain
  `Int32` only.

### Inferred struct literals

`.{ ... }` constructs a struct value whose type comes from context, extending the
`.` prefix convention PS-4 established for enum variants:

```mdl
return .{ .status = 3, .position = position };   // named context
return .{ 3, position };                          // positional context
```

- an inferred literal requires an exact expected type from an explicitly typed
  declaration or assignment, a parameter, a result, a struct field, or a return;
- a named-entry literal is admitted for named anonymous types and for nominal
  struct types (the inferred counterpart of the S-013 `Name{...}` form);
- a positional-entry literal is admitted only for positional anonymous types;
- entries must be all named or all positional; full coverage is required and the
  existing S-013/S-014/S-035 field rules apply to named entries;
- entries evaluate left to right under S-026; and
- a context-free inferred literal is rejected rather than guessed, exactly like
  PS-4's context-free `.variant`.

### Projection

Consumption follows the type's shape:

- named anonymous values use existing `.field` postfix projection, including
  directly on call results: `parser.bracket_validation(p).status`;
- positional anonymous values use `value[k]`, where `k` is a nonnegative decimal
  integer literal, zero-based, checked at compile time against the arity; and
- there is no positional access on named values and no named access on positional
  values.

Reordering a named result type's fields must never silently rebind callsites;
forbidding positional consumption of named types makes reordering either harmless
(projection) or a type error. The `[k]` bracket surface is the static sibling of
the S-032/S-033 checked list indexing selection.

### Destructuring statement

```mdl
var right: List<Int32>;
...
|const left, right| <= parser.get_pair();
|left, right| <= tape.move_right(left, right);
|const status, _| <= parser.bracket_validation(program);
```

- the statement form is `|` targets `|` `<=` expression `;`;
- each target is `const Name` (fresh immutable binding), `var Name` (fresh mutable
  binding), a bare `Name` (assignment to an existing writable binding under S-002),
  or `_` (discard);
- the operand must be a positional anonymous struct value; arity must match the
  target list exactly; duplicate target names are rejected;
- fresh targets receive the component's exact type by inference (S-003 semantics;
  no annotation is spellable in a target list);
- the operand is evaluated exactly once and completely; component write-back then
  proceeds left to right, so reading and writing the same bindings in one
  statement is well defined;
- `_` in target position is always a discard, never a binding; and
- definite assignment follows S-008: fresh and bare targets are definitely
  assigned after the statement.

`<=` is the existing comparison token reused where comparison cannot occur; no
other statement begins with `|`. The rejected alternative `<-` is recorded in
S-041. Named anonymous types and nominal structs are not destructurable in PS-5;
by-name destructuring is deferred breadth, not an oversight — inference plus
projection covers the named case.

### Inferred declarations

`:=` implements the S-003 reservation:

```mdl
const tape := parser.get();
var fuel := starting_fuel(case);
```

- the binding's static type is the initializer's exact type, fixed for its
  lifetime under S-002;
- initializers without a context-free type are rejected: inferred enum literals,
  inferred struct literals, and coordinate `StaticDecimal` forms need an
  annotated declaration;
- uninitialized `var` declarations still require an annotation; and
- explicit annotation remains the S-001 default policy; `:=` is the visibly
  marked opt-in S-003 selected.

### Module and export boundary

Structural identity is the module-boundary payoff: two modules can exchange
`{ status: Int32, position: Int32 }` without either owning a nominal declaration,
relieving the known Phase-1 limitation that a module cannot spell another module's
nominal type. The qualified-type-path module repair remains separate work and is
not hidden inside PS-5.

A datapack caller cannot uphold structural invariants, so PS-5 rejects anonymous
struct types, recursively through nominal structs, in `DatapackExport` parameters
and results — the same conservative posture PS-4 takes for enums. Root exports keep
existing ABI types and may use anonymous structs internally.

## Semantic contract

1. An anonymous struct type is identified structurally by its ordered component
   sequence; the same spelling anywhere names the same type.
2. Named, positional, and nominal struct types never implicitly interconvert.
3. Inferred literals require an exact expected type and evaluate entries left to
   right under S-026.
4. Values carry the PS-2 copy semantics of all MDL aggregates; projection,
   indexing, and destructuring read the source value and never mutate it.
5. A destructuring statement evaluates its operand exactly once and completely,
   then writes components to targets left to right.
6. `[k]` requires a compile-time literal with `0 <= k < arity`.
7. `:=` fixes the binding's static type to the initializer's exact type; later
   assignments follow S-002 unchanged.
8. No implicit conversions are introduced anywhere in this surface.

## Representation and IR ownership

### AST and checked HIR

AST retains exact spans for anonymous type components, literal entries, index
suffixes, destructure targets and their roles, and `:=` declarations, including
recovery nodes.

Checked HIR owns:

- interned structural identities for anonymous struct types (one canonical
  identity per distinct ordered component sequence, shared across modules);
- `ValueType` extended with the two anonymous forms;
- typed inferred literals resolved to their exact expected type;
- typed projection and compile-time-checked index nodes;
- a typed destructuring statement with resolved target bindings and roles; and
- inferred declaration types recorded as if written explicitly.

HIR verification independently recomputes structural identity interning, literal
coverage and entry kinds, index bounds, destructure arity/duplicate/writability
rules, definite assignment through destructure targets, and the export-ABI
rejection. The checker is not the only trusted owner of those invariants.

### HIR to Core

Anonymous structs flatten at the same aggregate-scalarization boundary as nominal
structs, in declared component order. Projection and indexing select the
corresponding leaf bundle; a destructuring statement lowers to component moves
into the target places after operand evaluation. `:=` is erased entirely by type
checking.

**PS-5 adds no Core operation, no Core type, and no new terminator.** Every
existing exhaustive Core census is unaffected. This is the deliberate architectural
consequence of building multiple results as structs: the feature's lowering is the
struct lowering.

### Core evaluation and optimization

The Core evaluator and both optimization policies see ordinary scalar leaf values
and existing operations; no evaluator change is expected beyond fixtures. The
four-policy differential remains the equivalence authority.

## Mechanical Minecraft lowering

None is added. Scalarized components occupy ordinary typed score/data homes through
the existing realization model, and existing footprint/cost reporting covers the
generated products. The fast structural suite asserts reuse, not new recipes.

## Relationship to `mut` parameters

S-004 selected `mut` parameter mode syntax (declaration and callsite). PS-5 does
not implement it, but records the lowering strategy: a `mut` parameter is
copy-in/copy-out sugar that lowers to an ordinary parameter plus an additional
result written back at the callsite — exactly the multiple-result ABI PS-5 builds.
The dependency order is deliberate: multiple results are the substrate, `mut` is
later sugar. When `mut` lands it subsumes the update-in-place destructuring uses
(`tape.move_right(mut left, mut right)`), while produced-fresh results remain
PS-5's territory. S-026 deferred the aliasing/write-back rules for overlapping
`mut` places; `f(mut x, mut x)` needs an explicit verdict at that time, with
rejection as the working lean.

## Test architecture

### Frontend tests

- lexer coverage for `:=`, `[`, `]`, statement-start `|`, `.{`, and the
  punctuation ambiguities: `:=` vs `:` `=`, `x<-1` remaining comparison, `<=` in
  expressions vs destructure position, `.{` vs `.name`, and index brackets vs
  future list brackets;
- parser AST goldens for both anonymous type forms, nesting, inferred literals
  (named, positional, nested, trailing commas), index suffixes, destructure
  statements with every target role, `:=` declarations, and bounded recovery;
- structural-identity tests: same spelling across modules is one type; reordered
  fields, named-vs-positional, and anonymous-vs-nominal are distinct with
  near-miss diagnostics;
- expected-type tests for inferred literals in every admitted context and
  rejection of every context-free position, including `:=` initializers;
- projection/index tests: named-by-name only, positional-by-index only, bounds
  and non-literal index rejection, chaining off calls;
- destructure tests: arity mismatch, duplicate targets, non-writable bare
  targets, `_` discard, wrong operand shape (named, nominal, scalar), definite
  assignment through all/some target roles, evaluation-once semantics; and
- export-ABI rejection tests, recursively through nominal structs.

### Core and optimizer tests

- flattening round trips for named/positional/nested anonymous structs through
  parameters, results, joins, branches, and recursive frames;
- destructure lowering to component moves with correct write-back order when
  targets alias operand inputs;
- four-policy equivalence on the integration fixture; and
- scale tests for wide anonymous structs asserting bounded compile behavior.

### Source and target integration

Add `tests/programs/anon-structs/` as ordinary MDL source: pair-returning
functions across a module boundary, a two-list zipper move updated by
destructuring, `:=` declarations, named projection, positional indexing, and a
scalar `Int32` checksum export. Compile under all four Core/Minecraft policy
combinations; compare through the Core evaluator and the independent expected
table. One ignored pinned Java 26.2 server test runs the checksum entry points in
a single lifecycle across all four products.

### Regression and quality gates

- existing PS-1 through PS-4 suites remain unchanged and passing;
- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace --all-targets` at tranche gates and completion; and
- the exact official-server command only at the target tranche and final audit.

## Non-goals

- by-name destructuring, target renames, or nested destructure patterns;
- positional access or destructuring on named anonymous or nominal structs;
- `.0`-style member syntax in any form;
- runtime, negative, or end-relative tuple indices (S-032's end-relative form
  remains a list decision);
- aggregate equality for any struct form;
- anonymous struct types in `DatapackExport` parameters or results;
- `List` of anonymous structs or any `List<T>` generalization;
- functional-update/spread literal syntax;
- implementing S-004 `mut` parameters;
- expression-position destructuring or pipe lambdas — if a future closure syntax
  wants pipes in expression position, statement-position destructuring remains
  unambiguous and that interaction is decided then; and
- rewriting the PS-3 Brainfuck package onto this surface (an optional follow-up,
  not a completion requirement).

## Exit criteria

- both anonymous struct forms work in every admitted type position with
  structural cross-module identity;
- inferred literals, projection, indexing, destructuring, and `:=` implement the
  frozen contract with exact diagnostics for every rejection class;
- HIR verification independently owns the new invariants;
- no new Core operation or Minecraft recipe exists; flattening reuse is proven by
  the existing censuses remaining unchanged;
- the integration fixture is equivalent under all four policies and on the pinned
  server;
- `grammar.ebnf` absorbed the delta atomically with the parser; and
- the handoff records deferred breadth (by-name patterns, `mut`, export ABI,
  `List<T>`) without silently expanding Stage 9.

## Research basis

- [Zig language reference: anonymous struct literals, tuples, and result-location
  inference](https://ziglang.org/documentation/master/)
- [Zig 0.12 release notes: destructuring syntax for tuples](https://ziglang.org/download/0.12.0/release-notes.html)
- [Rust Reference: tuple types and the `.0` lexer special case rejected
  here](https://doc.rust-lang.org/reference/types/tuple.html)
- [Go specification: multiple return values, rejected as non-first-class](https://go.dev/ref/spec#Return_statements)
- [MDL S-003 inferred declarations](../../syntax/README.md)
- [MDL S-004 caller-visible mutation](../../syntax/README.md)
- [MDL S-026 argument evaluation order](../../syntax/argument-evaluation-order.md)
- [MDL S-032/S-033 list indexing selections](../../syntax/list-indexing.md)
- [PS-3 Brainfuck capstone evidence of the missing surface](ps-3-handoff.md)
