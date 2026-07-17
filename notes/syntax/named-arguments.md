# S-027 — Named Arguments

**Status:** core named-argument form selected on 2026-07-13; its permitted use is
constrained by S-028.

A call may identify an optional parameter by writing a leading-dot designator, `=`,
and the argument expression:

```mdl
summon(
    position,
    .rotation = rotation,
    .silent = false
);
```

The selected form is:

```text
.parameter_name = expression
```

The leading dot distinguishes a parameter designator from a local binding. `=`
supplies the argument value, while `:` remains reserved for type annotations in this
part of the grammar. The visual form intentionally parallels S-013 struct field
initializers, but a named call remains a call rather than construction of an implicit
struct.

Named arguments are associated with parameters by name rather than declaration
position. Unknown or duplicate parameter designators are errors. Supporting named
arguments makes externally visible optional-parameter names part of a callable's
source-level API.

Arguments retain the written left-to-right evaluation order selected by S-026,
regardless of the order in which the parameters were declared:

```mdl
consume(
    value,
    .second = evaluate_first(),
    .first = evaluate_second()
);
```

Here `evaluate_first()` still executes first because its argument is written first.

S-025 and S-035 apply to named calls: commas separate arguments and a final comma is
optional.

S-028 limits named arguments to defaulted parameters and requires them to follow all
ordinary positional arguments. Required parameters cannot be named.

The following remain separate decisions:

- whether named arguments may be reordered by formatting tools;
- parameter aliases or source-compatible parameter renaming; and
- whether a defaulted parameter may use a caller-visible `mut` mode.
