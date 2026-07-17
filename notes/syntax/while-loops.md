# S-020 — While Loops

**Status:** core form selected on 2026-07-13.

A pre-test loop uses `while` with a mandatory parenthesized condition:

```mdl
while (machine.tape[machine.pointer] != 0) {
    step(mut machine);
}
```

The condition must have source type `Bool` according to S-019. It is evaluated before
the first iteration and again before every subsequent iteration, so the body may
execute zero times.

The source construct describes ordinary sequential repetition. It does not select a
Minecraft implementation strategy. Lowering may use function recursion, macro
dispatch, specialized unrolling, destructive traversal of a private working value,
or another strategy that preserves the source behavior.

Likewise, S-020 does not imply that a loop may silently cross a tick boundary.
Scheduling and multi-tick execution remain explicit future language decisions.

S-021 requires braces for the initial language and reserves a fat-arrow shorthand as
a possible future extension. The following remain separate decisions:

- labels and control transfer across nested loops;
- post-test loops;
- infinite-loop syntax; and
- iteration over collections and ranges.
