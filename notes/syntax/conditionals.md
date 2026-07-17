# S-016 — Parenthesized Conditionals

**Status:** core statement form selected on 2026-07-13.

An `if` statement places its condition in mandatory parentheses and uses a braced
body:

```mdl
if (ready) {
    step();
} else {
    wait();
}
```

The parentheses are part of the grammar rather than an optional formatting choice:

```mdl
if ready {
    step();
} // error: missing condition parentheses
```

`else` is optional when no fallback behavior is required:

```mdl
if (ready) {
    step();
}
```

S-017 requires braces even for a single-statement body, S-018 selects C-style
`else if` chains, and S-019 requires conditions to have source type `Bool`. The
following remain separate:

- whether `if` may be used as a value-producing expression; and
- compile-time conditional syntax.

S-007 uses the same parenthesized-subject convention for `switch`.
