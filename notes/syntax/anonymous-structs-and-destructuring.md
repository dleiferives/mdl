# S-041 — Anonymous Structs, Multiple Results, and Pipe Destructuring

**Status:** selected for PS-5 on 2026-07-19; not yet implemented.

The authoritative implementation design is
[`../compiler/pre-scheduler/ps-5-anon-structs-destructuring-plan.md`](../compiler/pre-scheduler/ps-5-anon-structs-destructuring-plan.md).
The exact proposed grammar delta is
[`ps-5-grammar-delta.ebnf`](ps-5-grammar-delta.ebnf).

PS-5 selects:

- structural anonymous struct types in type position: named
  `{ status: Int32, position: Int32 }` and positional `{ Int32, Int32 }`;
- context-inferred struct literals `.{ .status = 0, .position = p }` and
  `.{ 0, p }`, extending the S-013 nominal literal surface the same way PS-4's
  `.variant` extends enum values;
- consumption follows the type's shape: named values are read by `.field`
  projection, positional values by compile-time-checked `[index]` and by
  destructuring;
- the destructuring statement `|const left, right| <= expression;` with per-target
  `const`/`var`/existing-binding/`_` roles and exact arity;
- `<=` as the destructuring arrow, mirroring PS-4's `=>` (`patterns => body` sends
  control rightward; `|targets| <= value` sends the value leftward); and
- `:=` inferred declarations, implementing the S-003 reservation without changing
  S-001's explicit-annotation default.

`<-` was considered for the arrow and rejected: as a longest-match token it would
turn today's `x<-1` comparison spelling into a parse error and permanently require
spacing awareness. `<=` is already a token, and statement-start `|` makes the
position unambiguous.

Positional destructuring and indexing are deliberately not offered for named
anonymous types: reordering a named result type's fields must never silently rebind
positional callsites. A function's result type is therefore a statement about how it
is consumed — named results are projected, positional results are unpacked.

This decision relates to earlier ledger entries as follows: it implements S-003;
extends S-013 literals; keeps the S-032/S-033 checked bracket surface consistent
between static tuple indices and future runtime list indices; and records S-004
`mut` parameters as future sugar that lowers through this multiple-result ABI via
copy-in/copy-out.

By-name destructuring, target renames, nested patterns, positional access on named
types, aggregate equality, export-ABI anonymous structs, and expression-position
destructuring require another explicit decision.
