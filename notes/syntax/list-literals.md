# S-031 — Contextual Bracket List Literals

**Status:** selected on 2026-07-13.

A list value is constructed with a bracket literal:

```mdl
const bytes: []Int8 = [1, 2, 3];
var empty: []Int8 = [];
```

When an expected `[]ElementType` is available, every element is checked against that
element type. The empty literal is therefore valid when its type comes from an
explicit declaration, parameter, return context, or another context that requires a
specific list type.

A nonempty literal may infer its element type for an inferred declaration:

```mdl
const bytes := [1, 2, 3];
```

All elements must resolve to one compatible element type under the language's normal
typing and conversion rules. This does not introduce an implicit heterogeneous list
type.

An empty literal cannot determine its element type by itself:

```mdl
const empty := []; // error: cannot infer the list element type
```

The literal syntax is deliberately shorter than the rejected typed compound forms
such as `[]Int8 { 1, 2, 3 }`. Type information belongs in the surrounding type
annotation when inference is insufficient.

This decision does not yet select:

- the evaluation order of element expressions;
- spread, repetition, or comprehension syntax; or
- list allocation, copying, aliasing, and storage representation.

S-035 applies the language-wide separator rule: list elements are comma-separated
and a final comma is optional, independent of layout.
