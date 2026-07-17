# S-032 — Bracket Indexing with End-Relative Negative Indices

**Status:** selected on 2026-07-13.

List elements are accessed with an integer expression in brackets:

```mdl
const first: Int8 = tape[0];
const current: Int8 = tape[machine.pointer];
```

A negative index counts backward from the end of the list:

```mdl
const last: Int8 = tape[-1];
const previous: Int8 = tape[-2];
```

For a list of length `n`, a negative index `i` refers to the same element as
`n + i`. Therefore `-1` is the last element and `-n` is the first element.

This rule applies to integer expressions evaluated at runtime as well as negative
literals:

```mdl
const value: Int8 = tape[index];
```

If `index` is negative at runtime, it is interpreted relative to the list's current
length. Negative indexing is source-language behavior, not merely a direct spelling
for the subset of indices accepted by a Minecraft command. Lowering may use literal
NBT indices, macros, arithmetic normalization, or another strategy while preserving
the same result.

Indexing may also identify an element place for operations that require one:

```mdl
tape[index] = value;
delete tape[-1];
```

Such writes are allowed only through a writable access path under S-004 and S-009.
The indexed expression's element type is the element type carried by `[]ElementType`.

This decision does not yet determine:

- what happens when an index is outside `-length .. length - 1`;
- whether an explicit unchecked indexing operation also exists;
- the evaluation order between the list expression and index expression; or
- the lowering and cost model for dynamically indexed Minecraft storage.
