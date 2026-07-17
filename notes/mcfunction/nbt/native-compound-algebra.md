# Native Compound Algebra on Vanilla Java 26.2

The NBT compound is the closest vanilla carrier to a string-keyed dictionary. It
holds string keys and arbitrary NBT values, but its native operations are tree
operations rather than a complete map API.

The observations here come from
[`test/nbt_dictionaries.mcfunction`](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/nbt_dictionaries.mcfunction).

## Measured basal operations

| Intent | Native form | 26.2 observation |
| --- | --- | --- |
| Allocate/replace | `data modify ... path set value {}` | Replaces the complete subtree |
| Deep copy | `... set from ...` | Nested lists/compounds are copied, not aliased |
| Size | `data get ... compound` | Command result is the direct child count |
| Read/test field | path suffix `.key` / `execute if data` | Missing path fails |
| Set field | `... compound.key set ...` | Creates the final field |
| Create parents | set through `missing.parent.child` | Missing compound parents were created automatically |
| Wrong-type parent | set through `scalar.child` | Failed and retained the scalar |
| Remove | `data remove ... compound.key` | Present field returned/succeeded with `1`; absent field returned `0` |
| Merge | `... compound merge value {...}` | Recursively merges compound/compound pairs; source wins conflicts |
| Clear | set the compound to `{}` | No key enumeration is needed |

The fixture began with three direct keys, and `data get` returned `3`. A merge of
`{b:2,nested:{y:9,z:3},replace_me:{now:"compound"}}` into
`{a:1,nested:{x:1,y:2},replace_me:[1,2]}` produced:

```snbt
{
  a: 1,
  b: 2,
  nested: {x: 1, y: 9, z: 3},
  replace_me: {now: "compound"}
}
```

Nested compounds merged recursively. The incompatible list was replaced by the
source compound. The changed merge reported result/success `1`; repeating the same
merge reported `0`. The result is therefore useful as a changed/no-change channel,
not as a count of changed leaves.

## Creation and failure are path-sensitive

The measured successful write through `missing_parent.child` corrects a tempting
but wrong rule that every parent must already exist. The better rule is:

- absent compound nodes along a creatable static path may be constructed;
- an existing incompatible intermediate node blocks traversal; and
- list indices/filters have their own creation behavior and cannot be inferred
  from compound fields.

The compiler must retain command success if source semantics distinguish insertion
from replacement, or missing-path failure from ordinary assignment.

## Merge is not a general patch language

Compound merge gives a useful recursive update:

- add missing fields;
- replace scalar, list, array, or incompatible fields;
- recursively combine nested compounds; and
- report whether anything changed.

It cannot express deletion. A patch protocol needs a separate removal list,
tombstone convention, or generated `data remove` commands. It also cannot express
list edit scripts; lists are replaced as values unless the compiler emits list
operations separately.

## Matching and exact equality differ

`execute if data ... {pattern}` is recursive subset matching. It is excellent for
schema tests and record selection but permits extra compound keys. It is not exact
dictionary equality.

Exact runtime equality can use the no-op overwrite oracle:

1. deep-copy one value into owned scratch;
2. attempt `set from` the other value; and
3. interpret result/success `0` as exactly equal and `1` as different.

This respects nested shape and NBT numeric tag type. It destroys scratch when the
values differ, so it is not safe on a live operand unless consumption is intended.

## Atomicity and hybrid invariants

One native compound write is one command, but a dictionary operation that updates
both values and a key list is a multi-command transaction. Callbacks, raw commands,
or forked contexts can observe the intermediate state unless MDL owns the storage
and proves no interleaving observation. The IR should represent the logical map
operation before expanding it into separate mutations.

## Related sources

- [Mojang's original `data modify` release notes](https://www.minecraft.net/en-us/article/village---pillage-out-java-)
- [Native value and NBT type inventory](../native-value-carriers.md)
- [List matching and exact equality measurements](../list-operation-algebra.md)
