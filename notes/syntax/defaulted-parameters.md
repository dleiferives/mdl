# S-028 — Defaulted Parameters Are Named-Only at Calls

**Status:** selected on 2026-07-13.

A function declares required parameters first, followed by an optional suffix of
parameters with explicit default values:

```mdl
fn step(
    mut machine: Machine,
    budget: Int32 = 64,
    trace: Bool = false
) {
    // ...
}
```

Required parameters are supplied positionally and in declaration order. A defaulted
parameter may be omitted; when the caller supplies one explicitly, it must use the
named-argument form selected by S-027:

```mdl
step(mut machine);
step(mut machine, .budget = 32);
step(mut machine, .trace = true);
step(mut machine, .trace = true, .budget = 32);
```

Consequently, every positional argument appears before every named argument. The
following calls are errors:

```mdl
step(.machine = machine);          // error: a required parameter cannot be named
step(mut machine, 32);             // error: a defaulted parameter is named-only
step(.trace = true, mut machine);  // error: positional argument after named argument
```

Named optional arguments may be written in any order. They still evaluate in their
written left-to-right order under S-026. An omitted parameter receives its declared
default. An unknown name, a duplicate name, or an explicitly supplied optional value
with the wrong type is an error.

The declaration-side `=` is consistent with struct field defaults: it supplies a
value and does not declare a type. `:` remains the type annotation delimiter.

This design deliberately avoids using named arguments as a general alternative to
ordinary positional calls. Names exist at the call site to make optional choices
clear and to allow callers to skip one default while overriding another.

The following remain separate decisions:

- when and where default expressions are evaluated;
- what names and earlier parameters a default expression may reference;
- whether default expressions must be compile-time values or may have effects;
- whether defaulted parameters may use `mut`; and
- compatibility rules for renaming or reordering optional parameters.
