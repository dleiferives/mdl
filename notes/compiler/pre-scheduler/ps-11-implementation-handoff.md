# PS-11 Implementation Handoff — Read This First

Date: 2026-07-20
Status: **active handoff to the next implementer. PS-11A is mid-flight.**

You are picking up the implementation of the value-crossing macro/reference system.
This document is the *prompt*: it records everything already decided and verified so you
do not re-derive it. Read it fully before touching code. It assumes you have also read,
in this order:

1. [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md) — the
   architecture of record (the "why" and the target shape). **Non-negotiable context.**
2. [`ps-11-macro-reference-crossing-plan.md`](ps-11-macro-reference-crossing-plan.md) — the
   PS-11 milestone plan (Tier A) and its exit criteria.
3. [`ps-11-macro-reference-crossing-todo.md`](ps-11-macro-reference-crossing-todo.md) — the
   tranche checklist, with what has already landed.
4. [`stage-10-handoff.md`](stage-10-handoff.md) — what Stage 10 shipped (the seed).

Everything below is the concrete engineering: the current repo state, the exact PS-11A
wiring with file:line anchors and signatures I verified, the one genuinely open design
decision, and blueprints for PS-11B and PS-11C.

---

## 0. The one-paragraph mental model (so the rest reads right)

A runtime value reaches a command in one of four *encodings* of a single "value-crossing
edge": **Const** (compile-time literal), **Ref** (indirect through a home via `from
storage`/`store`/`operation`), **Subst** (a Minecraft function macro `$(var)` at a
`.mcfunction` boundary), or **Dispatch** (a branch tree). Macros and references are mirror
images of the *same* mechanism. "Macro-ness" should be a property of an *operand*, derived
by a generic pass, not authored per-intrinsic. PS-11 makes that real, starting (Tier A =
PS-11A) by making the one existing dynamic intrinsic — a runtime written-book page index —
actually work end-to-end, which forces the runtime-value bridge into existence.

---

## 1. Current repo state (verified)

- **Branch:** `trunk`. The project has a linear, direct-to-trunk history; commit on trunk.
- **Committed baseline:** `fd744b5` "PS-11: value-crossing design + Stage 10 macro fixes to
  green". At this SHA the whole crate is green: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test -p mdl-compiler` all pass. That commit also landed the design note, this
  PS-11 plan/todo, and fixed three latent Stage-10 macro defects (see the todo's "Landed so
  far"). **If you are reading this in a later commit, `git log` to see what moved.**
- **Uncommitted WIP (PS-11A, partial):** `git diff` shows two files —
  `crates/mdl-compiler/src/frontend/check.rs` and `crates/mdl-compiler/src/frontend/hir.rs`.
  This is the *start* of PS-11A step A (see §3). It compiles and the tree is still green
  (731 lib tests pass, clippy clean) because nothing yet exercises the runtime path
  end-to-end. **These edits are safe and correct as far as they go; keep them.** If a WIP
  commit was made after this note was written, it will be the tip of `trunk`.
- **`.commandcode/`** is untracked local tooling state — do **not** commit it.

### What the WIP edits already did (frontend step A, partial)

- `check.rs` (~line 5089, the `ReadMainHandWrittenBookLiteralPage` arm): the non-literal
  argument path no longer discards the checked expression. It now requires the argument to
  be `Int32`, emits a `TYPE_MISMATCH` diagnostic otherwise, and stores the checked
  `HirExpression` into the HIR node.
- `hir.rs`: `HirMinecraftOperationAttributes::BookPageRuntime` now carries
  `page_index: Box<HirExpression>` (was just `page_origin`). HIR verification (~line 1804)
  independently re-checks the index expression's type is `Int32`.

### What is NOT done (the rest of PS-11A) — this is your job

The runtime value is stored in HIR but **not yet threaded to Core, preflight, or emit**, so
a runtime book page still cannot execute correctly. The placeholder `ValueId::from_index(0)`
and the empty macro frame are still in place. §3 is the exact remaining wiring.

---

## 2. The single most important architectural determination (read twice)

**The runtime index value must flow as the External instruction's OPERAND, and preflight/emit
must read it from that operand — NOT from the Core attribute.**

Why (all verified):

- The value is a *function-body-level SSA `ValueId`*, created during body lowering.
- `MinecraftOperationAttributes::BookPage { page_index: MacroOrStatic<u8>, page_origin }`
  (`ir/core/minecraft.rs:32`) is declared at **program level** in
  `declare_external_operations` (`frontend/lower.rs:1093`), which runs **before** any
  function body is lowered. At that point the runtime `ValueId` does not exist yet.
- The body-lowering context `BodyLoweringContext<'program>` holds
  `program: &'program CoreProgram` — an **immutable** borrow (`frontend/lower.rs:1821-1822`).
  So `CoreProgram::patch_book_page_index` (`ir/core/minecraft.rs:218`, currently
  `#[allow(dead_code)]`) **cannot be called during body lowering.** The "patch the attribute
  later" idea the Stage-10 handoff floated is blocked by this borrow. Do not fight it.
- Therefore the operand is the source of truth. `InstData::operands() -> &[ValueId]`
  (`ir/core/mod.rs:1035`) is available anywhere you have the instruction — in particular in
  preflight, which already holds the `InstData` (`lower/minecraft/preflight.rs:754`,
  `instruction_data`). Preflight reads `operands[0]` as the macro index value and stores it
  in the recipe; emit resolves that `ValueId`'s home and bridges it.

Consequence: the attribute's `MacroOrStatic::Macro(ValueId::from_index(0))` stays a
**vestigial placeholder** whose only job is to signal "this is dynamic" so preflight picks
the macro recipe. Leave a comment saying so. (PS-11B/C removes the ValueId from the attribute
entirely when `MinecraftOperationAttributes` moves onto the generic `Operand<T>` + slot
model; do not try to fix that wart inside PS-11A.)

---

## 3. PS-11A — exact remaining wiring, step by step

Do these in order and `cargo build -p mdl-compiler` after each. Anchors are from the
`fd744b5` tree; they will drift as you edit — re-grep if a line looks wrong.

### Step A — frontend threading (PARTIALLY DONE; finish the review)

Already done in the WIP (see §1). Nothing more required here **except** confirm the Debug
dumper still reads (`hir.rs:1157` uses `{ .. }` → `"page_index=<runtime>"`, unchanged — fine)
and that `behavior.rs:566` (`BookPageRuntime { .. }` in the ambient-requirements match) still
compiles. Both do. See §4 (open decision) about whether the checker should *also* reject a
runtime index that contains a call/nested external.

### Step B — operand *type* signature in `declare_external_operations`

File: `frontend/lower.rs`, function `declare_external_operations` (starts 1093).

- The attribute mapping is 1125-1160. The `BookPageRuntime` arm (1154-1159) currently
  produces `MinecraftOperationAttributes::BookPage { page_index: MacroOrStatic::Macro(
  ValueId::from_index(0)), page_origin }`. **Keep this** (it is the placeholder — add a
  comment referencing §2).
- The `results` vec is computed at 1179-1186 (special-cases the book-page key →
  `vec![CoreType::String]`). Add a **parallel `parameters` computation**: for a
  `MinecraftOperation` whose attribute is `BookPageRuntime`, the operand type signature is
  `vec![CoreType::I32]`; for everything else `vec![]`.
- The declaration call is `program.declare_external_op(binding, parameters, results, origin)`
  at 1187-1192. Signature (verified): `declare_external_op(&mut self, binding:
  ExternalSemanticBinding, parameters: Vec<CoreType>, results: Vec<CoreType>, origin)`
  (`ir/core/external.rs:263`). Pass the new `parameters` instead of `vec![]`.
- `CoreType` variants are `Bool | I32 | ListI32 | String` (`ir/core/mod.rs:96`).

### Step C — operand *value* in `lower_external`

File: `frontend/lower.rs`, `lower_external` (2297-2324). It currently does
`self.builder.external_with_identity(operation, vec![], origin)`. Signature (verified):
`external_with_identity(&mut self, operation: ExternalOpId, operands: Vec<ValueId>, origin)
-> Result<(InstId, Vec<ValueId>), BuildError>` (`ir/core/builder.rs:530`).

- To lower the stored index expression you need `block` and `environment`, which
  `lower_external` does **not** currently receive. `lower_expression`'s signature (2725) is
  `lower_expression(&mut self, block: &mut BlockId, environment: &mut Environment,
  expression: &HirExpression) -> Result<ValueBundle, CoreGenerationFailure>`.
- **Change `lower_external` to accept `block: &mut BlockId, environment: &mut Environment`**
  and update its two callers: `lower.rs:2020` (statement position, inside `lower_statement`
  which has both) and `lower.rs:2765` (expression position, inside `lower_expression` which
  has both).
- In `lower_external`: look up `self.checked.external_op(external)`'s semantic. If it is
  `HirExternalSemantic::MinecraftOperation { attributes:
  HirMinecraftOperationAttributes::BookPageRuntime { page_index, .. }, .. }`, then
  `let bundle = self.lower_expression(block, environment, page_index)?;` and take its single
  scalar `ValueId` (confirm the `ValueBundle` scalar accessor — grep `ValueBundle`; there is
  a `ValueBundle::scalar(...)` constructor used all over `lower_expression`, and an
  `into_values()` used at 2292; use whichever yields the one `ValueId`). Set
  `operands = vec![that_value_id]`. Otherwise `operands = vec![]`.
- **Ordering matters:** lower the index expression *before* creating the External
  instruction so the index-computing Core instructions precede the External op. Since you
  call `lower_expression` first and `external_with_identity` second, ordering is correct.
- Pass `operands` to `external_with_identity`.

### Step D — round-trip verification arm (WILL BREAK otherwise)

File: `frontend/lower.rs`, the HIR↔Core semantic round-trip equality check (the
`verify_source_semantic_correlations` machinery; the BookPage attribute comparison is around
1330-1355 — I edited this region for a clippy fix at ~1344 to pattern-match
`MinecraftOperationAttributes::BookPage { page_index: MacroOrStatic::Static(core_n), .. }`
with a trailing `_ => false`).

There is currently **no arm** pairing `(HirMinecraftOperationAttributes::BookPageRuntime,
MinecraftOperationAttributes::BookPage { page_index: MacroOrStatic::Macro(_), .. })`, so a
runtime book page falls through to `_ => false` and the round-trip verification **fails**.
Add an arm that matches `(BookPageRuntime { page_origin, .. }, BookPage { page_index:
Macro(_), page_origin: core_origin })` and returns `page_origin == core_origin` (do **not**
compare the `ValueId` — it is the placeholder on one side and the real operand's id is not
stored in the attribute). Re-read the exact match shape before editing; it moved when I did
the clippy cleanup.

### Step E — preflight reads the operand into the recipe

File: `lower/minecraft/preflight.rs`.

- The recipe-selection loop `select_reachable_recipes` (745-806) holds `instruction_data`
  (the `InstData`) at 754 and matches `CoreOp::External(external)` at 764. It calls
  `select_recipe(target, operation, operation_declaration)` at 801.
- **Thread the operands through:** pass `instruction_data.operands()` (a `&[ValueId]`) into
  `select_recipe`.
- In `select_recipe` (966+), the BookPage arm is 1085-1102. For
  `MacroOrStatic::Macro(_)` (1098), set `Java26_2MacroBookPage { operation, index_value }`
  where `index_value = operands[0]` (guard: require exactly one operand; on mismatch push a
  diagnostic, do not panic). **Ignore the attribute's placeholder ValueId.**
- `SelectedSemanticRecipe::Java26_2MacroBookPage { operation, index_value: ValueId }` is
  defined at `preflight.rs:273`; `is_unusable_inline()` returns true only for it (line 499);
  its macro `command_kind()` (449-486) builds a two-line `MacroCommand` whose one variable
  has key `"index"` and slot `MacroSlot::NbtIndex`. **The frame key is `"index"` — the emit
  bridge must write to that key.**

### Step F — emit bridges the value into the macro frame

File: `lower/minecraft/emit.rs`, the `InstructionPlan::External { helper }` arm (300-345).
Today it (1) seeds `mdl:__mdl/macro` storage, key `args`, to an empty compound, then (2)
emits `CommandKind::FunctionWithStorage(FunctionWithStorage::new(helper, args))`. **Insert
the value bridge between (1) and (2).**

- Get the recipe's index `ValueId`. Add an accessor to `SelectedSemanticRecipe`, e.g.
  `pub(crate) fn macro_index_value(&self) -> Option<ValueId>` returning `Some(index_value)`
  for `Java26_2MacroBookPage`. You already have the recipe via
  `plan.selected_semantic_recipe(*external)`.
- Resolve the home. The page index is `I32`, so it lives in a scoreboard.
  `plan.value_home(function, value_id) -> Option<HomeId>` (`lower/minecraft/plan.rs:600`);
  then `context.score(home) -> Result<ScoreRef>` (`lower/minecraft/construct.rs:230`, which
  wraps `plan.score(home)` at `plan.rs:483`). You need the `function: FunctionId` being
  emitted — it is available in the emit context (see how `control.rs` calls
  `plan.value_home(function, …)` at e.g. 339/450; mirror that access here). A `ScoreRef` is
  `{ holder: SingleScoreHolder, objective: ObjectiveName }`.
- Emit the bridge command. Target: `StoragePath(StorageId "mdl:__mdl/macro", NbtPath["args",
  "index"])` (the `args` compound's `index` key). The command is:
  `execute store result storage mdl:__mdl/macro args.index int 1 run scoreboard players get
  <holder> <obj>`. Build it as a `CommandKind::Execute(ExecuteCommand::new(modifiers, run))`
  where the single modifier is `ExecuteModifierKind::Store(StoreChannel::Result,
  StoreDestination::Storage { target, numeric_type: StorageNumericType::Int, scale:
  FiniteF64::new(1.0) })` and `run = CommandKind::Score(ScoreCommand::PlayersGet { score:
  score_ref })`. **Confirm exact variant names** (`StoreChannel::Result` vs `Success`;
  `StorageNumericType::Int` — the render test uses `StorageNumericType::Byte`, so `Int`
  should exist; grep the enum in `ir/minecraft/execute.rs`). Use `StoreChannel::Result`
  (you want the numeric result, not success).
- Keep the empty-compound seed as the frame initializer (it establishes `args` as a compound
  before the `store` writes `args.index`), or fold it into the bridge — either works; the
  seed-then-store order is safest.

After Step F, a runtime book page emits: seed frame → bridge score into `args.index` →
`function <helper> with storage mdl:__mdl/macro args`, and the helper body is
`$data modify … pages[$(index)].raw`.

---

## 4. The one genuinely OPEN design decision (I was mid-investigation here)

**Problem:** the runtime index `HirExpression` now lives in the external-op table, and the
function body references the external op only by id (`HirExpressionKind::External(id)` /
`HirStatementKind::External(id)` — see `hir.rs:2741`, `1474`, `3264`, `3191`). Several HIR
walkers traverse the *body* but never descend into an external op's attribute expression:

- `build_call_graph` / `collect_block_calls` (`frontend/behavior.rs:195-218`) — collects the
  set of functions each function calls. If a user function `g` is called **only** inside a
  `book.page(g())` runtime index, the call graph misses the `g` edge → behavior inference
  (recursion/termination) is wrong, and any reachability/DCE keyed on it could prune `g`
  while Core still lowers the call → dangling reference.
- `record_external_operation_occurrences` / `record_expression_externals`
  (`hir.rs:3185-3274`) — assert each external op occurs exactly once. A **nested external**
  inside the index would be recorded zero times.

Neither walker currently sees the attribute expression. Two clean resolutions:

- **Option 1 (RECOMMENDED for PS-11A):** in the checker, restrict the runtime index to a
  *simple* `Int32` expression that cannot contain a call or a nested external. Walk the
  checked `HirExpression` once and reject `HirExpressionKind::Call` / `::External` (and
  anything that transitively holds them) with a clear diagnostic like
  `"a runtime book-page index may not contain a function call yet"`. This makes **all** the
  walker gaps moot, is honest about the PS-11A boundary, and is trivially testable. Loosen in
  a later stage. **Pick this unless you have a strong reason not to.**
- **Option 2 (full generality, more work):** give `collect_block_calls` access to the
  external-ops table and, wherever a body walker hits `External(id)`, descend into that op's
  `BookPageRuntime.page_index` (reusing the existing expression walkers). Do the same for
  `record_expression_externals`. More invasive; defer unless a real client needs a call
  inside an index.

Whichever you pick, **test with a simple index (a parameter or a local `Int32`)** so the
happy path is proven regardless.

---

## 5. Testing PS-11A (how to actually prove it works)

- **Unit/lowering level (fastest to iterate):** build the Core IR directly and assert the
  emitted commands. Look at existing lowering/emit tests (e.g. the `reconcile.rs` test module
  builds a `book_page_core()` fixture at ~900; the `render.rs` test module shows how to
  assert exact rendered text). Assert the caller emits: the frame seed, the
  `execute store result storage mdl:__mdl/macro args.index int 1 run scoreboard players get …`
  bridge, and `function <helper> with storage mdl:__mdl/macro args`; and that the helper body
  renders `$data modify … pages[$(index)].raw`.
- **Source level / differential:** add an MDL program under `tests/programs/` that reads a
  runtime page index (from a parameter or local) and produces observable output. Follow an
  existing `tests/programs/*` for the harness shape and the four-policy differential. The
  Core evaluator treats macro/external ops as un-evaluable (per the design note / Stage-11
  boundary) — check how the differential handles un-evaluable externals so the test is
  meaningful; you may need the evaluator to *recognize* the macro external and report it
  rather than crash (this is the "Core evaluator macro support" item — small, do it if the
  differential requires it).
- **Pinned server:** one ignored Java 26.2 lifecycle over the entry point, matching how other
  PS milestones gate the server test.
- **Gates every step:** `cargo fmt --all -- --check`; `cargo clippy --workspace
  --all-targets -- -D warnings`; `cargo test -p mdl-compiler`.

---

## 6. Verified reference map (so you don't re-derive)

Core IR:
- `MacroOrStatic<T> = Static(T) | Macro(ValueId)` — `ir/core/macro_or_static.rs`. (PS-11B
  renames this to `Operand<T> = Const(T) | Runtime(ValueId)`.)
- `MinecraftOperationAttributes` (closed: Say/Teleport/MoveBy/BookPage) — `ir/core/minecraft.rs:13`;
  `BookPage { page_index: MacroOrStatic<u8>, page_origin }` at :32; `patch_book_page_index`
  (dead, unusable during body lowering) at :218.
- `CoreType = Bool | I32 | ListI32 | String` — `ir/core/mod.rs:96`.
- `InstData`: `op() -> &CoreOp` at `ir/core/mod.rs:1029`; `operands() -> &[ValueId]` at 1035;
  `CoreProgram/FunctionBody::instruction(InstId) -> Option<&InstData>` at 1229.
- `declare_external_op(binding, parameters: Vec<CoreType>, results: Vec<CoreType>, origin)` —
  `ir/core/external.rs:263`.
- `external_with_identity(operation, operands: Vec<ValueId>, origin) -> (InstId,
  Vec<ValueId>)` — `ir/core/builder.rs:530`.

Frontend:
- `HirMinecraftOperationAttributes` — `frontend/hir.rs:551`; `BookPageRuntime { page_index:
  Box<HirExpression>, page_origin }` at :568 (WIP-edited).
- `HirExpression { kind, ty, origin }` — `hir.rs:860`; `HirExpressionKind` at :881
  (`External(SourceExternalOpId)` and `Call(HirCall)` are the two that matter for §4).
- `ValueType = Bool|Int32|ListI32|String|Struct|Enum|AnonymousStruct` — `hir.rs:20`.
- `CheckedExpression { expression: Option<HirExpression>, ty: Option<ValueType>, span }` —
  `check.rs:5475`; `check_expression`/`check_expression_expected` at 3462/3474.
- `HirExternalSemantic::MinecraftOperation { key, receiver_kind, attributes, … }` —
  `hir.rs:538`; `HirExternalOp { id, semantic, origin }` at :525.
- `declare_external_operations` — `lower.rs:1093`; attribute map 1125-1160; results 1179-1186;
  declare call 1187.
- `lower_external` — `lower.rs:2297` (callers 2020, 2765); `lower_expression` — 2725;
  `BodyLoweringContext { program: &CoreProgram, … }` — 1821.
- Behavior call graph — `behavior.rs:195-218`; external-op occurrence walkers —
  `hir.rs:3185-3274`.

Lowering / target:
- Recipe: `SelectedSemanticRecipe` — `preflight.rs:255`; `Java26_2MacroBookPage { operation,
  index_value: ValueId }` at :273; `is_unusable_inline()` at :499; macro `command_kind()`
  (frame key `"index"`, slot `NbtIndex`) at 449-486; `select_reachable_recipes` loop
  745-806 (`instruction_data` at 754); `select_recipe` 966; BookPage selection arm 1085-1102.
- Plan homes: `LoweringPlan::value_home(function, value) -> Option<HomeId>` — `plan.rs:600`;
  `home_type` 589; `score(home) -> Option<ScoreRef>` 483; `string_storage`/`list_storage`
  497-505; `Home`/`HomeId`/`HomeRole` 45-80.
- Emit: `FunctionLoweringCx` — `construct.rs:215`; `score(home) -> Result<ScoreRef>` 230;
  External call site `emit.rs:300-345`; static BookPage inline emission 345-400 (uses
  `context.plan().string_storage(result.home())` ~383); macro frame storage `mdl:__mdl/macro`
  key `args` at 315-334.
- Command IR: `CommandKind` (10 variants incl. `Macro`, `FunctionWithStorage`) —
  `ir/minecraft/command.rs:83`; `render_macro` and `FunctionWithStorage` rendering —
  `ir/minecraft/render.rs` (both fixed in fd744b5); `ExecuteCommand`/`StoreDestination`/
  `StoreChannel`/`StorageNumericType` — `ir/minecraft/execute.rs`; `ScoreCommand::PlayersGet
  { score: ScoreRef }` and `ScoreRef` — `ir/minecraft/score.rs`; `MacroCommand`/
  `MacroArguments`/`MacroVariable`/`MacroSlot`/`MacroSegment` — `ir/minecraft/macro_command.rs`;
  `NbtPathSegment::Index(i32)` (PS-11B target) — `ir/minecraft/nbt.rs:129`;
  `StoragePath`/`NbtPath`/`NbtPathKey`/`StorageId` — `ir/minecraft/nbt.rs` + `names.rs`.

---

## 7. Gotchas that will bite you

1. **Do not try to patch the attribute during body lowering.** The program borrow is
   immutable there (§2). The operand is the mechanism.
2. **The round-trip verification WILL fail** for runtime book pages until you add the
   `BookPageRuntime ↔ BookPage{Macro}` arm (§3 Step D). This is silent until a test hits it.
3. **The emit bridge frame key is `"index"`**, and the frame storage is `mdl:__mdl/macro`,
   compound `args`. Match preflight's `command_kind()` exactly or the `$(index)` substitution
   finds nothing at runtime.
4. **`StoreChannel::Result`, not `Success`** — you want the score value, and use
   `StorageNumericType::Int` (verify the enum). Scale `1.0`.
5. **The `test interpreter` in `emit.rs` (loop-lowering harness, ~2837)** panics on
   `Macro`/`FunctionWithStorage` — that is intentional (loops never emit those). Do not route
   book-page tests through it; test emission via the render/plan path.
6. **Every exhaustive `match CommandKind`** already handles the two macro variants after
   fd744b5 (analysis/local, report, solve, syntax; ir/verify, dump, contract; render). If you
   add a variant later, ~11 sites plus the test interpreter need arms.
7. **clippy `match_same_arms`**: several sites merged `Raw|Macro|FunctionWithStorage`
   (unknown-step). If you give macros a *structured* contract later, you'll re-split them —
   fine, but expect the clippy lint to flip.

---

## 8. PS-11B blueprint (after A is green) — the generalization

Goal: replace the per-intrinsic seed with the reusable operand + slot machinery. From the
design note §9 and plan PS-11B:

- Rename `MacroOrStatic<T>` → **`Operand<T> = Const(T) | Runtime(ValueId)`**
  (`ir/core/macro_or_static.rs` → `operand.rs`). Keep the meaning; add helpers
  (`is_const`, `as_runtime`, `map`) so callers stop matching raw variants (current raw
  matches: `preflight.rs:1098`, `lower.rs` round-trip). This is a wide but mechanical rename;
  the ~2 real match sites are the interesting ones.
- Promote `MacroSlot` → **`SyntaxSlot`** with three serializers per position: a static
  serializer (`T -> text`), an optional `indirection_form` (the Ref recipe, `None` ⇒ macro is
  forced), and the existing macro serializer. Positions without an indirection form
  (NbtIndex, ResourceId, coordinate, selector fragment, compound key) are the ones that force
  `Subst`. Back it with the Brigadier domains from `notes/mcfunction/native-value-carriers.md`.
- Generalize `NbtPathSegment::Index(i32)` → `Index(Operand<i32>)` (`ir/minecraft/nbt.rs:129`).
  `Display`/render `Const(n)` → `[n]` (unchanged path) and `Runtime` → `[$(key)]` **only**
  inside a macro line. Update every construction/match site (grep `NbtPathSegment::Index`),
  and `verify`/`dump`/`contract`.
- This is where the runtime book-page path stops being a hand-authored recipe and becomes
  "an NBT path with a `Runtime` index segment," which PS-11C then lowers generically.

## 9. PS-11C blueprint — the generic engine

Goal: derive macro-ness; delete the per-intrinsic recipe. From design note §4:

- Add `extract_crossings(command, plan)` under `lower/minecraft/` implementing COLLECT →
  FRAME → BRIDGE → RENDER → CALL generically over any `CommandKind`:
  COLLECT walks the operand tree for `Runtime` slots and classifies each `Ref` (indirectable)
  or `Subst` (forced); FRAME dedups by `ValueId` and builds `MacroArguments` (stable keys);
  BRIDGE reuses `plan.value_home → score/string_storage/list_storage` (the same vocabulary
  the PS-11A emit bridge uses by hand); RENDER emits `$`-lines; CALL seeds the frame and emits
  `FunctionWithStorage`.
- Collapse `SelectedSemanticRecipe`'s `Java26_2*`/`Java26_2MacroBookPage` pair into one
  slotted recipe; make `is_unusable_inline()` **derived** ("has ≥1 `Subst` slot after crossing
  selection") rather than a hand-written arm. Delete the bespoke macro `command_kind()`
  (preflight.rs 449-486) once the engine generates it.
- PS-11A's hand-written emit bridge becomes the BRIDGE step of this engine — i.e. PS-11A is a
  deliberate, throwaway-shaped first instance of the general mechanism. That's fine and
  intended: it proves the runtime path and the bridge vocabulary before the generalization.

## 10. Tier B / Tier C (later PS milestones, not PS-11)

First-class `Reference` (promote `Home`/`HomeId`), source-level `DataRef<T>`, `Operand<T>`
across *all* `CommandKind` slots, `CommandTemplate` typed raw-command interpolation, comptime
language macros, crossing-*placement* optimization (hoist/batch/unroll/cost selection),
`Dispatch`, and `mdl explain`. All specified in
[`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md) §5, §10, §2 and
the PS-11 plan's non-goals. Do not pull them into PS-11.

---

## 11. Definition of done for PS-11A

- A runtime `book.page(<Int32 expr>)` compiles and, when emitted, produces: the frame seed,
  the score→`args.index` `execute store result … run scoreboard players get …` bridge, and
  `function <helper> with storage mdl:__mdl/macro args`, with the helper body
  `$data modify … pages[$(index)].raw`.
- The round-trip verification arm for `BookPageRuntime ↔ BookPage{Macro}` exists and passes.
- The chosen §4 option is implemented (recommended: checker rejects calls/nested externals in
  the index).
- A test proves the above end-to-end (lowering/emit assertions at minimum; a `tests/programs`
  differential if the harness cooperates).
- `no ValueId::from_index(0)` is *relied upon* for correctness (it may remain as a documented
  placeholder in the attribute, but preflight/emit use the operand).
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings`,
  `cargo test -p mdl-compiler` all green.
- Update the PS-11 todo and write a short `ps-11a` progress note (or fold into the todo's
  "Landed so far"). Then continue to PS-11B.
