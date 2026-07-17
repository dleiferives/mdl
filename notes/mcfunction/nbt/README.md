# NBT and Dictionary Implementation Survey

This directory is the lab notebook for NBT compounds, dictionaries, dynamic keys,
references, reflection, snapshots, and identity indexes. It complements the native
value inventory in [`../native-value-carriers.md`](../native-value-carriers.md) and
the list survey in [`../lists/README.md`](../lists/README.md).

The target is vanilla Java 26.2, data-pack format 107.1. Community implementations
are design evidence, not code to copy. Several target older versions, and their
shared scratch storage or raw macro fragments are not automatically safe in an MDL
runtime.

## Sources inspected

| Source | Snapshot | Main lesson |
| --- | --- | --- |
| [Bookshelf `bs.dump`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.dump) | v4.1.0, commit `c719575`, 2026-06-16, 26.2, MPL-2.0 | Recursive arbitrary-NBT formatting, structural probes, key-stack reflection |
| [Compound Key Reader](https://github.com/RedstoneOre/compound-key-reader/tree/b127e27e425d54b598b61852cc275fc084ae868f) | commit `b127e27`, 2024-12-20, 1.21–1.21.4, BSD-2-Clause | Sign-based compound serialization and quote-aware key parsing |
| [MCF: Map](https://github.com/Wisoven/MCF-Map/tree/520fa57cfaf4e5755f0c9e2220d1339e1400e106) | commit `520fa57`, 2026-05-17, 26.1, MIT | Practical ordered dictionary as compound values plus a key list |
| [MCF: EntityMap](https://github.com/Wisoven/MCF-EntityMap/tree/fad53e3debdf8b8c8f6a0489ba351f9f90922d5f) | commit `fad53e3`, 2026-06-14, 26.1-era, README says MIT | Per-entity records and the cost of canonical UUID strings |
| [MCFPP NBT types](https://github.com/MinecraftFunctionPlusPlus/MCFPP/tree/a0ab9386e47379fd2eb4772930ff73aee749596c/src/main/kotlin/top/mcfpp/core/lang/nbt) | commit `a0ab938`, 2026-04-02, GPL-3.0 | Compiler-known versus dynamic dictionaries and delayed materialization |
| [stdmodulesystem collections and observer](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs) | commit `d85c51c`, 2025-01-05, 1.21, archived; no repository license found | Path references, linked ordered maps, multimaps, and snapshot change events |

## Findings by topic

- [Native compound algebra](native-compound-algebra.md): size, set, deep copy,
  recursive merge, remove, matching, exact equality, and failure behavior.
- [Dynamic keys and safe paths](dynamic-keys-and-path-safety.md): runtime key
  encoding, special characters, the empty-key hole, and syntax injection.
- [Dictionary representations](dictionary-representations.md): compounds, record
  lists, compound-plus-key-list hybrids, linked entries, and specialized indexes.
- [Compiler-known dictionaries](compiler-known-dictionaries.md): partial concrete
  aggregates, specialization, materialization, and invariant tracking.
- [Compound-key enumeration and reflection](compound-key-enumeration.md): why
  native key iteration is absent, how two libraries serialize around it, and the
  measured 26.2 compatibility results.
- [References, ownership, and identity](references-identity-and-ownership.md):
  path-string references, alias invalidation, UUID representations, and entity
  lifetime.
- [Snapshots, patches, observers, and schemas](snapshots-patches-and-schemas.md):
  schema-independent change detection, merge-as-patch limits, migrations, and
  missing-value semantics.
- [Research ledger](research-ledger.md): every source inspection, successful
  server observation, negative result, and unresolved question.

## Representation decision table

| Required semantics | Preferred starting representation |
| --- | --- |
| Known struct fields | Static NBT compound or scalarized fields |
| Runtime non-empty string keys, no order | Dynamic-key NBT compound |
| String keys plus stable iteration order | Values compound + explicit key list |
| Arbitrary NBT keys or empty string key | List of `{key,value}` records |
| Frequent ordered deletion/insertion | Dynamic-key entries with explicit links, after invariant tests |
| Small finite key domain | Compiler dispatch or scalarized fields |
| Bounded integer keys | Buckets, list trie, or scoreboard table |
| Entity-associated data | UUID-keyed storage record or entity-local data, chosen by persistence/lifetime needs |
| Debug reflection over unknown compound | Serialization side channel only, never the ordinary map representation |

## What MDL should take

1. Keep `struct`, `Dict<String,V>`, ordered map, and arbitrary-key map distinct in
   the IR even when more than one can use NBT.
2. Preserve compiler-known key sets and values. Materialize a runtime compound only
   when dynamic access or an observation boundary requires it.
3. Use a typed `MacroNbtKey` encoder. A source string is data; a rendered path
   segment is command syntax.
4. Make empty-key behavior explicit. Native 26.2 compound syntax cannot represent
   the full `String` key domain through `/data` commands.
5. When iteration is required, maintain key metadata at mutation time. Recovering
   keys from serialized SNBT is a debug/reflection fallback.
6. Treat key-list/value-compound consistency, linked-map consistency, and snapshot
   updates as compiler-visible invariants with owned frames and cleanup.
7. Use typed UUID int arrays directly for storage matching. Convert to canonical
   text only for an API that genuinely requires text.
