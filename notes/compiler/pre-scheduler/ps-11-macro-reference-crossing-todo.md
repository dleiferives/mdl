# PS-11 Generic Macro Engine and Value-Crossing Substrate Checklist

Status: **PS-11A–C landed (commit `7654bc0`) by a separate agent — generic engine, `Operand<T>`,
`SyntaxSlot`, `NbtPathSegment::Index(Operand<i32>)`, and `crossings.rs` all shipped. PS-11D
("more `Java26_2*`-style hardcoded recipes") is explicitly rejected — see
[`../entity-nbt-path-composability.md`](../entity-nbt-path-composability.md) and
[`ps-12-entity-nbt-paths-plan.md`](ps-12-entity-nbt-paths-plan.md), which redirect this
direction and also fix two confirmed bugs found in the landed PS-11C engine (a genuinely
runtime index's result never reaches the caller; the macro renderer hardcodes `entity @s`
regardless of the real selector). PS-12 owns those fixes and all further entity-NBT-path work.**

Landed so far:
- the crate's test build was red (uncommitted Stage 10 + un-linted PS-4/5 debt); it is
  now fully green: `cargo fmt --all --check`, `cargo clippy --all-targets -D warnings`,
  and `cargo test -p mdl-compiler` all pass;
- render defect fixed — `FunctionWithStorage` now emits the `function ` keyword
  (`render.rs`), with a new `macro_and_function_with_storage_render_exactly` golden;
- macro render defect fixed — a multi-line `MacroCommand` no longer emits a trailing
  blank line (`render_macro` separates *between* lines only);
- macro validation defect fixed — `MacroCommand::new` validates the *assembled* line
  (literals + `$(key)`), not each literal segment, and rejects unknown variable ids
  (`MacroCommandError::UnknownVariable`);
- test interpreter (`emit.rs`) and a Stage 10 test helper (`reconcile.rs`) updated for
  the two new `CommandKind` variants and the `MacroOrStatic<u8>` field.

Authoritative design: [`ps-11-macro-reference-crossing-plan.md`](ps-11-macro-reference-crossing-plan.md)
and the architecture of record [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md).

**Implementer handoff (read first): [`ps-11-implementation-handoff.md`](ps-11-implementation-handoff.md)**
— records the current WIP state, the verified PS-11A wiring (with file:line anchors and the
immutable-program constraint), the one open design decision (walker coverage), and blueprints
for PS-11B/C. PS-11A is mid-flight: frontend threading is partially done in the working tree.

Implement the tranches in order. Each tranche reaches its build/test gate before the
next one starts. A rename without the derived-engine payoff, or an
anonymous-macro-as-per-intrinsic shortcut, does not complete a tranche.

## PS-11.0 — Freeze model and fixtures

- [ ] Reconcile the design note four-encoding model and this plan; resolve any
      disagreement before touching code.
- [ ] Add a runtime-book-page source program under `tests/programs/` (reads a runtime
      page index, produces observable output) as a failing fixture before the fix.
- [ ] Record the independent expected-output table for that program.
- [ ] Confirm the three latent defects with a reproducing test each (missing
      `function ` keyword, empty macro frame, test interpreter panic).

Gate: the target behavior and the current failures are both written down.

## PS-11A — Wire the runtime value bridge (make the shipped path correct)

- [ ] `HirMinecraftOperationAttributes::BookPageRuntime` carries the checked argument
      value (`frontend/hir.rs:551`).
- [ ] `check.rs:5079` stops discarding the argument value (`let _`); the checked
      `ValueId` is retained.
- [ ] `lower.rs:1158` threads the real `ValueId` (via `patch_book_page_index` or
      direct construction) and drops `ValueId::from_index(0)`.
- [ ] `emit.rs:313-334` bridges the value into the frame: resolve `value_home`, then
      emit `execute store result storage <frame> index int 1 run scoreboard players
      get <holder> <obj>` (score home) before the `FunctionWithStorage` call; drop the
      empty-compound seed or keep it only as the frame initializer.
- [ ] Fix `render.rs:170` to emit the `function ` keyword for `FunctionWithStorage`.
- [ ] Add `Macro`/`FunctionWithStorage` arms to the test interpreter
      (`emit.rs:2837`).
- [ ] The runtime-book-page fixture now produces correct output through the Core
      evaluator and Minecraft lowering.

Gate: runtime `BookPage` works end-to-end; the three defects have passing regression
tests. (This tranche is intentionally still per-intrinsic; PS-11C generalizes it.)

## PS-11B — `Operand<T>` and `SyntaxSlot`

- [ ] Rename `MacroOrStatic<T>` → `Operand<T>` (`ir/core/macro_or_static.rs` →
      `operand.rs`), keep `Const`/`Runtime` names, add `is_const`/`as_runtime`/`map`.
