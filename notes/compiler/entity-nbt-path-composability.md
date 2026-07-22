# Composable Entity-NBT Paths — Retiring the Hardcoded Book Intrinsic

Date: 2026-07-20
Status: **architecture of record for PS-12 (PS-12.0 through PS-12E all landed — every stage in
Part 3's staging is complete, the retired intrinsic is fully deleted, no parallel/legacy path
remains); extends [`macro-reference-crossing-model.md`](macro-reference-crossing-model.md). See
[`pre-scheduler/ps-12-handoff.md`](pre-scheduler/ps-12-handoff.md) for the closing summary, and
[`block-entity-nbt-paths.md`](block-entity-nbt-paths.md) for BE-1, the landed next generalization
(block-entity/container reads, extending this system rather than replacing it).**

## Why this note exists

PS-11A–C landed a genuinely generic macro engine (`Operand<T>`, `SyntaxSlot`,
`NbtPathSegment::Index(Operand<i32>)`, the `crossings.rs` COLLECT→FRAME→BRIDGE→RENDER→CALL
pipeline). But the only client of that engine is still `main_hand_written_book_literal_page_or_empty`
— one hardcoded source method that reads exactly one fixed NBT path
(`equipment.mainhand.components."minecraft:written_book_content".pages[N].raw`) off the
current executor. Continuing in the direction of "PS-11D: add more `Java26_2*` recipes for
more hardcoded entity reads" repeats the mistake instead of fixing it. This note designs the
replacement: a general, composable, typed entity-NBT path expression, of which the book-page
read becomes one ordinary instance — and it retires the intrinsic entirely rather than
building alongside it.

This is not a cosmetic complaint. Investigation (recorded in full below) found the dynamic
half of the just-landed engine is **functionally broken** — a genuinely runtime page index
computes a value that is written to a scratch storage path and never reaches the caller. No
test caught it because every existing fixture uses a literal index. Fixing that bug and fixing
the design are the same piece of work, because the bug is a direct consequence of the
hardcoded-intrinsic shape not having a real "result" concept to route through.

## Part 1 — What is wrong today, precisely

### 1.1 The closed-verb system is the wrong tool for a path read

`MinecraftSemanticKey → MinecraftRecipeId → SelectedSemanticRecipe` (`ir/semantic/minecraft.rs`,
`lower/minecraft/preflight.rs`) is a **closed-verb** system: one semantic key names one
totally fixed command shape, triple-verified against a hardcoded contract
(`RecipeContractProjection::{matches_semantic_descriptor, matches_target_contract,
matches_local_cost}`), with exactly one recipe per target version
(`MinecraftRecipeId::for_semantic_key` is a total match over `(target, key)`). This is the
right abstraction for `Say`, `TeleportCurrentExecutor`, `MoveCurrentExecutorBy` — each is
genuinely one atomic, always-fixed-shape verb.

`ReadMainHandWrittenBookLiteralPage` was forced through the same mold even though it isn't a
verb at all — it's one specific instance of "read an NBT path off an entity," an open,
compositional family. Evidence this was the wrong fit:

- `written_book_page_path()` (`preflight.rs:465-479`) hand-builds a six-segment `NbtPath`
  literal inside the recipe. There is nothing else like it — no other recipe embeds an
  open-ended data structure; every other recipe's shape is truly fixed.
- The closed descriptor table (`ir/semantic/minecraft.rs`) has exactly 4 entries
  (confirmed exhaustively: `MinecraftSemanticKey::ALL`, `minecraft_source_methods().len() == 4`
  asserted in tests). Every future entity-NBT field (health, tags, other item components,
  other equipment slots) would need its own key, its own descriptor, its own recipe, its own
  contract projection — an intrinsic explosion, not composition.
- `frontend/hir.rs`'s `HirMinecraftOperationAttributes::{BookPage, BookPageRuntime}` are the
  only variants in that enum carrying an open path shape; everything else carries a small
  fixed tuple (a message, a position, an offset).

### 1.2 Confirmed bug: the dynamic path's result is lost

Traced end to end (`lower/minecraft/plan/assemble.rs`, `emit.rs`, `crossings.rs`,
`preflight.rs`):

- `InstructionPlan::External { helper: PlannedFunctionId }` (`plan.rs:182-185`) — note there is
  **no `results` field**, unlike its sibling `InstructionPlan::Minecraft { external, recipe,
  results: Box<[ScalarResultPlacement]> }` (`plan.rs:186-193`). `flatten_instruction_plan`
  routes any `is_unusable_inline()` recipe to `External`, discarding the result placement that
  had already been computed.
