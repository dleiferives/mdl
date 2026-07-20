# PS-4 Enums, Switch, and Inclusive Range Patterns Checklist

Status: **complete on 2026-07-19; deferred items remain intentionally open**

Authoritative design:
[`ps-4-enums-switch-ranges-plan.md`](ps-4-enums-switch-ranges-plan.md).

Implement the slices in order. A syntax-only merge or an unverified enum-as-integer
shortcut does not complete a tranche.

## PS-4.0 — Freeze fixtures and contracts

- [x] Review `notes/syntax/ps-4-grammar-delta.ebnf` against this plan and resolve
      any production/semantic disagreement before touching the parser.
- [x] Add the source grammar examples and negative examples from the plan as test
      fixtures before implementation.
- [x] Freeze fieldless nominal identity, dense private tag assignment, inferred and
      qualified variant syntax, admitted value positions, and rejected operations.
- [x] Freeze switch evaluation order, expression/statement body forms, commas,
      `=>`, inclusive `...`, no fallthrough, and no guards/captures.
- [x] Freeze total overlap rejection and enum/integer exhaustiveness behavior.
- [x] Freeze export-ABI rejection for enum-containing parameter/result types.
- [x] Define the small integration program and independent expected-result table,
      including signed range boundaries.
- [x] Record dispatch optimization and datapack-size selection as deferred; retain
      footprint/cost reporting only.

Gate: every later implementation decision can be checked against one written source
semantic contract.

## PS-4A — Tokens, AST, parser, and recovery

- [x] Merge the accepted PS-4 delta into `notes/syntax/grammar.ebnf` atomically with
      the parser change; leave no implemented-only or grammar-only production.
- [x] Add closed tokens for `enum`, `switch`, `=>`, and `...`; scan longest
      punctuation first and preserve exact spans.
- [x] Add AST declarations for enums/variants and AST switch patterns, prongs,
      expression bodies, and statement-block bodies.
- [x] Parse `const Name = enum { ... };` beside the existing struct item shape.
- [x] Parse `.variant` and `Type.variant` without weakening existing member access.
- [x] Parse switch expressions at primary-expression precedence and switch statements
      at the structured-statement boundary.
- [x] Parse signed exact/range endpoints, multiple patterns, `else`, required arrows,
      commas, and trailing commas deterministically.
- [x] Extend syntax-depth accounting to nested switch bodies and expressions.
- [x] Add bounded recovery for missing names, bounds, arrows, commas, bodies, braces,
      and semicolons without swallowing the next function item.
- [x] Extend AST dumps and exact lexer/parser golden tests.
- [x] Add a grammar-conformance fixture inventory mapping each changed production
      to at least one accepted and one rejected source case.
- [x] Add punctuation ambiguity cases for `1...3`, decimal numbers, member dots,
      inferred enum dots, coordinates, `->`, and `=>`.

Gate: clean syntax round-trips deterministically and malformed switches recover
within existing resource limits.

## PS-4B — Declaration collection and nominal enum checking

- [x] Add dense `SourceEnumId`/`SourceVariantId` identities and a canonical package
      enum inventory.
- [x] Unify struct/enum type-name collision detection without replacing stable
      per-kind identities with stringly lookups.
- [x] Reject empty enums, duplicate variants, reserved compiler names, and identity
      exhaustion with exact supporting spans.
- [x] Extend `ValueType`, signatures, bindings, structs, parameters, calls, results,
      display, dumps, and checked-output queries for nominal enums.
- [x] Type qualified and context-inferred enum literals; diagnose missing context,
      unknown variants, and wrong nominal enum types.
- [x] Permit same-enum `==`/`!=`; reject ordering, arithmetic, and enum/int mixing.
- [x] Reject enum-containing datapack-export parameters/results recursively through
      structs.
- [x] Add positive tests for locals, `var`, assignment, calls, returns, struct
      construction/projection, branches, loops, and private recursion.
- [x] Add negative tests for every declaration, resolution, operation, and ABI rule.

Gate: enum values are source-level nominal values, never untyped integer aliases.

## PS-4C — Switch checking, flow, and HIR verification

- [x] Add typed HIR enum literals, normalized scalar patterns, switch expressions,
      and switch statements with complete origins.
- [x] Evaluate/check the scrutinee once and constrain every pattern from its exact
      scrutinee type.
- [x] Implement dense enum coverage and deterministic missing-variant diagnostics.
- [x] Implement sorted disjoint inclusive integer intervals with `O(p log p)`
      overlap insertion and linear final coverage checking.
- [x] Reject backwards/out-of-range endpoints, any overlap, misplaced/duplicate
      `else`, unreachable `else`, and non-exhaustive switches.
- [x] Require one exact result type across expression arms, including aggregate and
      enum results.
- [x] Reuse branch-sensitive definite-assignment/continuation rules for statement
      arms, considering only continuing predecessors.
- [x] Preserve enclosing-loop meaning for `break`/`continue` inside prong blocks.
- [x] Extend HIR dumps, traversal, behavior inference, and function-reference walks.
- [x] Make HIR verification recompute identity, type, coverage, overlap,
      exhaustiveness, result, flow, and export-ABI invariants.
- [x] Add direct corruption tests for every new verifier check.
- [x] Add scale tests proving interval checking does not enumerate ranges or compare
      every pair.

Gate: checked HIR alone is sufficient to trust all admitted enum/switch semantics.

## PS-4D — Core range predicate and enum erasure

