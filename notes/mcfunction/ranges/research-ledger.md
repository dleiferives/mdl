# Range and Floating-Point Research Ledger

This is the do-not-repeat-work record. “Measured” means the isolated official Java
26.2 server at `127.0.0.1:25585` produced the observation. “Report-derived” means
the official server's generated `reports/commands.json`. “Source-derived” means a
pinned public implementation was inspected. Negative results are retained.

## Vanilla interval and progression probes

| ID | Status | Probe | Observation |
| --- | --- | --- | --- |
| R01 | Measured | Score exact/closed/one-sided ranges | Exact, both closed endpoints, `min..`, and `..max` behaved inclusively |
| R02 | Measured | Score integer extrema | `INT_MIN..INT_MIN` and `INT_MAX..INT_MAX` matched |
| R03 | Negative measured | Score macro range `8..5` | Macro function failed; no child observation |
| R04 | Negative measured | Score macro range `..` | Macro function failed; at least one endpoint is required |
| R05 | Measured | Intersection | Chained interval guards returned true for `5 ∈ [3,∞) ∩ (-∞,7]` |
| R06 | Measured | Complement | `unless matches 6..8` returned true for `5` |
| R07 | Measured | Union flag | Two interval branches produced one true flag |
| R08 | Measured | Runtime score bounds | `>= lower` plus `<= upper` matched without macro parsing |
| R09 | Negative measured | Random singleton `3` / `3..3` | Handler reported range must contain at least two values; success `0` |
| R10 | Measured | Random `-2..2` | Stored sample was within the interval |
| R11 | Measured | Stopwatch broad/open float ranges | Newly created stopwatch matched `..1000` and `0.0..`; query scaled by 1000 returned small elapsed milliseconds |
| R12 | Negative measured | Stopwatch `2.0..1.0` and `..` | Macro function calls failed |
| R13 | Measured | Selector distance | Entity exactly 3 blocks away matched `3..3` and `2.5..3.5` |
| R14 | Negative measured | Selector negative distance | Macro function call with `-1..1` failed |
| R15 | Measured | Wrapped yaw `170..-170` | Matched yaw `179`, rejected yaw `0` |
| R16 | Measured | Progression `0,5,2` | Produced `[0,2,4]` with exclusive stop |
| R17 | Measured | Progression `5,0,-2` | Produced `[5,3,1]` |
| R18 | Measured | Equal progression endpoints | Produced empty list successfully |
| R19 | Negative measured | Zero progression step | Function returned failure |

## Vanilla floating-point probes

| ID | Status | Probe | Observation |
| --- | --- | --- | --- |
| F01 | Measured | `16777217f` | Rounded to binary32 `16777216f` |
| F02 | Measured | `9007199254740993d` | Rounded to binary64 `9007199254740992d` |
| F03 | Measured | SNBT E notation | Positive and negative exponents parsed |
| F04 | Measured | Negative zero | Serialized as positive zero; replacement with positive zero was a no-op |
| F05 | Negative measured/documented | SNBT NaN/infinity literals | Official grammar excludes them; bare tokens were strings, negative bare token failed |
| F06 | Measured | `data get 1.9d` | Result `1` |
| F07 | Measured | `data get -1.9d` | Result `-2`, proving floor rather than truncation |
| F08 | Measured | `data get 1.25d 10` | Result `12` |
| F09 | Measured | `data get 1e30d` | Saturated to `INT_MAX` |
| F10 | Measured | `data get 1e-30d` | Result `0` |
| F11 | Measured | Score `±7` → float at scale `.5` | Stored `±3.5f` |
| F12 | Measured | Score `16777217` → float/double | Float rounded; double retained exact integer |
| F13 | Measured | Huge/tiny `execute store` scale | Produced `Infinityf` / `0.0f` |
| F14 | Negative measured | Brigadier scale `1e38` | Function line failed to parse; expanded decimal succeeded |
| F15 | Measured | Macro suffix retyping | Double→float and float→double worked; precision narrowed as expected |
| F16 | Measured | Display matrix `7/2`, reciprocal `1/4` | Produced `3.5f`, `0.25f` |
| F17 | Measured | Display matrix `0/0`, `1/0`, `-1/0` | Produced numeric NBT `NaNf`, `Infinityf`, `-Infinityf` internally |
| F18 | Measured | `data get` internal NaN/infinities | Returned `0`, `INT_MAX`, `INT_MIN` |
| F19 | Measured | Scheduled entity cleanup | Temporary item display and force-loaded chunk were removed |

