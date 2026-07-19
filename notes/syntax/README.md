# MDL Source Syntax Decisions

This directory is the durable design ledger for MDL source syntax. It records
decisions independently of frontend implementation work so that syntax design does
not accidentally inherit a temporary IR or runtime representation.

## Grammar authority

[`grammar.ebnf`](grammar.ebnf) is the authoritative grammar for syntax accepted by
the compiler on trunk. Syntax decision notes may describe selected future work and
therefore may be ahead of the parser; they do not enter the implemented grammar until
the parser, recovery behavior, AST tests, and EBNF update land together.

Planned syntax is written as an explicit delta rather than silently mixed into the
implemented grammar. The first such document is the
[`PS-4 grammar delta`](ps-4-grammar-delta.ebnf). Every future syntax-changing stage
must update `grammar.ebnf` in the same change and add positive/negative parser tests
for the changed productions.

The decisions below were reviewed on 2026-07-13. They are working language-design
decisions: changing one requires an explicit follow-up decision, not an incidental
parser or lowering change.

Simple statements require semicolons. See S-010 for the exact selected boundary and
the constructs that remain deferred.

## Decision index

- S-001 through S-007 are recorded in this file.
- [S-008 — Definite Assignment](definite-assignment.md)
- [S-009 — Read-Only Access Through `const`](const-access.md)
- [S-010 — Statement Terminators](statement-terminators.md)
- [S-011 — Delete Statement](delete-statement.md)
- [S-012 — Struct Declarations](struct-declarations.md)
- [S-013 — Struct Literals](struct-literals.md)
- [S-014 — Struct Literal Separators](struct-literal-separators.md)
- [S-015 — Explicit Struct Field Defaults](struct-field-defaults.md)
- [S-016 — Parenthesized Conditionals](conditionals.md)
- [S-017 — Mandatory Conditional Braces](conditional-braces.md)
- [S-018 — Conditional Chains](conditional-chains.md)
- [S-019 — Boolean Conditions](boolean-conditions.md)
- [S-020 — While Loops](while-loops.md)
- [S-021 — Loop Bodies](loop-bodies.md)
- [S-022 — Loop Control](loop-control.md)
- [S-023 — Return Statements](return-statements.md)
- [S-024 — Function Calls](function-calls.md)
- [S-025 — Parameter and Argument Separators](call-list-separators.md)
- [S-026 — Argument Evaluation Order](argument-evaluation-order.md)
- [S-027 — Named Arguments](named-arguments.md)
- [S-028 — Defaulted Parameters Are Named-Only at Calls](defaulted-parameters.md)
- [S-029 — Literal Parameter Defaults Initially](parameter-default-values.md)
- [S-030 — Prefix List Types](list-types.md)
- [S-031 — Contextual Bracket List Literals](list-literals.md)
- [S-032 — Bracket Indexing with End-Relative Negative Indices](list-indexing.md)
- [S-033 — Checked List Indexing](index-bounds.md)
- [S-034 — List Length Method Intrinsic](list-length.md)
- [S-035 — Uniform Optional Trailing Commas](optional-trailing-commas.md)
- [S-036 — List Mutation Methods](list-mutation-methods.md)
- [S-037 — Indexed List Removal Returns the Element](list-remove.md)
- [S-038 — Checked Last-Element `pop`](list-pop.md)
- [S-039 — `push` Appends and Returns `Void`](list-push.md)
- [S-040 — Enums, Switches, and Inclusive Range Patterns](enum-switch-range-patterns.md)

## S-001 — Explicit declarations

**Status:** selected.

Names precede their type annotations. `:` introduces the static type of a declared
binding.

```mdl
const alpha: Int32 = 10;
var beta: Int32 = get_beta();
var result: Int32;
```

- `const` declares a binding that must be initialized immediately and cannot later
  be updated.
- `var` declares a writable binding and may include an initializer.
- `var` may be declared without an initializer. The rules for reading such a
  binding are still unresolved; see Open Questions.
- Explicit type annotations are the initial/default language policy. Inference is a
  separate, visibly marked form described by S-003.
- A declaration fixes the binding's static type for its entire lifetime.

The colon form is also used for parameter declarations:

```mdl
fn read(machine: Machine) -> Int32 {
    return machine.value;
}
```

## S-002 — Assignment

**Status:** selected.

`=` assigns to an already declared writable binding.

```mdl
var alpha: Int32 = 10;
const beta: Int32 = 20;

alpha = beta;
```

The assignment target must already exist and be writable. The assigned expression
must have the target's static type; conversions must be explicit until concrete
conversion rules say otherwise.

Assignment never redeclares a name and never changes its type. A colon therefore
cannot be used to change the type of an existing binding:

```mdl
alpha: Float32 = to_float(alpha); // error: this is not assignment
```

