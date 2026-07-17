# S-019 — Boolean Conditions

**Status:** selected on 2026-07-13.

Every runtime `if` condition must have the exact source type `Bool`:

```mdl
if (ready) {
    step();
}
```

MDL does not implicitly interpret numbers, strings, lists, missing values, structs,
or user-defined types as true or false:

```mdl
const count: Int32 = get_count();

if (count) {
} // error: expected Bool, found Int32
```

The program writes the intended comparison explicitly:

```mdl
if (count != 0) {
    process();
}
```

There is no user-defined truthiness protocol and no second source-level `Condition`
type accepted by `if`.

This source rule does not require every boolean to be materialized as a stored value.
A Minecraft predicate may have source type `Bool` while the compiler represents and
lowers it directly as an `execute if`/`unless` condition or another target-specific
branch operation.

Boolean operators, explicit conversions to `Bool`, compile-time conditions, and
condition syntax in other control-flow constructs remain separate decisions.
