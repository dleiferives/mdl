# PS-2 Arithmetic and Same-Tick Control Flow

Status: **implemented and covered by the four-policy pinned-server rehearsal**

## Goal

Provide ordinary scalar computation and loops that lower to verified synchronous
Core CFGs. This is language functionality, not a miniature scheduler.

## Semantic scope

Audit the current scalar instruction set before adding syntax. Brainfuck requires at
least:

- `Int32` addition and subtraction;
- equality and ordered comparisons for counters/state;
- remainder or an explicit wrapping-byte normalization operation;
- Boolean conjunction/negation as needed by loop conditions;
- assignment to mutable source locals through SSA updates;
- `while` or equivalent loop;
- `break` and `continue`; and
- typed opcode dispatch, likely a closed enum/match client once aggregate/sum
  semantics are chosen.

Do not define signed overflow, negative remainder, division by zero, or conversion
behavior by whatever the scoreboard happens to do. Freeze MDL semantics, then make
the target recipe implement them or reject unsupported operations.

The Phase-1 surface is now frozen as follows:

- `+%` and `-%` are explicit left-associative wrapping `Int32` operators;
- plain `+` and `-` remain reserved rather than acquiring implicit overflow rules;
- comparisons retain their existing signed `Int32` semantics;
- division and remainder are not in the initial slice, so their zero and negative
  cases are not accidentally specified;
- a Brainfuck cell is a normalized `Int32` in `0..=255`; the library implements
  increment/decrement with boundary branches (`255 -> 0`, `0 -> 255`) instead of a
  target-dependent remainder trick; and
- expressions evaluate left-to-right.

## Source and CFG model

The provisional loop shape is conventional:

```mdl
while (condition) {
    body;
}
```

HIR retains a structured condition/body region and explicit `break`/`continue`
terminators. Nesting binds loop control to the innermost loop. Core lowers it to
header/body/exit blocks with block parameters for values assigned before the loop.
Values first assigned in a body cannot become definitely assigned after a possibly
zero-iteration loop. No `LoopInstruction`, physical program counter, or mcfunction
name enters semantic Core merely to simplify lowering.

`break` and `continue` cannot cross an outlined `run` boundary. A `run` body is an
independent generated function, so its loop-control context begins empty.

Evaluation order is source order. The condition is evaluated once per attempted
iteration. A body mutation cannot cause an `else`-like path to reevaluate the old
condition accidentally.

## Target strategies

The safe baseline may use existing function/return dispatch and CFG layout. Later
legal alternatives include:

- complete unrolling for tiny constant trip counts;
- partial unrolling under pack-growth limits;
- native selector traversal for entity iteration;
- native NBT bulk operations;
- recursive/dispatcher loops;
- list-tail worklists; and
- recognized reductions.

Recipe choice is not source-visible. Every retained cycle carries semantic and
work-bound facts separately.

## Analysis boundary

Distinguish:

```text
terminates for this semantic input
finite upper trip bound proven
hard sequence compatibility proven
soft budget satisfied
```

A loop can terminate at runtime without a compile-time finite bound. PS-2 accepts
such Core semantics, while compilation/deployment policy decides whether synchronous
Minecraft lowering may proceed. Brainfuck's fuel creates a finite source-level
bound; target work analysis must still account for the cost of one iteration.

## Required evidence

- arithmetic identity and boundary evaluator tests;
- verifier corruption for operand/result types and invalid CFG transfers;
- zero, one, many, nested, `break`, `continue`, and early return loops;
- mutable values carried through multiple backedges;
- None/Baseline loop semantic differential;
- target recipe and cost-report assertions without freezing block/function names;
- vanilla execution at safe command-limit boundaries; and
- explicit rejection or unknown reporting for an unbounded workload under strict
  deployment policy.

## Non-goals

- suspension or yield inside a loop;
- preserving execution context across ticks;
- arbitrary iterator/trait design before collections need it;
- advanced loop optimization before baseline correctness; and
- interpreting a target command result as language loop control without a typed
  conversion.

## Implemented evidence

- `ps2_wrapping_arithmetic.mdl` crosses HIR, Core, and native `-=` pack output;
- `ps2_structured_loops.mdl` covers carried mutation, `continue`, and `break` under
  all four optimization policies;
- `ps2_break_outside_loop.mdl` fixes the negative diagnostic contract; and
- `ps2_arithmetic_control_flow.rs` compares zero/one/many iteration results before
  and after Core baseline optimization with the bounded Core evaluator.

The remaining PS-2A evidence item is a pinned vanilla boundary scenario for a
bounded loop near the configured synchronous sequence limit.

## Design references

- [Rust loop expressions](https://doc.rust-lang.org/stable/reference/expressions/loop-expr.html)
- [Zig wrapping arithmetic operators](https://ziglang.org/documentation/0.15.2/)
- [MLIR structured control-flow dialect](https://mlir.llvm.org/docs/Dialects/SCFDialect/)
