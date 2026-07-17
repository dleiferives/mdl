# Traversal Direction and Structural Cost

The main performance lesson across the surveyed code is simple: semantic order and
physical traversal order are separate decisions.

## Three observed traversal families

### Consume the head

Bookshelf's `map`, `filter`, `all`, `any`, `zip`, and many other operations copy the
input, process `[0]`, remove `[0]`, and recurse. This is easy to audit and naturally
preserves left-to-right order, but front removal shifts the remainder of an
array-backed NBT list. A full traversal is structurally `O(n²)`.

### Consume the tail

Bookshelf's `reduce_right`, `find_last`, and `reverse` use `[-1]`; Arcensoth's
[iteration example](https://gist.github.com/Arcensoth/8a8a63df6985f289430a98fa4b5cbe99)
also pops the last element. Tail removal avoids list shifts, so destructive
traversal is structurally linear.

Tail traversal reverses encounter order. Preserve source order by one of these
compositions:

- prepend each output, accepting front-insertion shifts;
- append outputs, then reverse once with tail operations;
- begin with a reversed work copy;
- use a second stack/zipper whose logical orientation is reversed; or
- prove that the operation is order-independent.

Arcensoth's exact example appends each popped tail value to the result, so it also
reverses the list. It is a useful traversal skeleton, not an order-preserving map.

### Keep the list, advance an index

stdmodulesystem stores an index in a scoreboard, splices it into an NBT path macro,
and reads `list[$(index)]`. There are no structural shifts. On the current
array-backed `ListTag`, indexed get/set is structurally constant time; the loop is
linear, excluding macro parsing/cache and payload copies.

This is often the best non-destructive traversal when:

- the input must survive;
- callbacks need the logical index;
- the list length is stable; and
- macro overhead is lower than deep-copy plus mutation overhead.

## Mutation during iteration

stdmodulesystem recomputes length after callbacks in some loops. That permits
limited growth/shrinkage but makes semantics subtle. Removing the current element
while incrementing the index can skip the following element. Its predicate-removal
operation instead walks indexes from the tail down, which is the correct general
rule for in-place deletion from an array-backed list.

MDL should specify iterator invalidation rather than inherit incidental behavior:

```text
snapshot traversal: callback mutations do not change the visited set
live traversal:     mutations are visible under defined cursor rules
exclusive traversal: mutating the traversed list is rejected
```

## A compiler decision table

| Requirement | Preferred lowering |
| --- | --- |
| Consume list; order irrelevant | Pop tail |
| Consume list; right-to-left semantics | Pop tail |
| Preserve input; random access allowed | Dynamic index macro |
| Produce left-to-right result from tail walk | Append then reverse |
| Remove matching elements in place | Walk indices from tail to head |
| Move cursor both ways | Two-list zipper |
| FIFO with many dequeues | Two-stack queue or ring queue |
| Small compile-time-known length | Unroll/static indices |

## Cost caveat

Command count alone is misleading. Compare at least:

- command and function invocations;
- macro cache hits/misses;
- copied NBT payload volume;
- list elements shifted;
- callback frame saves/restores; and
- maximum same-tick recursion depth/work.

The community code supplies candidate algorithms, not comparative 26.2 benchmarks.
Those variants should be placed in the fixture before assigning final cost weights.

## Sources

- [Bookshelf collection source](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function)
- [stdmodulesystem list source](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/list)
- [Arcensoth's two-list traversal](https://gist.github.com/Arcensoth/8a8a63df6985f289430a98fa4b5cbe99)
