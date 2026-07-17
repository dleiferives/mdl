# Callbacks and Re-entrant Frames

Bookshelf and stdmodulesystem independently converge on the same core requirement:
a higher-order collection function needs both a callback calling convention and a
way to preserve its own state across that callback.

## Bookshelf's callback ABI

In [Bookshelf's map implementation](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/map),
the caller passes a macro argument named `run`. The collection operation publishes
callback inputs under `storage bs:lambda collection`:

- `value`: the current NBT element;
- `index`: its logical zero-based index;
- `accumulator`: present for folds/scans; and
- `result`: written by transforming callbacks.

A tiny macro trampoline performs the supplied command. Predicate operations store
the trampoline's command success; transforming operations read `collection.result`.
This cleanly distinguishes a Boolean predicate channel from an arbitrary NBT value
channel.

The useful abstraction is not Bookshelf's exact path names. It is:

```text
call_transform(frame, callback) -> NBT result
call_predicate(frame, callback) -> command success
call_consumer(frame, callback)  -> effect only
```

The callback should receive a logical index even when the physical implementation
walks a list backwards.

## Owned frame stack

Bookshelf prepends one compound frame to `collection.stack` for each operation.
The active operation owns `stack[0]`; the frame contains its copied input,
accumulator, result, callback command, index, and operation-specific temporaries.
It removes the frame on exit. A callback can invoke another collection operation
without overwriting the outer operation's state.

stdmodulesystem uses a more granular stack discipline. Before a callback it pushes
shared `storage io:` fields and local scoreboard values, then restores them after
the callback. The idea is equivalent to saving caller state, although it takes
many commands per call and is easier to get wrong.

## Steal this idea

- Every higher-order invocation owns a frame.
- Callback inputs and outputs have typed, documented roles.
- Predicate truth uses command success rather than manufacturing a Boolean NBT.
- A callback may invoke the same combinator recursively or invoke another one.
- Cleanup belongs to the operation epilogue, including early-return paths.

For MDL, append frames at the list tail and address `[-1]` when possible. Bookshelf's
front frame makes the active frame convenient at `[0]`, but every nested push/pop
shifts an array-backed list.

## Do not copy literally

Passing an arbitrary callback string through a function macro is command injection
if source values can reach it. MDL should lower a typed function value to a
validated resource location or a finite dispatch ID. Similarly, any dynamic NBT
path or key inserted into a macro needs a syntax-aware encoding step.

A global lambda storage also makes callback results ephemeral and alias-prone.
Generated frames should own results until the caller moves or copies them, and the
effect system should state whether a callback may mutate its input collection or
the frame itself.

## Compiler consequence

The IR needs distinct operations for:

```text
invoke_predicate(callback, value, index) -> success
invoke_transform(callback, value, index) -> value
invoke_fold(callback, accumulator, value, index) -> accumulator
```

Treating them as a generic opaque `function` call throws away useful information:
the compiler can short-circuit predicates, scalar-replace numeric accumulators,
fuse transforms, and choose whether a result needs NBT materialization.

## Sources

- [Bookshelf `map`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/map)
- [Bookshelf `fold`](https://github.com/mcbookshelf/bookshelf/tree/c719575fe139445f0d70cb1af733c731e7382ba0/modules/bs.collection/data/bs.collection/function/fold)
- [stdmodulesystem list `for_each`](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68/data%20packs/collections/data/collections/function/list/for_each)
- [stdmodulesystem API convention](https://github.com/Samu64d/stdmodulesystem/tree/d85c51c0beff81cbad6b38add0b325481c2c8f68)
