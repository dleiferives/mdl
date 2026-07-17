# S-038 — Checked Last-Element `pop`

**Status:** selected on 2026-07-13.

`pop()` removes and returns the final element of a writable list:

```mdl
const operation: Op = work.pop();
```

It accepts no arguments and has the same source semantics as:

```mdl
const operation: Op = work.remove(-1);
```

The result type is the list's element type. The receiver must be writable under
S-036.

Calling `pop()` on an empty list produces the checked bounds failure selected by
S-033. It does not return an optional value, fabricate a default, or silently perform
a no-op.

The returned value may be ignored:

```mdl
work.pop();
```

When the result is dead, the compiler may omit materializing or copying it. Code that
only intends to discard the last element may instead state that directly:

```mdl
delete work[-1];
```

A future non-failing operation, if needed, must have a distinct name such as
`try_pop` or `pop_or`; it does not change the semantics of `pop()`.

Copy-versus-move behavior for compound and nested-list elements remains part of the
general value and ownership design.
