# S-024 — Function Calls

**Status:** core call form selected on 2026-07-13.

An ordinary function is invoked with a parenthesized argument list:

```mdl
step(mut machine, input);
tick();
```

A call that returns a value is an expression and may appear wherever that value is
accepted:

```mdl
const value: Int32 = read(machine);
```

A call used as a simple statement requires a semicolon according to S-010. An empty
argument list is written explicitly as `()`.

The call-site `mut` marker selected by S-004 appears directly before the argument
whose corresponding parameter permits caller-visible mutation:

```mdl
update(mut machine, amount);
```

A bare function name does not invoke the function:

```mdl
tick;   // not a call
tick(); // call
```

Whether bare function names can be stored and passed as first-class values is a
future higher-order-function decision.

## Statements are not calls

Compiler-known statement syntax remains visibly distinct from invocation. In
particular, deletion uses S-011 rather than call notation:

```mdl
delete work[-1]; // selected
delete(work[-1]); // not the delete statement
```

S-025 and S-035 select comma-separated parameter and argument lists with an optional
final comma.
S-027 selects dot-designated named arguments. The following remain separate
decisions:

- default arguments;
- overload resolution;
- receiver or method-call syntax; and
- indirect and higher-order calls.
