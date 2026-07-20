# PS-12 Composable Entity-NBT Paths Checklist

Status: **in progress — PS-12.0 starting**

Authoritative design: [`ps-12-entity-nbt-paths-plan.md`](ps-12-entity-nbt-paths-plan.md) and
[`../entity-nbt-path-composability.md`](../entity-nbt-path-composability.md).

Implement the tranches in order. PS-12.0 is independent and must land first — every later
tranche builds on the engine actually being correct.

## PS-12.0 — Fix the two confirmed engine bugs (do first, small, high-value)

- [ ] Add a regression test that exercises a **genuinely runtime** (non-literal) book-page
      index end to end and asserts the read *value* — not just command shape — reaches the
      caller's result home. This test must fail against the current tree before the fix.
- [ ] Add a regression test asserting the macro-rendered entity read honors a non-`@s`
      `DataSource::Entity` selector (currently hardcoded).
- [ ] Fix `lower/minecraft/emit.rs`'s `InstructionPlan::External { helper }` arm (~300-335):
      resolve each of `data.results()` (already in scope, `InstData::results() -> &[ValueId]`)
      to its home via `plan.value_home(function, value_id)`, and route the macro helper's read
      to that home instead of the `mdl:preflight`/`book_page` placeholder.
- [ ] Fix `lower/minecraft/crossings.rs::render_data_modify_as_macro` (~line 159): render
      `{selector}` from the real `DataSource::Entity { selector, .. }` instead of the literal
      `"@s"`; stop discarding `selector` via `..` in both `collect_from_data_source` and this
      function.
- [ ] Both fixes green under `cargo test -p mdl-compiler`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

Gate: the existing generic engine is actually trustworthy before anything is built on top of it.

## PS-12A — Grammar, parser, AST