- `emit.rs`'s `InstructionPlan::External` arm (300-335) never references any result home. It
  seeds the macro frame, bridges runtime *operands* in, and calls the helper — full stop.
- The helper body itself (`emit.rs:539-552` → `crossings::render_as_macro`) is built from
  `recipe.command_kind()`, whose `target` for `Java26_2BookPage` is a **placeholder** storage
  path, `mdl:preflight`/`book_page` (`preflight.rs:426-435`) — a scratch location that exists
  only so preflight can validate command shape/cost. It is never read back into the caller's
  actual `String` value home. That wiring exists **only** on the static-index path
  (`emit.rs:365-398`, which correctly targets `context.plan().string_storage(result.home())`).
- Consequence: `book.main_hand_written_book_literal_page_or_empty(some_runtime_int)` computes
  the page text, writes it to `mdl:preflight book_page`, and the caller's binding keeps
  whatever was in its home before (typically the empty-string default from the earlier `set
  value ""` init on a *different* path) — the read is silently discarded.
- No test catches this: `tests/programs`/`source-fixtures/pre-scheduler/ps2_written_book_page.mdl`
  and the ignored server test both use a literal index (`0`), which takes the static/inline
  path where the wiring is correct. `emit.rs`'s own `runtime_book_page_emits_generic_bridge_and_macro_call`
  test only asserts the macro's *shape* (frame seed / bridge / call / `$`-line text) — it never
  asserts the read value reaches the caller.

**The fix is simple, not structural.** `InstData::results() -> &[ValueId]` (`ir/core/mod.rs:1041`)
already exposes the External instruction's own result values, and `data: &InstData` is already
in scope at the exact bug site (`emit.rs:289`). No plan-level plumbing is needed — the macro
path just has to do what the static path already does: resolve each result `ValueId`'s home via
`plan.value_home(function, value_id)` and target that home instead of a scratch path.

### 1.3 Confirmed bug: hardcoded `@s` in the macro renderer

`DataSource::Entity { selector: Selector, path: NbtPath }` (`ir/minecraft/data.rs:30`) carries
a real selector — `render.rs`'s ordinary (non-macro) renderer honors it correctly
(`from entity {selector} ` at `render.rs:295-298`). But `crossings.rs::render_data_modify_as_macro`
destructures `DataSource::Entity { path, .. }` (discarding `selector`) and hardcodes the literal
text `"... set from entity @s "` (`crossings.rs:159`). This is currently invisible because the
only caller always constructs `Selector::SelfExecutor` — but it means the "generic" engine is
not actually generic over *who* is read from, only over *what path* is read. Any composable
reference that lets `reader` be something other than "the current executor" (a captured
`EntityRef`, a queried entity) would silently emit the wrong selector. Fix: render `{selector}`,
matching `render.rs`.

## Part 2 — The general design

### 2.1 Scope discipline: schema-typed, not arbitrary NBT

Per `notes/mcfunction/nbt/dynamic-keys-and-path-safety.md`: rendering an arbitrary *runtime
string* as one safe NBT-path segment is an open, unsolved safety problem in this codebase
(`MacroNbtKey` encoding does not exist yet). This design does **not** open that door. Every
compound-key step in a path (`.equipment`, `.mainhand`, `.components`, the
`"minecraft:written_book_content"` component id) is resolved at **compile time** from a closed,
compiler-known schema table — never from a runtime string. Only **list indices** may be
runtime values (`Int32`), which is exactly the case `Operand<i32>` on `NbtPathSegment::Index`
already supports safely (a validated integer, never spliced text). This keeps the feature
inside ground already surveyed and frozen, and doesn't smuggle in the harder, unsolved
dynamic-key problem.

