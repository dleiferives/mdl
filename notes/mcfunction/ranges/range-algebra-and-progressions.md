# Interval Algebra, Dynamic Bounds, and Progressions

## Static integer membership

For a score `x`, a static closed interval `[a,b]` lowers directly:

```mcfunction
execute if score #x mdl.reg matches 3..7 run function mdl:inside
```

One-sided intervals use `..7` and `3..`. Exact equality uses `5`; `5..5` is also a
valid score membership range.

`matches` is numeric interval matching, not regular-expression matching. It cannot
recognize decimal digits, prefixes, scoreboard-holder names, or text patterns.

## Algebraic composition

| Source operation | Score lowering |
| --- | --- |
| Intersection `A ∩ B` | Chain two `if score ... matches` conditions |
| Complement `¬A` | `unless score ... matches` |
| Difference `A \\ B` | `if A unless B` |
| Union `A ∪ B` | Separate branches into a Boolean/result flag, or normalize to one interval if adjacent/overlapping |
| Closed clamp | Compare to bounds and assign lower/upper on the two outside branches |
| Empty test | Compile-time endpoint analysis for static linear intervals |

Do not duplicate a side-effecting continuation once per union arm unless the arms
are disjoint or an execution guard prevents duplicate effects.

For unions of many static integer intervals, first sort and coalesce overlapping or
adjacent intervals. The result is a small disjoint interval set suitable for branch
generation or a decision tree. A finite dense domain may be cheaper as a generated
dispatch table or predicate resource.

## Runtime integer endpoints

The score range parser contains command text, so it is static unless a macro line
reparses it. Runtime score endpoints need no macro:

```mcfunction
execute if score #x mdl.reg >= #lower mdl.reg \
        if score #x mdl.reg <= #upper mdl.reg run function mdl:inside
```

This is safer, pre-parsed, and naturally composes with calculated endpoints. Use a
macro only when the interval must occupy a syntax-only position such as a selector
range or `matches` string. Such macro inputs must be typed rendered syntax, never
arbitrary source strings.

## Linear versus cyclic intervals

A linear interval with both endpoints requires `lower <= upper`; otherwise it is
empty or invalid according to the source API. A cyclic angle interval has a period:

- if `lower <= upper`, test the ordinary closed interval;
- if `lower > upper`, test `x >= lower OR x <= upper` across the seam.

Vanilla selector rotations implement the second form natively. MDL must preserve a
`CyclicRange<Angle>` distinction so a generic normalization pass does not swap the
endpoints and change the selected arc.

## Membership intervals are not progressions

A progression needs at least:

```text
Progression { start, stop, step, stop_inclusive? }
```

The research fixture adopts the common half-open rule:

- positive step continues while `current < stop`;
- negative step continues while `current > stop`;
- `start == stop` produces an empty list successfully;
- `step == 0` fails;
- examples: `0..<5 by 2 → [0,2,4]`, `5..>0 by -2 → [5,3,1]`.

The fixture implements this with scoreboard state, tail append to NBT, and function
recursion. It is a capability probe, not a recommendation to materialize every
range. A loop should normally remain lazy and feed its body directly.

## Community progression warning

Bookshelf 4.1.0 documents `min` inclusive and `max` exclusive. Its tests establish
`0,5,2 → [0,2,4]`. The inspected entry function rejects every `min >= max`, while
the recursive helper nevertheless contains a negative-step branch requiring
`current > max`. Consequently the public entry path cannot reach that descending
branch. It also has no explicit zero-step error; a positive-direction zero step
returns an empty result.

This is useful design evidence, not criticism to copy mechanically: progression
invariants must be checked together. Validate the sign of `step` against the
endpoint ordering, define equal endpoints, and prevent score overflow from wrapping
an otherwise terminating loop into a nonterminating one.

## Compiler representation

Recommended distinct IR nodes:

- `LinearInterval<T> { lower?: T, upper?: T, closed flags }`;
- `CyclicInterval<Angle> { lower, upper, period }`;
- `IntervalSet<T>` as normalized disjoint intervals;
- `Progression<Int> { start, stop, step, inclusion }`;
- `SpatialBox` and `SelectorQuery` for world-space selection;
- `ArgumentConstraint<T>` for a command parser's accepted scalar domain.

Range analysis can then prove emptiness, combine guards, eliminate redundant
checks, select a fixed-point scale, prevent overflow, and specialize bounded loops.