- [ ] Extend `notes/syntax/grammar.ebnf`'s `PostfixSuffix` to `"." Name | "." StringLiteral |
      Arguments | "[" Expression "]"` (was `"[" IntegerLiteral "]"`). Record as a syntax
      decision note (`notes/syntax/`) matching the project's S-0xx convention if one doesn't
      already cover this.
- [ ] Parser: add the `.` `StringLiteral` postfix arm alongside the existing `.` `Name` arm in
      `parse_postfix`; extend the `[` arm to parse a general `Expression` instead of only
      `TokenKind::DecimalInteger`.
- [ ] AST: extend `AstExpressionKind::Member` (or add a sibling variant) to carry a
      string-literal key alternative to `Name`; extend `AstExpressionKind::Index`'s `index` from
      a bare `Span` to hold a full `Box<AstExpression>` (needed for non-literal indices; keep
      the existing literal-index behavior working through the same node).
- [ ] Lexer/parser golden tests for both new forms; **explicit regression proof** that every
      existing PS-5 fixture (`pair[0]`, named/positional anonymous structs, destructuring) still
      parses byte-identically.
- [ ] Bounded recovery for malformed string-literal keys and malformed bracket expressions,
      matching the project's existing recovery conventions.

Gate: clean syntax round-trips deterministically; zero behavior change to any program that
doesn't use the two new forms.

## PS-12B — Schema table and unified chained-postfix checker

- [ ] Define the schema table (design note §2.4): `EquipmentSlots`, `ItemStack`, `Components`
      (keyed by known resource-id string literals only), `WrittenBookContent`, `BookPageEntry`
      — exactly enough to express the book-page chain plus prove extensibility.
- [ ] Extend `check_member_expression` with an `Executor`/schema-node receiver case: `.equipment`
      on an `Executor` starts the chain; each further `.name` narrows via the schema table.
- [ ] Add the `.` `StringLiteral` checking arm: resolves the string content against the current
      schema node's known keys; unrecognized key is a compile-time diagnostic, never a runtime
      fallback.
- [ ] Extend `check_index_expression` (or unify with member checking) to accept a general
      `Int32` expression (const or runtime) when the receiver is a known NBT-list schema node,
      while leaving the existing PS-5 literal-only/arity-checked rule unchanged for
      `AnonymousStruct` positional receivers. Disambiguate by checked receiver type.
- [ ] Chain accumulates an `NbtPath` prefix + root proof behind the scenes; an intermediate step
      can be bound to a `const`/`var` and the chain continued from it (mirrors existing PS-5
      struct-chain ergonomics).
- [ ] Positive tests: full chain to `.raw`, chain broken at an intermediate binding and resumed,
      every schema node reachable. Negative tests: unknown key at every depth, tuple-index syntax
      on a schema node (rejected), NBT-list index syntax on an anonymous-struct tuple (rejected
      — still literal-only there), wrong receiver kind.

Gate: `reader.equipment.mainhand.components."minecraft:written_book_content".pages[index].raw`
type-checks end to end for both a literal and a variable `index`, with `String` as its type.

## PS-12C — HIR/Core representation, lowering, and deletion

- [ ] Add `HirExpressionKind::EntityNbtPath { root, segments, result_ty }` /
      `HirEntityPathSegment` to `frontend/hir.rs`; wire checker output to build it.
- [ ] Add `ExternalSemanticBinding::EntityNbtRead { selector_proof, path: NbtPath, result_ty:
      CoreType }` to Core; wire `frontend/lower.rs` to declare it (replacing the
      `MinecraftOperationAttributes::BookPage` construction).
- [ ] Preflight: no-runtime-segment paths lower inline via `DataCommand::Modify` targeting the
      real result home (generalizing the now-fixed PS-12.0 static-path shape); ≥1-runtime-segment
      paths derive `is_unusable_inline()` from segment inspection (no authored flag) and route
      through the (now-fixed) `crossings.rs` engine.
- [ ] Preserve the fail-soft default: type-appropriate init (`""`/`0`/`false`) before the read,
      generalized from the hardcoded `""` case.
- [ ] Delete every site in the design note's §2.7 list: `ir/semantic/minecraft.rs`
      (`ReadMainHandWrittenBookLiteralPage`, `BOOK_PAGE_DESCRIPTOR`, the `SOURCE_METHODS` row),
      `frontend/hir.rs` (`BookPage`/`BookPageRuntime`), `frontend/check.rs` (5064-5130),
      `frontend/lower.rs`, `frontend/behavior.rs`, `ir/core/minecraft.rs` (including the dead
      `patch_book_page_index`), `ir/core/verify.rs`, `ir/core/eval.rs`'s book-specific
      `SkippedMacroExternal` arm, `lower/minecraft/preflight.rs` (`Java26_2BookPage` and
      `written_book_page_path`/`written_book_literal_page_path`), `lower/minecraft/emit.rs`'s
      `book_page_index()` special-case, `lower/minecraft/plan/reconcile.rs`'s
      `reconcile_book_empty_fallback`, `lower/minecraft/support.rs`'s stale registry-dump string.
- [ ] `ir/core/eval.rs`: give `EntityNbtRead` its own uninterpretable-report story (mirrors the
      deleted one; do not silently drop macro-awareness from the evaluator).
- [ ] Replace `tests/source-fixtures/pre-scheduler/ps2_written_book_page.mdl` and
      `crates/mdl-test/tests/ps2_book_semantics.rs` with fixtures exercising the general chain,
      including a genuinely runtime index.

Gate: no `BookPage`-shaped code remains anywhere in the tree; `grep -r BookPage` (and
`ReadMainHandWrittenBookLiteralPage`, `main_hand_written_book_literal_page_or_empty`) returns
nothing under `crates/`.

## PS-12D — Differential and vanilla evidence

- [ ] Four-policy Core/Minecraft differential on the replacement fixture, including the runtime
      case.
- [ ] Compare against the independent expected-result table.
- [ ] One ignored pinned Java 26.2 server lifecycle over the replacement fixture's entry points.
- [ ] Structural golden asserting the generated command/helper shape for both the inline and
      macro-helper routes.
- [ ] Record footprint without a size threshold.

Gate: target-neutral and vanilla authorities agree on the general path expression.

## PS-12E — Audit, extensibility proof, documentation, handoff

- [ ] As part of the exit audit, add one genuinely new schema field (e.g. `.count` on
      `ItemStack`) and confirm it requires only a table row — no new Core op, no new recipe, no
      checker special-case. Record this as the worked "composability proof" in the handoff.
- [ ] Review every new/changed exhaustive match across HIR, Core, checker, preflight, emit,
      crossings, verify.
- [ ] Run formatting, strict clippy, targeted suites, full workspace/all-targets, and the
      ignored server command.
- [ ] Update `notes/syntax/README.md`/grammar status wording if a new S-0xx decision was
      recorded in PS-12A.
- [ ] Add any semantic ambiguity discovered during implementation to
      `notes/compiler/semantic-ambiguities.md` before choosing a behavior.
- [ ] Write `ps-12-handoff.md`: shipped surface, the two PS-12.0 bugs and their fixes, the
      extensibility proof, and what remains explicit follow-up (broader schema, writes, Tier B
      references).
- [ ] Update `notes/compiler/pre-scheduler/roadmap.md` and cross-link from
      `notes/mcfunction/README.md`.

Gate: PS-12 is reviewable as a complete vertical slice; the hardcoded intrinsic is fully gone,
not deprecated-in-place.

## Explicitly deferred

- [ ] Runtime/dynamic string keys in path position (open safety problem, not solved here).
- [ ] First-class `Reference`/source-level `DataRef<T>` (value-crossing model Tier B).
- [ ] Broader schema coverage beyond the book-page chain plus the extensibility proof field.
- [ ] Entity-NBT writes.
- [ ] `Dispatch` encoding, crossing-placement optimization, cost-directed selection (PS-11 later
      tiers).
