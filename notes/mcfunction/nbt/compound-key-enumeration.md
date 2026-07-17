# Compound-Key Enumeration and Reflection

Vanilla `/data` can count a compound, address a known key, match a pattern, merge,
copy, and remove. It has no ordinary “for each key” path selector. Two libraries
work around this by forcing the compound through a text-serialization boundary.

## Compound Key Reader: serialize and parse

The 1.21-era Compound Key Reader:

1. copies an arbitrary compound into shared storage;
2. puts an NBT-backed text component on a sign in a custom force-loaded dimension;
3. reads the resulting serialized text;
4. scans character slices while tracking single quote, double quote, and backslash
   escape state;
5. finds top-level key/value separators;
6. slices out each key; and
7. optionally uses the recovered key as a dynamic path to copy its value.

Its parser batches many character/key steps in manually unrolled functions before
recursing. That lowers function-call overhead but does not remove the fundamental
work proportional to serialized data and parsing.

The original pack targets 1.21–1.21.4 and uses an older text-component form. In an
adapted 26.2 fixture, writing `{storage,nbt}` directly into a sign message produced
a structured `extra` token tree containing braces, keys, separators, values, quote
tokens, colors, and escapes. Thus the sign is still a working data-to-text boundary,
but the old parser cannot simply be assumed compatible with the new tree shape.

## Bookshelf `bs.dump`: consume a copy and recover one key at a time

Current 26.2 Bookshelf recursively formats arbitrary values. For a compound it
keeps a work copy on a frame stack, tries to recover one key through an item-name
text component, descends into that child, then removes the key from the parent work
copy and repeats. Arrays/lists similarly copy the first element and delete it while
formatting.

This gives a general reflection/debugger architecture:

- structural probes choose compound/list/scalar formatting;
- an explicit stack carries value, key, quoting, and indentation;
- consuming a copy guarantees progress without native key iteration; and
- dynamic macro paths descend through the recovered key.

The compound loop is expensive, and its repeated front removal is structurally
quadratic for array-backed lists. More importantly, the minimal 26.2 reproduction
of the item-name step stored the unresolved NBT text component itself and had no
`custom_name.extra` field. The sign variant did resolve. This does not prove the
released Bookshelf module is broken—its full build/context may add behavior not in
the minimal probe—but MDL must not depend on the item side channel without a full
integration test.

## Why this should remain reflection, not map iteration

Serialization-based enumeration pays for:

- copying the compound;
- converting arbitrary nested values to text/components;
- parsing or walking the serialized tree;
- escaping and quote recovery;
- world resources such as a sign or item-bearing entity; and
- global scratch/re-entrancy management.

It can also expose an ordering chosen by compound serialization rather than a
language-level order contract. The measured four-key output was deterministic in
one run but should not be treated as insertion order.

For normal maps, maintain an explicit key list or links when the map is mutated.
Use reflection only for debugging unknown external NBT, migration tooling, or a
deliberately expensive raw-NBT API.

## Fixture results

[`test/nbt_compound_reflection.mcfunction`](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/nbt_compound_reflection.mcfunction)
records both outcomes:

- item custom name: `{nbt:"nbt_reflection.work",storage:"mdl:observations"}` and
  `extra_present:0b`;
- sign message: a resolved component with `extra_present:1b` and tokens for keys
  including a space, dot, and quote.

The fixture removes its temporary container/sign block and force-loaded chunk
before return.

## Sources

- [Compound Key Reader](https://github.com/RedstoneOre/compound-key-reader/tree/b127e27e425d54b598b61852cc275fc084ae868f)
- [Bookshelf `bs.dump` key recovery](https://github.com/mcbookshelf/bookshelf/blob/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.dump/data/bs.dump/function/key/get.mcfunction)
- [Bookshelf recursive compound loop](https://github.com/mcbookshelf/bookshelf/blob/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.dump/data/bs.dump/function/interpret/compound/loop.mcfunction)
