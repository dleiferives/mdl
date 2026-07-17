# Dictionary Representations Found in Real Data Packs

There is no single best Minecraft map layout. The representation should follow key
domain, iteration/order requirements, mutation workload, and what the compiler
already knows.

## 1. Plain compound

```snbt
{alpha: value_a, beta: value_b}
```

This is the natural `Dict<NonEmptyString,V>` when order is irrelevant. A dynamic
macro key provides direct get/set/remove/contains. Clear is one `set value {}` and
bulk nested updates may use compound merge.

Weaknesses are the missing native key iterator, unsafe raw path strings, empty-key
hole, and lack of a stable iteration-order contract.

## 2. List of entry records

```snbt
[{key: logical_key, value: value_a}, {key: other_key, value: value_b}]
```

This supports arbitrary NBT keys, including the empty string as a stored value,
and has explicit order. Exact lookup compares each record key; static compound
patterns can bulk-select known patterns.

Without an index, get/set/remove are linear. Exact runtime keys require a scan or
the no-op overwrite oracle, because NBT list filters accept static compound
patterns rather than another dynamic source value.

## 3. Values compound plus explicit key list

MCF: Map stores each named map record approximately as:

```snbt
{name:"warrior", keys:["health","class"], values:{health:150,class:"mage"}}
```

The compound gives direct lookup; `keys` gives stable enumeration order. `put`
appends a key only if it was absent, then writes the value. `value_list` walks keys
and performs a dynamic lookup for each. `remove` deletes the compound field and
searches the key list to delete its metadata entry.

This is the best general starting point for an ordered string map, but it creates
an invariant:

```text
keys has each logical key exactly once
iff
values has the corresponding physical field
```

Clear/copy/merge/remove must preserve both halves. MCF: Map demonstrates the API,
but its inspected implementation also shows pitfalls MDL can improve:

- raw macro `path` and key insertion are syntax-sensitive;
- shared global scratch and temporary objectives are not re-entrant;
- operations use `@s` scores and therefore assume a suitable executor;
- several loops repeatedly delete `[0]`, creating array shifts; and
- two-command metadata/value updates expose an intermediate state.

## 4. Linked dynamic-key entries

stdmodulesystem's iterable referenced map stores each compound field as something
like `{value,prev_key?,next_key?}` plus a `last_key` pointer. Direct lookup remains
available while iteration follows links. The inspected iterator begins at the last
key and follows `prev_key`, giving reverse insertion order.

Links avoid a linear key-list search when deleting a known key, but every mutation
must handle empty, singleton, head, tail, and interior cases. Missing/cyclic links
can truncate or loop traversal. MDL should only choose this after invariant and
corruption tests; a key list is simpler when maps are small or deletion is rare.

## 5. Composite multimaps

stdmodulesystem implements list/set multimaps by mapping each key to an existing
collection representation. This is good semantic composition:

```text
Map<K,List<V>>
Map<K,Set<V>>
```

The backend should still fuse common child operations so an append can mutate the
child path directly instead of copying the child collection out and back.

## 6. Specialized finite indexes

When the key domain is bounded or known, generic dynamic keys may be unnecessary:

- scalarize a fixed record into fields;
- dispatch a small enum to static paths;
- use a score table for hot integer values;
- use buckets for a bounded numeric domain; or
- generate a fixed-depth list trie for full signed-score keys.

These alternatives are detailed in the list survey's
[`specialized-indexes.md`](../lists/specialized-indexes.md). They exchange pack
size, setup, or specialized invariants for predictable lookup.

## Operation-oriented choice

| Workload | Candidate |
| --- | --- |
| Direct string lookup, rare/no iteration | Plain compound |
| Ordered iteration and direct string lookup | Compound + key list |
| Arbitrary key types or full string domain | Entry records |
| Frequent delete by known string key plus order | Linked entries |
| Tiny fixed schema | Scalarized/static compound |
| Bounded integer domain | Buckets/trie/score table |

The compiler should retain a logical `Map` operation until representation
selection can see the whole workload. Choosing only from the source type throws
away the reason these layouts differ.

## Sources

- [MCF: Map](https://github.com/Wisoven/MCF-Map/tree/520fa57cfaf4e5755f0c9e2220d1339e1400e106)
- [stdmodulesystem referenced iterable map](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/referenced_iterable_map)
- [MCFPP NBT map](https://github.com/MinecraftFunctionPlusPlus/MCFPP/blob/a0ab9386e47379fd2eb4772930ff73aee749596c/src/main/kotlin/top/mcfpp/core/lang/nbt/NBTMap.kt)
