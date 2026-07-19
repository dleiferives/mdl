# PS-2 Owned Lists, Stacks, and Zippers

Status: **`List<Int32>` tail-operation slice implemented and rehearsed**

## Goal

Provide a homogeneous collection value and enough efficient tail operations to
implement Brainfuck program/tape cursors as ordinary library code.

Backend research is recorded in
[`../../../mcfunction/lists/stacks-queues-and-zippers.md`](../../../mcfunction/lists/stacks-queues-and-zippers.md)
and [`../../../mcfunction/list-operation-algebra.md`](../../../mcfunction/list-operation-algebra.md).

## Implemented semantic algebra

The smallest likely public surface is:

```text
List.empty()
list.length()
list.push(value)          -> List<Int32>
list.last_or_zero()       -> Int32
list.without_last()       -> List<Int32>
```

The current slice deliberately supports only `List<Int32>`. Its frozen semantics
state:

- homogeneous `Int32` element typing;
- value copy versus consuming ownership;
- whether list assignment produces independent logical values;
- deterministic order;
- empty `last_or_zero()` returns zero and empty `without_last()` returns empty;
- evaluation order for list and element operands;
- maximum literal/runtime sizes and compiler analysis fallback; and
- call/return/recursion behavior.

A consuming optimization may destructively update a unique physical realization,
but it cannot make two source values alias unexpectedly.

## Core model

Prefer semantic list construction and tail operations over generic NBT paths. Retain
whole construction/append intent so constant folding, bulk materialization, copy
elimination, and future loop fusion remain possible.

The evaluator uses a host collection only as an implementation of the frozen
semantic list. It enforces the same limits and never defines target storage.

## Physical representation

The compiler uses compiler-owned command-storage NBT lists when a runtime collection is needed.
Legal alternatives include compile-time elimination, tuple-like scalarization for
small fixed lists, and explicit copy/materialization. Start with static tail paths:

```text
append
[-1]
remove [-1]
```

Score/NBT element conversions are explicit recipes. List storage must be safe across
ordinary calls, recursive frames, and serial selector children. A static scratch
list is legal only when overlap/liveness proves it cannot be reentered.

## Stack and zipper library

`Stack<T>` may be a thin semantic wrapper or ordinary module around `List<T>`.
Brainfuck uses two stacks for each cursor:

```text
left_reversed
right_with_current_at_tail
```

Movement transfers one tail element between lists and introduces zero/default tape
cells when crossing the visited boundary. The logical cursor order is tested through
public operations; physical list orientation is a library implementation detail.

## Dynamic access boundary

Do not add arbitrary runtime indexing merely because `List<T>` exists. If later
string/book parsing requires an index, compare:

- tail consumption/zipper movement;
- bounded static specialization;
- balanced dispatch; and
- one typed macro-indexed NBT operation.

The first client should prefer tail operations because they avoid runtime path
construction.

## Implemented evidence

- empty/one/many construction and length;
- push/peek/pop order and empty policy;
- copy independence and unique-destructive optimization equivalence;
- nested lists are explicitly unsupported in this slice;
- call/return, recursion, branch join, and serial context behavior;
- zipper left/right round trips and zero extension;
- exact NBT semantics on the pinned server;
- whole-target command/fork/storage analysis; and
- evaluator value-node limits independent from target generation.

## Non-goals

- random indexing in the initial slice;
- shared mutable list references;
- a universal iterator protocol;
- sorting/maps/sets/deques unless a new client justifies them;
- pretending amortized queue costs are exact worst-case costs; and
- scheduler-aware rebalancing.
