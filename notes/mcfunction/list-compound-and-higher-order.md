# Lists, Compounds, Dictionaries, and Higher-Order Operations

## Native list vocabulary

For block, entity, and storage destinations, Java 26.2 exposes:

```text
set       replace a selected value
merge     recursively merge compound values
append    add source value(s) at the end of a list
prepend   add source value(s) at the beginning
insert N  insert at a static signed index
remove    remove all values selected by an NBT path
```

Every modify mode can take a literal `value`, copy `from` block/entity/storage, or
construct a string from block/entity/storage data with optional signed start/end
indices. A source path is optional, meaning the complete source root can be copied.

Lists in the in-game 26.2 data abstraction may be heterogeneous. The three numeric
array types remain distinct representations with narrowing behavior and type
restrictions.

## Measured list behavior

The isolated 26.2 server confirmed:

- `xs[1] set value 21` replaced one element and returned result `1`;
- an out-of-range static index changed nothing and returned success/result `0`;
- negative indices count from the end;
- `insert 0` inserted at the front, `insert -1` appended, and `insert -2` inserted
  immediately before the last element; insertion and element access use different
  negative-index formulas;
- `remove xs[-1]` removed the final element;
- `remove xs[]` retained the list as `[]` and returned the number removed;
- `append from batch[]` extended a list with every selected source element;
- `append value [60,70]` appended one nested list instead;
- appending a list from its own `[]` path used a stable source snapshot and changed
  `[1,2,3]` into `[1,2,3,1,2,3]`;
- `groups[].xs append from batch[]` appended the complete batch to every selected
  target list in one native command; and
- a compound filter path such as `groups[{id:"a"}].xs` selected only matching
  list elements.

`data get` requires a single selected NBT value. Reading `list[]` failed because it
selected several values even though the same multi-match path is valid as a modify
source or target.

Idempotence is observable: setting a value to its current value returned failure
with result zero. Compiler rewrites must preserve command outcome when source code
can observe it.

## Compound values as dictionaries

An NBT compound is the closest native representation of a dictionary:

```snbt
{alpha: 1, "space key": 2, nested: {x: 3}}
```

Strong native operations are:

- static-key get, set, copy, and remove;
- recursive compound merge;
- creation of missing intermediate compounds during a static-path set; and
- subset matching through an NBT path predicate.

Measured recursive merge changed:

```snbt
{a:1,nested:{x:1,y:2}} + {b:2,nested:{y:9,z:3}}
```

into:

```snbt
{a:1,b:2,nested:{x:1,y:9,z:3}}
```

Weak or missing operations are just as important:

- there is no native dynamic-key argument;
- there is no native key enumeration operation;
- the tested `dict.*` path selected no children, so list-style wildcard traversal
  must not be assumed for compounds;
- NBT predicate matching is subset matching, not general exact equality; and
- there is no direct source-to-source comparison syntax, hash lookup, or numeric
  update inside a compound. Exact runtime comparison is nevertheless derivable by
  attempting to overwrite an established scratch target and observing that an
  equal value produces no modification.

This makes NBT compounds excellent for records and static resource-key maps, but not
automatically good runtime hash maps.

## Dynamic list and dictionary access

A minimal macro can place an integer into an NBT path:

```mcfunction
$return run data modify storage mdl:hof picked set from storage mdl:hof source[$(index)]
```

The server accepted positive and negative runtime indices. After `return run` was
added, an out-of-range index propagated success/result zero.

A simple dynamic compound key also works:

```mcfunction
$return run data modify storage mdl:hof picked set from storage mdl:hof dict.$(key)
```

`beta` selected a bare key, while a macro argument containing the encoded path token
`"space key"` selected a key with a space. This demonstrates capability, not a safe
source API: macro substitution reparses command syntax. A normal string must never
be accepted as `MacroNbtKey`. The compiler needs a path-segment encoder and should
prefer finite dispatch or static specialization when the key domain is known.

Candidate dictionary layouts remain:

