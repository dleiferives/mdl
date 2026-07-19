# S-040 — Enums, Switches, and Inclusive Range Patterns

**Status:** selected for PS-4 on 2026-07-19; not yet implemented.

The authoritative implementation design is
[`../compiler/pre-scheduler/ps-4-enums-switch-ranges-plan.md`](../compiler/pre-scheduler/ps-4-enums-switch-ranges-plan.md).
The exact proposed grammar delta is
[`ps-4-grammar-delta.ebnf`](ps-4-grammar-delta.ebnf).

PS-4 selects:

- `const Name = enum { ... };` for closed fieldless nominal enums;
- `Type.variant` and context-inferred `.variant` values;
- `switch (subject) { ... }` with `=>` prongs and no fallthrough;
- expression prongs containing expressions and statement prongs containing blocks;
- comma-separated patterns sharing one prong;
- inclusive `a...b` integer patterns;
- `else` as the only catch-all; and
- compile-time errors for overlap and non-exhaustiveness.

This refines and supersedes the provisional switch-body/pattern scope in S-007.
In particular, PS-4 does not admit arbitrary unbraced statements as statement-switch
arms, and it adds enum and closed integer-range patterns. Later syntax for guards,
captures, payload variants, destructuring, open-ended ranges, and labeled switches
requires another explicit decision.

MDL's inclusive source spelling maps directly in meaning, but not text, to the
Minecraft score-range spelling:

```text
MDL:       0...20
Minecraft: matches 0..20
```

Dispatch layout and datapack size are not source syntax or semantics. PS-4 uses one
mechanical lowering; later benchmarks may select a different equivalent target
shape.
