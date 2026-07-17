# How Do We Convert an Int to a Float?

## First distinguish three meanings

1. Change an NBT numeric tag's storage type.
2. Enter a floating-point computation domain.
3. Format a number as command syntax or text.

These are different source-language operations and should not share one lowering.

## Scoreboard integer to NBT float

Use the target type and scale of `execute store`:

```mcfunction
execute store result storage mdl:runtime out float 1 run scoreboard players get #value mdl.reg
```

For a fixed-point integer where `1000` represents `1.0`:

```mcfunction
execute store result storage mdl:runtime out float 0.001 run scoreboard players get #value mdl.reg
```

This is the preferred boundary conversion candidate: one ordinary pre-parsed
command, no macro.

## NBT number to NBT float through the result channel

Use `data get` as the command result and store it using the target numeric type:

```mcfunction
execute store result storage mdl:runtime out float 1 run data get storage mdl:runtime input
```

This path is lossy. The 26.2 probes establish that `data get` multiplies and floors,
then saturates into signed `i32`: `1.9 → 1`, `-1.9 → -2`, `1.25 × 10 → 12`, and
`1e30 → INT_MAX`. The final store converts that integer result to the requested NBT
type. Use `data modify ... set from` instead when the source is already the desired
native numeric type and the value must be copied without this projection.

## Macro numeric retyping

Because numeric macro arguments are inserted without their original NBT suffix, a
template can potentially attach a new suffix:

```mcfunction
$data modify storage mdl:runtime out set value $(value)f
```

Status: **Measured on 26.2**. Double→float and float→double suffix templates worked,
including expected binary32 narrowing at `16777217`. For a simple score conversion,
the ordinary `execute store` form remains the better pre-parsed baseline.

## Do not implement general Float arithmetic in NBT

NBT stores floats but does not provide a general arithmetic instruction set. Moving
every addition through NBT would create expensive command sequences.

Recommendation:

- represent computation-heavy real numbers as fixed-point scoreboard integers;
- carry a scale in the static type, such as `Fixed<1000>`;
- perform add/subtract and carefully scaled multiply/divide in scores;
- convert to NBT float/double only at Minecraft command boundaries;
- use a true NBT float when the value is primarily transported, not computed.

The type checker can prevent mixing incompatible scales, and range analysis can
detect possible 32-bit scoreboard overflow.

## Constant conversions

Compile-time constants should be converted by the compiler and emitted in the final
required syntax. They need no runtime command.

## Measured and remaining tests

- Measured: negative values, zero, float/double precision boundaries, scale,
  floor/saturation, macro suffix conversion, underflow, overflow, NaN, and both
  infinities produced through display transformations.
- Remaining: every numeric target width, subnormal/tie boundaries, performance,
  fixed-point multiply/divide algorithms, and an explicit source-language rounding
  and non-finite-value policy.

The full observations and representation alternatives are in
[`ranges/floating-point-semantics.md`](ranges/floating-point-semantics.md) and
[`ranges/floating-point-representations.md`](ranges/floating-point-representations.md).

## Sources

- [Mojang's macro numeric suffix change](https://feedback.minecraft.net/hc/en-us/articles/19703470383757-Minecraft-Java-Edition-1-20-2)
- [Mojang's documented scaled `data get` behavior](https://www.minecraft.net/en-us/article/minecraft-snapshot-17w45a)
