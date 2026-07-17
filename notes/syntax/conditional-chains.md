# S-018 — Conditional Chains

**Status:** selected on 2026-07-13.

Additional conditions in an `if` statement use C-style `else if` clauses:

```mdl
if (first) {
    handle_first();
} else if (second) {
    handle_second();
} else {
    handle_default();
}
```

`else if` is a dedicated continuation of the conditional chain. It is not an
unbraced `else` body and therefore does not create an exception to S-017's mandatory
braces for ordinary `if` and `else` bodies.

Conditions are considered from top to bottom. Each condition is evaluated only when
all preceding conditions in the chain were false, and exactly one selected body is
executed. The final `else` body is optional.

Every condition retains the mandatory parentheses selected by S-016, and every body
retains the mandatory braces selected by S-017.

Whether a condition must have type `Bool` or may use another truth-value conversion
is a separate decision.