- [x] Add a validated target-neutral closed inclusive `i32` range attribute/value
      with `min <= max`.
- [x] Add `CoreOp::I32InClosedRange`, builder support, signature, semantics, effect,
      speculation, equivalence, symmetry, and discardability classifications.
- [x] Extend Core verification, canonical printing/debug dumping, editing, operand/
      use enumeration, ambient analysis, and every exhaustive operation census.
- [x] Extend Core evaluation for inclusive signed boundaries and resource charging.
- [x] Extend SCCP, canonicalization, CSE, DCE, fusion, and pipeline reporting; fold a
      constant operand without adding interval analysis.
- [x] Flatten `ValueType::Enum` to `CoreType::I32` and variant literals to private
      dense tag constants at HIR-to-Core conversion.
- [x] Lower enum equality through existing `I32Compare` without exposing a source
      conversion.
- [x] Lower one switch scrutinee to shared ordinary CFG tests and arm bodies; do not
      duplicate a body for multiple patterns.
- [x] Join expression results with typed block parameters and statement state with
      existing sparse environment merges.
- [x] Use the final exhaustive/else prong as the structural fallback; emit no
      reachable unsupported terminator.
- [x] Add source-to-Core origin/correlation checks for patterns and generated tests.
- [x] Test enums nested in structs and carried through joins/calls/recursive frames.

Gate: all PS-4 source semantics execute correctly in the Core evaluator under both
Core optimization policies.

## PS-4E — Mechanical Minecraft lowering

- [x] Audit legality, demand, liveness, placement, allocation, realization,
      recursive-frame, call, and plan layers for the new pure Boolean operation.
- [x] Add one scalar recipe that initializes `0` and conditionally sets `1` through
      typed `Condition::ScoreMatches` and `ScoreRange`.
- [x] Canonicalize equal bounds to the exact Minecraft spelling and test full signed
      boundaries, negatives, zero, and crossing-zero ranges.
- [x] Extend symbolic planning, recipe/preflight contracts, resource collection,
      construction, emission, lowering reports, and exhaustive corruption tests.
- [x] Verify normalized Boolean ABI/storage invariants under both Minecraft policies.
- [x] Assert at least one compiled source range renders
      `execute if score ... matches 0..20`.
- [x] Do not fuse compare+branch, reorder cases, construct a balanced tree, invoke a
      function macro, or impose a datapack-size cap.
- [x] Retain deterministic function/line/byte and command/fork/storage cost reports.

Gate: the production compiler emits a verified datapack using the native inclusive
score-range predicate through one policy-independent legalization.

## PS-4F — Differential and vanilla evidence

- [x] Add `tests/programs/enums-switch-ranges/` with ordinary MDL source, a small
      case corpus, and no unsafe/private-runtime shortcut.
- [x] Cover negative lower bounds, `i32::MIN`, exact values, both closed endpoints,
      holes, adjacent ranges, `i32::MAX`, and `else`.
- [x] Exercise enum construction, inferred and qualified variants, equality,
      expression switch, statement switch, struct storage, calls, returns, mutable
      joins, and early return.
- [x] Compare all semantic results through the independent expected table and Core
      evaluator under both Core policies.
- [x] Compile all four Core/Minecraft policy products in distinct namespaces and
      compare public outputs.
- [x] Add concise structural golden assertions for source/HIR/Core/range command
      evidence without pinning an optimized layout.
- [x] Run one ignored pinned Java 26.2 server lifecycle over all boundary cases and
      policy products.
- [x] Preserve concise failure artifacts/log cleanup through the PS-1 harness.
- [x] Record footprint and conservative target analysis for every policy without a
      maximum-size pass/fail threshold.

Gate: independent target-neutral and vanilla authorities agree on the complete
admitted feature slice.

## PS-4G — Audit, documentation, and handoff

- [x] Review every new closed enum match across AST, HIR, Core, optimizer, lowering,
      evaluator, and testing code.
- [x] Review all source locations, recovery paths, ownership/identity tables, and
      scale-test complexity separately from happy-path tests.
- [x] Run formatting, strict clippy, targeted suites, the complete compiler suite,
      and the exact ignored server command; leave the slow cross-workspace matrix to
      occasional CI/pre-merge validation.
- [x] Update the source-language examples and `notes/mcfunction` range/control-flow
      notes with only newly established behavior.
- [x] Add any semantic ambiguity discovered during implementation to
      `notes/compiler/semantic-ambiguities.md` before choosing a behavior.
- [x] Write `ps-4-handoff.md` with the exact public surface, rejected/deferred
      breadth, policy evidence, artifact metrics, and Stage 9 relationship.
- [x] Mark roadmap/checklist completion only after the worktree and generated tests
      prove every exit criterion.

Gate: PS-4 is reviewable as a complete typed vertical slice, and later optimization
or `List<Enum>` work has an honest starting point.

## Explicitly deferred

- [ ] Benchmark linear, coalesced, balanced, DAG, and macro dispatch after real
      programs exist.
- [ ] Decide profile-specific datapack-size tradeoffs from those measurements.
- [ ] Add `List<Enum>` only with a typed generic/specialized collection design.
- [ ] Design payload variants/tagged unions and a general pattern matrix together.
- [ ] Design imported nominal type spelling/visibility independently of enum tags.
- [ ] Design a validated stable external enum ABI before exporting enum values.
- [ ] Keep persistence/yield/scheduling in Stage 9.
