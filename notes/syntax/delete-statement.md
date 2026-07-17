# S-011 — Delete Statement

**Status:** surface syntax selected on 2026-07-13; supported places and failure
semantics remain open.

Deletion is a keyword statement followed by a place expression:

```mdl
delete work[-1];
```

The required semicolon follows S-010. `delete` is not an ordinary function or
receiver method, and it does not produce a value:

```mdl
const removed := delete work[-1]; // error: delete is a statement
```

The operand is evaluated as a place rather than copied out as a value. Deletion
removes that place; it is distinct from assigning a type's default value.

Deletion is an explicit write operation, so it does not use the call-site `mut`
marker. A place reached through a `const` binding is still not writable:

```mdl
const work: WorkList = get_work();
delete work[-1]; // error: the place is read-only through work
```

Assignment-capable and deletion-capable are not automatically identical. The type
and place model must define which local collections and external Minecraft-backed
places support deletion.

The following remain separate decisions:

- behavior when the selected place does not exist;
- behavior for an invalid or out-of-bounds index;
- which collection removals shift later indices;
- whether deletion exposes command success through another construct; and
- how storage, entity, block, and score places are written in source.

Lowering may select `data remove`, a collection operation, or another target-specific
implementation as long as it preserves the selected source semantics.
