# PS-2: Brainfuck-Driven Capability Expansion

Status: **PS-3-ready slice complete; see [`ps-2-handoff.md`](ps-2-handoff.md)**

## Objective

Add the general synchronous language and backend capabilities needed for an ordinary
MDL package to implement the frozen Brainfuck contract. Brainfuck is the dependency
probe and first client; compiler operations must not be named or specialized around
Brainfuck.

PS-2 begins only after PS-1's evaluator and vanilla scenario boundaries are usable.
Every capability is implemented vertically and extends those oracles as appropriate.

## Capability dossiers

- [Brainfuck semantic decisions](ps-2/brainfuck-semantics.md)
- [Arithmetic and same-tick control flow](ps-2/arithmetic-and-control-flow.md)
- [Aggregate values](ps-2/aggregate-values.md)
- [Owned lists, stacks, and zippers](ps-2/lists-stacks-and-zippers.md)
- [Runtime strings and program parsing](ps-2/strings-and-parsing.md)
- [Books, items, and holders](ps-2/books-and-items.md)
- [Bounded synchronous execution](ps-2/bounded-execution.md)
- [Language, standard library, intrinsic, and runtime boundary](ps-2/standard-library-boundary.md)

The ordered implementation checklist is
[`ps-2-capability-expansion-todo.md`](ps-2-capability-expansion-todo.md).

## Semantic-first rule

Before adding syntax, freeze for each value family:

- value identity and equality;
- copy, move, mutation, alias, and escape behavior;
- evaluation order and failure behavior;
- ownership across calls, returns, branches, recursion, and run scopes;
- which observations raw/external Minecraft behavior may make;
- deterministic limits and diagnostics; and
- target-independent meaning versus target-specific operations.

Source meaning must not be inferred from the selected NBT or scoreboard layout.

## Core and HIR shape

Core remains target-independent, typed SSA. New operations should preserve semantic
intent long enough for optimization and representation selection:

```text
AggregateConstruct / AggregateProject
ListConstruct / ListLength / ListPush / ListPop
StringLength / StringSlice or StringConsume
Loop structure represented by ordinary verified CFG
```

The exact algebra is decided per dossier. Do not add NBT paths, objectives, macro
fragments, book component spellings, or Minecraft holder slots to Core value types.
Minecraft-facing book acquisition is a typed external operation whose result is a
semantic string/list value or an explicit absence/error.

All new operations require exhaustive verifier, printer, editor, effect,
speculation, optimization, demand/liveness, evaluator, and lowering decisions. An
operation may initially block an optimization, but it cannot silently inherit a
wrong default classification.

## Physical realization

Stage 8's separation remains authoritative:

```text
semantic value
  != mutable storage
  != realization occurrence
  != physical ABI mode
```

PS-2 adds representations only when a semantic client requires them. Candidate
classes include:

- scalarized aggregate fields in scores/frames;
- typed NBT compounds for materialized aggregates;
- owned NBT lists;
- NBT strings;
- compile-time constants and fully elided values; and
- mixed realizations with explicit materialization recipes.

Start with a small deterministic policy. Retain legal alternatives and costs, but do
not build Stage 11's global layout search. Every conversion between scores, NBT, and
macro syntax is explicit and reconciled with emitted structured commands.

## Ownership and observation boundary

The initial aggregate/list model should favor values over shared mutable references:

- source `var` is updated through SSA;
- assignment and call/return behavior are explicit value operations;
- no escaping interior reference is introduced merely because NBT paths can alias;
- an owned destructive list operation may consume its input when the type/API says
  so; and
- raw commands are conservative observation barriers for externally visible or
  exposed storage.

This makes scalar replacement, copy elimination, and zipper lowering defensible.
References and shared mutable views are separate future capabilities.

## Same-tick loops

PS-2 adds ordinary language loops without suspension. They lower to existing Core
CFG cycles and synchronous functions/branches. The compiler may unroll, fuse, use a
native bulk operation, use selector contexts, or retain recursive/dispatcher
control, but source behavior is independent of the recipe.

Every loop must be classified separately for:

