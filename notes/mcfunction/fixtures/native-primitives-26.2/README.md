# Vanilla 26.2 Native-Primitive Research Pack

This is a research fixture, not MDL runtime code. It probes native scoreboard,
NBT/list/compound, macro-index, callback, map/filter/fold, range/progression, native
floating-point, and recursive worklist behavior on the official Java 26.2 server.

Keep the server world outside the repository and bind it to a non-default loopback
port. For example:

```properties
server-ip=127.0.0.1
server-port=25585
enable-rcon=false
enable-query=false
enable-status=false
```

Copy `datapack/` into the disposable world's `datapacks/` directory, start or
reload the server, then run:

```mcfunction
function mdl_native:test/scoreboard_semantics
function mdl_native:test/nbt_semantics
function mdl_native:test/nbt_dictionaries
function mdl_native:test/nbt_compound_reflection
function mdl_native:test/range_semantics
function mdl_native:test/floating_point
function mdl_native:test/list_algebra
function mdl_native:test/higher_order
function mdl_native:test/scale
data get storage mdl:observations
data get storage mdl:observations ranges
data get storage mdl:observations floats
data get storage mdl:hof results
```

`test/list_algebra` records the exact ordering, bounds, empty-selection,
deep-copy, wildcard-set, NBT matching, no-op-set equality/count idioms,
filtered-path, array type-checking, and basic stack/queue/reverse behavior used by
the list-operation research notes. The queue probe includes both a shifting
front-pop and an amortized two-stack queue.

`test/nbt_dictionaries` records native compound CRUD/merge results, dynamic-key
path encoding, structural type-probe limits, direct int-array filters, and the
snapshot-comparison primitive used by change observers. The separate
`test/nbt_compound_reflection` probe attempts Bookshelf's serialization side
channel for recovering compound keys. The isolated 26.2 run retained an unresolved
NBT text component instead of producing the expected structured `extra` list, so
the fixture deliberately preserves this negative/compatibility result. It also
adapts Compound Key Reader's sign-based boundary, which did resolve the compound
into a structured text token tree on 26.2. The temporary block and force-loaded
chunk used by this probe are removed before it returns.

The deeper wildcard probe is deliberately separate because it begins with
`scoreboard players reset *` and destroys every score in the disposable world:

```mcfunction
function mdl_native:test/scoreboard_star_destructive
data get storage mdl:observations scoreboard_star
```

Never run that function in a world containing score data you care about.

`test/scale` constructs and doubles 1,000 integers through same-tick tail-style
function recursion. It proves capability and should not be interpreted as a safe
production work budget.

The fixture deliberately keeps callback IDs and dynamic path arguments visible.
Production lowering must validate and encode these macro syntax values; arbitrary
source strings are unsafe.

`test/range_semantics` records closed score ranges, interval composition, runtime
bounds, invalid macro ranges, random cardinality, ascending/descending half-open
progressions, selector distance, wrapped rotation, and stopwatch float ranges.
`test/floating_point` records binary32/binary64 precision, signed zero, scientific
SNBT, `data get` floor/saturation, `execute store` narrowing/overflow/underflow,
macro suffix conversion, and display-transformation division/NaN/infinities.

The selector and display probes force-load isolated chunk `0,0`, then schedule
entity creation and measurement across ticks because freshly summoned entities are
not selectable in the same server tick in this no-player fixture. They remove both
their temporary entities and force load. Wait a few ticks before reading their
observations.
