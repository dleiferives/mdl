# S-017 — Mandatory Conditional Braces

**Status:** selected on 2026-07-13.

Every `if` and `else` body must be a braced block, including a body containing only
one statement:

```mdl
if (ready) {
    step();
} else {
    wait();
}
```

Unbraced bodies are syntax errors:

```mdl
if (ready)
    step(); // error: expected a braced block
```

Braces remain required when the complete conditional fits on one line:

```mdl
if (ready) { step(); }
```

An empty body is written explicitly:

```mdl
if (ready) {
}
```

This gives every conditional body one structural shape and removes dangling-`else`
ambiguity. Formatting of the braced block is a formatter concern rather than an
alternate grammar.

S-017 applies to `if` and `else`. It does not change S-007, which permits a switch arm
to contain either one simple statement or a braced block.
