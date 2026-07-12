# How Do We Access a Dynamic Field or Index?

## Static fields should never be dynamic

For a typed struct expression such as:

```mdl
cell.user
```

the compiler knows `user` at compile time. It should emit a direct NBT path, a direct
score slot, or no access at all after scalar replacement. A macro is unnecessary.

The expensive case is a runtime key or index:

```mdl
prison.cells[input.id]
record[field_name]
values[index]
```

## Candidate 1: macro-indexed NBT path

```mcfunction
# index is an integer in the macro argument compound
$data modify storage mdl:runtime result set from storage mdl:heap prison.cells[$(index)]
```

Advantages:

- compact generated pack;
- direct expression of the desired access;
- supports truly runtime indexes.

Costs:

- scoreboard indexes need a storage bridge;
- macro cache misses cause command reparsing;
- bounds failure is observable;
- arbitrary keys require a safe NBT-path encoder.

## Candidate 2: specialize a small finite domain

If `CellId` is statically bounded to a small set, emit ordinary functions or
branches for each index. Constant propagation may then erase most accesses.

This increases pack size but removes macro parsing. It is a serious candidate for
small enums, tuple indexes, fixed-size arrays, and frequently accessed prison cells.

## Candidate 3: balanced dispatch

For a larger bounded integer domain, compare the score against ranges and descend a
generated decision tree. Leaf functions contain static NBT paths.

This changes dynamic access from a macro parse to approximately logarithmic static
comparisons/function calls. The break-even point depends on macro cache locality and
must be benchmarked.

## Candidate 4: change the layout

Possible layouts for `prison.cells[id].user` include:

- array of struct compounds in NBT;
- struct of parallel arrays;
- compound keyed by stable IDs;
- one scoreboard row/holder per cell and field;
- one entity per cell, selected by score/tag;
- hybrid: hot numeric fields in scoreboards, cold/string fields in NBT.

The optimal layout depends on operations, not merely types. If most code reads the
same field across many cells, a struct-of-arrays layout may enable bulk operations.
If most code reads all fields of one cell, array-of-structs may copy better.

Entity-backed tables should be treated cautiously: selectors can scan/fork over
world entities, and one source line can create many execution contexts.

## Candidate 5: eliminate the lookup with context

If code is already executing as the relevant player or cell entity, capture the
current context once and carry it through the IR. Re-running a dynamic lookup for
each field is worse than preserving the proven reference.

```mdl
capture cell = prison.cells[input.id]
cell.user.say("hello")
cell.locked = true
```

should perform at most one lookup. The result can be represented by an execution
context, an index, or scalarized fields depending on subsequent uses.

## Compiler requirements

- bounds/range analysis;
- common-subexpression elimination for repeated lookups;
- alias analysis;
- scalar replacement of aggregates;
- structure splitting and layout selection;
- specialization for finite IDs;
- cost comparison between macro access and static dispatch;
- explicit failure semantics for missing keys/indexes.

## Required benchmarks

- macro cache hit/miss by index distribution;
- linear and balanced dispatch at multiple domain sizes;
- NBT array versus keyed compound;
- array-of-structs versus struct-of-arrays;
- scoreboard fake-player table versus NBT;
- entity-backed lookup with varying world/entity counts;
- repeated fields after one captured lookup.

