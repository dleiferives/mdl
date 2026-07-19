# Written Books on Java 26.2

Status: **Measured on the official Java 26.2 dedicated server on 2026-07-18.**

Java 26.2 no longer stores living-entity hand items under `HandItems`. A controlled
armor stand summoned with a written book serialized the relevant value as:

```snbt
equipment: {
  mainhand: {
    id: "minecraft:written_book",
    count: 1,
    components: {
      "minecraft:written_book_content": {
        pages: [{raw: "ab😀+"}],
        author: "mdl",
        title: {raw: "test"},
        resolved: 1b
      }
    }
  }
}
```

The directly readable path for a page whose `raw` component is a literal string is:

```mcfunction
data modify storage mdl:state page set from entity @s equipment.mainhand.components."minecraft:written_book_content".pages[0].raw
```

The `raw` value normalized to a string, not `{text: ...}`. Appending `.text` fails.
The older `HandItems[0]` path also fails. Both failures were observed before the
working path was pinned; this is why target-version recipe selection owns the path.

## Phase-1 compiler contract

The initial typed intrinsic reads one compile-time page index from the current exact
executor's main-hand written book. It initializes its result to `""`, then attempts
the entity read. Missing equipment, a wrong item, an absent page, or a non-string
`raw` component therefore yields an empty string without retaining a stale result
from a prior invocation.

This narrow operation deliberately does not claim to flatten arbitrary text
components. Inline literal strings are supported. Styled compound/list components,
translation, selectors, scores, keybinds, and NBT interpolation require a future
typed `Text -> String` contract rather than guessing client rendering.

The page index is compiler-validated in `0..99` and never comes from runtime string
data, so no user value is interpolated into an NBT path or command.

## String slicing observed in the same fixture

Given `"ab😀+"`:

```mcfunction
data modify storage mdl:state tail set string storage mdl:state whole -1
data modify storage mdl:state prefix set string storage mdl:state whole 0 -1
```

produced `tail: "+"` and `prefix: "ab😀"`. Minecraft's indices are Java UTF-16
code-unit indices. Removing the final ASCII opcode is safe; traversing a
supplementary character one code unit at a time exposes surrogate halves and is not
Unicode-scalar traversal.

## Evidence

- Vanilla path, slice, wrong-item, and compiled-intrinsic regression:
  [`../../crates/mdl-test/tests/ps2_book_semantics.rs`](../../crates/mdl-test/tests/ps2_book_semantics.rs)
- Four-policy compiler fixture:
  [`../../crates/mdl-compiler/tests/source-fixtures/pre-scheduler/ps2_written_book_page.mdl`](../../crates/mdl-compiler/tests/source-fixtures/pre-scheduler/ps2_written_book_page.mdl)
- [Java 26.2 release](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Item stack and written-book component introduction](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-20-5)
- [Inline SNBT text components](https://www.minecraft.net/en-us/article/minecraft-snapshot-25w02a)
