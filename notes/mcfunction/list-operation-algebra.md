# List Operation Algebra on Vanilla Java 26.2

This note narrows the question from “can NBT hold a list?” to “which basal list
operations exist, what are their exact semantics, and which useful structures can
we compose from them?” Evidence comes from Mojang release notes, inspection of the
official 26.2 server classes, community mcfunction implementations, and the
reproducible `test/list_algebra` fixture.

## The basal vocabulary

Assume `xs` is an NBT list in command storage.

| Intent | Vanilla operation | Important behavior |
| --- | --- | --- |
| Allocate/replace | `data modify ... xs set value []` | Creates missing static parents; unchanged set fails |
| Deep copy | `data modify ... ys set from ... xs` | Copies the complete value; no alias remains |
| Length | `execute store result score ... run data get ... xs` | Result is list length |
| Non-empty | `execute if data ... xs[0]` | Empty and absent both fail this test |
| Static get | `data modify ... tmp set from ... xs[2]` | Negative indices address from the tail |
| Dynamic get | macro containing `xs[$(index)]` | Macro argument is reparsed command syntax |
| Static replace | `data modify ... xs[2] set value ...` | Out of range fails; it does not grow the list |
| Push back | `data modify ... xs append value ...` | Tail insertion |
| Push front | `data modify ... xs prepend value ...` | Shifts the existing array-backed contents |
| Insert | `data modify ... xs insert N value ...` | `N` is a static signed insertion position |
| Extend | `data modify ... xs append from ... ys[]` | `[]` selects elements; omitting it nests `ys` |
| Broadcast set | `data modify ... xs[] set value ...` | Sets every element; on empty `xs`, creates one |
| Remove one | `data remove ... xs[N]` | Negative indices address from the tail |
| Clear, retain list | `data remove ... xs[]` | Returns count removed; empty list is unchanged/failure |
| Delete list field | `data remove ... xs` | Removes the list tag itself |
| Compound select | `xs[{kind:"a"}]` | Selects every compound containing the pattern |
| Bulk nested edit | `xs[{kind:"a"}].value set ...` | One command can edit all selected records |

Ordinary lists can be heterogeneous in 26.2. Byte, int, and long array tags remain
homogeneous and reject the wrong element type. MDL can therefore use homogeneous
`List<T>` types while reserving heterogeneous NBT lists for tuples, sum values,
frames, or explicitly untyped data.

## There are two negative-index rules

Element access and insertion do not normalize a negative index the same way.

```text
access/remove/set: position = size + index
insert:            position = size + index + 1
```

For `[1,2,3]` this means:

| Expression | Position/effect |
| --- | --- |
| `xs[-1]` | element `3` |
| `remove xs[-1]` | remove `3` |
| `insert -1 value 9` | `[1,2,3,9]` |
| `insert -2 value 9` | `[1,2,9,3]` |
| `insert 0 value 9` | `[9,1,2,3]` |
| `insert 3 value 9` | `[1,2,3,9]` |

Valid insertion positions are `0..size`, including both ends. The lowest valid
negative insertion index is `-(size + 1)`. This was confirmed both in the 26.2
server implementation and by the fixture. It corrects the easy but wrong analogy
that `insert -1` must mean “before the last element.”

## Source selections are batches, except for `set`

With `items = [7,8]` and `xs = [1,2,3]`, all three insertion modes preserve source
order:

```text
append  from items[] -> [1,2,3,7,8]
prepend from items[] -> [7,8,1,2,3]
insert 1 from items[] -> [1,7,8,2,3]
```

The server first copies the selected source values, then mutates each target. That
makes self-extension stable:

```text
[1,2,3] append  from itself[] -> [1,2,3,1,2,3]
[1,2,3] prepend from itself[] -> [1,2,3,1,2,3]
```

`set from items[]` is different: the server selects only the last source, so the
target becomes `8`. A multi-match source must therefore mean one of two explicit IR
operations: `set_last_selected` or `insert_range`. It should not be represented by
one vague “copy from path” operation.

An empty source selection makes append/prepend/insert fail with result zero and
leaves the target unchanged. In contrast, appending `value []` appends one empty
nested list.

## Empty-list and wildcard traps

These superficially similar operations are observably different:

| Operation on empty `xs` | Result |
| --- | --- |
| `remove xs[]` | no change; failure/result `0`; `xs` remains `[]` |
| `set xs value []` | no change; failure/result `0` |
| `set xs[] value 9` | success/result `1`; `xs` becomes `[9]` |
| `append from empty[]` | no change; failure/result `0` |
| `remove xs` | deletes the field, if present |

The surprising wildcard-set result comes from the path node's “get or create”
behavior. It is useful as a compact “fill, or initialize singleton” primitive, but
it is not a conventional `for each`: a compiler lowering `map` must not accidentally
turn an empty source list into a one-element list.

Every `data modify` source is copied. Nested compounds and lists are deep copied;
mutating the original after `set from` does not change the clone.

