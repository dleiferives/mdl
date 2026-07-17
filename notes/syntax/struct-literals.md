# S-013 — Struct Literals

**Status:** core form selected on 2026-07-13. Entry separators are selected by S-014,
and omitted-field behavior is selected by S-015.

A struct value is constructed by naming its type and providing a braced list of
field designators:

```mdl
const machine: Machine = Machine {
    .pointer = 0,
    .instruction = 0,
    .tape = tape,
};
```

The selected field-initializer form is:

```text
.field = expression
```

The leading dot makes the field designator distinct from an ordinary local binding.
`=` supplies the field's value, while `:` remains the punctuation used to introduce
a type annotation in declarations.

A struct literal is an expression. The final semicolon in the example terminates the
surrounding `const` declaration; it is not part of the literal itself.

This decision does not introduce Zig's model of anonymous struct types bound as
compile-time values. `Machine` remains the dedicated named type declared by S-012.

The following remain separate decisions:

- shorthand when a local binding has the same name as a field;
- destination-inferred literals such as `.{ .pointer = 0 }`; and
- positional or constructor-call initialization as additional forms.
