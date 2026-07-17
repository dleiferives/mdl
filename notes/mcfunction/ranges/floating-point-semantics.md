# Measured Native Floating-Point Semantics

All “Measured” results below came from the official Java 26.2 server on the isolated
loopback port. The runnable probes are in
[`test/floating_point.mcfunction`](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/floating_point.mcfunction).

## Numeric domains that meet at commands

| Domain | Effective type | What it can do |
| --- | --- | --- |
| Scoreboard | signed wrapping `i32` | General integer arithmetic and comparisons |
| Command result | signed `i32` | The numeric result channel consumed by `execute store` |
| NBT `float` | IEEE-754-like binary32 storage | Transport, copy, command/entity fields |
| NBT `double` | IEEE-754-like binary64 storage | Transport, copy, positions and other fields |
| Brigadier float/double argument | parsed scalar with optional declared bounds | Feeds a particular command; not stored as a general variable |
| Stopwatch range | floating seconds | The only true runtime command float interval comparison |

NBT does not expose add, subtract, multiply, divide, or compare commands. `data
modify ... set from` copies a numeric tag losslessly; `data get` turns it into a
lossy command result.

## Literal precision

Measured storage serialization:

| Input | Stored observation |
| --- | --- |
| `16777217f` | `1.6777216E7f` (`16777216`) |
| `9007199254740993d` | `9.007199254740992E15d` (`9007199254740992`) |
| `1.25e3f` | `1250.0f` |
| `1.25e-3d` | `0.00125d` |
| `-0.0f`, `-0.0d` | serialized as positive zero |

Replacing the stored `-0.0f` with `0.0f` reported “nothing changed”, so command NBT
does not preserve an observable signed-zero distinction in this path.

Mojang documents E notation and explicitly says that NaN and infinity literals are
not accepted in the expanded SNBT number grammar in
[25w09a](https://www.minecraft.net/ru-ru/article/minecraft-snapshot-25w09a).
On 26.2, bare `NaNf` and `Infinityf` supplied to `data ... value` became strings,
not numbers; `-Infinityf` was rejected because an unquoted string may not start
with `-`.

## `data get`: float/double to command result

Measured results:

| NBT input and scale | Command result |
| --- | --- |
| `1.9d`, scale `1` | `1` |
| `-1.9d`, scale `1` | `-2` |
| `1.25d`, scale `10` | `12` |
| `1e30d`, scale `1` | `2147483647` |
| `1e-30d`, scale `1` | `0` |
| `NaNf` produced by entity math | `0` |
| `Infinityf` produced by entity math | `2147483647` |
| `-Infinityf` produced by entity math | `-2147483648` |

This is floor after scaling, followed by saturation into signed `i32`. The negative
case is decisive: it is not truncation toward zero. Mojang's original scaled
behavior is documented in
[17w45b](https://www.minecraft.net/en-us/article/minecraft-snapshot-17w45a),
but the precise rounding/saturation results above are target-version measurements.

Any code that reconstructs a float after `data get` has already lost information.
Repeated float→result→float conversion is not a neutral transport operation.

## `execute store`: command result to NBT number

Measured results:

| Integer result and target scale | Stored value |
| --- | --- |
| `7`, float scale `0.5` | `3.5f` |
| `-7`, float scale `0.5` | `-3.5f` |
| `16777217`, float scale `1` | `16777216f` |
| `16777217`, double scale `1` | exact `16777217d` |
| `16777217`, float scale `10^38` | `Infinityf` |
| `7`, float scale `10^-50` | `0.0f` |

The command result is multiplied by the double scale and converted to the requested
NBT width. Float overflow and underflow are therefore reachable even though SNBT
does not have special-value literals.

The Brigadier double argument used for `execute store` scale did not accept E
notation (`1e38`) in a function line; the equivalent expanded decimal did. SNBT
and Brigadier numeric syntax must not share one renderer.

## Macro suffix retyping

Mojang documents that numeric macro arguments lose their original suffix. The
fixture proved that a macro template can attach another:

```mcfunction
$data modify storage mdl:observations out set value $(value)f
```

`1.25d` became `1.25f`, `1.25f` became `1.25d`, and `16777217d` became the rounded
float `16777216f`. This is useful when syntax generation is already necessary; an
ordinary `execute store` remains the pre-parsed boundary for score values.

## Internally produced NaN and infinity

Display transformation decomposition produced `0/0 → NaNf`, `1/0 → Infinityf`,
and `-1/0 → -Infinityf`.
The tags could be copied and serialized even though they could not be written as
SNBT numeric literals. `data get` mapped them to `0`, `INT_MAX`, and `INT_MIN`
respectively.

MDL therefore needs an explicit special-value policy. Options include allowing
native propagation at boundary types, rejecting non-finite values at checked
boundaries, or using a tagged software representation. Assuming “SNBT cannot spell
it, therefore it cannot occur” is incorrect.