## NBT matching is useful, but it is not equality or regex

An NBT match path can begin with a compound pattern:

```mcfunction
execute if data storage mdl:list_match {value:{xs:[2]}}
```

In the official server matcher:

- a compound pattern is recursive subset matching;
- numeric tag classes matter, so `1b`, `1`, and `1L` are not interchangeable;
- a non-empty list pattern is unordered containment;
- every expected list element searches the complete actual list independently;
- matches are not consumed, so expected `[2,2]` matches actual `[1,2,3]`; and
- an expected empty list matches only an actual empty list.

Consequently `{xs:[3,1]}` matches `{xs:[1,2,3]}`. This is excellent for static
“contains literal/pattern” tests, but cannot implement sequence equality, prefix,
subsequence, multiplicity, or regular-expression matching.

List-element path filters accept compound patterns:

```mcfunction
data remove storage mdl:data xs[{kind:"temporary"}]
data modify storage mdl:data xs[{kind:"active"}].enabled set value true
```

They select all matches. The fixture updated two matching records and then removed
both in one command. There is no equivalent scalar syntax such as `xs[2]` meaning
“every element equal to 2”; `[2]` is always an index. A root compound match can test
literal scalar containment, but a runtime scalar needle still needs a scan or a
different representation.

There is no direct `execute if data source == other_source` syntax, but community
libraries expose a better indirect primitive: a `data modify ... set from ...`
command returns failure/result zero when the target already equals the source.
Copying one operand into scratch storage and attempting to overwrite it with the
other is therefore an exact runtime equality test. It is destructive when values
differ, so it must operate on scratch unless consuming the operand is intended.
The target-server fixture confirms that nested values compare structurally and
numeric NBT classes remain significant (`1b` differs from `1`).

The same return behavior enables a compact exact-count operation. Copy a list,
broadcast-set every element to the needle, and subtract the command's changed count
from the list length. stdmodulesystem uses this construction. It performs one
native list pass plus a deep copy; it is particularly attractive when the list is
already disposable.

## Cost shape from the 26.2 implementation

The official `ListTag` stores a Java `ArrayList<Tag>`. This gives a useful initial
cost model, excluding command dispatch and the cost of copying nested payloads:

| Operation | Structural cost |
| --- | --- |
| Tail get/set/remove | `O(1)` |
| Tail append | amortized `O(1)` |
| Front or middle insert/remove | `O(n)` shift |
| Deep copy list | `O(n + payload)` |
| Compound-filter scan | `O(n × pattern work)` |
| List-pattern containment | up to `O(expected × actual × nested match)` |

This changes how higher-order loops should be built. The existing research loop
that repeatedly removes `work[0]` is simple and processed 1,000 elements, but its
array shifts make the traversal structurally quadratic. It is evidence of
capability, not the preferred general representation.

## Compositions from the basal operations

### Stack

Use the list tail:

```text
push(x) = append x
peek()  = xs[-1]
pop()   = copy xs[-1] to result; remove xs[-1]
```

This is the natural NBT list structure: all three operations avoid front shifts.
Pop is a two-command protocol and is not atomic with respect to arbitrary callbacks
between the copy and remove.

### Reverse

The fixture implements a linear destructive-worklist reverse:

```text
work = deep_copy(source)
out = []
while work is non-empty:
    append work[-1] to out
    remove work[-1]
```

Measured result:

```text
[1,"two",{n:3},[4]] -> [[4],{n:3},"two",1]
```

All mutation is at list tails. This `reverse` becomes a useful primitive for queues,
order-preserving tail traversals, and zipper rebalancing.

### FIFO queue

The obvious representation enqueues with `append` and dequeues with `[0]`. It is
correct but every dequeue shifts the remainder.

A better composition uses `in` and `out` stacks:

```text
enqueue(x): push x onto in
dequeue():
    if out is empty:
        move elements from in[-1] to out[-1] until in is empty
    pop out[-1]
```

Each element moves at most once between stacks, so queue operations are amortized
`O(1)` structurally. The fixture enqueued `1,2,3`, dequeued `1`, enqueued `4`, then
dequeued `2`; its remaining representation was `out:[3], in:[4]`.

For a hot, long-lived queue, a growable ring buffer is another option. A published
mcfunction implementation keeps scoreboard read/write cursors, addresses slots
through macro indices, uses tombstone values, and appends an overflow region until
the ring can be resized. That trades a more involved invariant for fewer element
moves.

### Deque and zipper

Two stacks also form a deque or zipper. The current focus is at one pair of tails;
moving left or right pops one side and pushes the other. A working Minecraft
Brainfuck interpreter uses exactly this layout for both memory and program cursors.
This is often better than editing the middle of one large list.

### Concatenation and flatten-one-level

For `out = left ++ right`:

```text
out = deep_copy(left)
append into out from right[]
```

This preserves order. `append from right` without `[]` instead adds `right` as one
nested element. Repeated concatenation should use a builder/rope/chunk strategy or
append into one owned destination, because repeatedly copying the growing prefix is
quadratic.

