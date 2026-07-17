# S-039 — `push` Appends and Returns `Void`

**Status:** selected on 2026-07-13.

`push(value)` appends an element to the end of a writable list and returns `Void`:

```mdl
work.push(operation);
```

The argument must have the receiver list's element type under the language's normal
typing and conversion rules. The receiver must be writable under S-036.

`push` does not implicitly produce the new length, inserted index, receiver, or a
backend command-success flag. Code that needs the current length requests it
explicitly:

```mdl
work.push(operation);
const count := work.length();
```

Keeping the result `Void` prevents an otherwise unused result from forcing length
materialization or mutable receiver-return semantics. A well-typed list operation
also does not expose incidental Minecraft command success as its source result.

The compiler may aggregate consecutive pushes, fold pushes into initial list
construction, use an NBT append operation, or choose another representation-aware
lowering when observable semantics permit it.

This decision does not yet determine whether one call may accept multiple elements,
whether a separate bulk-extension method exists, or whether element insertion copies
or moves compound/list values.
