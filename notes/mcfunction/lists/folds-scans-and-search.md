# Folds, Scans, Predicates, and Search

Bookshelf implements the usual functional family—left/right fold and reduce,
left/right scan, `all`, `any`, `none`, `find`, and `find_last`—which exposes several
lowering rules beyond merely “call a callback in a loop.”

## Fold versus reduce

A fold receives an explicit initial accumulator. A reduce takes its first visited
element as the seed and invokes the callback only for the remainder. Consequently:

- fold has a defined result for an empty list;
- reduce needs an empty-list failure/option policy; and
- reduce callback indices begin after the seed, not at zero.

Bookshelf stores the accumulator in NBT callback storage. MDL should keep a numeric
accumulator in a scoreboard when its type and callback permit it, materializing NBT
only at a boundary.

## Left and right order

Bookshelf's left variants consume `[0]`; right variants consume `[-1]`. This proves
that right folds map naturally to the physical tail. Left folds should not copy that
head-removal strategy blindly. Equivalent compiler options include an indexed walk
or reversing once and consuming tails.

Order matters for non-associative callbacks:

```text
fold_left([a,b,c], z, f)  = f(f(f(z,a),b),c)
fold_right([a,b,c], z, f) = f(a,f(b,f(c,z)))
```

A right fold is not simply “run the left-fold callback in reverse”; the callback's
operand order must also match the language definition.

## Scans

A scan returns every intermediate accumulator. Bookshelf initializes scan-fold's
result with the explicit initial value, then appends one accumulator after each
element. Scan-reduce begins with the first input element. This convention should be
fixed in the source language because libraries differ on whether the initial seed
appears in the result.

Useful compiler rule: a scan must materialize all intermediates, but a fold does
not. Do not lower fold through scan and discard the result.

## Predicate short circuit

Bookshelf captures callback command success and uses `return` immediately:

- `any` returns on the first success;
- `all` returns on the first failure;
- `none` returns on the first success;
- `find` returns on the first left match; and
- `find_last` returns on the first right match.

This is real command-level short circuiting: later callbacks do not execute. MDL's
effect semantics must preserve that property. Lowering `any(xs, f)` by mapping all
Booleans and reducing them changes observable callback effects and work.

## Result shape for search

Bookshelf's find family returns both the matched value and index, with `-1` as the
not-found index and an absent result value. A typed language should prefer
`Option<{value,index}>` or an equivalent tagged union; the sentinel can remain an
internal ABI detail.

Traversing from the tail can still report the original index. Initialize it to
`length - 1` and decrement after each miss. That gives `find_last` a linear
tail-oriented lowering with no reversal.

## Take/drop while

`take_while` and `drop_while` are also short-circuit searches:

- `take_while` accumulates the passing prefix and stops at the first failure;
- `drop_while` consumes passing elements, then returns the entire untouched suffix.

The second operation is an important fusion opportunity. Once the predicate fails,
there is no reason to visit or copy suffix elements individually; one range/suffix
copy suffices if the representation supports it.

## Steal this idea

- Make callback success a direct predicate result.
- Compile early exits with `return`, not a flag checked after needless callbacks.
- Specify empty reduce, scan seed inclusion, callback index, and callback order.
- Specialize accumulator representation.
- Fuse terminal searches with their consumers where possible.

## Sources

- [Bookshelf folds/reduces/scans](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function)
- [Bookshelf `all`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/all)
- [Bookshelf `any`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/any)
- [Bookshelf `find_last`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/find_last)
