# References, Dictionaries, Ordered Maps, and Multimaps

stdmodulesystem's collections show two different dictionary problems: dynamic key
lookup in an NBT compound, and stable iteration order over those keys.

## Plain compound map

Its ordinary map stores values directly as fields of a compound. A macro quotes the
runtime key in a path such as `map.'$(key)'`. Set/get/remove/has-key then become one
dynamic path operation.

Advantages:

- direct lookup without a list scan;
- values can be arbitrary NBT;
- native compound storage is compact and inspectable; and
- setting an existing key is simple replacement.

Constraints:

- only string-like path keys fit directly;
- key text is command syntax and needs safe quoting/validation;
- native compound iteration order is not an API to rely on;
- there is no native “for each compound key” command; and
- a complete dynamic reference requires target kind, resource/storage ID, and path.

MDL should use a compound map for `Map<String,V>` when enumeration order is not
needed or when a separate key index is maintained.

## Reference-path ABI

The “referenced” variants pass strings such as `storage namespace:path sub.path`
through `storage io:` and splice them into macros. A get-reference operation can
return another path string pointing directly at a stored value. Later functions
mutate that location without copying the whole container through shared I/O.

This is a useful emulation of references, but it is not a native NBT reference:

- the string is a command fragment;
- its lifetime depends on the underlying path remaining valid;
- a list reference can be invalidated by insert/remove/reordering;
- aliasing is invisible to vanilla; and
- untrusted strings can inject command syntax.

MDL can adopt the model with a typed internal value:

```text
DataRef = { target_kind, resource_id, validated_path }
```

The compiler should construct it, track aliases/effects, and render it to macro
syntax only at the command boundary. It should not expose arbitrary raw fragments
as safe references.

## Iterable/ordered map as linked entries

stdmodulesystem's referenced iterable map stores each dynamic compound-key entry as
roughly `{value, prev_key?, next_key?}` and stores `last_key` on the map. Inserting
a new key links it after the old last key. Removing a key patches its neighbors and
updates/removes `last_key`. Iteration starts at the last key and follows `prev_key`,
so the inspected traversal is reverse insertion order.

This solves the missing compound-key enumeration primitive without maintaining a
separate list of all keys. Lookup/update/removal remain direct by key; iteration is
linear through links.

Tradeoffs:

- each entry pays two key strings of link overhead;
- deletion must preserve four head/interior/tail/singleton cases;
- a corrupted/missing link can truncate or cycle iteration;
- reverse insertion order is easy, forward order requires a first-key pointer or
  following `next_key`; and
- callbacks require saved frame state because key/value I/O is shared.

The transferable representation is a dynamic-key compound plus explicit order
links. Whether links beat a separate key list depends on update/iteration workload.

## Multimaps

stdmodulesystem builds list and set multimaps by mapping each key to a collection:

- list multimap preserves duplicate values and insertion order per key;
- set multimap rejects duplicates using its set implementation; and
- missing-key insertion first initializes an empty child collection.

This is a compositional design worth keeping. `Map<K,List<V>>` should normally use
the existing map and list primitives, with specialized fused operations to avoid
copying the child list out and back for every insertion. A reference to the child
path enables in-place append.

## Representation choices

| Required semantics | Candidate representation |
| --- | --- |
| String key, lookup only | NBT compound fields |
| String key, stable insertion order | Compound entries + key list or links |
| Arbitrary NBT key, small map | List of `{key,value}` records + exact scan |
| Bounded integer key | Buckets, score table, or trie |
| Multiple values per key | Map to list/set, preferably through child reference |
| Persistent external entity/block paths | Typed `DataRef` with invalidation rules |

## Sources

- [stdmodulesystem map](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/map)
- [stdmodulesystem referenced iterable map](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/referenced_iterable_map)
- [stdmodulesystem list multimap](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/list_multimap)
- [stdmodulesystem referenced collections overview](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68)
