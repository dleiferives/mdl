# S-023 — Return Statements

**Status:** selected on 2026-07-13.

A value-producing function returns a value with `return expression;`:

```mdl
fn add_one(value: Int32) -> Int32 {
    return value + 1;
}
```

The returned expression must have the function's declared result type. Conversions
must be explicit unless a later conversion rule states otherwise.

A `Void` function may return early with `return;`:

```mdl
fn process(value: Int32) {
    if (value < 0) {
        return;
    }

    consume(value);
}
```

Both forms are simple statements and require semicolons according to S-010.

Reaching the closing brace is an implicit `return;` for a function whose result is
omitted or explicitly `Void`. Reaching the closing brace of a value-producing
function is an error unless control-flow analysis proves that point unreachable.
S-006 separately forbids normal completion of a `Never` function.

MDL does not use an implicit final-expression return. Evaluating an expression as the
last statement in a block does not return it.

The following remain separate decisions:

- multiple return values;
- returning references or places;
- interaction with future suspendable or scheduled functions;
- `return` from macro or compile-time bodies; and
- whether any expression form may itself have type `Never`.
