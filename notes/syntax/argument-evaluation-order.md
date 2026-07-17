# S-026 — Argument Evaluation Order

**Status:** selected on 2026-07-13.

Call arguments are evaluated completely from left to right:

```mdl
consume(next_value(), next_value());
```

The first `next_value()` call and its observable effects complete before evaluation
of the second argument begins. The callee body begins only after every argument has
been evaluated.

The same ordering applies when argument expressions mix ordinary reads, calls, world
queries, and `mut` place expressions:

```mdl
update(read_state(), mut machine, read_after_machine());
```

S-026 orders evaluation of the arguments and resolution of their places. The exact
aliasing and write-back rules when multiple arguments refer to overlapping places
remain a separate decision.

The compiler may reorder, combine, or eliminate argument computations only when it
proves that the transformation is unobservable under left-to-right source semantics.
This is ordinary as-if optimization freedom, not unspecified evaluation order.

Evaluation order for future default arguments, named arguments, macro arguments, and
compile-time calls must preserve or explicitly extend this rule when those features
are designed.
