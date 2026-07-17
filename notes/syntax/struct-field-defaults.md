# S-015 — Explicit Struct Field Defaults

**Status:** selected on 2026-07-13.

A struct field may declare an explicit default value with `=` after its type:

```mdl
struct Machine {
    pointer: Int32 = 0;
    instruction: Int32 = 0;
    tape: Tape;
};
```

A struct literal may omit exactly those fields that have declaration-site defaults:

```mdl
const machine: Machine = Machine {
    .tape = tape,
};
```

This construction is source-equivalent to explicitly supplying the defaults:

```mdl
const machine: Machine = Machine {
    .pointer = 0,
    .instruction = 0,
    .tape = tape,
};
```

A field without an explicit default remains required:

```mdl
const machine: Machine = Machine {
    .pointer = 1,
}; // error: tape has no initializer or declared default
```

Writing a field in the literal overrides its declared default. The initializer and
default expressions must type-check as the field's declared static type.

MDL does not infer universal defaults such as zero integers, false booleans, empty
lists, or recursively zeroed structs. Every omitted value must be justified by an
explicit field default.

A default does not make its field constant. Field writability still follows the
access path and binding rules in S-009.

The compiler may avoid materializing a default whose value cannot be observed, but
that is an optimization rather than a change to struct construction semantics.

The following remain separate decisions:

- whether defaults may refer to other fields or `self`;
- default-expression evaluation order;
- whether defaults must be compile-time evaluable;
- diagnostics for defaults with effects; and
- base-value construction or struct-update syntax.
