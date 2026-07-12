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

## NBT integer to NBT float

Use `data get` as the command result and store it using the target numeric type:

```mcfunction
execute store result storage mdl:runtime out float 1 run data get storage mdl:runtime input
```

The exact rounding/truncation path for non-integral sources and large values must be
tested on 26.2.

## Macro numeric retyping

Because numeric macro arguments are inserted without their original NBT suffix, a
template can potentially attach a new suffix:

```mcfunction
$data modify storage mdl:runtime out set value $(value)f
```

Status: **Candidate**. Mojang documents suffix removal for macro numeric arguments,
but this use still needs range, formatting, cache, and special-value tests. For a
simple conversion, the ordinary `execute store` form is likely the better baseline.

## Do not implement general Float arithmetic in NBT

NBT stores floats but does not provide a general arithmetic instruction set. Moving
every addition through NBT would create expensive command sequences.

Initial recommendation:

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

## Required tests

- negative values and zero;
- integer boundaries and scoreboard overflow behavior;
- float precision boundaries;
- conversion scale and rounding direction;
- NaN and infinities, if SNBT accepts or commands can produce them;
- macro suffix conversion versus `execute store` cost;
- fixed-point multiply/divide algorithms.

## Sources

- [Mojang's macro numeric suffix change](https://feedback.minecraft.net/hc/en-us/articles/19703470383757-Minecraft-Java-Edition-1-20-2)
- [Mojang's documented scaled `data get` behavior](https://www.minecraft.net/en-us/article/minecraft-snapshot-17w45a)

