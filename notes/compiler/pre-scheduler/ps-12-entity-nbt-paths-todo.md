# PS-12 Composable Entity-NBT Paths Checklist

Status: **PS-12.0, PS-12A, and PS-12B landed (checking is complete and shipped); PS-12C's
representation half is landed but Core/Minecraft lowering is explicitly not — the compiler
currently checks the new syntax cleanly and then fails Core generation with a clear,
non-panicking, honestly-labeled error. See PS-12C below before starting the remaining
lowering work.**

Authoritative design: [`ps-12-entity-nbt-paths-plan.md`](ps-12-entity-nbt-paths-plan.md) and
[`../entity-nbt-path-composability.md`](../entity-nbt-path-composability.md).

Implement the tranches in order. PS-12.0 is independent and must land first — every later
tranche builds on the engine actually being correct.

## PS-12.0 — Fix the two confirmed engine bugs (do first, small, high-value) — DONE

- [x] Add a regression test that exercises a **genuinely runtime** (non-literal) book-page
      index end to end and asserts the read *value* — not just command shape — reaches the
      caller's result home. This test must fail against the current tree before the fix.
      Landed as `crates/mdl-compiler/tests/ps12_book_page_runtime.rs`
      (`runtime_book_page_index_routes_to_caller_result_home`); confirmed failing
      pre-fix (macro helper wrote to `mdl:preflight "book_page"` while the caller's
      `ends_with_ascii` lowering read the value's real, independently assigned home
      `ps12_book:__mdl/runtime/v0 "s1"`). Deliberately embeds its source inline rather than
      under `tests/source-fixtures/` so it isn't swept into `source_fixtures.rs`'s four-policy
      differential (PS-12D's job) — `CoreOptimizationLevel::Baseline` would constant-fold the
      index and never exercise the runtime/macro path.
- [x] Add a regression test asserting the macro-rendered entity read honors a non-`@s`
      `DataSource::Entity` selector (currently hardcoded). Landed as a `#[cfg(test)]` unit test
      in `lower/minecraft/crossings.rs`
      (`render_data_modify_as_macro_honors_non_self_selector`), constructing a
      `DataSource::Entity` with `AtMostOneSelector::NearestPlayer` and asserting the rendered
      macro text contains `@p` and never `@s`.
- [x] Fix `lower/minecraft/emit.rs`'s `InstructionPlan::External { helper }` arm: added
      `retarget_to_result_home`, called from `define_external_helper`'s `MinecraftOperation`
      arm, which resolves `data.results()` (`InstData::results() -> &[ValueId]`) to its home via
      `plan.value_home(function, value_id)` / `plan.string_storage(home)` and rewrites the
      recipe's `CommandKind::Data(DataCommand::Modify)` target before `render_as_macro` runs.
      `function: FunctionId` threaded through `define_external_helpers` →
      `define_external_helper` to make this possible (needed `#[allow(clippy::
      too_many_arguments)]`, matching existing precedent in this module).
- [x] Fix `lower/minecraft/crossings.rs::render_data_modify_as_macro`: renders `{selector}` from
      the real `DataSource::Entity { selector, path }` instead of the literal `"@s"`.
      `collect_from_data_source` still discards `selector` via `..` — left as-is, since no
      `Selector` variant carries a runtime component today, so there is nothing to collect;
      revisit if/when PS-12B introduces selector expressions with runtime parts.
- [x] Both fixes green under `cargo test --workspace --all-targets`, `cargo clippy --workspace
      --all-targets -- -D warnings`, `cargo fmt --all -- --check` (734 passed in
      `mdl-compiler`, 0 failed, workspace-wide 0 failed).

Gate: the existing generic engine is actually trustworthy before anything is built on top of it.
**Met.**

## PS-12A — Grammar, parser, AST — DONE

- [x] Extended `notes/syntax/grammar.ebnf`'s `PostfixSuffix` to `"." Name | "." StringLiteral |
      Arguments | "[" Expression "]"` (was `"[" IntegerLiteral "]"`). Already recorded as
      [`../../syntax/entity-paths-and-general-indexing.md`](../../syntax/entity-paths-and-general-indexing.md)
      (S-042) before implementation started.
- [x] Parser: added the `.` `StringLiteral` postfix arm alongside the existing `.` `Name` arm in
      `parse_postfix` (checked via `self.eat(TokenKind::StringLiteral)` before falling through to
      `expect_identifier`); widened the `[` arm to `self.parse_expression()` instead of
      `self.expect(TokenKind::DecimalInteger, ..)`.
- [x] AST: added a **sibling variant** `AstExpressionKind::MemberKey { receiver, dot, key: Span }`
      rather than modifying `Member` in place — `Member`'s `member: AstName` has ~10 existing
      Name-only consumers (builtin/run-modifier/entity-query dispatch) that must keep rejecting a
      string literal there; a sibling variant means none of them needed a
      "must be Name, else reject" arm added. Changed `AstExpressionKind::Index`'s `index` from
      `Span` to `Box<AstExpression>`; `check_index_expression` now requires
      `AstExpressionKind::DecimalInteger` at check time (was enforced at parse time via the token
      kind) — same accepted-program set, moved one layer up per the design note's "disambiguation
      by checked receiver type, not grammar." `MemberKey` gets one checker arm today: an honest
      `UNRESOLVED_MEMBER` rejection ("only valid on a schema-typed entity-NBT path"), since no
      schema receiver exists until PS-12B — this is not throwaway stub code, it is the permanent
      "no valid receiver" diagnostic PS-12B's checker will need regardless.
- [x] Golden tests added in `frontend::parser::tests`: `parses_string_literal_member_key`,
      `string_literal_member_key_chains_with_ordinary_member_access`,
      `bracket_index_accepts_both_a_literal_and_a_general_expression`, and an **exact** dump
      regression, `ps5_positional_index_dump_is_unchanged_by_the_general_bracket_grammar`, proving
      `pair[0] +% pair[1]` dumps byte-identically through the widened grammar. Full workspace
      suite (740 passed in `mdl-compiler`, up from 734) confirms no existing fixture changed
      behavior.
- [x] Bounded recovery: `malformed_member_key_recovers_before_the_next_statement` (`x.;`) and
      `malformed_bracket_index_recovers_before_the_next_statement` (`pair[;`) both prove a syntax
      error inside the new forms produces one `Error` statement and does not desync parsing of
      the following statement — same shape as the project's existing call/paren recovery tests.

Gate: clean syntax round-trips deterministically; zero behavior change to any program that
doesn't use the two new forms. **Met** — `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace --all-targets` all green.

## PS-12B — Schema table and unified chained-postfix checker — DONE

- [x] Defined the schema table as closed, `'static` compile-time data in a new module,
      `frontend/entity_schema.rs`: `SchemaKey::{Identifier, ResourceId}` and
      `SchemaNode::{Scalar, Compound, List}`, with `EquipmentSlots`/`ItemStack`/`Components`
      (`ResourceId`-keyed)/`WrittenBookContent`/`BookPageEntry` — exactly what the book-page
      chain needs. **`ItemStack.id`/`.count` are deliberately not included yet** — adding `.count`
      as a pure table-row diff is PS-12E's extensibility proof; pre-populating it here would
      leave nothing to prove. §4.3's key-form/uniqueness invariants are enforced by a test that
      walks the whole table (`every_registered_table_satisfies_key_form_and_uniqueness_invariants`)
      rather than a runtime registration API — the table is fixed Rust data, not user data, so
      "registration-time validation" is a compile-time-authored, test-verified property.
      Versioning (`JavaEditionTarget -> RootSchemaTable`) is not wired: the frontend checker is
      target-independent everywhere else in this compiler (only `LoweringOptions` carries a
      target), so there is exactly one table; keying `root_schema` by target is follow-up once a
      second target exists, not a gap this design introduces.
- [x] Did **not** extend `check_member_expression`/`check_index_expression` in place. Instead,
      added `expression_roots_in_executor_capture` (a read-only peek down a `Member`/`MemberKey`/
      `Index` spine to its base `Name`) to decide, before any checking happens, whether an
      expression is a schema chain at all — an executor capture is never an ordinary `ValueType`
      binding, so this can never misfire against a real struct/tuple receiver. When it returns
      true, `check_expression_expected`'s `Member`/`MemberKey`/`Index` arms route to
      `check_entity_nbt_path_expression` instead of the existing struct/tuple checkers, which are
      otherwise completely untouched. This satisfies the same "disambiguate by receiver, not
      grammar" goal the design note describes, by receiver-root detection instead of
      receiver-type-after-checking (the two are equivalent here because the domains are disjoint).
- [x] `.` `StringLiteral` checking arm: `check_entity_nbt_path_step`'s `MemberKey` case decodes
      the literal and resolves it via `SchemaNode::field_by_resource_id`; an unrecognized id is
      `UNKNOWN_MEMBER`, never a runtime fallback.
- [x] `check_entity_nbt_path_step`'s `Index` case accepts any `Int32` expression (const or
      runtime) when the receiver narrows to a `SchemaNode::List`; the existing
      `check_index_expression` (now taking `index: &AstExpression` per PS-12A) is completely
      unchanged and still literal-only for `AnonymousStruct` positional receivers. Runtime indices
      reuse the existing `runtime_index_contains_forbidden` rule (no function calls yet — the
      same live-range limitation `BookPageRuntime` was already guarding against).
- [x] **Scope cut, recorded here rather than silently dropped:** intermediate-binding support
      (`const item := reader.equipment.mainhand;` then continuing the chain from `item`) is **not
      implemented**. `check_entity_nbt_path_expression` requires the chain to reach a `Scalar`
      node in one uninterrupted expression; reaching a non-terminal node is rejected with
      `TYPE_MISMATCH` ("this entity-NBT path is not a complete field yet"), proven by
      `an_intermediate_non_scalar_chain_position_is_rejected`. This is not required by PS-12's
      actual exit criteria (`ps-12-entity-nbt-paths-plan.md`'s exit criteria name the one-shot
      chain, not intermediate binding), and building it means a new binding-classification
      side-table threaded through declaration-checking, destructuring, and run-scope capture
      floors — real, separable follow-up work, not a corner silently cut inside this tranche.
- [x] Tests, all in `frontend::check::tests` unless noted: full chain to `.raw` with a literal
      index and with a genuinely runtime index (both asserted via the HIR dump, which now renders
      `[N]` for a literal index and `[<runtime>]` otherwise); every equipment slot reachable to
      three different terminal fields (`.author`, `.resolved`, `.title.raw`); unknown key rejected
      at every chain depth (5 cases); NBT-list `[...]` syntax rejected on a non-list schema node;
      ordinary positional-tuple indexing proven unaffected (still literal-only); the intermediate-
      binding cut (above); a stale executor capture rejected through the new chain (mirrors the
      existing `typed_say` staleness test); a runtime index containing a call rejected; a
      non-`Int32` index rejected. Plus `crates/mdl-compiler/tests/ps12b_entity_nbt_path_checker.rs`
      — an integration test proving a full program using the new syntax checks with zero
      diagnostics and *then* fails Core generation with a clear, structured,
      non-panicking `CoreGenerationFailure::Invariant` (see PS-12C note below), not a crash.

Gate: `reader.equipment.mainhand.components."minecraft:written_book_content".pages[index].raw`
type-checks end to end for both a literal and a variable `index`, with `String` as its type.
**Met.** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --all-targets` all green (753 passed in `mdl-compiler`, up from 740).

## PS-12C — HIR/Core representation, lowering, and deletion

Representation is landed; **lowering is not** — see the explicit boundary below.

- [x] Added `HirExternalSemantic::EntityNbtRead { receiver_kind, executor_proof, segments,
      result_ty, receiver_origin }` / `HirEntityPathSegment::{Key(Box<str>), Index(Box<
      HirExpression>)}` to `frontend/hir.rs`, and wired the checker (PS-12B, above) to build it —
      **not** a new `HirExpressionKind` variant as the design note's illustrative sketch named it:
      the codebase already routes every checked external operation through the existing
      `HirExpressionKind::External(SourceExternalOpId)` + `HirExternalOp.semantic:
      HirExternalSemantic` pair (this is exactly how `Say`/`Teleport`/`MoveBy`/the retired
      `BookPage` all worked), so `EntityNbtRead` is a third `HirExternalSemantic` variant instead,
      reusing that existing plumbing rather than adding a parallel one. Wired into every exhaustive
      match this touches during *checking*: the HIR dumper (two sites), HIR verification (two
      sites, including the "value type of an expression" match that would otherwise have silently
      rejected every entity-NBT-read expression), and `frontend/behavior.rs`'s ambient-requirements
      inference (real values: requires the current executor, `WorldEffect::Read`,
      `ObservableEffect::None`, `ForkBound::None`, `TransitiveWork::Finite` — identical real-world
      behavior to the retired `ReadMainHandWrittenBookLiteralPage` verb it generalizes).
  - [ ] **Not done:** `ExternalSemanticBinding::EntityNbtRead` in Core. `frontend/lower.rs`'s
        `declare_external_operations` currently returns a new, dedicated, honest error —
        `CoreGenerationFailure::Invariant(CoreGenerationInvariant::
        EntityNbtReadLoweringNotImplemented)` — the instant it sees an `EntityNbtRead` semantic,
        proven by `ps12b_entity_nbt_path_checker.rs`. This is a deliberate stopping point, not an
        oversight: the design note requires `EntityNbtRead` to bypass `SelectedSemanticRecipe`/
        `MinecraftRecipeId` entirely, and every existing `InstructionPlan`/`AssignedInstructionPlan`
        arm for `CoreOp::External` structurally assumes "either a selected recipe exists, or the
        op becomes an opaque helper call with no constructed command" — there is no existing slot
        for "an External op that builds a real structured command without a recipe." Building that
        slot means new, parallel machinery across `preflight.rs`, `plan/assemble.rs`,
        `plan/symbolic.rs`, `resources.rs`, `emit.rs`, and `plan/verify.rs` — real, multi-file,
        deeply-interconnected lowering work carrying the same correctness risk PS-12.0 just spent
        real effort fixing, and it deserves its own dedicated pass with its own full differential/
        server verification, not to be rushed inside the same tranche as the checker.
- [ ] Preflight: no-runtime-segment paths lower inline via `DataCommand::Modify` targeting the
      real result home (generalizing the now-fixed PS-12.0 static-path shape); ≥1-runtime-segment
      paths derive `is_unusable_inline()` from segment inspection (no authored flag) and route
      through the (now-fixed) `crossings.rs` engine.
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
