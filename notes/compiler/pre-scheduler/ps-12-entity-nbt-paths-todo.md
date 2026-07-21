# PS-12 Composable Entity-NBT Paths Checklist

Status: **PS-12.0, PS-12A, PS-12B, PS-12C, and PS-12D are all landed. The general schema-typed
entity-NBT path expression compiles end to end — parses, type-checks, lowers to Core, and lowers
to real `.mcfunction` commands on both the inline (constant index) and macro-helper (runtime
index) routes, for `String` and `Bool` result types — and carries four-policy differential
evidence plus an ignored pinned-server test. PS-12E (deleting the retired
`main_hand_written_book_literal_page_or_empty` intrinsic, adding `.count` as the extensibility
proof, final handoff) has not started — see the "Sequencing" note this milestone's plan recorded:
land C+D fully proven first, delete the old path once its evidence is green, which it now is.**

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

## PS-12C — HIR/Core representation and lowering — DONE

Deletion of the retired `BookPage` intrinsic is **not** part of this tranche — see PS-12E.

- [x] Added `HirExternalSemantic::EntityNbtRead { receiver_kind, executor_proof, segments,
      result_ty, receiver_origin }` / `HirEntityPathSegment::{Key(Box<str>), Index(Box<
      HirExpression>)}` to `frontend/hir.rs`, and wired the checker (PS-12B, above) to build it —
      **not** a new `HirExpressionKind` variant as the design note's illustrative sketch named it:
      the codebase already routes every checked external operation through the existing
      `HirExpressionKind::External(SourceExternalOpId)` + `HirExternalOp.semantic:
      HirExternalSemantic` pair (this is exactly how `Say`/`Teleport`/`MoveBy`/the retired
      `BookPage` all worked), so `EntityNbtRead` is a third `HirExternalSemantic` variant instead,
      reusing that existing plumbing rather than adding a parallel one.
- [x] Added `ir/core/entity_nbt.rs`: `EntityNbtReadId`/`EntityNbtReadDecl`/`EntityNbtPathSegment`,
      a new, independent Core declaration table mirroring `MinecraftOperationDecl`'s shape but
      deliberately outside the closed-verb system — no `MinecraftSemanticKey`/descriptor
      cross-verification, since a schema-driven path has no fixed shape to verify against. Keys
      are plain `Box<str>` (Core stays target-independent; strings become real `NbtPathKey`s only
      during Minecraft-target lowering). `ExternalSemanticBinding` gained a 4th variant, wired into
      every match it touches: `is_well_formed`, `function_references`, ambient-requirements
      analysis, the canonical/debug printers, `ir/core/verify.rs`'s dense-table check, and the
      command-limit audit.
- [x] `frontend/lower.rs`: `declare_external_operations` builds real declarations — an `Index`
      segment becomes `Operand::Const(n)` for a literal HIR `Int32`, else the same
      `Operand::Runtime(ValueId::from_index(0))` placeholder `BookPageRuntime` already used (real
      per-occurrence resolution happens later, from instruction operands, not the declaration).
      `lower_external` lowers each runtime index expression to a real Core value via the existing
      general `lower_expression`, feeding it as an instruction operand. Verification and
      source/Core correlation tracking (`record_semantic_operation`,
      `verify_source_semantic_correlation`) cover the new binding kind for real, not as a stub.
