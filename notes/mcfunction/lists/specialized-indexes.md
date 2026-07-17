# Specialized Indexes: Buckets and Tries

Generic list scans are universal, but two small public implementations show how
known key domains can compile into much better structures.

## Fixed-domain bucket sort

intsuc's [bucket-sort example](https://gist.github.com/intsuc/f7a9aa18cea4bb35491928af04e827d9)
allocates one sentinel-filled list slot per possible key, consumes input from the
tail, reads the numeric key into a score, dispatches to one static bucket position,
and finally removes sentinel slots.

For unique keys in a tiny bounded range, this turns comparison sorting into:

```text
initialize k buckets
for each item: buckets[item.key] = item
remove empty buckets
```

The output is sorted because bucket positions are key order. The example overwrites
duplicates; a true stable bucket sort would store a list per bucket and append.

This pattern generalizes beyond sorting:

- dense integer maps;
- histograms/counting sort;
- enum-key dispatch tables;
- priority queues with a small priority range; and
- deduplication/occupancy sets.

Modern macros can replace the hand-written branch per bucket when a validated
dynamic index is acceptable. Static dispatch may still win for very small domains
or when avoiding macro parsing.

## Fixed-depth integer trie

intsuc's [list-mapped trie](https://gist.github.com/intsuc/0901df9d487f7829d97491613a12d351)
uses nested two-child lists to encode the bits of a signed 32-bit scoreboard key.
At each generated level it doubles the score; the sign chooses a branch. A “touch”
operation lazily creates the required branch structure, after which a fixed nested
NBT path reaches the leaf.

Transferable ideas:

- integer keys can become a fixed-depth path rather than a linear record scan;
- a compiler can generate/unroll all 32 branch steps;
- lazy path creation separates lookup shape from stored leaf value; and
- list orientation/placeholder choices can make the final leaf path static.

Costs and warnings:

- 32 levels and generated code are large for small maps;
- nested path copies in older Minecraft implementations could erase theoretical
  `O(log n)` gains;
- deletion and branch compaction are non-trivial;
- the representation is specialized to scoreboard-width integers; and
- the public gist is an experiment, not a maintained general map library.

This is a candidate for benchmarking, not a default dictionary.

## Tree indexing can still copy too much

Another intsuc experiment notes that repeatedly copying a selected child into
scratch can make nominal logarithmic traversal expensive because each copy includes
the remaining subtree. The compiler cost model must include payload volume, not
only path depth or asymptotic node visits.

The best tree traversal keeps a path/reference to the original structure or stores
nodes separately by ID. Vanilla NBT does not provide true references, so a flat node
table plus typed IDs may outperform a deeply nested tree.

## Specialization rule

Choose a specialized index when the compiler knows at least one of:

- a small closed key domain;
- a useful numeric bound;
- unique versus duplicate keys;
- lookup-heavy versus iteration-heavy usage;
- an upper bound on collection size; or
- that order is irrelevant.

Otherwise retain the simple exact-scan or compound-map fallback.

## Sources

- [intsuc bucket sort](https://gist.github.com/intsuc/f7a9aa18cea4bb35491928af04e827d9)
- [intsuc list-mapped trie](https://gist.github.com/intsuc/0901df9d487f7829d97491613a12d351)
- [intsuc nested-list indexing cost experiment](https://gist.github.com/intsuc/60a7fcb76576812a7fc41918200e19f7)
