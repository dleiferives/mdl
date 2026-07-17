# Collection Shaping: Map, Filter, Flatten, Partition, Slice, Window, and Zip

Bookshelf's breadth is useful as an operation inventory. The most transferable
lesson is which operations can be expressed as one traversal and which ones should
share an implementation kernel.

## Map, filter, and partition

All three need one visit per element:

- map appends the callback's result;
- filter appends the original element when predicate success is true; and
- partition appends the original element to either the true or false result list.

Partition should be one pass, not two filters, because a callback may have effects
and should run exactly once per element.

A fused filter-map can call the predicate and transform only passing elements. A
more general pipeline IR can combine adjacent operations until an ordering,
aliasing, or effect boundary requires materialization.

## Flat map and one-level flatten

Bookshelf's flat-map callback returns a list, then the implementation appends the
selected elements of that result directly to the output. That is the right basal
composition:

```text
mapped = callback(value)
out append from mapped[]
```

It avoids building `List<List<T>>` and flattening afterward. One-level flatten is
the same append-range kernel without a callback.

## Deep flatten and structural type probing

Bookshelf deep-flatten keeps a worklist. It tries a list-only mutation on a copied
current value and uses command success to decide whether the value behaves like a
list. If so, it prepends the children to the worklist to preserve depth-first
order; otherwise it appends the leaf to the result.

The insight is valuable: command validity/success can serve as a structural type
probe. The exact probe is too implicit for typed MDL. Ordinary lists, byte/int/long
arrays, and malformed/unexpected values have different mutation rules, and
front-prepending children makes the worklist costly. Prefer an explicit runtime
tag for dynamic values or a statically known recursive type, then use tail stacks.

Deep flatten also needs a language rule for strings, compounds, arrays, and mixed
lists. “Anything that accepted this mutation” is an implementation accident, not a
good source-level type definition.

## Slice, take, and drop

Bookshelf normalizes negative bounds with scores, then discards a prefix and copies
the requested range. This establishes useful API questions:

- Are bounds half-open `[start,end)`?
- Are negative bounds relative to length?
- Do out-of-range bounds clamp or fail?
- What happens when `start > end`?

The inspected source contains defensive checks, including one condition that
appears contradictory. We should adopt the normalized-bound concept, not copy its
validation literally; semantics need dedicated 26.2 tests.

`take(n)` and `drop(n)` should lower through the same normalized slice primitive.

## Chunk and sliding windows

Chunk partitions into consecutive non-overlapping lists. Sliding emits windows of
size `n` separated by `step`. Bookshelf copies the remaining source for each window
and then removes heads to advance, making the clear reference implementation
potentially expensive.

Compiler alternatives:

- compile-time unroll for bounded fixed sizes;
- dynamic indexed copies for stable input;
- maintain a deque/ring for streaming windows;
- share immutable input plus `(start,length)` slice views inside an optimization
  region; or
- materialize only when a view escapes.

Window semantics also need tests for partial final windows and `step > size`.

## Zip

Bookshelf copies both inputs, pairs their heads, and stops at the shorter input.
This gives conventional truncating zip semantics. MDL may additionally expose a
strict zip that rejects unequal lengths and a longest zip with fill values.

The physical implementation should use synchronized indices or synchronized tail
traversal plus a final reverse. Repeatedly removing both heads doubles the shifting
problem.

## Generators

Bookshelf includes `range`, `repeat`, fixed-limit `generate`, and
predicate-controlled `generate_while`. They are list producers rather than input
traversals, and therefore naturally append at the tail. A zero step in range must
be rejected; signed step determines the termination comparison.

Generators should remain lazy/streamed inside a fused pipeline when the consumer
does not require a complete list. `range(...).map(...).fold(...)` can be one score
loop with no NBT list.

## Sources

- [Bookshelf collection module](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection)
- [Bookshelf `flat_map`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/flat_map)
- [Bookshelf `flatten_deep`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/flatten_deep)
- [Bookshelf `sliding`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/sliding)
- [Bookshelf `zip`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/zip)
