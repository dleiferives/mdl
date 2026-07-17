# Exact Equality, Matching, Membership, and Count

Two community libraries expose a powerful fact that is easy to miss: the result of
an attempted mutation can answer a comparison question.

## Exact equality by attempted overwrite

Bookshelf's `contains` and `distinct` implementations attempt to overwrite a
scratch element with the runtime needle. If the command makes no change, its
success/result is zero, which means the two NBT values were already exactly equal.

Derived protocol:

```text
probe = deep_copy(candidate)
changed = result(probe = deep_copy(needle))
equal = (changed == 0)
```

This compares arbitrary nested NBT values and distinguishes numeric tag classes.
The 26.2 `test/list_algebra` fixture confirms equal nested compounds/lists return
zero, unequal values return one, and `1b` differs from `1`.

The operation is destructive on inequality. A search may deliberately consume a
work copy, as Bookshelf does; a reusable equality operation needs scratch storage.
If the destination path is missing, failure can also mean “could not address the
target,” so the compiler must establish the destination first.

## Exact membership by consuming a work copy

For a runtime needle:

```text
work = deep_copy(list)
while work non-empty:
    if overwrite(work[-1], needle) changed nothing: found
    remove work[-1]
not found
```

Use the tail rather than Bookshelf's head-oriented implementation. It preserves the
same Boolean semantics without repeated shifts. If the index is required, retain a
length/index score or use dynamic indexed reads.

## Count all exact matches in one native pass

stdmodulesystem uses a clever bulk construction:

```text
work = deep_copy(list)
length = len(work)
changed = result(set every work[] element to needle)
equal_count = length - changed
```

The broadcast set reports how many elements changed. Elements already equal to the
needle are the complement. This needs one deep copy and one native list traversal,
not one function call per element. It also inherits the empty-wildcard trap:
`work[] set ...` creates a singleton when `work` is empty. Therefore branch on
empty first and return zero. The fixture separately records the unguarded empty
composition: it mutates `[]` to `[needle]` and computes the bogus count `-1`.

## Pattern membership is a different operation

NBT compound/list matching and filtered NBT paths are useful for static patterns:

```text
records[{kind:"active"}]
execute if data ... {records:[{kind:"active"}]}
```

But they implement recursive subset/containment rules, not exact equality. List
patterns are unordered containment and do not consume duplicate matches. This is
valuable for record queries and dangerous as an equality substitute.

stdmodulesystem explicitly exposes both exact and “like” operations. Its “like”
variant inserts an SNBT matcher into a filtered list path, allowing one command to
get/update/remove all matching compounds. That distinction belongs in MDL's API:

```text
equals(value, value)
contains_exact(list, value)
matches(value, static_or_typed_pattern)
select_where_nbt(list, compound_pattern)
```

Do not name all four `contains` without types making the semantics unambiguous.

## Regex boundary

None of these operations is regex. Score ranges match integers; NBT patterns match
typed structure; exact no-op overwrite compares values. String regex requires a
separate engine or a finite compiled automaton. The useful lesson is that command
success/result can avoid building such an engine for equality and structural
queries, but it cannot turn selectors or scoreboards into regex matchers.

## Steal this idea

- Model command `success` and `result` separately in IR.
- Recognize attempted no-op mutation as an equality primitive.
- Recognize bulk broadcast-set plus changed-count as an exact-count primitive.
- Guard that count primitive before wildcard set when the input is empty.
- Keep exact equality and NBT subset matching as separate typed operations.
- Prefer filtered paths for static compound predicates because vanilla performs the
  whole scan inside one command.

## Sources

- [Bookshelf `contains`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/contains)
- [Bookshelf `distinct/contains_check`](https://github.com/mcbookshelf/bookshelf/blob/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/distinct/contains_check.mcfunction)
- [stdmodulesystem list `count`](https://github.com/Samu64d/stdmodulesystem/blob/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/list/count.mcfunction)
- [stdmodulesystem “like” list operations](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/list)
