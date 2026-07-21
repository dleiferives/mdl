# PS-12: Composable Entity-NBT Paths (Retires the Hardcoded Book Intrinsic)

Status: **PS-12.0/PS-12A/PS-12B landed (bug fixes, grammar, schema+checker); PS-12C's HIR/Core
representation is landed but Minecraft-side lowering is not yet built — see
[`ps-12-entity-nbt-paths-todo.md`](ps-12-entity-nbt-paths-todo.md) PS-12C for the exact boundary
and why it was stopped there.**

## Identity

- **Capability:** replace the single hardcoded `main_hand_written_book_literal_page_or_empty`
  source method with a general, typed, composable entity-NBT path expression
  (`reader.equipment.mainhand.components."minecraft:written_book_content".pages[index].raw`),
  built on a compiler-known schema table and the already-generic PS-11 macro/crossing engine;
- **Substage:** PS-12, following PS-11 (Tier A landed in commit `7654bc0`);
- **Owner documents:** this plan, [`ps-12-entity-nbt-paths-todo.md`](ps-12-entity-nbt-paths-todo.md),
  the architecture of record [`../entity-nbt-path-composability.md`](../entity-nbt-path-composability.md),
  the syntax decision [`../../syntax/entity-paths-and-general-indexing.md`](../../syntax/entity-paths-and-general-indexing.md)
  (S-042), and three full specifications this plan implements against:
  [`../nbt-schema-system.md`](../nbt-schema-system.md) (the compiler-known dictionary mechanism,
  §2.4's table generalized), [`../references-design.md`](../references-design.md) (Tier B `ref`/
  `DataRef<T>` — related but not required for PS-12's exit), and
  [`../macro-system-specification.md`](../macro-system-specification.md) (the full `SyntaxSlot`
  taxonomy and ABI the crossing engine implements, including the exact PS-12.0 bug fixes);
- **Redirects:** the informally-discussed "PS-11D" (more `Java26_2*`-style hardcoded recipes)
  is explicitly rejected in favor of this milestone. See the design note Part 1 for why.
- **Depends on:** PS-11A–C (`Operand<T>`, `SyntaxSlot`, `NbtPathSegment::Index(Operand<i32>)`,
  the `crossings.rs` engine) — reused, not rebuilt.

## Why PS-12 exists

PS-11 built a genuinely generic macro-crossing engine but its only client remained one
hardcoded intrinsic with a hand-built six-segment NBT path baked into a "recipe." Investigation
for this plan found that intrinsic's dynamic-index path is **functionally broken** — a runtime
page index computes a value that is written to a scratch storage path and never reaches the
caller (`InstructionPlan::External` has no result-routing at all), and the macro renderer
separately hardcodes `entity @s` regardless of the real selector on `DataSource::Entity`. Full
detail, exact file:line evidence, and the fix shape are in the design note Parts 1.2–1.3.

PS-12 fixes both bugs, then builds the general mechanism the design note specifies: grammar for
compile-time string-literal member keys and general (const-or-runtime) bracket indices, a
unified chained-postfix checker replacing the bare-`Name`-only Minecraft-method dispatch for
this family, a compiler-known entity/item/component schema table, and a Core-level
`ExternalSemanticBinding::EntityNbtRead` that lowers directly to the existing generic engine —
never through the closed-verb `MinecraftSemanticKey`/`MinecraftRecipeId` system.

## Frozen scope decisions (see design note Part 2 for full rationale)

1. **Schema-typed, not arbitrary NBT.** Every compound-key path step (`.equipment`, `.mainhand`,
   `."minecraft:written_book_content"`, ...) resolves at compile time from a closed schema
   table. An unrecognized key is a compile error. Only **list indices** may be runtime `Int32`
   values. Runtime *string* keys stay explicitly out of scope — they are a documented open
   safety problem (`notes/mcfunction/nbt/dynamic-keys-and-path-safety.md`), not something to
   smuggle in here.
2. **Two additive grammar extensions, nothing else.** `PostfixSuffix` gains `"." StringLiteral`
   (compile-time compound key) and widens `"[" IntegerLiteral "]"` to `"[" Expression "]"`
   (checker enforces literal-only for the existing PS-5 tuple case, const-or-runtime for the new
   NBT-list case). No new top-level expression form; every existing accepted program still
   parses identically.
3. **Fail-soft default, generalized.** Preserve the frozen Phase-1 contract (init result to a
   type-appropriate default, attempt the read, keep the default on any broken link) for every
   path read, not just strings — this is not a new decision, it's the existing book-page
   contract made type-generic instead of hardcoded to `""`.
4. **The External-op mechanism is kept; the closed-verb recipe system is not used for this
   family.** Entity-NBT reads stay `CoreOp::External` (ambient-context/effect tracking is
   legitimate and worth keeping) but get a new `ExternalSemanticBinding::EntityNbtRead` variant
   instead of routing through `MinecraftOperationAttributes`/`MinecraftSemanticKey`/
   `MinecraftRecipeId`/`SelectedSemanticRecipe`. That system is unchanged and still correct for
   `Say`/`Teleport`/`MoveBy`, which remain genuine fixed-shape verbs.

## Semantic contract

1. An entity-NBT path expression's compile-time type is the terminal schema node's `ValueType`;
   the whole chain behaves as an ordinary value of that type from the checker onward (assignable,
   passable, comparable) — no residual "book page" special case anywhere past checking.
2. Every compound-key step must resolve to a known schema entry at compile time; there is no
   general dynamic-key lookup.
3. A list-index step accepts any `Int32` expression, constant or runtime; a constant lowers as a
   plain `NbtPathSegment::Index(Operand::Const)`, a runtime value as
   `Index(Operand::Runtime(_))`, exactly as `NbtPathSegment::Index` already supports.
4. The existing PS-5 positional-tuple `[k]` rule (literal-only, arity-checked at compile time) is
   unchanged for `AnonymousStruct` receivers; disambiguation from the new NBT-list case is by
   checked receiver type, not by grammar.
5. A read's destination is always the real per-occurrence result home (chosen by the existing
   plan/home-resolution machinery), never a placeholder/scratch path, on both the inline and
   macro-helper lowering routes.
