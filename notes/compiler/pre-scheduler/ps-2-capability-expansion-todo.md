# PS-2 Capability Expansion Checklist

Status: **PS-3-ready slice complete; deferred breadth is recorded in the handoff**

Authoritative design: [`ps-2-capability-expansion-plan.md`](ps-2-capability-expansion-plan.md).

The capability dossiers define semantic questions and local exit criteria. This
checklist defines implementation order. Do not begin a dependent tranche before its
gate is satisfied.

## PS-2.0 — Freeze the capstone contract and budgets

Design: [`ps-2/brainfuck-semantics.md`](ps-2/brainfuck-semantics.md) and
[`ps-2/bounded-execution.md`](ps-2/bounded-execution.md).

- [x] Decide cell domain, wrapping, tape growth/direction, input/output bytes,
      ignored source characters, bracket errors, and page joining.
- [x] Decide maximum book/program size and mandatory execution fuel semantics.
- [x] Decide whether actual player-held-book automation is a PS-3 required gate or
      an opt-in client boundary after an equivalent holder-independent book proof.
- [x] Freeze synchronous compile/deployment policies for exact, finite, unbounded,
      and unknown work.
- [x] Derive the minimal general source/API capability inventory from the frozen
      interpreter design.
- [x] Freeze the language, public standard library, typed intrinsic, and private
      runtime ownership model from `ps-2/standard-library-boundary.md`.
- [x] Classify every required Brainfuck capability into exactly one primary layer
      and record any reference-implementation/intrinsic pairing.
- [x] Select the Phase-1 `std` module root, prelude policy, bundled-source loading,
      and sealed intrinsic identity/signature mechanism.
- [x] Freeze the proof obligations for any handwritten optimized mcfunction helper.
- [x] Set independent frontend, Core, representation, macro, evaluator, and scenario
      resource limits.
- [x] Record explicitly deferred Brainfuck dialects and Minecraft holder surfaces.

Gate: the compiler work is driven by a closed semantic client rather than an
ever-expanding example.

## PS-2A — Arithmetic, mutation, and same-tick control flow

Design: [`ps-2/arithmetic-and-control-flow.md`](ps-2/arithmetic-and-control-flow.md).

- [x] Freeze signed `Int32` wrapping arithmetic and comparison semantics needed by
      the interpreter; keep division/remainder reserved.
- [x] Define wrapping-byte normalization without pretending Minecraft has a native
      source `UInt8` representation.
- [x] Add source expressions/operators and precise diagnostics.
- [x] Add loop, `break`, and `continue` syntax with explicit evaluation order.
- [x] Lower mutable source places and loops into verified SSA/CFG structure.
- [x] Extend Core editing, optimization classifications, and printers exhaustively.
- [x] Extend the Core evaluator with arithmetic and cyclic CFG execution limits.
- [x] Implement synchronous Minecraft lowering using existing control/calling
      contracts before optional unrolling/bulk recipes.
- [x] Add zero/one/many/nested/early-exit loop source fixtures.
- [x] Add four-policy evaluator/server differentials and reuse the pinned PS-1
      command-limit boundary authorities.

Gate: bounded scalar loop programs compile and execute correctly without scheduling.

## PS-2B — Fixed aggregates

Design: [`ps-2/aggregate-values.md`](ps-2/aggregate-values.md).

- [x] Freeze nominal identity, field order, construction/projection, copy, mutation,
      equality, call/return, and recursive behavior for the initial struct slice.
- [x] Add canonical aggregate types and bounded source declarations.
- [x] Add typed HIR aggregate operations and verifier coverage; recursively flatten
  them before scalar Core rather than retaining unused aggregate Core operations.
- [x] Extend evaluator, source-to-Core maps, deterministic printers, and diagnostics.
- [x] Implement scalarized field realizations first through Stage 8's physical model.
- [x] Prove typed NBT compound realization/materialization has no retained PS-3
      client and omit it.
- [x] Define aggregate ABI expansion and recursive frame fields without aliasing callers.
- [x] Add scalar replacement by construction while retaining source-level
      equivalence tests.
- [x] Add construction, projection, update, branch, call, return, serial-context,
      and recursion scenarios under all policies.
- [x] Prove unused compound support is absent.

Gate: interpreter state can be represented as ordinary immutable SSA aggregate
values with explicit source updates.

## PS-2C — Owned lists, stacks, and zippers

Design: [`ps-2/lists-stacks-and-zippers.md`](ps-2/lists-stacks-and-zippers.md).

- [x] Freeze `List<Int32>` value/copy/ownership semantics and empty-operation
      behavior.
- [x] Add the minimal list operation algebra: construct, empty/length, push, peek,
      pop, and required whole-list movement/copy.
- [x] Decide whether consuming operations are explicit in types/syntax or optimized
      from value operations through ownership analysis.
- [x] Add typed HIR/Core operations, verifier, printer, editor, effect, and evaluator
      support.
- [x] Add NBT-list physical realization and explicit score/NBT element bridges.
- [x] Implement ordinary tail-operation recipes before dynamic indexing.
- [x] Implement stack and two-list zipper as ordinary MDL code in the rehearsal.
- [x] Add alias/copy, empty, one, many, nested-aggregate, call/return, and
      serial-context cases.
- [x] Run pinned-server NBT semantics and four-policy composition scenarios.
- [x] Bound evaluator value nodes and keep list IR/target generation independent of
      runtime list size.

