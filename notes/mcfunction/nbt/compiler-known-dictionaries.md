# Compiler-Known and Partially Concrete Dictionaries

MCFPP provides the most compiler-relevant lesson in this survey: a dictionary can
be structurally known even when not every value is constant.

## Concrete does not have to mean fully constant

Its NBT dictionary design separates a dynamic runtime compound from a concrete or
partially concrete compiler object. The compiler-side object retains the key set
and per-field variables. Some values may themselves be dynamic. Operations on a
known key can update compiler metadata or emit a direct static path; a truly
runtime string key forces materialization/dynamic behavior.

This suggests an MDL lattice richer than “constant versus runtime”:

```text
UnknownMap
KnownKeys({k1,...,kn}, per-key value facts)
KnownShape(schema, per-field representation)
FullyConstant(value)
```

Useful facts include known presence/absence, key order, value type, constant value,
and whether a field has escaped to observable NBT.

## Delay materialization

For a literal or statically constructed map, MDL can:

- fold `contains`, `size`, known-key get, and known-key remove;
- scalar-replace hot fields;
- emit direct static paths for dynamic values under known keys;
- specialize iteration into straight-line calls; and
- build one compound only when a raw command, dynamic key, serialization, or
  external API observes the aggregate.

When materialization is needed, emit one `set value` for the known constant portion
and then only the commands needed to fill dynamic leaves. Repeated one-field
construction loses both pack size and runtime efficiency.

## Key-list metadata belongs in the IR

MCFPP's runtime map also uses a key list plus value dictionary. The inspected code
illustrates why the compiler must own their relationship:

- one dynamic indexed assignment path appends key metadata without an obvious
  runtime contains guard in that path;
- map merge concatenates key lists while compound merge overwrites fields, so
  deduplication is required for set-like key metadata; and
- parent/path wiring for the two physical subobjects is easy to get wrong.

These are not reasons to reject the hybrid representation. They are evidence that
`MapPut`, `MapRemove`, and `MapMerge` should remain single verified IR operations,
with a representation verifier checking the physical invariant after lowering.

## Typed dictionaries and schemas

MCFPP associates map/dictionary values with types and supports conversion toward
structured template-like data. MDL should distinguish:

- closed structs with statically named fields;
- homogeneous `Dict<K,V>`;
- heterogeneous raw `NbtCompound`; and
- schema/versioned external data.

A closed struct can often be scalarized or validated statically. A raw compound
needs dynamic failure paths and may contain any NBT value. Treating both as one
untyped dictionary would discard optimization and safety information.

## Licensing boundary

MCFPP is GPL-3.0. This notebook extracts architecture and cautions, not source text.
Any direct code reuse would need a separate license decision.

## Source

- [MCFPP `NBTDictionary.kt`](https://github.com/MinecraftFunctionPlusPlus/MCFPP/blob/a0ab9386e47379fd2eb4772930ff73aee749596c/src/main/kotlin/top/mcfpp/core/lang/nbt/NBTDictionary.kt)
- [MCFPP `NBTMap.kt`](https://github.com/MinecraftFunctionPlusPlus/MCFPP/blob/a0ab9386e47379fd2eb4772930ff73aee749596c/src/main/kotlin/top/mcfpp/core/lang/nbt/NBTMap.kt)
- [MCFPP native dictionary methods](https://github.com/MinecraftFunctionPlusPlus/MCFPP/blob/a0ab9386e47379fd2eb4772930ff73aee749596c/src/main/java/top/mcfpp/mni/NBTDictionaryData.java)
