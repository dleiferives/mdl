# Stacks, Queues, Deques, and Zippers

Community implementations repeatedly turn one bad operation—removing the front of
an array-backed list—into several cheap tail operations. This family is directly
useful for worklists, interpreters, schedulers, cursors, and breadth-first search.

## Stack: the basal structure

An NBT list tail is already a stack:

```text
push = append
peek = [-1]
pop  = copy [-1], then remove [-1]
```

stdmodulesystem exposes both ordinary and reference-path stacks, but the physical
idea is identical. Empty-pop policy still needs an option/failure definition.

## Two-stack FIFO queue

The local 26.2 fixture uses `{in:[], out:[]}`:

- enqueue appends to `in`;
- dequeue pops `out[-1]`;
- when `out` is empty, move `in[-1]` to `out[-1]` until `in` is empty.

Each element is appended once and transferred at most once before being removed,
so the queue is amortized linear over a sequence of operations. The fixture
enqueued 1,2,3, dequeued 1, enqueued 4, dequeued 2, and retained an internal state
representing 3,4 in FIFO order.

This should be the default unbounded FIFO representation unless latency spikes
during a refill are unacceptable. Refill can be spread across ticks or bounded.

## Growable ring plus overflow

intsuc's [resizable queue](https://gist.github.com/intsuc/dcc55f14bd04f68b994b15c27305e1aa)
keeps a fixed-size ring region at the front of one list and a variable overflow
region after it. Scoreboards track read, write, and ring size. Push writes into a
free ring slot through a macro index; once the ring is full, it appends to overflow.
When the ring drains, the overflow becomes the new larger ring. Empty slots use a
sentinel.

Transferable advantages:

- normal push/pop avoid list shifts;
- growth is batched instead of copying on every push;
- modulo index arithmetic stays in scoreboards; and
- capacity can shrink/reset when fully empty.

Costs/tradeoffs:

- random indexed macro access on every normal operation;
- sentinel values require either an impossible value or separate occupancy state;
- counters must maintain invariants; and
- one backing NBT list mixes capacity slots with logical contents.

Use it when predictable dequeue latency matters more than representation
simplicity. Benchmark it against the two-stack queue on 26.2.

## Two-list zipper

intsuc's [Brainfuck interpreter](https://gist.github.com/intsuc/ab0be47a4c51798ca2c108a7eef431d2)
represents the tape and program cursor as two lists. Moving right transfers the
right list's tail to the left list's tail; moving left does the inverse. The current
cell/instruction remains at a chosen tail. Both directions use only append and
remove-last.

This generalizes to a zipper:

```text
left_reversed: elements before cursor
right:         current element at tail, then elements after it in reverse view
```

Choose orientations so cursor moves touch tails. A zipper supports local insert,
delete, replace, and bidirectional traversal without dynamic indexing or front
shifts. Materializing ordinary list order requires combining one side with a
reverse of the other.

## Deque

A deque can be built from two balanced tail stacks, one representing each end.
Pop/push at either logical end is a tail operation. When one side empties, split and
reverse part of the other side. This is a derived candidate not yet tested in the
fixture. It needs a balance policy to avoid moving the entire structure back and
forth under alternating operations.

## Steal this idea

- Treat stack, queue, deque, and zipper as distinct semantic types even if all use
  NBT lists internally.
- Expose amortized/batched work to the scheduler so a refill can cross ticks.
- Select two-stack queue for simplicity and ring queue for steadier latency.
- Use a zipper whenever the workload moves a cursor locally in both directions.

## Sources

- [intsuc growable ring queue](https://gist.github.com/intsuc/dcc55f14bd04f68b994b15c27305e1aa)
- [intsuc Brainfuck two-list zipper](https://gist.github.com/intsuc/ab0be47a4c51798ca2c108a7eef431d2)
- [stdmodulesystem stack](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/stack)
- [Local measured queue/reverse fixture](../fixtures/native-primitives-26.2/datapack/data/mdl_native/function/test/list_algebra.mcfunction)
