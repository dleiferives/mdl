# Floating-Point Representation Choices

There is no universally best representation. Choose by operations, required
precision, magnitude, world-coordinate use, and whether a value must survive as
native NBT.

## Decision table

| Need | Starting representation | Main risk |
| --- | --- | --- |
| Hot add/subtract/compare in a known bounded range | `Fixed<scale>` in scores | `i32` overflow and rescaling rules |
| Native command/entity field with little arithmetic | NBT `float` or `double` | No general ALU or comparator |
| Exact whole/fraction world coordinate over large world range | Split `{whole, fraction}` scores | Normalization and sign/carry complexity |
| Large decimal dynamic range with software arithmetic | `{coefficient:i32, exponent10:i32}` | Precision loss, many commands, global scratch if poorly framed |
| Binary-float decomposition/math specialized to float32 | NBT float plus scaled `data get`, tables, scores | Intricate range partitions and finite-value assumptions |
| Occasional division/reciprocal on native float | Display transformation decomposition | Entity/chunk state, float narrowing, re-entrancy |
| Compile-time constant | Compiler-folded literal in target syntax | Correct target-specific numeric rendering |

## Fixed point

`Fixed<1000>` stores `1.234` as score `1234`. Addition, subtraction, comparison,
min/max, and clamping are ordinary score operations when scales agree. Multiplying
two values requires a widened intermediate and one rescale; division requires
pre-scaling the numerator. Because the scoreboard has no wider integer, algorithms
must split operands, reorder operations, use NBT/entity helpers, or prove safe
bounds.

The scale belongs in the static type. Implicitly mixing `Fixed<100>` with
`Fixed<1000>` should not compile without an explicit rounding/rescale policy.

## Split coordinate

Iris keeps each axis as an integer score plus a fractional score in millionths. It
does this because multiplying a full X/Z world coordinate by a high precision scale
overflows `i32`. Separating magnitude from sub-block precision makes local geometry
possible without sacrificing world reach.

A canonical split representation must define:

- fraction interval, preferably `0 <= fraction < scale`;
- normalization after addition/subtraction;
- negative coordinates (`-1.25` may be whole `-2`, fraction `.75` under floor
  semantics);
- carry/borrow and comparison order;
- conversion to/from native coordinate syntax without unsafe raw strings.

## Decimal coefficient/exponent

The surveyed stdmodulesystem represents a number as `{number:int,p:int}`, logically
`number × 10^p`. Add/sub aligns exponents and deliberately discards low-significance
digits; multiplication and division bridge through native doubles and display
transformations. Its implementation normalizes to roughly eight decimal digits and
dispatches exponent ranges through generated functions.

This proves a broad software float is possible, but MDL should improve the ABI:
owned frames instead of global objectives/storage, documented rounding, explicit
finite/exponent bounds, and checked overflow. Decimal semantics may be attractive
for user-facing quantities but should not be mislabeled IEEE binary64.

## Native float plus integer projections

Bookshelf's current `frexp`/`ldexp` keeps the principal value as an NBT float. It
uses carefully chosen powers-of-two scales in `data get` to determine exponent
regions, score ranges to refine them, a table of powers of two, and `execute store`
to reconstruct a normalized float. Its square root composes that decomposition with
score approximations and `ldexp`.

The transferable technique is range-partitioned observation: choose a scale that
projects a useful band of a native float into `i32`, branch on that integer, then
reconstruct at a controlled precision. It is not a lossless general conversion.

## Display transformation arithmetic

An item display accepts a 4×4 float transformation matrix and exposes a decomposed
`translation` and `scale`. Setting matrix element `m03=x` and `m33=y`, then reading
`translation[0]`, yields approximately `x/y`. Reading scale from a matrix with the
projective denominator yields a reciprocal. `gm` composes reciprocal and division
to implement multiplication; stdmodulesystem uses the same division mechanism.

The 26.2 fixture measured `7/2 = 3.5f`, `1/4 = 0.25f`, `0/0 = NaNf`, and
`1/0 = Infinityf`, and `-1/0 = -Infinityf`.

Treat this as a specialized effectful intrinsic:

- it needs an entity in a loaded chunk;
- the transformation narrows to float;
- the entity and scratch storage must be owned per invocation or serialized;
- save/restore or disposable entities are required;
- NaN, infinity, and decomposition edge cases need policy;
- server-version compatibility must be tested.

It may be worthwhile for infrequent native-float division, but not as the default
implementation of every arithmetic expression.

## Representation transitions

The compiler should make these transitions explicit:

```text
score i32 --execute store(scale)--> NBT float/double
NBT number --data get(scale, floor+saturate)--> result i32 --> score/NBT integer
NBT number --data modify set from--> same native NBT type/value
fixed<S1> --checked rescale/round--> fixed<S2>
split coordinate --normalize/render--> command coordinate
software decimal --explicit conversion--> native float/double
```

Only the `set from` copy is a lossless native-number move. The other arrows carry
range, rounding, overflow, and sometimes effect constraints that belong in IR.