- [ ] Migrate every caller to the helpers; remove raw variant matches where the helper
      suffices (`preflight.rs:1091`, `frontend/lower.rs:1350`).
- [ ] Promote `MacroSlot` → `SyntaxSlot` with `syntax_kind`, `static_serializer`,
      `indirection_form: Option<…>`, `macro_serializer`; existing macro positions keep
      their serializers, indirectable positions gain an `indirection_form`.
- [ ] Generalize `NbtPathSegment::Index(i32)` → `Index(Operand<i32>)`
      (`ir/minecraft/nbt.rs:133`); `Display`/render `Const(n)` → `[n]`, `Runtime` →
      `[$(key)]` only inside a macro line; update every construction/match site.
- [ ] Update `verify`, `dump`, `contract` for the new path segment shape.

Gate: one operand type and one slot taxonomy exist; all existing goldens updated;
`NbtPath` carries dynamic indices with clean static rendering unchanged.

## PS-11C — Generic `extract_crossings` engine

- [ ] Add `extract_crossings(command, plan)` under `lower/minecraft/` implementing
      COLLECT → FRAME → BRIDGE → RENDER → CALL (design note §4), generic over
      `CommandKind`.
- [ ] COLLECT walks the operand tree; classify each `Runtime` slot `Ref` (indirectable)
      or `Subst` (forced); PS-11 uses the structural rule, no cost model.
- [ ] FRAME dedups by `ValueId`, assigns stable keys, builds `MacroArguments`.
- [ ] BRIDGE reuses `plan.value_home → score/string_storage/list_storage`.
- [ ] RENDER produces `MacroLine`s with `$`-prefix derivation; Ref slots render via
      their indirection bridge.
- [ ] Collapse `SelectedSemanticRecipe` `Java26_2*`/`Java26_2Macro*` into one slotted
      recipe (`preflight.rs:255`); delete `Java26_2MacroBookPage`.
- [ ] Make `is_unusable_inline()` derived from `Subst`-slot presence
      (`preflight.rs:500`).
- [ ] Re-point runtime `BookPage` at the generic engine; delete the per-intrinsic
      macro `command_kind()` template (`preflight.rs:449-486`).
- [ ] Unit-test `extract_crossings` over a hand-built mixed-slot command: frame keys,
      dedup, bridge choice by home kind, `$`-prefixing.

Gate: no per-intrinsic macro recipe remains; macro helpers are generated; runtime
`BookPage` still passes on the generic path.

## PS-11D — Differential and vanilla evidence

- [ ] Compile the runtime-book-page fixture under all four Core/Minecraft policies in
      distinct namespaces; compare public outputs.
- [ ] Compare all semantic results through the Core evaluator and the independent
      expected table.
- [ ] Add structural golden assertions for the generated helper, frame, and bridge
      shape.
- [ ] Run one ignored pinned Java 26.2 server lifecycle over the fixture entry points
      and policy products.
- [ ] Record footprint without a size threshold.

Gate: target-neutral and vanilla authorities agree on runtime `BookPage`.

## PS-11E — Audit, documentation, handoff

- [ ] Review every new/changed closed match across Core, target IR, preflight,
      assemble, emit, render, verify, dump.
- [ ] Run formatting, strict clippy, targeted suites, full workspace/all-targets, and
      the ignored server command.
- [ ] Update [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md)
      §9 anchors to reflect the shipped names; update the roadmap status.
- [ ] Add any semantic ambiguity found to `notes/compiler/semantic-ambiguities.md`
      before choosing a behavior.
- [ ] Write `ps-11-handoff.md`: the shipped engine surface, what Tier B/C inherit, and
      an explicit statement that no placement optimization, `Dispatch`, or reference
      source surface was smuggled in.

Gate: PS-11 is reviewable as a complete vertical slice and PS-12 has an honest start.

## Explicitly deferred (Tier B/C — later PS milestones)

- [ ] First-class `Reference` value promoted from `Home`/`HomeId`; source-level
      `DataRef<T>` capability with invalidation/alias/effect analysis.
- [ ] `Operand<T>` across all `CommandKind` slots; `CommandTemplate` typed
      raw-command interpolation; comptime language macros.
- [ ] Crossing-placement optimization: hoisting, batching, loop-unrolling to eliminate
      macros, callee-vs-caller resolution, profile-guided/cost-directed selection.
- [ ] `Dispatch` bounded decision trees and the layout-change encoding.
- [ ] `mdl explain` per-operand encoding transparency and the macro cache cost model.
- [ ] Open measurements: 26.2 macro cache capacity/escaping, shared scratch frames
      across forks, macro-vs-dispatch-vs-layout break-even, recursive frame liveness.
