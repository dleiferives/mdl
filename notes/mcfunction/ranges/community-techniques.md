# Patterns from Public Datapacks and Libraries

## Bookshelf 4.1.0: range partitioning around native float

Pinned source: [commit `c719575`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0),
Minecraft 26.2, MPL-2.0.

Inspected:

- `bs.collection:range` and its tests;
- `bs.math:frexp`, `ldexp`, `sqrt`;
- the generated power-of-two constant list.

Lessons:

- a materialized sequence API commonly wants an exclusive stop;
- endpoint order, step sign, zero step, and overflow must be validated as one set
  of invariants;
- `data get` scaling plus score interval tests can classify a native float into
  exponent bands;
- macro dynamic indexing is effective for a static lookup table;
- retain the native float as transport and use scores only for bounded observations
  rather than pretending the score contains the whole value.

Negative finding: the inspected public range entry rejects descending ranges even
though its helper contains a negative-step branch. Empty equal endpoints are also
reported as an error rather than an empty sequence. MDL should specify these cases
instead of inheriting them accidentally.

## gm: coordinates and transformations as a float ALU

Pinned source: [commit `d68b6c9`](https://github.com/gibbsly/gm/tree/d68b6c9fd7fd56a2b9c5ebcb9c6fe0651b7f73cd),
Unlicense.

Inspected `add`, `floor`, `round`, `multiply`, `divide`, `reciprocal`, and `sqrt`.

Patterns:

- addition is performed by stacking relative execution positions and reading an
  entity's resulting `Pos`;
- floor uses `align` on the execution position;
- round composes a `+0.5` position with `align`, so its negative/tie policy must be
  examined rather than assumed;
- projective transformation decomposition supplies reciprocal and division;
- multiplication composes reciprocal and division;
- square root uses iterative approximation over these primitives.

Costs exposed by the source:

- coordinate addition/floor is limited to the usable world-coordinate range (the
  library documents roughly `-20,000,000..19,999,999` output);
- one fixed global item-display UUID and global scratch make operations
  non-re-entrant unless externally serialized;
- entity/chunk state is part of the effect;
- transformation values narrow to float.

MDL can adopt the intrinsic idea while allocating owned frames/entities and making
range/effect requirements visible.

## stdmodulesystem: decimal software float

Pinned source: [commit `d85c51c`](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/math/data/math/function/floating_point),
targeting 1.21. The repository was archived and no repository license was found, so
only abstract lessons should be reused without separate permission.

Representation: `{number:int,p:int}` means `number × 10^p`.

Inspected behavior:

- add/sub normalizes mantissas and aligns decimal exponents;
- comparison aligns representations before comparing score coefficients;
- floor has a special negative-fraction branch, acknowledging Minecraft's floor
  versus truncation hazards;
- multiplication uses a macro-generated `execute store ... double <scale>` bridge;
- division writes numerator/denominator into a display matrix and reads decomposed
  translation;
- conversion to NBT float/double dispatches decimal exponent bands through many
  generated functions.

Lesson: general software real arithmetic is possible, but its semantic contract
must state precision, rounding, exponent bounds, special values, and concurrency.

## Iris: split world coordinates

Pinned source: [commit `af3657b`](https://github.com/Aeldrion/Iris/tree/af3657b369e1ec3364f2c62da908f285c1338494),
ISC license.

Iris represents each coordinate as two scores: an integer part and a fractional
part scaled by `1,000,000`. Its README explains that a single high-scale
`execute store` over full X/Z world coordinates would overflow. The implementation
extracts/canonicalizes the components, performs local ray/hitbox arithmetic in
scores, clamps fractions when rendering coordinates, and reconstructs movement
through macro-generated command syntax.

Lesson: representation should follow proven magnitude. Split forms are often
better than lowering all reals to one global fixed scale.

## What to adopt, not copy blindly

1. Range-partitioned algorithms and lookup tables.
2. Explicit fixed/split/decimal/native representation choices.
3. Minecraft subsystems as carefully typed arithmetic intrinsics.
4. Compile-time specialization of scale and exponent bands.
5. Owned re-entrant frames, effect declarations, cleanup, and checked bounds.
6. Differential tests around negative numbers, ties, extrema, NaN, and infinity.

Licenses, target versions, global-state assumptions, and undocumented edge behavior
must be reviewed separately for every source.
