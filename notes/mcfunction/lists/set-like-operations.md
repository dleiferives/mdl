# Distinct and Set-like List Operations

Bookshelf implements `distinct`, `union`, `intersect`, `difference`, and symmetric
difference over arbitrary NBT values. These functions answer two design questions:
what order should list-backed sets preserve, and when does the representation stop
scaling?

## Observed semantics

Bookshelf's operations are stable and distinct their outputs:

- `distinct(xs)` keeps the first occurrence of each value;
- `union(a,b)` concatenates then distincts, so values from `a` win ordering;
- `intersect(a,b)` keeps distinct values from `a` that occur in `b`;
- `difference(a,b)` keeps distinct values from `a` absent from `b`; and
- symmetric difference computes both directed differences and concatenates them.

Equality is exact NBT equality via the no-op overwrite technique described in
[`equality-membership-and-count.md`](equality-membership-and-count.md).

These are set-valued operations with list-defined stable order. That order is
useful and should be explicit in MDL rather than dismissed as an accident.

## Baseline complexity

For each source element, Bookshelf linearly scans a copied candidate list and often
also scans the accumulated output. The general arbitrary-NBT baseline is therefore
quadratic, with additional deep copies and front removals.

That is acceptable for small configuration lists and provides a correctness
reference. It is the wrong universal lowering for large numeric or keyed data.

## Representation specializations

| Element/domain knowledge | Better strategy |
| --- | --- |
| Small arbitrary NBT list | Stable linear-search baseline |
| Bounded integer range | Score/bitset or bucket occupancy |
| Records with static key field | Compound-key dictionary or bucket index |
| Small compile-time-known literals | Unrolled comparisons |
| Values already canonicalized to IDs | Score-backed membership table |
| Need multiplicity | Multiset/count map, not set operations |

Minecraft compounds provide dynamic string-key access through macros, but key
encoding, ordering, and persistence must be defined. A hash representation also
needs collision handling because scoreboard arithmetic is 32-bit.

## Fusion opportunities

- `distinct(map(xs,f))` can insert transformed values directly into the chosen set
  representation.
- `intersect` can build an index for the smaller side and traverse the other once.
- `any(distinct(xs),p)` does not need to materialize the distinct list if duplicate
  callback effects are irrelevant or prohibited.
- Symmetric difference need not materialize two directed differences when a keyed
  membership/count representation is available.

Effect semantics matter: removing duplicate callback invocations is only legal if
the language says the callback is pure or if the unfused definition would not have
called it for duplicates.

## Steal this idea

- Preserve first-seen order for list-backed set results.
- Define exact equality separately from pattern matching.
- Use the list implementation as a universal small-input fallback.
- Let representation selection choose buckets, dictionaries, or score sets from
  domain information.

## Sources

- [Bookshelf `distinct`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/distinct)
- [Bookshelf `intersect`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/intersect)
- [Bookshelf `difference`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/difference)
- [Bookshelf `symmetric_difference`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/symmetric_difference)
