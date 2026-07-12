# How Do We Implement Loops?

## There is no single loop lowering

The best implementation depends on the iteration domain and whether the loop body
can be summarized as a bulk command.

## Compile-time range or collection

Unroll it, then optimize the resulting region. Adjacent operations may fold into a
single command or batched NBT update. Avoid unrolling when pack-size/load-time cost
would be excessive.

## Entities

Use native command-context forking when the loop is genuinely “for each selected
entity”:

```mcfunction
execute as @a[tag=prisoner] run function mdl:prison/tick_player
```

This is syntactically one line but dynamically creates one context/function
execution per selected entity. Mojang's documented fork and command-operation limits
mean the optimizer must estimate cardinality and avoid accidental products such as
`as @e at @e`.

## NBT list: bulk operation first

Before generating a loop, ask whether a wildcard path can express the operation in
one native command. Examples to investigate include setting/removing the same field
across selected compound elements and appending a selected range.

The IR should preserve map/filter/bulk intent long enough to discover this.

## NBT list: destructive worklist

For an arbitrary dynamic list, a compiler can copy or own a working list, process an
end element, remove it, and recurse:

```text
while work is not empty:
    current = work[-1]
    process(current)
    remove work[-1]
```

Taking from the end is expected to avoid shifting the remaining list, but that is a
performance hypothesis requiring measurement. This traversal reverses order unless
the source or semantics permit it.

Advantages:

- static NBT path `[-1]`;
- no dynamic index macro;
- natural termination test.

Costs:

- copying when the original list must be preserved;
- destructive NBT writes per element;
- recursive function invocation;
- frame management for nested/recursive bodies.

## NBT list: indexed macro loop

Maintain an index, bridge it into macro storage, and use a macro-indexed path. This
preserves order and avoids copying the whole list but adds macro and index-management
cost per iteration.

This is more attractive when:

- the original list must remain intact;
- elements are large;
- indexes repeat enough to benefit from macro caching;
- iteration order matters.

## Bounded loop specialization

When a maximum trip count is known, generate a chain of pre-parsed steps with early
return. This can avoid macro indexing and general recursion while retaining a bound
on generated size.

## Loop optimizations the IR must support

- constant-trip-count unrolling;
- loop-invariant code motion;
- induction-variable simplification;
- fusion of adjacent loops over the same domain;
- fission when only part of a body can use a bulk command;
- dead-iteration elimination;
- reduction recognition;
- append batching;
- entity selector/condition fusion;
- hoisting one dynamic lookup outside the body;
- tick splitting for work that cannot fit safely in one tick.

## Control flow and `return`

Modern functions can return values and use `return run`. This can implement early
exit and tail-style recursion, but success and result propagation are observable.
The compiler's effect model must distinguish a normal value return, command failure,
and no execution context.

## Required benchmarks

- recursion/function-call overhead;
- destructive end-pop versus indexed macro traversal;
- front versus end removal by list size;
- unroll thresholds and pack reload cost;
- wildcard bulk update versus per-element loop;
- selector cardinality and nested fork products;
- early return and reductions;
- same-tick loop versus scheduled/tick-split work.

The hard-limit and continuation design is developed separately in
[command-limits-and-multi-tick.md](command-limits-and-multi-tick.md).

## Source

- [Mojang's function, operation, and fork semantics](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)

