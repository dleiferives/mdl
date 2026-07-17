# Range Domains in Java 26.2

## True command interval parsers

The official 26.2 `reports/commands.json` exposes only two parser types whose
argument is itself an interval:

| Parser | Command positions | Shape |
| --- | --- | --- |
| `minecraft:int_range` | `execute if/unless score ... matches`, `random value`, `random roll` | `n`, `min..max`, `min..`, `..max`; closed integer endpoints |
| `minecraft:float_range` | `execute if/unless stopwatch` | Same surface shape, floating-point seconds |

Mojang introduced score `matches` as a range comparison in
[18w02a](https://feedback.minecraft.net/hc/en-us/articles/360004167991-Minecraft-Java-Edition-Snapshot-18W02A),
the random command in
[23w31a](https://feedback.minecraft.net/hc/en-us/articles/18619031671821-Minecraft-Java-Edition-Snapshot-23w31a),
and floating stopwatch comparison in
[25w41a](https://feedback.minecraft.net/hc/en-us/articles/40290141596301-Minecraft-Java-Edition-Snapshot-25w41a).

Measured details:

- score endpoints are inclusive, including `-2147483648` and `2147483647`;
- one omitted endpoint is accepted, but bare `..` is rejected;
- inverted linear score and stopwatch ranges are rejected at macro parse time;
- `random value 3..3` and `random value 3` both execute unsuccessfully because the
  random command requires at least two possible values;
- a random result from `-2..2` was inside the closed range.

The parser type does not completely determine command semantics: score membership
accepts a singleton while `random` adds a cardinality constraint.

## Selector ranges

Selectors have their own subgrammar rather than nodes in the command report.
Mojang documented the transition from separate minimum/maximum options to range
syntax in [17w45b](https://www.minecraft.net/en-us/article/minecraft-snapshot-17w45a).

| Selector option | Domain | Special rule |
| --- | --- | --- |
| `scores={objective=range}` | signed score integer | Per-objective closed membership interval |
| `level=range` | nonnegative integer | Player-only experience level |
| `distance=range` | nonnegative double | Euclidean distance from selector origin |
| `x_rotation=range` | double angle | Normalized cyclic pitch selection |
| `y_rotation=range` | double angle | Normalized cyclic yaw selection |

The isolated probe placed entities exactly three blocks apart: both `3..3` and
`2.5..3.5` matched. A macro-generated `distance=-1..1` failed. For yaw,
`170..-170` matched `179` and rejected `0`, proving that an apparently inverted
rotation interval wraps around the cyclic seam.

`x`, `y`, and `z` are scalar selector-origin coordinates. `dx`, `dy`, and `dz`
define an axis-aligned volume. They form a spatial query, not three general-purpose
numeric intervals, and negative deltas describe boxes in the opposite direction.

## JSON/SNBT predicate bounds

Many data-driven predicates encode bounds as either a number or an object with
optional `min` and `max`. Examples include player statistics, light, collection
size/count, loot `value_check`, food saturation, and spawn conditions. Mojang's
[19w38a notes](https://feedback.minecraft.net/hc/en-us/articles/360033496792-Minecraft-Java-Edition-Snapshot-19W38B)
describe early integer predicate ranges, while
[26.1 Snapshot 1](https://feedback.minecraft.net/hc/en-us/articles/42011663817357-Minecraft-Java-Edition-26-1-Snapshot-1)
documents current integer and float min/max predicate examples.

These are declarative resources evaluated by the relevant game system. They are
valuable compilation targets for static conditions, but are not runtime values
that mcfunction can freely construct, intersect, or iterate without generating or
selecting resources.

## Scalar parsers with declared bounds

A scalar argument constrained by its parser is not a range value. The 26.2 command
report gives these representative bounds:

| Command family | Scalar domain |
| --- | --- |
| `damage ... amount` | float, minimum `0` |
| `place template ... integrity` | float, `0..1` |
| `playsound ... pitch` | float, `0..2` |
| `playsound ... minVolume` | float, `0..1` |
| `spreadplayers ... spreadDistance` | float, minimum `0`; max range minimum `1` |
| `tick rate` | float, `1..10000` |
| `time rate` / `time of clock rate` | float, `0.00001..1000` |
| `worldborder set/add ... distance` | double, `-59999968..59999968` |
| `attribute`, `data get` scale, `execute store` scale, stopwatch query scale | unbounded Brigadier double |

Other arguments use `minecraft:time`, a scalar duration converted to ticks with a
command-specific minimum. It appears in `schedule`, `tick`, `time`, `title`,
`weather`, and timed world-border commands. It is not interval syntax.

The compiler should retain a `CommandArgumentConstraint` for these bounds. It can
prove constants valid, emit runtime clamps/checks when appropriate, or reject a
source value before Minecraft's parser/handler does.

## Other things named range

- `container.*` and similar slot groups are wildcard domains, not numeric ranges.
- `execute if blocks` compares two three-dimensional regions.
- A world border is spatial state with a center and size, not an interval object.
- A list slice has ordered endpoints and often half-open semantics.
- A generated arithmetic sequence has start/stop/step and is covered separately.

Sharing the word “range” is not enough reason to give these one IR operation.
