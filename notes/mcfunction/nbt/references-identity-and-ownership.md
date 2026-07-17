# NBT References, Identity, and Ownership

NBT values are copied by `/data`; vanilla does not create an alias from one path to
another. Libraries that expose “references” store enough command syntax to address
the original location later.

## Path references are capabilities

stdmodulesystem passes strings resembling:

```text
storage namespace:id some.path
```

Referenced collection functions splice the fragment into macro commands, allowing
in-place child mutation without copying an entire container through I/O storage.

The useful semantic model is:

```text
DataRef<T> = {
  target_kind,
  target_identity,
  validated_path,
  type,
  lifetime/invalidation facts
}
```

This is not an ordinary `String`. It grants read/write authority and becomes
invalid when its target disappears or its path changes.

## Invalidation rules

- A compound-field reference survives unrelated field insert/remove, but fails if
  that field or an ancestor is removed/replaced.
- A list-index reference can silently refer to a different element after insertion,
  removal, sorting, or reversal.
- An entity reference may become temporarily unresolvable when an entity is
  unloaded and permanently invalid when killed.
- A block reference depends on dimension, loaded chunk, block-entity type, and
  replacement.
- A command-storage reference is the cleanest compiler-owned reference, but raw
  commands or other packs can still mutate a public path.

MDL needs alias/effect analysis and explicit unsafe boundaries around raw paths.
Deep-copy assignment should remain the default aggregate value semantics unless
the source language intentionally exposes references.

## EntityMap and UUID representation

MCF: EntityMap stores records keyed by canonical UUID strings. To build the string,
its inspected implementation splits four signed 32-bit UUID integers into eight
16-bit chunks, indexes a 65,536-element hexadecimal string ROM, and concatenates
the pieces with hyphens. The lookup-table file is about 459 KB.

That conversion is justified only if a downstream syntax requires the canonical
string. For storage lookup, the 26.2 fixture showed a cheaper representation:

```snbt
[{UUID:[I;181,0,0,3],value:"old"}, ...]
```

A typed int-array macro argument was embedded directly into the compound list
filter, and the matching record changed to `"updated"` in one command. No hex ROM
or text conversion was required.

Recommended split:

- preserve UUID as `[I; four ints]` for NBT storage, equality patterns, and copies;
- carry an execution-context/entity-selector proof while the entity is already
  selected; and
- convert to a canonical UUID string only for a command domain that demands it.

## Identity is not synchronization

EntityMap's README explicitly says its per-entity storage is not synchronized with
entity NBT. A UUID-keyed record is an external database row, not fields physically
attached to the entity. MDL must define:

- creation/deletion policy;
- behavior while the entity is unloaded;
- stale record cleanup;
- UUID reuse assumptions;
- clone/copy semantics; and
- whether entity removal cascades to linked records.

The same README warns that its entity-link feature is not stable/usable. The lesson
is to treat graph links as a separate invariant-heavy abstraction, not merely
another field in a map.

## Sources

- [stdmodulesystem referenced collections](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections)
- [MCF: EntityMap](https://github.com/Wisoven/MCF-EntityMap/tree/fad53e3debdf8b8c8f6a0489ba351f9f90922d5f)
- [Measured UUID macro filter](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/nbt/set_uuid_record.mcfunction)
