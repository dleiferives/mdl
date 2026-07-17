# S-037 — Indexed List Removal Returns the Element

**Status:** selected on 2026-07-13.

`remove` accepts an index, removes the element at that position, and returns the
removed element:

```mdl
const removed: Op = work.remove(index);
const last: Op = work.remove(-1);
```

Its result type is the receiver list's element type. The index follows the ordinary
list-indexing rules selected by S-032, including end-relative negative indices.

`remove` is bounds-checked under S-033. If the list is empty or the index is outside
`-length .. length - 1`, the operation produces the selected bounds failure and does
not fabricate a value.

The receiver must be writable under S-036. The operation has one source-level
read-and-remove meaning: it obtains the selected element and removes that same
element, with no source operation interposed between those effects.

The returned value may be ignored:

```mdl
work.remove(index);
```

When the result is dead, the compiler may omit materializing or copying it. Source
code that never needs the element can state that intent more directly with S-011:

```mdl
delete work[index];
```

`remove` does not search for an equal element. A future value-searching operation
must use a distinct name such as `remove_value`, `find_remove`, or another explicitly
selected spelling, avoiding ambiguity for lists whose elements are themselves
integers.

The precise copying or move behavior for a returned compound/list element remains
part of the general value and ownership design.