This matches `notes/mcfunction/nbt/compiler-known-dictionaries.md`'s `KnownShape(schema,
per-field representation)` point on the representation lattice: a compound can be "concrete"
(known key set, known per-field types) without every value being constant.

### 2.2 Source syntax

Two small, orthogonal grammar extensions (both additive — neither changes an existing
production's accepted set):

1. **Compile-time string-literal member key**, for compound keys that aren't valid `Name`s
   (resource ids contain `:`):
   ```
   PostfixSuffix = "." Name | "." StringLiteral | Arguments | "[" Expression "]" ;
   ```
   `.components."minecraft:written_book_content"` parses via the new `"." StringLiteral` arm.
   `StringLiteral` already exists as a token/production (`grammar.ebnf:216`); this only adds it
   to `PostfixSuffix`. Checked identically to `.Name` except the key comes from string content,
   not spelling, and the checker requires it to be a **known** key in the receiver's schema —
   never a general dynamic-dictionary lookup.
2. **General index expression**, widening the existing PS-5 bracket production:
   ```
   PostfixSuffix = ... | "[" Expression "]" ;   (was "[" IntegerLiteral "]")
   ```
   `Expression` is a strict superset of `IntegerLiteral`, so every existing PS-5 positional
   tuple-index program (`pair[0]`) still parses identically. The **checker**, not the grammar,
   enforces "tuple index must be a compile-time literal" (`check_index_expression`'s existing
   PS-5 rule, unchanged) vs. "NBT list index may be any Int32 expression, const or runtime"
   (the new rule, for a receiver typed as a known NBT-list schema). Disambiguation is by the
   checked receiver type, exactly like the existing `check_member_expression`/
   `check_index_expression` split already disambiguates struct vs. anonymous-struct receivers.

Both extensions are additive to `PostfixExpression = PrimaryExpression, { PostfixSuffix }` — no
new top-level expression form, no change to precedence or existing suffixes (`.Name`,
`Arguments` are untouched).

### 2.3 The checker: unify chained postfix resolution

Today `check_member_expression` and `check_index_expression` only understand `Struct`/
`AnonymousStruct` receivers, and Minecraft-method dispatch (`check_minecraft_method_call`) is a
**separate mechanism** triggered only at `Call` sites, hard-requiring `receiver.kind` to be a
bare `AstExpressionKind::Name` present in `active_executor_captures` (`check.rs:4824`) — so
`reader.equipment.mainhand.say()` fails today; only `reader.say()` works. This is the actual
parser/checker gap: **no chained receiver resolution exists at all.**

The fix is not a new parallel mechanism; it's teaching `check_member_expression` and
`check_index_expression` a new receiver-type case, so chaining falls out of the existing
recursive-descent shape (`check_member_expression` already calls `self.check_expression(receiver,
...)` first, which recurses through nested `Member`/`Index` nodes naturally):

- `SemanticType::Executor(ExecutorType)` gains an entity-NBT-schema root. `.equipment` on an
  `Executor` receiver checks against a new `EntityPathSchema` table (§2.4) instead of falling
  into the current catch-all `UNRESOLVED_MEMBER`.
- Each further `.name`, `."string"`, or `[index]` step looks up the **current** schema node's
  known children and narrows to the child's schema/result type. A step is an outright compile
  error — not a runtime fallback — if the key isn't in that node's known set (e.g.
  `.components."minecraft:unknown_component"` never compiles); this preserves MDL's typed-source
  philosophy (no untyped NBT roaming) and matches the closed-key-set spirit of the descriptor
  table it replaces.
- The terminal step's schema node carries the produced `ValueType` (`String`, `Int32`, `Bool`,
  ...), which becomes the whole chain's checked type — an ordinary value from here on, usable
  anywhere a value of that type is usable (assigned, passed, compared), same as any other
  expression. No new "book page" special case.
- `check_minecraft_method_call`'s bare-`Name`-receiver restriction is unaffected for the
  remaining real verbs (`say`, `teleport`, `move_by`) — those still dispatch by call-site name
  match; only the entity-NBT path family moves to the general member/index checker.

### 2.4 Schema table: compiler-known entity/item/component dictionary

A small, explicit, extensible table — not a hardcoded path, a **data-driven closed dictionary**
matching the 26.2-measured shape in `notes/mcfunction/written-books-26.2.md`:

```text
Executor
  .equipment              -> EquipmentSlots
EquipmentSlots (compound, fixed keys)
  .mainhand, .offhand, .head, .chest, .legs, .feet   -> ItemStack
ItemStack (compound)
  .id                      -> String            (resource id text)
  .count                   -> Int32
  .components              -> Components
Components (compound, keyed by known resource-id string literals only)
  ."minecraft:written_book_content"  -> WrittenBookContent
WrittenBookContent (compound)
  .pages                   -> List<BookPageEntry>   (runtime OR const Int32 index via [ ])
  .author                  -> String
  .title                   -> BookPageEntry
  .resolved                -> Bool
BookPageEntry (compound)
  .raw                     -> String