Gate: program and tape cursors move in both directions through public list/stack
operations without source-visible NBT paths.

## PS-2D — Runtime strings and opcode parsing

Design: [`ps-2/strings-and-parsing.md`](ps-2/strings-and-parsing.md).

- [x] Freeze target-aligned UTF-16 units, tail bounds, Unicode behavior, copy, and
      failure policy; omit unneeded concatenation.
- [x] Choose the smallest parser-facing operation algebra and preserve higher-level
      intent where representation selection benefits.
- [x] Add typed source/HIR/Core/evaluator support for the chosen pure string
      operations.
- [ ] Implement constant folding and constant program parsing (optimization follow-up;
      runtime parsing is the PS-3 authority).
- [x] Add NBT-string realization and explicit conversions.
- [x] Determine with pinned evidence whether runtime consumption requires a typed
      macro template or can use a non-macro recipe.
- [x] Avoid a target macro: static tail slices implement the admitted algebra.
- [x] Implement opcode recognition, ignored-character handling, and bracket
      validation as library code unless a semantic primitive is justified.
- [x] Add empty, ASCII, Unicode/non-opcode, malformed-bracket, and limit cases.
- [x] Compare parser results through the evaluator and vanilla observations.

Gate: runtime text can become a validated opcode sequence without raw command
interpolation.

## PS-2E — Books, items, and holders

Design: [`ps-2/books-and-items.md`](ps-2/books-and-items.md).

- [x] Pin the target command-report/component shape for the selected Java 26.2
      written-book and slot operations.
- [x] Freeze the narrow typed executor/main-hand/literal-page/empty-fallback semantics.
- [x] Keep inventory-holder capability separate from command-executor capability
      checks.
- [x] Add semantic descriptors and typed external operations with complete context,
      cardinality, effect, outcome, and target requirement contracts.
- [x] Select and retain Java 26.2 recipes during target preflight.
- [x] Add structured target commands/arguments rather than rendered component text.
- [x] Materialize literal book pages into the semantic string boundary explicitly.
- [x] Add missing/wrong/empty fallback, static multiple-page access, Unicode, and
      explicit unsupported-rich-component coverage.
- [x] Run holder-independent clientless vanilla cases.
- [x] Decide the controlled non-player holder is the PS-3 required gate without
      building a general Minecraft client into `mdl-test`.

Gate: an exact-one typed holder can supply written-book program text through a
verified public MDL API.

## PS-2F — Bounded interpreter execution contract

Design: [`ps-2/bounded-execution.md`](ps-2/bounded-execution.md).

- [x] Add the chosen finite-fuel API and result/error model as ordinary MDL types.
- [x] Prove every interpreter dispatch consumes fuel exactly once according to the
      frozen semantics.
- [ ] Add a language-visible parse-work result separate from the evaluator node cap.
- [ ] Extend target work analysis to compose runtime fuel through loop/list/string bounds without
      replacing no-finite-bound/unknown with optimistic estimates.
- [ ] Add a strict deployment mode for workloads outside configured hard limits;
      the current analysis report remains nonfatal.
- [x] Test completion at fuel zero/exact need, one short, and surplus fuel.
- [x] Test invalid programs independently from fuel exhaustion.
- [x] Retain the Stage 8 abnormal Minecraft command-sequence interruption and
      cleanup/recovery separately from semantic fuel exhaustion.
- [x] Add cost explanations relating source fuel/program limits to conservative
      target sequence/fork bounds.

Gate: supported interpreter invocations have deterministic termination results and
honest synchronous deployment evidence.

## PS-2G — Composition rehearsal and audit

- [x] Implement a small non-capstone parser/tape rehearsal using only public MDL
      facilities.
- [ ] Add stable compiler-injected `std` distribution/authentication; PS-3 first uses
      explicit ordinary package modules, as recorded in the handoff.
- [x] Differentially compare reference MDL behavior with target
      recipes where both exist.
- [x] Prove scalar/no-import programs emit no list/string/book private support.
- [x] Reject forged, signature-mismatched, or target-incompatible typed Minecraft
      declarations.
- [x] Compile it under all four policies and compare Core/server observations.
- [x] Audit every new operation's exhaustive verifier/editor/effect/optimization
      integration.
- [x] Audit physical storage identity, materializations, ABI, recursive frames,
      cleanup, and unused-support elimination.
- [x] Audit source-to-HIR/Core/target/pack correlation and diagnostics.
- [x] Run formatting, lints, fast workspace tests, scale tests, and pinned ignored
      server suites.
- [x] Update `semantic-ambiguities.md`, mcfunction research notes, and the testing
      workflow with every resolved/remaining target behavior.
- [x] Write the PS-2 to PS-3 handoff listing only public capabilities and remaining
      capstone assembly work.

Gate: PS-3 can be implemented as a normal MDL package without opening compiler
internals or introducing a new semantic primitive.

## Explicitly deferred

- [ ] Persistent continuations, yield, and multi-tick scheduling remain Stage 9.
- [ ] General language macros remain outside the narrow target macro client.
- [ ] Shared mutable references and escaping interior list/struct references remain
      deferred.
- [ ] Global representation search, PGO, and e-graphs remain Stage 11.
- [ ] Broad Minecraft inventory/component coverage remains capability-driven.
