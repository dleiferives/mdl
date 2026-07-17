# S-035 — Uniform Optional Trailing Commas

**Status:** selected on 2026-07-13; supersedes the conflicting final-comma rules in
S-014 and S-025.

Every comma-separated construct permits one optional comma after its final item. The
rule does not depend on whether the construct is written on one line or many lines.

All four of these forms are valid:

```mdl
const short := [1, 2, 3];
const short_with_final := [1, 2, 3,];

const tall := [
    1,
    2,
    3
];

const tall_with_final := [
    1,
    2,
    3,
];
```

The rule applies uniformly to every currently selected comma-separated list,
including:

- function parameters;
- positional and named call arguments;
- struct-literal field initializers; and
- list-literal elements.

Future syntax that chooses comma-separated items inherits this rule unless an
explicit later decision changes the language-wide convention.

Commas between adjacent items remain mandatory. A newline is whitespace, not a
separator, and never changes whether a final comma is accepted:

```mdl
const invalid := [
    1
    2
]; // error: missing comma between elements
```

An empty construct contains no comma:

```mdl
const empty := [];
call();
const value: Empty = Empty {};
```

More than one final comma is invalid. Formatter policy may prefer a particular form,
but formatting does not alter the accepted grammar.
