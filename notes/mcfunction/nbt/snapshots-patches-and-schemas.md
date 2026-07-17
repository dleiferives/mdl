# NBT Snapshots, Patches, Observers, and Schemas

NBT's deep-copy and changed/no-change behavior compose into a schema-independent
observer. stdmodulesystem's data observer is a useful concrete example.

## Snapshot comparison as an event machine

Each observed descriptor stores roughly:

```snbt
{
  path: "storage ...",
  start_event: "...",
  tick_event: "...",
  end_event: "...",
  state: {changed:false, prev_data:{...}}
}
```

On each tick it reads current data, copies it to scratch, and attempts to overwrite
scratch with `prev_data`. Because an equal `set from` reports no change:

```text
different = command success/result of scratch := previous
```

Together with the previous `changed` flag, this yields:

| Previous flag | Current differs? | Event |
| --- | --- | --- |
| false | true | change started, then change tick |
| true | true | change tick |
| true | false | change ended/stabilized |
| false | false | none |

After callbacks, the observer deep-copies current into `prev_data` and stores the
new flag. The 26.2 fixture confirmed equal nested data returns `0`, a changed leaf
returns `1`, and the snapshot copy is independent.

## Costs and ownership

The observer is generic because it does not know the schema, but each check may
deep-copy and compare the complete subtree. A compiler can improve it when effects
are known:

- mark a value dirty at its MDL mutation sites;
- compare only fields that escaped to raw/external mutation;
- hash/version large structures if maintaining the version is cheaper;
- coalesce multiple writes before callbacks; and
- allocate per-observer/per-call frames so nested callbacks cannot overwrite
  shared `io:` scratch.

Polling arbitrary external NBT still needs snapshot comparison or a domain-specific
event source.

## Merge as a patch primitive

Native recursive compound merge is a useful positive patch:

```text
patch = additions + replacements + recursive nested updates
```

It has no deletion marker and no list edit language. A complete MDL patch type can
be represented as:

```snbt
{
  set_or_merge: {...},
  remove_paths: [validated_path, ...],
  list_edits: [...],
  expected_version: 7
}
```

Paths in such a patch must be typed/validated syntax values, not raw strings.
`expected_version` allows optimistic conflict detection when callbacks or other
packs can mutate the same data.

## Missing, null, and defaults

NBT has no user-level null tag. Useful source-level options are:

- missing field = `None`;
- explicit tagged union such as `{kind:"none"}` / `{kind:"some",value:...}`;
- reserved sentinel only when the value type excludes it; or
- separate presence bit/key metadata.

`get_or_default` should normally read without inserting. `get_or_insert_default`
is a different mutating operation and must update ordered-map metadata if used on a
hybrid representation.

## Schema versions and migration

Persistent/public compounds should carry a schema version. Migration functions can
combine:

- subset matching to recognize an old shape;
- direct rename as copy-then-remove;
- recursive merge for new defaults;
- explicit removal for retired fields;
- type/range checks before conversion; and
- a final version write only after successful migration.

MDL should make migration idempotent and avoid publishing a new version while the
physical representation is half-updated.

## Source

- [stdmodulesystem data observer](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/data_observer)
- [Measured observer equality probe](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/nbt_dictionaries.mcfunction)
