# PS-5 Anonymous Structs, Multiple Results, and Destructuring Checklist

Status: **complete — shipped in commit d5321eb**

Authoritative design:
[`ps-5-anon-structs-destructuring-plan.md`](ps-5-anon-structs-destructuring-plan.md).

Implement the slices in order. A syntax-only merge or an unverified
anonymous-struct-as-nominal shortcut does not complete a tranche.

## PS-5.0 — Freeze fixtures and contracts

- [ ] Review `notes/syntax/ps-5-grammar-delta.ebnf` against this plan and resolve
      any production/semantic disagreement before touching the parser.
- [ ] Add the source examples and negative examples from the plan as test fixtures
      before implementation.
- [ ] Freeze structural identity (ordered components, named/positional/nominal
      separation, cross-module sharing) and the admitted type positions.
- [ ] Freeze inferred-literal context rules, entry-kind exclusivity, and full
      coverage.
- [ ] Freeze consumption-by-shape: named projection only, positional
      index/destructure only, compile-time index bounds.
- [ ] Freeze destructure target roles, exact arity, duplicate rejection, `_`
      discard, single complete operand evaluation, and left-to-right write-back.
- [ ] Freeze `:=` semantics, its rejected context-free initializers, and the
      unchanged S-001 annotation default.
- [ ] Freeze export-ABI rejection recursively through nominal structs.
- [ ] Record `mut` parameters (S-004) as deferred sugar over this ABI with the
      overlapping-place verdict left to that decision.
- [ ] Define the integration program and independent expected-result table.

Gate: every later implementation decision can be checked against one written
source semantic contract.

## PS-5A — Tokens, AST, parser, and recovery

- [ ] Merge the accepted PS-5 delta into `notes/syntax/grammar.ebnf` atomically
      with the parser change; leave no implemented-only or grammar-only
      production.
- [ ] Add `:=`, `[`, and `]` tokens; scan `:=` longest-match before `:`; preserve
      exact spans; confirm `x<-1` still lexes as comparison.
- [ ] Parse both anonymous struct type forms in every ValueType position with the
      one-Name-then-colon disambiguation, including nesting and trailing commas.
- [ ] Parse `.{` inferred literals, distinguishing named entries (`.` Name `=`)
      from positional entries that begin with an inferred enum literal.
- [ ] Parse index suffixes at postfix precedence beside member access and calls.
- [ ] Parse the destructuring statement at statement start with every target
      role, trailing comma, and required `<=`.
- [ ] Parse `:=` declarations beside annotated declarations for `const` and
      `var`.
- [ ] Extend syntax-depth accounting to nested anonymous types and literals.
- [ ] Add bounded recovery for unclosed braces/brackets/pipes, missing `<=`,
      missing commas, and malformed targets without swallowing the next item.
- [ ] Extend AST dumps and exact lexer/parser golden tests.
- [ ] Add a grammar-conformance fixture inventory mapping each changed production
      to at least one accepted and one rejected source case.

Gate: clean syntax round-trips deterministically and malformed input recovers
within existing resource limits.

## PS-5B — Structural types, inference, and checking

- [ ] Intern structural anonymous type identities canonically across the package;
      identical spellings in different modules resolve to one identity.
- [ ] Extend `ValueType`, signatures, bindings, struct fields, parameters, calls,
      results, display, dumps, and checked-output queries for both forms.
- [ ] Reject reordered-field identity conflation with a near-miss diagnostic;
      keep named/positional/nominal fully separate.
- [ ] Type inferred literals from every admitted expected-type context; reject
      every context-free position, mixed entry kinds, and partial coverage.
- [ ] Type named projection and compile-time index access; reject cross-shape
      access, non-literal indices, and out-of-bounds indices.
- [ ] Implement `:=` inference with exact initializer types; reject inferred
      literals, coordinate forms, and `Void` calls as initializers.
- [ ] Reject anonymous struct types in `DatapackExport` parameters/results
      recursively through nominal structs.
- [ ] Add positive tests through locals, `var`, assignment, calls, returns,
      struct fields, branches, loops, and private recursion.
- [ ] Add negative tests for every declaration, identity, context, access, and
      ABI rule.

Gate: anonymous struct values are exactly typed structural values, never nominal
aliases or positional/named hybrids.