- semantic termination knowledge;
- finite static trip/count bounds;
- hard command-sequence compatibility;
- per-expansion fork compatibility; and
- optional soft cost reporting.

No-finite-bound-proven is not synonymous with divergence, but a strict synchronous
deployment policy may reject it. Brainfuck uses explicit fuel to obtain a finite
semantic bound.

## Minecraft macro boundary

Runtime string slicing, NBT indexing, or resource syntax may require a Minecraft
function macro. If so, PS-2 adds a closed typed template operation:

```text
instantiate(validated_template, typed_argument_compound)
```

It must provide:

- syntax-position-specific serializers;
- complete escaping/range validation;
- explicit missing-field and invalid-instantiation behavior;
- a verified storage/frame ABI;
- ordinary/macro command separation in target IR;
- cost evidence for parsing and cache behavior; and
- specialization/static-dispatch alternatives where applicable.

An ordinary runtime `String` never becomes a command fragment.

## Library versus compiler ownership

Prefer library composition when the compiler can already express and optimize it:

- stack and zipper algorithms;
- Brainfuck opcode enumeration;
- parser state machine;
- interpreter state and fuel handling; and
- book-page joining policy where normal collection operations suffice.

Compiler ownership is justified for:

- source type/operation semantics;
- target-independent Core operations needed to retain optimizable intent;
- physical representations and ABIs;
- typed Minecraft operations;
- native bulk/macro recipes; and
- legality, cost, and target-version facts.

The capstone must use the same public facilities available to users.

The detailed four-layer policy and provisional Brainfuck capability assignment are
recorded in
[`ps-2/standard-library-boundary.md`](ps-2/standard-library-boundary.md). Every PS-2
operation must be classified there before its implementation begins. In particular,
an optimized handwritten mcfunction is private target support behind a typed
intrinsic contract, not automatically public standard-library source.

## Diagnostics and limits

Add independent bounded limits for parsing, types, fields, aggregate depth, literal
elements, runtime representation tables, generated helper functions, macro
templates, and evaluator cases as their first consumers appear. Do not reuse one
large global cap.

Important owned diagnostics include:

- invalid arithmetic operands and illegal conversions;
- loop control outside a loop;
- aggregate construction/projection mismatch;
- ownership or use-after-consume errors if consuming operations are exposed;
- empty-pop policy violations;
- invalid book holder/slot/component access;
- malformed Brainfuck brackets;
- program/input limits;
- fuel exhaustion as a runtime result rather than a compiler crash; and
- no proven safe synchronous deployment bound.

## Testing rule per capability

Each dossier must leave:

1. positive and negative source fixtures;
2. typed HIR/Core/verifier tests;
3. evaluator examples and boundaries where target-independent;
4. `None`/`Baseline` Core differential evidence;
5. physical-plan and structured-target corruption tests;
6. `None`/`Baseline` Minecraft lowering evidence;
7. a vanilla scenario for every target-defined semantic claim;
8. exact or conservative cost/limit reporting;
9. scale/adversarial cases for dynamic structures; and
10. an updated ambiguity ledger for unresolved behavior.

Do not freeze entire packs for features whose valid representation is expected to
change.

## Non-goals

- persistent scheduling or yields;
- general shared references or arbitrary aliasing;
- a universal collection framework before the required operations are known;
- source-visible NBT layout;
- general runtime command construction;
- every Minecraft book/item component;
- every possible Brainfuck dialect;
- a performance-optimal representation search; and
- implementing the final capstone inside compiler tests instead of an MDL package.

## Exit criteria

- The frozen Brainfuck contract can be implemented using ordinary typed MDL.
- Every new value/control operation has verified target-independent semantics.
- Applicable pure cases execute in the Core evaluator.
- Minecraft-specific acquisition/effects execute on the pinned server.
- Aggregate/list/string storage never becomes source identity accidentally.
- All four optimization-policy combinations are semantically equivalent.
- The interpreter can be given finite program and execution bounds that produce an
  honest hard-limit compatibility result.
- Unused aggregate/list/string/macro runtime support disappears from programs that
  do not need it.
- PS-3 needs to write a program, not add hidden compiler functionality.
