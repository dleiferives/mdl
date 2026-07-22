# PS-12 Handoff

Status: **complete on 2026-07-21**

PS-12 replaces the one hardcoded book-page intrinsic
(`main_hand_written_book_literal_page_or_empty`) with a general, typed, composable
entity-NBT path expression built on a compiler-known schema table and the PS-11 generic
macro/crossing engine, then deletes the retired intrinsic entirely once the replacement had
full differential and structural evidence. Full design:
[`../entity-nbt-path-composability.md`](../entity-nbt-path-composability.md). Staged
implementation record: [`ps-12-entity-nbt-paths-plan.md`](ps-12-entity-nbt-paths-plan.md),
[`ps-12-entity-nbt-paths-todo.md`](ps-12-entity-nbt-paths-todo.md).

## Public language surface

- `receiver.key1.key2...` — compile-time compound-key member access on an entity-NBT chain,
  keyed off a closed schema table per `EntityKind` (`frontend/entity_schema.rs`). An
  unrecognized key is a compile error.
- `receiver.key."minecraft:resource_id".key2` — string-literal key access, for schema fields
  whose real NBT key is not a valid MDL identifier (e.g. namespaced component IDs).
- `receiver.key[index]` — list-index access into a schema `List` node. `index` may be a
  compile-time constant or a genuinely runtime `Int32` expression; either lowers correctly.
- The whole chain's compile-time type is the terminal schema node's type; from the checker
  onward it behaves as an ordinary value of that type (assignable, passable, comparable) — no
  residual special case.
- Fail-soft contract, preserved from the original book-page intrinsic and generalized to every
  type: the result is initialized to a type-appropriate default (`""`/`0`/`false`), the read is
  attempted, and the default survives any broken link (wrong item, missing component, absent
  field) rather than failing the command.
- Current schema coverage (`ArmorStand` root): `.equipment.{mainhand,offhand,head,chest,legs,
  feet}` → `ItemStack { .components."minecraft:written_book_content" {
  .pages[i].raw: String, .author: String, .title.raw: String, .resolved: Bool }, .count: Int32
  }`.

## Compiler representation and lowering

Chains are schema-typed, not arbitrary NBT: every compound-key step resolves at compile time
against `frontend/entity_schema.rs`'s closed `SchemaNode` table; only list indices may be
runtime values. HIR carries `EntityNbtPath { root, segments, result_ty }`; Core carries a
dedicated `EntityNbtReadDecl` table (`ir/core/entity_nbt.rs`) with plain `Box<str>` keys —
Core stays target-independent, matching how `MinecraftOperationAttributes` never references
`ir::minecraft` rendering types.

`ExternalSemanticBinding::EntityNbtRead` deliberately bypasses the closed-verb
`MinecraftSemanticKey`/`MinecraftRecipeId`/`SelectedSemanticRecipe` system entirely (§2.5 of
the design note): that system exists to triple-verify one fixed command shape per semantic key,
and a schema-driven path has no fixed shape to verify against. Instead, Minecraft-target
lowering builds a third, parallel, minimal system mirroring that system's *shape* (one
preflight-resolved table, one `InstructionPlan::EntityNbtRead` variant, one emit arm) without
its *contract-verification content*.

- **Inline route** (every index segment constant): lowers directly to `data modify ... set
  from entity ...`, targeting the caller's real result home, with no separate helper function.
- **Macro-helper route** (≥1 runtime index segment): routes through the unchanged PS-11
  `crossings.rs` engine — proof that generalizing that engine from "one optional runtime
  index" to "any number of runtime index segments" was real, since this path required zero
  changes to the engine itself, only to how the caller builds the base command.
- **Representation split**: `String`/`ListI32` results live in NBT storage homes; `Bool`/`I32`
  results live in scoreboard homes. Since `ir::minecraft::DataCommand::Get` has no
  entity-source form, a `Bool`/`I32` read is a two-step bridge — read into a shared scratch NBT
  slot (`LoweringPlan::entity_nbt_scalar_scratch()`), then `execute store result score ... run
  data get storage <scratch>` to convert.

## Evidence

- `crates/mdl-compiler/tests/ps12c_entity_nbt_path_lowering.rs`: structural pack-text
  assertions for the inline route (no macro infrastructure), the macro-helper route (frame
  seeded, runtime index reaches the real result home — the exact PS-12.0 regression, now
  through the general path), the `Bool` scratch/score conversion, and the `.count`
  extensibility proof on a shorter, differently-shaped chain.