Code that needs the converted value declares a new binding instead:

```mdl
const alpha_float: Float32 = to_float(alpha);
```

How this rule extends from local bindings to general Minecraft-backed places is not
yet settled.

## S-003 — Inferred declarations

**Status:** syntax selected; implementation may be deferred.

`:=` declares a binding and infers its static type from the initializer.

```mdl
var alpha := beta;
const result := calculate();
```

Both `var` and `const` may use the inferred form. The inferred type is fixed at the
declaration and is subject to the same later-assignment rules as an explicitly
written type:

```mdl
var alpha := beta; // inferred as Int32

alpha = other_int; // valid when other_int is Int32
alpha = 3.14;      // error when this is not Int32
```

The surface syntax is reserved now even if the first frontend deliberately requires
explicit annotations everywhere.

## S-004 — Caller-visible mutation

**Status:** selected.

Function parameters do not permit caller-visible mutation by default. A parameter
that may update the caller's place is marked `mut` in both the declaration and the
call.

```mdl
fn step(mut machine: Machine) {
    machine.pointer += 1;
}

var machine: Machine = create_machine();
step(mut machine);
```

`mut` is a parameter mode and permission, not part of the `Machine` type. Repeating
it at the call site makes the effect locally visible and records the caller's
consent.

The argument to a `mut` parameter must be a writable place. Constants and temporary
values are rejected:

```mdl
const fixed: Machine = create_machine();

step(mut fixed);            // error: fixed is not writable
step(mut create_machine()); // error: temporary is not a writable place
```

This is a source-level effect contract, not a promise about physical pointer or copy
representation. Lowering remains free to use direct mutation, copy-in/copy-out, or
another representation that preserves the observable semantics.

## S-005 — Function result types

**Status:** selected.

A value-producing function writes its result type after `->`:

```mdl
fn current_cell(machine: Machine) -> Int32 {
    return machine.tape[machine.pointer];
}
```

The general header shape is:

```text
fn name(parameters) -> ResultType
```

`->` is the type-directed arrow: it introduces the output type of a callable. A
similar header shape may later be useful for macros or scheduled callables, but no
macro or task syntax is selected by this decision.

## S-006 — `Void`, omitted results, and `Never`

**Status:** selected; precise termination operations remain open.

A function that returns normally without producing a value may omit its result
annotation or write `-> Void` explicitly. These declarations have the same result
contract:

```mdl
fn step(mut machine: Machine) {
    machine.pointer += 1;
}

fn step_explicit(mut machine: Machine) -> Void {
    machine.pointer += 1;
}
```

`Never` is distinct from `Void`:

```mdl
fn abort(message: String) -> Never {
    // Must not return normally.
}
```

- `Void` permits normal completion and produces no value.
- `Never` promises that normal completion is impossible.
- Falling off the end of a `Never` function is an error.

Which Minecraft-level operations establish `Never` is not yet decided.

S-023 selects explicit C-style return statements and the fallthrough rules for
`Void`, value-producing, and `Never` functions.

## S-007 — Switch arms

**Status:** selected.

Switches use fat-arrow arms:

```mdl
switch (op) {
    ">" => machine.pointer += 1;

    "[" => {
        begin_loop(mut machine);
        machine.depth += 1;
    }

    else => invalid_op(op);
}
```

The source semantics are:

- require parentheses around the switch subject;
- evaluate the switch subject exactly once;
- select and execute exactly one arm;
- do not fall through implicitly;
- do not require `break` after an arm;
- permit a single statement or a braced block as the arm body;
- use `else` as the default arm; and
- initially support literal case values, leaving richer patterns for a later
  decision.

The two arrows intentionally have different working roles:

- `->` introduces a callable's output **type**;
- `=>` maps a selected **value/branch** to its behavior.

This distinction is provisional in the sense that the language may eventually find
more uses for either arrow, but switch arms remain `=>` unless explicitly changed.

### Lowering opportunity

The compiler must preserve a source switch as a switch long enough to choose an
appropriate lowering. One candidate is macro-based dynamic function dispatch: encode
the selected literal into a compiler-generated function path, call that path through
a macro, and route an absent generated case to the `else` arm.

This optimization is not part of source semantics. In particular, failure inside an
existing case body must not be confused with failure to find a case. Resource-path
encoding, reliable existence detection, macro cache behavior, and the small-switch
cost crossover still require backend design and measurement.

## Open questions outside S-001 through S-007

The following have been discussed but are not selected syntax or semantics:

- the source model and syntax for storage, entity, block, score, and other external
  Minecraft places;
- copy depth, ownership, aliasing, first-class references, and `&`/`*` syntax;
- raw mcfunction escape syntax;
- macro and compile-time evaluation syntax;
- task, scheduling, yielding, and awaiting; and
- richer patterns, tagged unions, and generic types.
