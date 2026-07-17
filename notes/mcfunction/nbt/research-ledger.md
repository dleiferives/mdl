# NBT and Dictionary Research Ledger

This is the do-not-repeat-work record. “Measured” means the isolated official Java
26.2 server at `127.0.0.1:25585` produced the observation. “Source-derived” means
the pattern was read from the pinned implementation. Negative results remain here
even when the breaking command was removed from the runnable fixture.

## Vanilla 26.2 probes

Most rows are implemented by
[`test/nbt_dictionaries.mcfunction`](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/nbt_dictionaries.mcfunction).
Reflection rows use
[`test/nbt_compound_reflection.mcfunction`](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/nbt_compound_reflection.mcfunction).

| ID | Status | Probe | Observation | Analysis |
| --- | --- | --- | --- | --- |
| N01 | Measured | `data get` compound | Result was direct key count (`3`) | [`native-compound-algebra.md`](native-compound-algebra.md) |
| N02 | Measured | Recursive compound merge | Preserved `nested.x`, replaced `y`, added `z` | same |
| N03 | Measured | Merge incompatible list/compound | Source compound replaced target list | same |
| N04 | Measured | Merge result | Changed merge returned `1`; identical repeat returned `0`, not leaf count | same |
| N05 | Measured | Remove present/absent field | Present `1`, absent `0` | same |
| N06 | Measured | Set missing nested parents | Created `missing_parent:{child:8}` | same |
| N07 | Measured | Set through scalar parent | Failed and retained scalar `9` | same |
| N08 | Measured | Compound structural probe `path{}` | True for empty/nonempty compounds; false for list | same |
| N09 | Measured | Element probe `path[]` | True for nonempty list and byte array; false for empty list | same |
| N10 | Measured | `data get` empty list | Success `1`, result/length `0`; distinguishes present empty from command failure when both channels retained | same |
| N11 | Measured | Encoded dynamic keys | Plain, space, dot, quote, backslash, numeric-looking keys all stored correctly | [`dynamic-keys-and-path-safety.md`](dynamic-keys-and-path-safety.md) |
| N12 | Negative measured | Dynamic empty-key segment | Runtime macro instantiation produced no mutation/status | same |
| N13 | Negative measured | Static SNBT empty key | `{"":7}` caused function load failure: parser expected a non-empty key; removed after recording | same |
| N14 | Measured | Typed UUID int-array in list filter | Directly matched and updated one UUID record | [`references-identity-and-ownership.md`](references-identity-and-ownership.md) |
| N15 | Measured | Exact snapshot equality | Equal nested values returned `0`; one changed leaf returned `1` | [`snapshots-patches-and-schemas.md`](snapshots-patches-and-schemas.md) |
| N16 | Negative measured | Item-name reflection reproduction | Stored unresolved `{storage,nbt}` component; no `extra` tree | [`compound-key-enumeration.md`](compound-key-enumeration.md) |
| N17 | Measured | Sign reflection adaptation | Sign message resolved compound to structured tokens with keys/quotes/values | same |
| N18 | Measured | Reflection cleanup | Test removed its temporary block and force load | same |

The pre-existing NBT fixture also measured all numeric tag widths, three numeric
array types, Unicode strings, heterogeneous lists, nested compounds, string slicing,
array narrowing, list operations, and deep copies. Those results remain in
[`../native-value-carriers.md`](../native-value-carriers.md) and
[`../list-operation-algebra.md`](../list-operation-algebra.md).

## Community source inspections

| ID | Evidence | Source area | Finding | Analysis |
| --- | --- | --- | --- | --- |
| S01 | Source-derived | MCF: Map record layout | Named map records contain `keys:[]` and `values:{}` | [`dictionary-representations.md`](dictionary-representations.md) |
| S02 | Source-derived | MCF: Map put/get/remove | Compound gives direct lookup; key list gives order; remove scans key metadata | same |
| S03 | Source-derived | MCF: Map loops/runtime | Front-removal loops, global scratch, temporary objectives, `@s` score assumptions | same |
| S04 | Source-derived | std plain map | Quoted macro key gives direct compound access but remains syntax-sensitive | [`dynamic-keys-and-path-safety.md`](dynamic-keys-and-path-safety.md) |
| S05 | Source-derived | std referenced collections | Command-path strings emulate mutable references | [`references-identity-and-ownership.md`](references-identity-and-ownership.md) |
| S06 | Source-derived | std iterable map | Entry links plus `last_key`; inspected traversal is reverse insertion order | [`dictionary-representations.md`](dictionary-representations.md) |
| S07 | Source-derived | std multimaps | Map values compose list/set collections and child references | same |
| S08 | Source-derived | std data observer | Previous snapshot + no-op overwrite derives start/tick/end change events | [`snapshots-patches-and-schemas.md`](snapshots-patches-and-schemas.md) |
| S09 | Source-derived | MCFPP dictionary types | Retains compiler-known keys and partial concrete values before materialization | [`compiler-known-dictionaries.md`](compiler-known-dictionaries.md) |
| S10 | Source-derived | MCFPP map | Uses key list + value dictionary; metadata dedup/invariant needs verification | same |
| S11 | Source-derived | MCF: EntityMap UUID | Canonical string conversion uses a 65,536-entry hex ROM around 459 KB | [`references-identity-and-ownership.md`](references-identity-and-ownership.md) |
| S12 | Source-derived | MCF: EntityMap links | Project README warns links are unstable/unusable | same |
| S13 | Source-derived | Compound Key Reader conversion | Uses force-loaded sign as serialization boundary | [`compound-key-enumeration.md`](compound-key-enumeration.md) |
| S14 | Source-derived | Compound Key Reader parser | Scans string slices, tracks quote/escape state, returns selectable key/value/sequence fields | same |
| S15 | Source-derived | Bookshelf `bs.dump` dispatch | Uses `value[]`, `value{}`, NBT-reference probing, and explicit recursive frames | same |
| S16 | Source-derived | Bookshelf compound loop | Recovers one key, descends, removes it from a work copy, repeats | same |
| S17 | Source-derived | Bookshelf list loop | Consumes head of copied lists; simple but structurally quadratic | same |
| S18 | Negative survey | General JavaScript/Java/C#/Python NBT libraries | Parse binary files outside the in-game command runtime; useful format references but not mcfunction dictionary implementations | this row |

## Open questions

| ID | Question | Required next work |
| --- | --- | --- |
| U01 | Total 26.2 key encoder | Probe control characters, newlines, both quote choices, Unicode edge cases, and reserved physical-key remapping |
| U02 | Complete Bookshelf integration | Build/install released `bs.dump` with its normal pipeline and retest item-name key recovery |
| U03 | 26.2 sign token stability | Probe nested compounds/lists, all numeric suffixes, empty compound, long strings, and text-component format changes |
| U04 | Compound iteration order | Confirm no usable guarantee through Mojang source/behavior; do not expose observed order meanwhile |
| U05 | Dictionary benchmarks | Compare compound, record scan, hybrid, linked, bucket, and trie layouts by size/workload |
| U06 | Hybrid transaction safety | Test nested callbacks/forked contexts observing put/remove between metadata and value writes |
| U07 | Dynamic structural type detection | Build a total probe over empty/nonempty lists, all arrays, compounds, strings, and numbers without text parsing if possible |
| U08 | Persistent identity lifecycle | Test unloaded entities, death/removal, stale UUID rows, and explicit cleanup policies |
| U09 | Merge/patch schema | Specify deletion, list edits, conflict/version behavior, and idempotent migration |
| U10 | Reference invalidation | Prototype typed refs and verify compound replacement plus list insertion/removal/sort cases |