### Map, filter, and fold

There are four main traversal plans:

1. Native bulk path update when every matched record receives the same edit.
2. Static unrolling for tiny fixed lists.
3. Read-only cursor with a safe macro index; no list shifts, but macro cache and
   dynamic-command costs matter.
4. Tail worklist. It is linear structurally, but observes reverse order. Append
   outputs and reverse once afterward to restore stable order.

A commutative fold can consume tail elements directly. A non-commutative fold must
preserve the specified direction, either with a cursor, a preliminary reverse, or
an algebraically valid reassociation. Map/filter pipelines should be fused so an
element crosses the scoreboard/NBT bridge only once.

### Contains, count, find, any, and all

- Static compound predicate: use `xs[{...}]`; `execute if data` gives existence and
  its command result gives the number of selected matches.
- Static scalar literal: root list-containment matching can answer existence.
- Runtime scalar exact membership: traverse a scratch copy and use no-op overwrite
  as the equality test; short-circuit with `return`.
- Runtime scalar exact count on a non-empty list: copy, broadcast-set to the needle,
  and compute `length - changed_count`; guard empty lists first.
- Callback predicate: traverse and short-circuit with `return`.
- `find_index`: maintain a cursor explicitly; filtered paths return values, not
  their positions.
- `all`: fail/return as soon as one element violates the predicate.

### Sort, grouping, and indexing

Vanilla has no general sort. A published bucket-sort implementation consumes input
from the tail, dispatches a numeric key into a fixed bucket, removes sentinel empty
buckets with a compound filter, and obtains sorted output. This is a good lowering
for a small known key range. General comparison sort needs generated dispatch,
macro indexing, a heap/tree representation, or compilation outside the game.

A list-mapped trie implementation demonstrates another direction: nested short
lists can encode bounded branching and use sizes plus tail paths as structural
state. It is specialized, but shows why `List<T>` should not force one physical
layout in the compiler.

## What vanilla still does not provide

There is no direct list slice, source-to-source equality syntax, runtime scalar
filter path, arbitrary comparator sort, deduplication, set intersection, zip,
lambda value, or numeric map/reduce. Exact equality can nevertheless be derived
from the result of an attempted no-op overwrite. There is no mutation operator that
returns the removed NBT value in one atomic command. Dynamic indices require macro
substitution or finite generated dispatch.

These are library/compiler protocols built from the basal algebra, not hidden
scoreboard features.

## Compiler consequences

Keep these semantic operations visible in IR until representation selection:

```text
list_new / list_copy / list_length
list_get / list_set
list_push_tail / list_pop_tail
list_insert / list_remove
list_extend
list_set_each / list_remove_each / list_select_compounds
list_reverse
map / filter / fold / find / any / all
```

Important plan facts include ownership, alias visibility, order sensitivity,
fixed/maximum length, element payload size, whether command success/result is
observable, and whether a predicate is a static NBT pattern. Those facts decide
between scalarization, literal construction, an NBT vector, stack pair, zipper,
ring buffer, compound map, score table, or scheduled state machine.

## Reproduction

Run the isolated research pack on a non-default loopback port:

```mcfunction
function mdl_native:test/list_algebra
data get storage mdl:observations list_algebra
```

The fixture records batch order, both negative-index rules, bounds, empty-source
outcomes, wildcard behavior, deep-copy/self-alias behavior, matcher semantics,
exact equality/count idioms, filtered updates, array type rejection,
stack/queue/reverse results, and the two-stack FIFO composition.

The source-by-source community survey and exhaustive experiment coverage record are
indexed under [`lists/`](lists/README.md).

The exact implementation claims were checked in these official 26.2 server classes:
`DataCommands`, `NbtPathArgument.NbtPath`, `IndexedElementNode`,
`AllElementsNode`, `MatchElementNode`, `ListTag`, and `NbtUtils.compareNbt`.

## Primary sources and implementations

- [Mojang's original `data modify` list operations](https://www.minecraft.net/en-us/article/village---pillage-out-java-)
- [Mojang's heterogeneous-list changes](https://feedback.minecraft.net/hc/en-us/articles/34593421146637-Minecraft-Java-Edition-Snapshot-25w09a)
- [Mojang's function macros and return additions](https://www.minecraft.net/en-us/article/minecraft-snapshot-23w31a)
- [Resizable mcfunction ring-buffer queue](https://gist.github.com/intsuc/dcc55f14bd04f68b994b15c27305e1aa)
- [Two-list zipper in a Minecraft Brainfuck interpreter](https://gist.github.com/intsuc/ab0be47a4c51798ca2c108a7eef431d2)
- [Tail-consuming fixed-bucket sort](https://gist.github.com/intsuc/f7a9aa18cea4bb35491928af04e827d9)
- [List-mapped trie](https://gist.github.com/intsuc/0901df9d487f7829d97491613a12d351)
