# Scoreboard Operations and Representation Semantics

## Complete 26.2 command surface

Mojang's generated 26.2 command report exposes these objective operations:

| Family | Operations |
| --- | --- |
| Lifetime | `objectives add`, `remove`, `list` |
| Display slot | `objectives setdisplay <slot> [objective]` |
| Objective metadata | `modify ... displayname`, `displayautoupdate`, `rendertype integer|hearts` |
| Default formatting | `modify ... numberformat [blank|fixed|styled]` |

It exposes these score-holder operations:

| Family | Operations |
| --- | --- |
| Literal mutation | `players set`, `add`, `remove`, `reset` |
| Arithmetic/copy | `players operation` with `= += -= *= /= %= < > ><` |
| Read/introspection | `players get`, `list [holder]` |
| Trigger | `players enable` |
| Per-score display | `players display name`, `display numberformat` |

The display operations are metadata. They should not appear in the compiler's
arithmetic IR. `enable` is tied to trigger-style gameplay objectives rather than a
general Boolean or permission primitive.

Score comparisons live under `execute if|unless score` and cover `<`, `<=`, `=`,
`>=`, `>`, plus inclusive `matches` ranges. There is no distinct scoreboard test
command in 26.2.

## Arithmetic semantics measured on vanilla 26.2

Scores are signed wrapping 32-bit integers:

```text
2147483647 + 1  -> -2147483648
-2147483648 - 1 ->  2147483647
50000 * 50000   -> -1794967296
INT_MIN / -1    -> INT_MIN
```

Division is floor division, not truncation toward zero. Remainder has the divisor's
sign and satisfies `a = (a / b) * b + (a % b)`:

```text
-7 /  3 -> -3     -7 %  3 ->  2
 7 / -3 -> -3      7 % -3 -> -2
```

Division by zero failed, wrote success/result `0`, and left the destination score
unchanged. The same case must be tested separately for remainder before treating
the failure contracts as identical.

The remaining operators are:

- `=` copy source to target;
- `<` assign the minimum;
- `>` assign the maximum; and
- `><` swap target and source.

These are mutations, not pure expressions. Pure source operations need a temporary
or an ownership proof allowing destructive reuse.

## Missing scores materialize as zero in operations

In the measured operation path, an absent source behaved as zero and became a real
zero score. Copying it set the destination to zero, and a subsequent `players get`
on the source succeeded with zero. An absent target was likewise initialized from
zero before `+=`.

This differs from plain `players get` on an absent score, which failed with
success/result zero. Consequently:

```text
score existence != numeric value is zero
```

MDL should normally provision compiler-owned score homes explicitly. It should not
depend on accidental materialization because existence is visible to selectors,
wildcards, display behavior, and raw commands.

## `*` is global across tracked holders

The target `*` is more expansive than the earlier notebook assumed. In a test with
two existing `mdl_vector` holders and twelve holders that existed only in another
objective:

```mcfunction
scoreboard players add * mdl_vector 3
```

reported fourteen targets and created an `mdl_vector` score of `3` for holders that
previously had no value in that objective. For example, `#other` changed from only
`mdl_native = 100` to also having `mdl_vector = 3`.

Therefore an objective is not by itself a closed vector membership set. `*` ranges
over the scoreboard's globally tracked holders and can materialize the selected
objective for them. Compiler use requires a closed-world proof about every tracked
holder, which is rarely available when interoperating with other data packs.

A selector target is narrower and direct multi-entity scoreboard mutation remains a
native bulk operation, but it incurs selector scan costs and only addresses entities.

The parser accepts no glob or regex form: only exact `*` is special, while names
such as `#group*`, `#group?`, and `#group[ab]` are literals. Wildcard expansion in
display, trigger, outcome-store, reset, operation-source, and team positions—and its
snapshot, alias, and partial-failure hazards—are developed in
[`scoreboard-wildcard.md`](scoreboard-wildcard.md).

## Multiple targets and sources form a Cartesian update

Both target and source `score_holder` parsers accept multiple values in 26.2. With
four target entities at `10`, four at `20`, four sources at `1`, and four at `2`:

```mcfunction
scoreboard players operation @e[tag=target] mdl_multi += @e[tag=source] mdl_multi
```

produced `22` for every first target and `32` for every second target:

```text
10 + 4*1 + 4*2 = 22
20 + 4*1 + 4*2 = 32
```

This is a Cartesian repeated update, not a zip. It can express broadcast/reduction
patterns for associative operations, but source order is dangerous for `=`, `-=`,
`/=`, `%=` and swaps. Baseline compiler recipes should require an at-most-one source
unless they intentionally model the repeated-update semantics.

## Command results

Measured command results included:

- a scalar `players operation` returned the new target value;
- a multi-target arithmetic command returned an aggregate of final target values in
  the tested cases;
- `players get` returned the score;
- failure stored success/result zero.

Aggregation semantics should be pinned per command shape before using a result as a
reduction. A more robust compiler reduction uses an explicit accumulator unless a
native result contract is both exact and cheaper.

## Representation guidance

- Use scores for hot `Int32`, normalized Boolean values, counters, comparisons, and
  fixed-point arithmetic.
- Allocate constant scores only when an operator requires a score source; fold or
  specialize constants elsewhere.
- Keep values in NBT when they are mainly transported or serialized.
- Scalarize hot numeric compound fields into scores and write back at observation
  boundaries.
- Do not use `*` as a compiler-private vector without a global holder-membership
  proof. Prefer explicit entity selectors, generated known holders, or another
  layout.
- Preserve wrapping and floor-division semantics in Core before applying algebraic
  rewrites.

## Remaining experiments

- exact result aggregation for every operator and empty target/source sets;
- source ordering for noncommutative multi-source operations;
- `%=` by zero;
- objective/holder naming and count limits;
- selector scan cost versus explicit fake-holder commands;
- criteria, trigger, display, and persistence behavior relevant to public APIs;
- interference with other data packs using the same global scoreboard service.