## PS-5C — Destructuring, flow, and HIR verification

- [ ] Add the typed destructuring statement with resolved targets, roles, and
      operand type to checked HIR.
- [ ] Check exact arity, duplicate targets, bare-target writability under S-002,
      and `_` discard semantics.
- [ ] Type fresh `const`/`var` targets from component types with S-003 semantics.
- [ ] Enforce single complete operand evaluation before left-to-right write-back,
      including self-referencing operands.
- [ ] Integrate destructure targets into S-008 definite assignment and
      continuation analysis.
- [ ] Extend HIR dumps, traversal, behavior inference, and function-reference
      walks.
- [ ] Make HIR verification independently recompute identity interning, literal
      coverage, index bounds, destructure invariants, definite assignment, and
      export rejection.
- [ ] Add direct corruption tests for every new verifier check.

Gate: checked HIR alone is sufficient to trust all admitted PS-5 semantics.

## PS-5D — Core flattening reuse

- [ ] Flatten both anonymous forms at the existing aggregate-scalarization
      boundary in declared component order; add no Core operation, type, or
      terminator.
- [ ] Lower projection/indexing to leaf selection and destructuring to ordered
      component moves into target places.
- [ ] Erase `:=` entirely at type checking; Core sees ordinary typed bindings.
- [ ] Prove census neutrality: every exhaustive Core operation match compiles
      unchanged.
- [ ] Test flattening through parameters, results, joins, branches, recursive
      frames, and nesting in nominal structs.
- [ ] Test destructure write-back order when targets alias operand inputs.
- [ ] Verify evaluator equivalence under both Core policies on all fixtures.

Gate: all PS-5 source semantics execute correctly in the Core evaluator with zero
new Core surface.

## PS-5E — Differential and vanilla evidence

- [ ] Add `tests/programs/anon-structs/` with ordinary MDL source, a module
      boundary crossed by a structural type, a destructured zipper move, `:=`
      declarations, and a scalar checksum export.
- [ ] Compare all semantic results through the independent expected table and
      Core evaluator under both Core policies.
- [ ] Compile all four Core/Minecraft policy products in distinct namespaces and
      compare public outputs.
- [ ] Add concise structural golden assertions that no new command recipe shape
      appears — reuse is the asserted fact.
- [ ] Run one ignored pinned Java 26.2 server lifecycle over the checksum entry
      points and policy products.
- [ ] Record footprint and conservative target analysis for every policy without
      a size threshold.

Gate: independent target-neutral and vanilla authorities agree on the complete
admitted feature slice.

## PS-5F — Audit, documentation, and handoff

- [ ] Review every new closed match across AST, HIR, checking, verification, and
      lowering code.
- [ ] Review source locations, recovery paths, and identity-interning tables
      separately from happy-path tests.
- [ ] Run formatting, strict clippy, targeted suites, full workspace/all-targets,
      and the exact ignored server command.
- [ ] Update `notes/syntax/README.md` status wording for S-003 and S-041 to
      implemented, atomically with the grammar merge.
- [ ] Add any semantic ambiguity discovered during implementation to
      `notes/compiler/semantic-ambiguities.md` before choosing a behavior.
- [ ] Write `ps-5-handoff.md` with the exact public surface, rejected/deferred
      breadth, policy evidence, and the `mut`/`List<T>`/module-path relationship.
- [ ] Mark roadmap/checklist completion only after the worktree and generated
      tests prove every exit criterion.

Gate: PS-5 is reviewable as a complete typed vertical slice, and `mut` or module
type-path work has an honest starting point.

## Explicitly deferred

- [ ] Implement S-004 `mut` parameters as copy-in/copy-out sugar over this ABI,
      deciding overlapping-place rules with S-026.
- [ ] Design by-name destructuring, renames, and nested patterns together if a
      real client justifies them.
- [ ] Decide list runtime indexing (S-032/S-033) sharing the bracket surface.
- [ ] Design aggregate equality explicitly before any struct form supports `==`.
- [ ] Design a stable external ABI before anonymous structs cross
      `DatapackExport`.
- [ ] Rewrite the PS-3 Brainfuck package onto this surface as an optional
      demonstration, not a requirement.
- [ ] Keep persistence/yield/scheduling in Stage 9.