- [x] **Built the parallel, recipe-free lowering system** the design note requires (bypassing
      `SelectedSemanticRecipe`/`MinecraftRecipeId` entirely, since a schema-driven path has no
      fixed shape to select among):
  - `preflight.rs`: `ResolvedEntityNbtSegment`/`ResolvedEntityNbtRead` +
    `select_reachable_entity_nbt_reads`, resolving each `Operand::Runtime` placeholder against the
    real per-occurrence instruction operand — the exact mechanism `select_recipe`'s `BookPage` arm
    already used for its one optional runtime index, generalized to any number of runtime
    segments. `is_unusable_inline()` is the derived predicate (any segment is `Index(Runtime)`).
  - `plan.rs`: new `InstructionPlan::EntityNbtRead { external, results }` — the inline (all-const)
    case, shape-identical to `Minecraft` minus the `recipe` field. No `AssignedInstructionPlan`
    change was needed: `CoreOp::External`'s draft/assigned results are already computed generically
    regardless of binding kind: only `plan/assemble.rs::flatten_instruction_plan` needed a new
    branch choosing between `EntityNbtRead` (inline) and the existing `External { helper }`
    (macro) path based on `preflight.selected_entity_nbt_read(..).is_unusable_inline()`.
  - `resources.rs`: `external_requires_helper` checks `selected_entity_nbt_read` before defaulting
    to "always needs a helper" (the default that's correct for `UnsafeTargetFragment`/
    `MinecraftRunScope`, but was wrongly claiming a helper for the all-constant inline case too
    until fixed).
  - `emit.rs`: `InstructionPlan::EntityNbtRead`'s inline arm and `define_external_helper`'s new
    `ExternalSemanticBinding::EntityNbtRead` arm both build the command directly from resolved
    segments, targeting the real result home from construction (no PS-12.0-style retarget needed
    this time). `String` results write straight to their NBT home, preserving the fail-soft
    contract generalized from the hardcoded `""` case (type-appropriate default — `""`, `0b`, or
    `0` — written before the read is attempted, exactly the retired `BookPage` shape). `Bool`/`I32`
    results (scoreboard-based homes — Minecraft scoreboards only hold integers) go through a new
    shared scratch NBT slot (`LoweringPlan::entity_nbt_scalar_scratch`, mirroring the existing
    `string_unit_scratch`/`list_i32_scratch` pattern) and an `execute store result score … run
    data get storage …` conversion, since `data get` has no entity-source form in this IR yet —
    the default-then-attempt fail-soft sequence happens in the scratch slot first, so the
    conversion sees the right value either way. The macro-helper arm reuses
    `crossings::collect_runtime_operands`/`build_frame`/`render_as_macro` **completely
    unchanged** — proof that PS-11's generic engine and the PS-12.0 selector fix were worth doing:
    a second, structurally different command family flows through them with zero code changes.
  - **Found and fixed a real bug via the new integration tests, not by inspection:** the
    caller-side macro-frame-seeding dispatch in `emit.rs::lower_instruction`'s
    `InstructionPlan::External` arm only ever checked `plan.selected_semantic_recipe(..)`. For a
    runtime-indexed entity-NBT read (which has no semantic recipe), this silently fell through to
    a **plain `function ns:path` call with no `mdl:__mdl/macro` frame seeded at all** — the
    generated command would have referenced an unseeded frame and been wrong. Now this dispatch
    also checks `plan.preflight().selected_entity_nbt_read(..)`.
  - `plan/verify.rs`: a real `InstructionPlan::EntityNbtRead` verification arm (results/shape
    checks mirroring `Minecraft`'s); widened the macro-external "has real operands/results"
    exemption to also cover `EntityNbtRead`'s macro route (it was previously assuming only
    recipe-routed macros could have non-empty operands/results, which would have misflagged every
    valid runtime-indexed read as a shape violation).
  - `plan/reconcile.rs`: a new `reconcile_entity_nbt_read_command`, verifying **both** legal
    constructed shapes against the resolved segments — the `String` 2-command shape (mirrors
    `reconcile_book_empty_fallback`'s exact pattern) and the `Bool`/`I32` 3-command
    scratch-then-convert shape (new; the two are structurally different enough that one reused
    check would have silently passed the wrong thing for one of them). The pre-existing `BookPage`
    reconciliation is untouched and still runs alongside this.
- [x] Three new integration tests (`ps12c_entity_nbt_path_lowering.rs`) compile the general chain
      and inspect the generated command text directly — not just structural shape assertions but
      the literal emitted `.mcfunction` text, the same technique that caught PS-12.0's bugs:
      a literal index lowers inline with **no** macro helper and **no** `mdl:__mdl/macro` frame,
      landing on the real result home; a genuinely runtime index routes through the macro-helper
      engine to that same real home (this is what caught the caller-side dispatch bug above); and
      the `Bool` field (`.resolved`) proves the scratch/score conversion route end to end.

Gate: `reader.equipment.mainhand.components."minecraft:written_book_content".pages[index].raw`
compiles, type-checks, and executes correctly for both a compile-time-constant and a genuinely
runtime `index`, on the Core evaluator differential and the pinned server. **Met** for the
compile/lower/structural half (PS-12D below adds the differential/server evidence). The
`BookPage`-deletion half of the ORIGINAL PS-12C scope is deliberately deferred to PS-12E — see the
implementation plan's "Sequencing" note: land the new path fully proven first, delete the old one
once evidence is green (it now is).

## PS-12D — Differential and vanilla evidence — DONE

- [x] New fixture, `tests/source-fixtures/pre-scheduler/ps12_entity_nbt_path.mdl`, using the
      general chain with a **genuinely runtime** index — closing the exact untested gap that let
      PS-12.0's two bugs ship silently. Registered with `source_fixtures.rs`'s `// MDL:`/`// HIR:`/
      `// CORE:` semantic-check header convention. Added **alongside**, not replacing,
      `ps2_written_book_page.mdl` — the retired intrinsic isn't deleted yet (PS-12E).
- [x] Four-policy Core/Minecraft differential (`source_fixtures.rs` already runs every registered
      fixture, including this one, across all four `CoreOptimizationLevel × MinecraftOptimizationLevel`
      combinations, checking both determinism and the `// HIR:`/`// CORE:` pattern assertions) —
      confirmed the runtime-index classification is stable across every policy: whether a path
      segment is `Const` vs. `Runtime` is decided once, from source AST shape, at frontend
      lowering, before any Core optimization runs — optimization can simplify the *value* flowing
      into a runtime operand, but never rewrites a declaration's parameter shape after the fact.
- [x] One `#[ignore]`d pinned Java 26.2 server lifecycle test,
      `crates/mdl-test/tests/ps12_entity_nbt_path_semantics.rs`, mirroring
      `ps2_book_semantics.rs`'s exact shape (summon armor stand with a written book, run the
      compiled function, assert the correct page and the fail-soft wrong-item fallback) over the
      new fixture. Compiles; will not run in this sandboxed environment without `MDL_SERVER_JAR`.
- [x] Structural golden asserting both the inline and macro-helper generated command shapes: this
      is `ps12c_entity_nbt_path_lowering.rs` (PS-12C, above) — already exactly this evidence, not
      duplicated into a second file.

Gate: target-neutral and vanilla authorities agree on the general path expression. **Met** for
everything that runs in this environment; the pinned-server assertion is written and compiles but
requires a real server bundle to actually execute.

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
