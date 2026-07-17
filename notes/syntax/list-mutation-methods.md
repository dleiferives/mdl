# S-036 — List Mutation Methods

**Status:** core method family selected on 2026-07-13; individual operation semantics
are selected in follow-up decisions beginning with S-037.

Lists provide ordinary member-call syntax for their fundamental mutation operations:

```mdl
work.push(value);
const operation := work.pop();
const removed := work.remove(index);
```

The receiver is the list being mutated. A mutating method call therefore requires a
writable receiver access path:

```mdl
var work: []Op = parse(source);
work.push(operation); // valid

const fixed: []Op = parse(source);
fixed.push(operation); // error: receiver is read-only
```

The call does not repeat a `mut` marker on the receiver. It is treated like assignment
or the `delete` statement: the operation directly names the place it may mutate, and
the compiler checks that place for write permission.

This does not remove S-004's caller-visible mutation rule for ordinary function
parameters:

```mdl
fn process(mut operations: []Op) {}

process(mut work); // `mut` remains required here
```

`push`, `pop`, and `remove` are type-provided list operations. Their member-call
syntax does not require an mcfunction dispatch; the compiler may lower them directly
to NBT operations, fuse adjacent mutations, aggregate construction, or choose another
representation-specific implementation.

This decision intentionally leaves general user-defined method declaration and
receiver syntax open.

S-037 selects `remove(index) -> ElementType` with checked, negative-capable indexing.
S-038 selects no-argument `pop() -> ElementType` as checked removal of the final
element.
S-039 selects `push(value) -> Void` as end insertion of one element.
