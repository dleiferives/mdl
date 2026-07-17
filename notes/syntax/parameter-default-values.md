# S-029 — Literal Parameter Defaults Initially

**Status:** selected on 2026-07-13; future comptime expansion reserved.

In the initial language, a parameter default must be a literal value:

```mdl
fn step(
    mut machine: Machine,
    budget: Int32 = 64,
    trace: Bool = false
) {
    // ...
}
```

Defaults may not initially call functions, inspect another parameter, read runtime
state, or otherwise evaluate a general expression:

```mdl
fn slice(
    data: List,
    end: Int32 = data.length
) {} // error: parameter-dependent default

fn connect(
    address: String,
    timeout: Int32 = configured_timeout()
) {} // error: call in default
```

`const` means that a binding cannot be reassigned; it does not by itself mean that
the initializer is evaluated at compile time. Consequently, an ordinary `const`
name is not automatically valid as a parameter default.

The precise literal forms admitted here follow the language's literal syntax and
type-check against the parameter type. This decision does not yet define constant
folding or a general constant-expression grammar.

A future comptime system may widen the right-hand side from literals to explicitly
compile-time-evaluated expressions without changing the declaration form
`name: Type = value` or the named-only call rule in S-028.
