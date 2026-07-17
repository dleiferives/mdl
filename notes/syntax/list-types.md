# S-030 — Prefix List Types

**Status:** selected on 2026-07-13.

A homogeneous variable-length list type is written by prefixing its element type
with `[]`:

```mdl
var tape: []Int8;
const program: []Op = parse(source);

fn execute(ops: []Op, mut machine: Machine) {
    // ...
}
```

The colon still introduces the type annotation. `[]Int8` and `[]Op` are the actual
types written within that annotation: `[]` is part of the type's identity, not merely
a way to display an annotation and not punctuation attached to the declared name.
`Int8` and `[]Int8` are therefore distinct types.

Nested list types compose from right to left:

```mdl
var rows: [][]Int8;
```

Here `[]Int8` is a list of `Int8`, and `[][]Int8` is a list whose elements are
themselves `[]Int8` lists.

This spelling does not introduce a general angle-bracket generic syntax. Whether the
language eventually has parameterized user-defined types is a separate design.

This decision deliberately selects only the source spelling and the fact that the
list is variable-length and homogeneous. It does not yet decide:

- whether list assignment and ordinary parameter passing perform deep or shallow
  copies;
- whether lists may alias;
- whether a separate borrowed slice or view type exists;
- how list capacity and growth behave;
- how a list maps onto Minecraft storage; or
- the syntax and semantics of fixed-length arrays.

The related spelling `[length]ElementType` remains a plausible future fixed-array
form, but is not selected by this decision.
