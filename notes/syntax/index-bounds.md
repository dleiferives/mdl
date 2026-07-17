# S-033 — Checked List Indexing

**Status:** selected on 2026-07-13.

Ordinary list indexing is bounds-checked. For a list of length `n`, an index is valid
exactly when it lies in the range `-n .. n - 1`:

```mdl
const value: Int8 = tape[index];
tape[index] = replacement;
delete tape[index];
```

If `index` is invalid, the operation produces a defined language-level bounds
failure. It must not silently return a fabricated default value, perform a no-op, or
inherit whatever failure behavior happens to result from a particular Minecraft
command sequence.

The rule applies uniformly to reads and to every operation using an indexed place,
including assignment and `delete`. An empty list has no valid index, including `0`
and `-1`.

When both the list length and index are known sufficiently for the compiler to prove
an access invalid, the compiler reports the error during compilation. Otherwise the
generated program checks at runtime. The compiler may eliminate any bounds check it
can prove redundant.

This is a source-semantic guarantee rather than a required lowering strategy. A
backend may use command success values, generated dispatch, macros, explicit length
comparisons, or another mechanism as long as invalid access has the selected bounds
failure behavior.

The following remain separate decisions:

- the exact runtime failure and diagnostic mechanism;
- whether bounds failure terminates a function, task, or entire execution context;
- whether an explicitly unchecked indexing construct exists; and
- evaluation order when the list, index, or assigned value has effects.
