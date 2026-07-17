# S-014 — Struct Literal Separators

**Status:** selected on 2026-07-13; amended by the uniform rule in S-035.

Struct-literal field initializers are separated by commas. Under S-035, a comma after
the final initializer is optional:

```mdl
const machine: Machine = Machine {
    .pointer = 0,
    .instruction = 0,
    .tape = tape,
};
```

The same literal is valid without the final comma:

```mdl
const machine: Machine = Machine {
    .pointer = 0,
    .instruction = 0,
    .tape = tape
};
```

The comma belongs to the field-initializer list. The semicolon after `}` terminates
the surrounding declaration according to S-010.

Newlines remain whitespace and cannot replace commas:

```mdl
const machine: Machine = Machine {
    .pointer = 0
    .instruction = 0
}; // error: missing commas
```

An empty literal has no entry and therefore no comma:

```mdl
const empty: Empty = Empty {};
```