6. `DataSource::Entity`'s selector is rendered faithfully on every route, including the macro
   renderer — never hardcoded to `@s`.

## Representation and IR ownership

See design note §2.5–2.6 for exact shapes. Summary:

- **HIR:** new `HirExpressionKind::EntityNbtPath { root: HirEntityPathRoot, segments:
  Vec<HirEntityPathSegment>, result_ty }` with `HirEntityPathSegment = Key(SchemaKeyId) |
  Index(Box<HirExpression>)`, replacing `HirMinecraftOperationAttributes::{BookPage,
  BookPageRuntime}` entirely.
- **Core:** new `ExternalSemanticBinding::EntityNbtRead { selector_proof, path: NbtPath,
  result_ty: CoreType }`.
- **Lowering:** no-runtime-segment paths lower inline (`DataCommand::Modify` with the real
  result home); ≥1-runtime-segment paths route through the (now-fixed) `crossings.rs` engine.
  `is_unusable_inline()` becomes derived ("has a `Runtime` index segment"), not authored.

## Test architecture

- **PS-12.0:** targeted regression tests for both bugs — a runtime-index book-page read that
  asserts the *value* reaches the caller's result home (not just command shape), and a
  macro-rendered entity read with a non-`@s` selector asserting the rendered text matches.
- **PS-12A:** lexer/parser goldens for `.` `StringLiteral` and `[Expression]` (const and
  identifier-expression forms), confirming every existing PS-5 fixture still parses unchanged
  (regression, not just addition).
- **PS-12B:** schema resolution tests — full chain success, unknown-key rejection at every
  schema depth, tuple-vs-NBT-list disambiguation by receiver type, chain result stored in an
  intermediate variable and continued.
- **PS-12C:** HIR/Core lowering tests mirroring PS-11's shape (inline vs. macro route), plus the
  integration fixture replacing `ps2_written_book_page.mdl` — critically including a genuinely
  runtime page index, closing the untested gap that let the PS-12.0 bug ship silently.
- **PS-12D:** four-policy differential + one ignored pinned Java 26.2 lifecycle over the new
  fixture, matching every prior PS milestone's exit gate.
- Gates every tranche: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --
  -D warnings`; `cargo test --workspace --all-targets`.

## Non-goals

- Runtime/dynamic string keys in path position (stays an open, explicitly deferred problem).
- First-class `Reference`/source-level `DataRef<T>` (Tier B of the value-crossing model —
  related, not required by this milestone; PS-12's schema-typed chain is deliberately narrower
  and does not need general aliasing/invalidation analysis).
- Growing the schema table beyond what's needed to express the book-page chain plus enough
  structure (`ItemStack`, `EquipmentSlots`) to prove the mechanism is genuinely extensible —
  broader schema coverage (other components, block NBT, other entity fields) is explicit
  follow-up, added by table rows, not by repeating this milestone's mechanism work.
- Entity-NBT **writes** (only reads are in scope; the design note's mechanism is read-shaped).
- `Dispatch` encoding, cost-directed encoding selection, crossing placement optimization (owned
  by PS-11's later tiers, unaffected by this milestone).

## Exit criteria

- Both PS-12.0 bugs are fixed with regression tests proving the fix (value reaches the caller;
  selector is honored).
- `reader.equipment.mainhand.components."minecraft:written_book_content".pages[index].raw`
  compiles, type-checks, and executes correctly for both a compile-time-constant and a genuinely
  runtime `index`, on the Core evaluator differential and the pinned server.
- `main_hand_written_book_literal_page_or_empty`, `MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage`,
  and every site in the design note's §2.7 deletion list are gone — no parallel/legacy path
  remains.
- Adding a new schema field (documented in the handoff as a worked example, e.g. `.count` on
  `ItemStack`) requires only a new table row plus its checker/HIR/lowering generalization
  already covers it — proven by actually adding one as part of the exit audit.
- The closed-verb recipe system (`MinecraftRecipeId`/`SelectedSemanticRecipe`) is unchanged in
  behavior for `Say`/`Teleport`/`MoveBy` and contains no `BookPage`-shaped remnant.

## Research basis

- [`../entity-nbt-path-composability.md`](../entity-nbt-path-composability.md) — full design,
  bug evidence, deletion surface.
- [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md) — the
  value-crossing model this extends.
- [`../../mcfunction/written-books-26.2.md`](../../mcfunction/written-books-26.2.md),
  [`../../mcfunction/nbt/compiler-known-dictionaries.md`](../../mcfunction/nbt/compiler-known-dictionaries.md),
  [`../../mcfunction/nbt/dynamic-keys-and-path-safety.md`](../../mcfunction/nbt/dynamic-keys-and-path-safety.md)
- [`ps-5-anon-structs-destructuring-plan.md`](ps-5-anon-structs-destructuring-plan.md) — the
  `[k]`/member-projection precedent this generalizes.
