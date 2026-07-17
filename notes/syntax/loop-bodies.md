# S-021 — Loop Bodies

**Status:** mandatory braces selected on 2026-07-13; fat-arrow shorthand reserved as
a possible future extension.

Every `while` body initially requires braces, including a body containing only one
statement:

```mdl
while (ready) {
    step();
}
```

Unbraced bodies are not part of the initial grammar:

```mdl
while (ready)
    step(); // error: expected a braced loop body

while (ready) step(); // error: expected a braced loop body
```

This matches the mandatory conditional blocks selected by S-017 and gives the
initial language one structural shape for loop bodies.

## Reserved future shorthand

The following fat-arrow form is intentionally reserved for possible later adoption:

```mdl
while (ready) => step();
```

It is not currently valid MDL. If adopted later, `=>` would map the true condition
value to one simple body statement, consistent with its value/branch-to-behavior role
in switch arms. The extension would not remove or change the canonical braced form.

The exact restrictions, formatting, interaction with labels, and support for a
braced body after `=>` must be decided before enabling the shorthand.