## Official report and documentation inspections

| ID | Evidence | Finding |
| --- | --- | --- |
| D01 | Report-derived | Only command interval parsers are `minecraft:int_range` and `minecraft:float_range` |
| D02 | Report-derived | `int_range` occurs at score `matches` and random value/roll |
| D03 | Report-derived | `float_range` occurs only at stopwatch conditional comparison |
| D04 | Report-derived | `minecraft:time` is a scalar duration parser with command-specific minima |
| D05 | Report-derived | Brigadier scalar float/double arguments carry bounds for damage, integrity, sound, tick/time rates, spread players, and world borders |
| D06 | Documented | Selector ranges replaced separate min/max fields in 17w45b |
| D07 | Documented | SNBT added E notation but explicitly excludes NaN and infinity in 25w09a |
| D08 | Documented | Stopwatch range compares real elapsed seconds with at most millisecond accuracy |
| D09 | Documented | Predicates widely use exact-or-`{min,max}` integer/float bounds |

## Community source inspections

| ID | Evidence | Source area | Finding |
| --- | --- | --- | --- |
| S01 | Source-derived | Bookshelf collection range | Inclusive start, exclusive stop; ascending examples tested |
| S02 | Negative source-derived | Bookshelf range entry/helper | Entry rejects descending range although helper has negative-step branch |
| S03 | Negative source-derived | Bookshelf zero step | No explicit failure; positive-direction call returns empty |
| S04 | Source-derived | Bookshelf `frexp` | Scaled `data get` plus score ranges locate binary exponent bands |
| S05 | Source-derived | Bookshelf `ldexp` | Macro table lookup supplies a power-of-two scale and reconstructs float |
| S06 | Source-derived | Bookshelf `sqrt` | Combines float decomposition, score approximation, and reconstruction |
| S07 | Source-derived | gm add/floor/round | Uses execution coordinates and `align` as arithmetic operations |
| S08 | Source-derived | gm divide/reciprocal/multiply | Uses projective display transformation decomposition |
| S09 | Source-derived | gm sqrt | Iterative approximation composed from the world/entity float primitives |
| S10 | Source-derived | std floating point | Decimal `{number,p}` representation with exponent alignment |
| S11 | Source-derived | std multiply/divide | Uses `execute store` scale and display matrix division |
| S12 | Source-derived | std conversion | Generated exponent-range dispatch converts decimal pair to native float/double |
| S13 | Source-derived | Iris coordinate representation | Whole plus millionth-scale fractional scores avoid full-world fixed-point overflow |
| S14 | Source-derived | Iris reconstruction | Clamps fraction and uses macro-rendered coordinate movement |

## Open questions

| ID | Question | Required next work |
| --- | --- | --- |
| U01 | Subnormal boundaries | Probe smallest normal/subnormal float/double, ties, and underflow rounding |
| U02 | All conversion targets | Measure float/double to byte/short/int/long and scale ordering |
| U03 | Float equality/order | Specify native special-value comparison or forbid it; stopwatch does not compare arbitrary NBT |
| U04 | Rotation pitch normalization | Probe values outside canonical angles and wrap semantics for `x_rotation` |
| U05 | Predicate interval edge rules | Build 26.2 predicates with exact/min/max/inverted/empty float and int bounds |
| U06 | Progression overflow | Add checked/saturating/wrapping modes and prove termination |
| U07 | Interval decision cost | Benchmark chained comparisons, normalized branches, dispatch, and predicates |
| U08 | Fixed-point algorithms | Compare split multiply/divide, NBT scale bridges, and entity ALU |
| U09 | Entity ALU compatibility | Differential-test signs, zero, subnormal, huge values, and 26.2 transformations |
| U10 | Re-entrant native-float intrinsic | Prototype owned entity/frame allocation and nested calls |
| U11 | Rounding contract | Define floor, trunc, ceil, nearest-even/away for every representation transition |
