# Range and Floating-Point Survey

This directory separates the several unrelated things Minecraft calls a “range”
and records how real-number representations can be composed with them. The target
is vanilla Java 26.2, data-pack format 107.1.

The most important conclusion is that MDL needs more than one range type:

- an inclusive linear interval used for membership tests;
- a cyclic rotation interval where a lower endpoint greater than the upper endpoint
  deliberately crosses the `-180/180` seam;
- a progression with start, exclusive stop, nonzero step, and direction;
- a parser constraint attached to a scalar command argument;
- a spatial box or selector query, which is not merely a pair of numbers.

Likewise, Minecraft has native `float` and `double` storage, but not a general NBT
floating-point ALU. A source-level `Float` may therefore lower to an NBT binary
float, fixed-point score, coefficient/exponent pair, split coordinate, or a
specialized boundary value depending on its uses.

## Notes

- [Range domains in Java 26.2](range-domains.md)
- [Interval algebra, dynamic bounds, and progressions](range-algebra-and-progressions.md)
- [Measured native floating-point semantics](floating-point-semantics.md)
- [Floating-point representation choices](floating-point-representations.md)
- [Patterns from public datapacks and libraries](community-techniques.md)
- [Research and negative-result ledger](research-ledger.md)

## Immediate MDL rules

1. Do not normalize every `min > max` range by swapping its endpoints. That is an
   invalid linear interval but a valid wrapped rotation interval.
2. Keep inclusivity in the type. Minecraft command ranges use closed endpoints;
   common sequence APIs use an exclusive stop.
3. Lower static score membership to `matches`; lower runtime endpoints to score
   comparisons. A macro is only needed when command syntax itself must vary.
4. Represent union, intersection, complement, emptiness, and clamping in the IR,
   then select command compositions appropriate to the value carrier.
5. Treat every `data get` bridge as a lossy signed-`i32` observation. It floors,
   saturates, and discards native float precision.
6. Use fixed point for hot arithmetic unless the required range/precision says
   otherwise. Use NBT float/double primarily for transport and command boundaries.
7. Track overflow, underflow, NaN, infinity, signed-zero policy, and rounding mode
   as semantics—not incidental implementation details.

## Sources inspected

| Source | Snapshot | Transferable lesson |
| --- | --- | --- |
| [Official Java 26.2 command report](../fixtures/native-primitives-26.2/README.md) | Generated from the official 26.2 server jar | Exact command parser kinds and declared scalar bounds |
| [Bookshelf `bs.collection` and `bs.math`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules) | v4.1.0, 2026-06-16, 26.2, MPL-2.0 | Exclusive-stop progression and native-float decomposition through scaled `data get` |
| [`gm`](https://github.com/gibbsly/gm/tree/d68b6c9fd7fd56a2b9c5ebcb9c6fe0651b7f73cd) | 2024-11-22, Unlicense | Coordinates and display transformations as a float ALU |
| [stdmodulesystem floating point](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/math/data/math/function/floating_point) | 2025-01-05, 1.21, archived; no repository license found | Decimal coefficient/exponent software float and display-matrix division |
| [Iris](https://github.com/Aeldrion/Iris/tree/af3657b369e1ec3364f2c62da908f285c1338494) | 2025-05-29, ISC | Split world coordinates into integer and millionth-scale fractional scores |

Community sources are evidence about designs and failure modes. Their code and
licenses are not interchangeable.