```

Adding a new field later (another item component, another equipment-adjacent entity field) is
one new table row with its known key and result schema — not a new `MinecraftSemanticKey`, not
a new recipe, not a new contract projection. This directly satisfies "composable, not
hardcoded": the *mechanism* (typed path walk → NBT path → generic engine) never changes; only
the *data* describing what's known grows.

Each intermediate step is a real value that (like the struct/anonymous-struct chains PS-5
already supports) can be stored in a variable — `const item := reader.equipment.mainhand;` then
`item.components...` — with the checker tracking the accumulated `NbtPath` prefix and target
kind (entity + selector) behind the scenes, not the source spelling.

### 2.5 HIR/Core representation

One new HIR expression kind replaces `HirMinecraftOperationAttributes::{BookPage,
BookPageRuntime}` entirely:

```text
HirExpressionKind::EntityNbtPath {
    root: HirEntityPathRoot,          // Executor { proof: HirContextStep, kind: EntityKind }
    segments: Vec<HirEntityPathSegment>,
    result_ty: ValueType,
}
HirEntityPathSegment = Key(SchemaKeyId) | Index(Box<HirExpression>)   // Index is Int32-typed, const or runtime
```

Lowered to Core as a **new** `ExternalSemanticBinding` variant — not routed through
`MinecraftOperationAttributes`/`MinecraftSemanticKey` at all:

```text
ExternalSemanticBinding::EntityNbtRead {
    selector_proof: <ambient executor/entity proof>,
    path: NbtPath,                    // NbtPathSegment::Key for schema keys (always Const),
                                       // NbtPathSegment::Index(Operand<i32>) for list indices
    result_ty: CoreType,
}
```

This keeps the legitimate value of the External-op mechanism (ambient-context requirements,
effect tracking, ordering barriers — the same reasons `Say`/`Teleport` are External ops) while
dropping the closed-verb recipe apparatus, which has nothing to offer a structural path walk:
there is no alternative "recipe" for reading a fixed NBT path on a target version — only the
*schema* (which keys are valid) is version-dependent, and that's a frontend/typing-time concern
already owned by §2.4, not a lowering-time recipe choice.

### 2.6 Lowering — reuse, don't re-hardcode

- **No runtime segments** (`path.segments()` has no `Operand::Runtime`): lower inline, directly
  to `DataCommand::Modify { target: <real result home>, mode: Set, source: DataSource::Entity {
  selector, path } }` — exactly the shape `emit.rs`'s current static book-page arm already
  builds correctly (365-398), generalized to any schema-derived path/selector/result type
  instead of one hand-built path. No recipe/contract-projection table involved.
- **≥1 runtime segment**: `is_unusable_inline()` becomes the **derived** predicate "path has a
  `Runtime` index segment" (matching the value-crossing model's Tier-A intent) rather than a
  hand-authored match arm — route to a helper via `crossings.rs`, generalized (per §1.2, §1.3)
  to target the real result home and the real selector instead of the placeholder/`@s` bugs.
- **Fail-soft default preserved, generalized**: keep the existing two-command shape (init result
  home to a type-appropriate default — `""`/`0`/`false` — then attempt the read) for *every*
  entity-NBT-path read, not just book pages. This is the frozen Phase-1 contract
  (`written-books-26.2.md`: "initializes its result to \"\", then attempts the entity read...
  missing equipment, a wrong item, an absent page... yields an empty [default] without retaining
  a stale result"), generalized by making the default type-driven instead of hardcoded to the
  string case.

### 2.7 Deletion surface (exact, from full-tree trace)

Everything below is BookPage-specific and has no reuse once §2.5/§2.6 land; each is deleted, not
kept as a parallel path:

- `ir/semantic/minecraft.rs`: `ReadMainHandWrittenBookLiteralPage` key,
  `MinecraftAttributeKind::WrittenBookPageIndex`/`MinecraftValidatorKind::WrittenBookPageIndex`,
  `BOOK_PAGE_ATTRIBUTES`, `BOOK_PAGE_DESCRIPTOR`, its `minecraft_descriptor` arm, and the
  `main_hand_written_book_literal_page_or_empty` `SOURCE_METHODS` row.
- `frontend/hir.rs`: `BookPage`/`BookPageRuntime` variants and all match arms (551-588,
  1154-1158, 1793-1817, 2741-2757).
- `frontend/check.rs`: the `5064-5130` arm; the arity special-case at 4927 shrinks by one.
- `frontend/lower.rs`: 1148-1164, 1186-1188, 1193-1197, 1352-1367, 2334-2343.
- `frontend/behavior.rs`: 565-566 (folds into the remaining ambient-requirement group).
- `ir/core/minecraft.rs`: the `BookPage` attribute variant and all its arms; the already-dead
  `patch_book_page_index` (215-231, zero call sites — pure Stage-10 leftover, delete regardless).
- `ir/core/verify.rs`: 216-219. `ir/core/eval.rs`: 579-604 (`SkippedMacroExternal` special-case
  — the new `EntityNbtRead` binding needs its own, likely identical, "uninterpretable, report
  don't crash" story, but not this one's code).
- `lower/minecraft/preflight.rs`: `Java26_2ReadMainHandWrittenBookLiteralPage`/
  `Java26_2BookPage` throughout, including `written_book_page_path`/
  `written_book_literal_page_path` (superseded by schema-driven path construction) and the
  placeholder-target hack.
- `lower/minecraft/emit.rs`: the `book_page_index()` special-case (365-398) generalizes into
  §2.6's uniform inline-read lowering.
- `lower/minecraft/plan/reconcile.rs`: 189-204, `reconcile_book_empty_fallback` (233-291) —
  generalizes into a schema-agnostic "empty-init precedes matching entity read" check.
- `lower/minecraft/support.rs`: the registry-dump test string loses the old row, gains nothing
  in its place (schema fields aren't source methods anymore — they're type-checked path steps).
- Fixtures: `tests/source-fixtures/pre-scheduler/ps2_written_book_page.mdl`,
  `crates/mdl-test/tests/ps2_book_semantics.rs` — replaced by fixtures exercising the general
  path expression (§2.4's chain), including a genuinely runtime index (closing the untested gap
  from §1.2).

## Part 3 — Staging

- **PS-12.0 (do first, independent, small):** fix the two confirmed bugs (§1.2 result routing,
  §1.3 `@s` hardcoding) in the *existing* engine without waiting on the grammar/schema work —
  this makes the already-shipped generic engine actually trustworthy, and every later tranche
  depends on it being correct.
- **PS-12A:** grammar/parser/AST (§2.2) — additive, no behavior change to existing programs
  (verified by the existing full test suite staying green).
- **PS-12B:** schema table + unified chained-postfix checker (§2.3, §2.4).
- **PS-12C:** HIR/Core representation + lowering (§2.5, §2.6), replacing the intrinsic; delete
  per §2.7.
- **PS-12D:** differential/vanilla evidence on the new fixtures (mirrors every prior PS
  milestone's exit gate: four-policy differential, pinned Java 26.2 lifecycle).

## Sources

- [`macro-reference-crossing-model.md`](macro-reference-crossing-model.md) — the value-crossing
  model this extends.
- [`nbt-schema-system.md`](nbt-schema-system.md) — the full specification of the schema mechanism
  §2.4 summarizes; read this before implementing PS-12B.
- [`references-design.md`](references-design.md) — the full Tier B `ref`/`DataRef<T>` design;
  related to this note's chains but not required for PS-12's own exit criteria.
- [`macro-system-specification.md`](macro-system-specification.md) — the full `SyntaxSlot`
  taxonomy and ABI the PS-12.0 bug fixes (§1.2, §1.3) and the crossing engine implement against.
- [`../syntax/entity-paths-and-general-indexing.md`](../syntax/entity-paths-and-general-indexing.md)
  (S-042) — the formal syntax decision for §2.2's grammar extensions.
- [`pre-scheduler/stage-10-handoff.md`](pre-scheduler/stage-10-handoff.md),
  [`pre-scheduler/ps-11-macro-reference-crossing-plan.md`](pre-scheduler/ps-11-macro-reference-crossing-plan.md)
- [`../mcfunction/written-books-26.2.md`](../mcfunction/written-books-26.2.md) — measured 26.2
  entity/item/component shape and the frozen fail-soft contract.
- [`../mcfunction/nbt/compiler-known-dictionaries.md`](../mcfunction/nbt/compiler-known-dictionaries.md) —
  `KnownShape(schema, per-field representation)` lattice point this schema table implements.
- [`../mcfunction/nbt/dynamic-keys-and-path-safety.md`](../mcfunction/nbt/dynamic-keys-and-path-safety.md) —
  why runtime string keys stay out of scope.
- [`ps-5-anon-structs-destructuring-plan.md`](pre-scheduler/ps-5-anon-structs-destructuring-plan.md) —
  the existing `[k]` bracket/member-projection precedent this generalizes.
