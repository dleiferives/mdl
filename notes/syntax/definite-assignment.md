# S-008 — Definite Assignment

**Status:** selected on 2026-07-13.

A `var` declaration may omit its initializer:

```mdl
var alpha: Int32;
```

This does not create a default value or expose a runtime "missing" value. Instead,
the binding begins in a compile-time uninitialized state. Every read must occur only
where the compiler can prove that the binding has been assigned on every reachable
path.

```mdl
var alpha: Int32;

alpha = get_alpha();
use(alpha); // valid
```

Reading before assignment is an error:

```mdl
var alpha: Int32;
use(alpha); // error: alpha is not definitely assigned
```

## Control flow

A binding is definitely assigned after a branch only when every branch that can
continue past the conditional assigns it:

```mdl
var alpha: Int32;

if (condition) {
    alpha = 10;
} else {
    alpha = 20;
}

use(alpha); // valid
```

An assignment in only one branch is insufficient:

```mdl
var alpha: Int32;

if (condition) {
    alpha = 10;
}

use(alpha); // error: alpha may be uninitialized
```

An assignment that occurs only inside a loop does not establish definite assignment
after the loop unless the compiler can prove that the assignment executes before
the loop exits.

## Reads and mutable calls

An operation that reads the old value requires prior definite assignment. This
includes compound assignment and passing the binding to the currently selected
`mut` parameter mode:

```mdl
var alpha: Int32;

alpha += 1;       // error: reads alpha before assignment
update(mut alpha); // error: mut is in/out, not write-only
```

A future write-only or `out` parameter mode, if the language gains one, would be a
separate decision.

## Rationale and lowering boundary

Definite assignment prevents accidental reads without requiring hidden zeroing
commands. Ordinary MDL values therefore do not inherit the runtime absence behavior
of scoreboards or NBT paths. The compiler may choose any physical representation as
long as a well-typed program observes the source-level assignment rules above.

`const` declarations remain unaffected: S-001 requires every `const` to have an
initializer at its declaration.