- `tests/source-fixtures/pre-scheduler/ps12_entity_nbt_path.mdl`: four-policy
  (`CoreOptimizationLevel::{None,Baseline} × MinecraftOptimizationLevel::{None,Baseline}`)
  differential via `source_fixtures.rs`, with a genuinely runtime index — closing the exact
  untested gap that let the original two engine bugs (PS-12.0) ship silently.
- `crates/mdl-test/tests/ps12_entity_nbt_path_semantics.rs`: `#[ignore]`d pinned Java 26.2
  server lifecycle test (summon an armor stand with a written book, run the compiled function,
  assert the correct page and the fail-soft wrong-item fallback).
- `tests/programs/brainfuck/src/interpreter.mdl` (`run_book_showcase`) exercises the inline,
  `String`-result route 100 times as part of the PS-3 capstone's own four-policy differential
  and pinned-server evidence — real, load-bearing usage, not a purpose-built test fixture.

## Deletion (PS-12E)

The retired intrinsic (`main_hand_written_book_literal_page_or_empty` /
`MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage` /
`MinecraftOperationAttributes::BookPage` / `SelectedSemanticRecipe::Java26_2BookPage` /
`MinecraftRecipeId::Java26_2ReadMainHandWrittenBookLiteralPage`) is fully deleted — every match
arm across the frontend, Core, and Minecraft-target lowering, plus tests whose entire premise
was the old intrinsic. `grep -rn "BookPage\|WrittenBookPageIndex\|ReadMainHandWrittenBookLiteralPage\|main_hand_written_book_literal_page_or_empty\|Java26_2BookPage" crates/`
returns nothing. The brainfuck capstone (the one real, non-test-fixture program using the
intrinsic) was migrated to the new syntax; its pinned exact-footprint numbers needed **no
changes**, since the new inline route renders through the same `NbtPath` `Display` impl the
old recipe used, producing byte-identical `.mcfunction` text for the literal-index,
`String`-result case. `ir/core/eval.rs`'s `SkippedMacroExternal` special case (BookPage-only,
zero consumers anywhere) was deleted rather than given an `EntityNbtRead` equivalent, since
`EntityNbtRead` was already correctly falling through to the generic `UnsupportedExternal`
path.

## Extensibility proof

`.count: Int32` was added to `ItemStack` in `frontend/entity_schema.rs` as a pure table-row
change — confirmed by grep that only the schema table literal and its two new tests changed;
no new Core op, recipe, or checker special-case was needed. Proved at two levels: a
schema-table unit test (reachability/typing) and a full compiler-level lowering test (a real
source program using `.count` compiles and lowers correctly), deliberately on a shorter chain
shape (`equipment.mainhand.count`, no `components`/resource-id step) than the book-page family,
so the proof covers genuine chain-shape variation, not a renamed copy.

## Deliberately deferred

- Runtime/dynamic string keys in path position — open safety problem
  (`notes/mcfunction/nbt/dynamic-keys-and-path-safety.md`), not solved here.
- First-class `Reference`/source-level `DataRef<T>` (the value-crossing model's Tier B) —
  related, not required by this milestone's schema-typed chain.
- Broader schema coverage (other components, block NBT, other entity kinds/fields beyond the
  book-page chain and the `.count` proof) — explicit follow-up, added by table rows.
- Entity-NBT writes — only reads are in scope; the mechanism is read-shaped.
- `Dispatch` encoding, cost-directed encoding selection, crossing placement optimization — owned
  by PS-11's later tiers, unaffected by this milestone.

## Reproduction

Fast target-independent and lowering evidence:

```sh
cargo test -p mdl-compiler --test ps12c_entity_nbt_path_lowering
cargo test -p mdl-compiler --lib frontend::entity_schema
cargo test -p mdl-test --test ps3_brainfuck
```

Pinned vanilla evidence:

```sh
MDL_SERVER_JAR=/path/to/minecraft-server-26.2.jar \
MDL_JAVA=/path/to/java25 \
cargo test -p mdl-test --test ps12_entity_nbt_path_semantics \
  entity_nbt_path_runtime_index_matches_java_26_2 \
  -- --ignored --nocapture
```