| Workload | Candidate layout |
| --- | --- |
| Static fields/record | NBT compound or scalarized fields |
| Small finite key enum | Generated dispatch with static paths |
| Dense bounded integer keys | NBT list, parallel arrays, or score table |
| Dynamic arbitrary string keys | Macro-keyed compound after safe encoding |
| Frequent scans/order | Entry list `[{key,value}, ...]` |
| Hot numeric values | Score holders/objectives plus separate key mapping |

## Higher-order behavior is compiler-generated protocol

Minecraft functions are not first-class function values. Higher-order source
operations therefore need one of these representations:

1. **Static specialization:** generate a map/filter/fold body with the callback
   inlined or directly called.
2. **Finite function dispatch:** represent a callback as an enum/ID and dispatch to
   known pre-parsed functions.
3. **Macro-selected resource ID:** validate a `MacroResourceId<Function>` and place
   it into a `function $(callback)` command.
4. **Function tag broadcast:** run every member, useful for hooks/composition but
   not for selecting one callback.
5. **Native bulk recognition:** replace the higher-order operation with one NBT or
   scoreboard command when the callback has matching algebraic semantics.

The research datapack measured a destructive front-worklist protocol:

```text
copy source -> work
while work[0] exists:
    bridge work[0] to a score
    run callback
    append result
    remove work[0]
```

Results on vanilla 26.2:

```text
static map double [1,2,3,4] -> [2,4,6,8]
macro callback square       -> [1,4,9,16]
filter even                 -> [2,4]
fold sum                    -> 10
```

The same recursive front-worklist map processed 1,000 integers in one tick and produced
a list of length 1,000 with endpoints `2` and `2000`. This proves capability, not a
safe production bound. It consumed several commands plus a function invocation per
element and remains subject to command-sequence and tick-time budgets. Because the
worklist removes index zero from an array-backed list, this particular traversal is
structurally quadratic; tail worklists, cursors, fusion, or representation changes
are preferable for general lowering.

The exact basal algebra, index formulas, matcher behavior, cost shape, stacks,
two-stack queues, zippers, reverse, and community implementation survey are in
[`list-operation-algebra.md`](list-operation-algebra.md).

The function-tag experiment started with callback input `5`. A double callback
returned `10`, then a square callback observed the mutated `10` and returned `100`.
Invoking the tag produced result `110`: both ran and their results were aggregated.
Tags are therefore ordered multi-cast/composition in this protocol, not function
pointers.

## Preferred lowering order

For `map`, `filter`, `fold`, `contains`, `find`, and dictionary traversal, try plans
in this order:

1. constant evaluation;
2. scalar replacement or complete unrolling for tiny fixed aggregates;
3. native bulk path/score operation;
4. fused static specialized loop;
5. bounded generated dispatch;
6. recursive static-index worklist;
7. macro-indexed/keyed loop;
8. scheduled multi-tick state machine.

The IR should keep higher-order intent visible until this choice. Lowering source
`map` immediately to a generic loop would hide bulk updates, fusion, and
representation changes.

## Remaining experiments

- end-pop versus front-pop NBT cost as list size grows;
- copy cost for large elements and lists;
- map/filter fusion and append batching;
- stable iteration and mutation/alias failure semantics;
- macro cache hit/miss behavior for indices, keys, and callback IDs;
- finite dispatch break-even points;
- callback captures, recursion, re-entrancy, and fork-safe frames;
- command counts and wall time for 10, 100, 1,000, and tick-split workloads;
- exact list/compound equality and key-set algorithms;
- sorted-table, trie, entry-list, and score-table map representations.

## Primary sources

- [`data modify` list operations](https://www.minecraft.net/en-us/article/village---pillage-out-java-)
- [Multi-match NBT paths and data operations](https://feedback.minecraft.net/hc/en-us/articles/360018363512-Minecraft-Java-Edition-Snapshot-18W43C)
- [Heterogeneous lists in Java 1.21.5](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-5)
- [Function macros](https://www.minecraft.net/en-us/article/minecraft-snapshot-23w31a)
- [Function return behavior](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-3)
