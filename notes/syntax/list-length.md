# S-034 — List Length Method Intrinsic

**Status:** selected on 2026-07-13.

The current length of a list is obtained with a member-call expression:

```mdl
const count := tape.length();

while (work.length() > 0) {
    // ...
}
```

`length()` is an intrinsic operation provided by the receiver's `[]ElementType`.
The static type tells the compiler that the operation exists and how to type-check
it; the dynamic list type does not itself contain a compile-time length.

The parentheses are intentional. Unlike a field access such as `machine.pointer`, a
length query may require runtime work. For a storage-backed list, one possible
lowering obtains the result of `data get`, whose result for a list is its element
count. Other representations and uses may admit different lowerings.

Member-call syntax does not require the backend to emit an mcfunction call. The
compiler may lower `length()` directly to commands, use already tracked metadata,
fold a known literal length, hoist or reuse a query, or avoid materializing the count
entirely. For example, a comparison such as `work.length() > 0` may be implemented as
an element-existence test when that preserves the same observable behavior.

The compiler should therefore preserve list-length intent in typed IR until a
representation-aware lowering or optimization can select the implementation.

This decision selects one built-in member call but does not yet establish:

- general user-defined method declaration syntax;
- how mutating receiver calls display caller-visible `mut` permission;
- whether other types use `length()` or a differently named size operation;
- the exact integer type returned by `length()`; or
- any constant-time cost guarantee.

Reference: [Minecraft `/data` command results](https://minecraft.wiki/w/Commands/data).
