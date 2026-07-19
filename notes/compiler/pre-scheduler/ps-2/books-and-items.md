# PS-2 Books, Items, and Holders

Status: **narrow literal-page intrinsic implemented and pinned on Java 26.2**

## Goal

Expose a small typed API that obtains program text from a written book while keeping
Minecraft holder/cardinality/context semantics explicit.

## Separate semantic roles

Do not collapse these types merely because one command mentions all of them:

```text
EntityQuery<T, Cardinality>
Executor<T>
EntityRef<T>
InventoryHolder<K>
ItemSlot<K>
ItemStack<ItemKind>
WrittenBook
Text
String
```

Players, mobs, containers, and blocks may share inventory/slot capabilities without
sharing executor methods or every slot. Method availability follows proven
capabilities, not inheritance from one giant `Entity` struct.

## Implemented first API shape

An exact-one proven holder should be able to request a typed slot and inspect an
optional compatible item. A written-book operation then returns pages as structured
text or semantic strings according to a frozen conversion contract.

The broad conceptual model remains:

```text
holder.inventory().slot(main_hand).written_book()
book.pages()
pages.to_program_string()
```

The current narrow spelling is
`executor.main_hand_written_book_literal_page_or_empty(static_index) -> String`.
The page index is restricted to `0..99`. Missing equipment, wrong item, absent page,
and non-string `raw` content produce an empty string after compiler-owned fallback
initialization. Rich absence/error variants remain a later API refinement.

## Context and cardinality

A possibly-many query cannot masquerade as one inventory value. The source must
either prove exact/at-most-one and handle absence, or use an explicit `run.as(...)`
fork whose body receives one executor capability per invocation.

Reading a slot does not implicitly change position or dimension. Player-held-book
attribution and subsequent `say` execution depend on the current executor separately.

## Target boundary

For Java 26.2 the pinned entity path is:

```text
equipment.mainhand.components."minecraft:written_book_content".pages[i].raw
```

The recipe uses structured entity-data source IR; no rendered user component or
runtime path enters command syntax. See
[`../../../mcfunction/written-books-26.2.md`](../../../mcfunction/written-books-26.2.md).

The implemented boundary:

- pin the generated command report and item/component syntax;
- measure the chosen read path on the official server;
- represent item slots, components, holders, and structured text as validated target
  atoms/commands;
- retain target preflight recipe IDs and exact source correlation;
- keep read effects/cardinality/outcomes distinct from inventory mutation; and
- materialize pages into compiler-owned semantic values through explicit recipes.

No user string is rendered as an NBT path, component fragment, or command.

## Clientless versus player evidence

Most book representation and extraction behavior can be tested with a controlled
non-player holder or item/container if the exact command/component path is shared.
However, “the book in a real player's main hand” requires a connected player. A
vanilla server cannot summon a player.

PS-2 should prove the holder-independent typed operation clientlessly and research a
narrow PS-3 bot/client fixture. Do not implement the Minecraft network protocol or a
general client simulator inside `mdl-test` merely to satisfy this case.

## Required evidence

- absent holder/empty slot/wrong item/written book distinctions;
- empty, one-page, and multi-page books;
- page order, delimiter, formatting, and Unicode conversion;
- maximum content and command/NBT length boundaries;
- exact-one versus possibly-many holder typing;
- executor attribution independent from inventory access;
- cross-policy structured lowering and vanilla observations; and
- no book/item runtime support in programs that do not use it.
