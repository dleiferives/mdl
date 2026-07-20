# PS-11: Generic Macro Engine and the Value-Crossing Substrate

Status: **planned; implementation started**

## Identity

- **Capability:** turn the per-intrinsic Stage 10 macro seed into one generic
  value-crossing engine — `Operand<T>` operands, a `SyntaxSlot` taxonomy, a derived
  `extract_crossings` pass that generates macro helpers/frames/bridges for *any*
  command, and a wired runtime value bridge — plus the internal reference substrate
  those share;
- **Substage:** PS-11 (Tier A of the value-crossing model);
- **Owner documents:** this plan, [`ps-11-macro-reference-crossing-todo.md`](ps-11-macro-reference-crossing-todo.md),
  the implementer handoff [`ps-11-implementation-handoff.md`](ps-11-implementation-handoff.md)
  (verified PS-11A wiring + open decisions), and the architecture of record
  [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md);
- **Depends on:** Stage 10 (`stage-10-handoff.md`); and
- **Optimization posture:** PS-11 lands the *mechanism*, not the cost-directed
  optimizer. Encoding selection uses the structural rule (indirectable → `Ref`, else
  `Subst`); hoisting/batching/unrolling/profile-guided selection are deferred to PS-12.

PS-11 is the first executable slice of the [value-crossing
model](../macro-reference-crossing-model.md). It scopes **Tier A**. Tier B
(first-class `Reference`/source `DataRef<T>`) and Tier C (all-command generalization,
`CommandTemplate`, comptime macros) are later PS milestones; the design note owns the
full vision and this plan does not silently expand into it.

## Why PS-11 exists

Stage 10 shipped `MacroOrStatic<T>`, the macro IR, and the `CommandKind::Macro` /
`FunctionWithStorage` variants, but wired to exactly one field. The consequences the
design note records:

- macro support is per-intrinsic: `BookPage` is the only dynamic op, and each new one
  needs a hand-authored `Java26_2Macro*` recipe, `command_kind()` template, and
  `is_unusable_inline()` arm (`lower/minecraft/preflight.rs:255,410,500`);
- the runtime bridge is unwired — `frontend/lower.rs:1158` hardcodes
  `ValueId::from_index(0)`, `frontend/check.rs:5079` discards the argument value with
  `let _`, and `emit.rs:313-334` seeds an **empty** argument compound — so a runtime
  `BookPage` cannot execute correctly; and
- three latent defects ride along: `ir/minecraft/render.rs:170`
  (`FunctionWithStorage` omits the `function ` keyword), the empty-frame bridge, and
  the test interpreter at `emit.rs:2837` missing the two new arms.

PS-11 replaces the per-intrinsic pattern with the generic engine and, in the same
work, makes runtime `BookPage` correct end-to-end as the first real client.

## Frozen model (from the design note)

Four encodings of one abstract value-crossing edge; PS-11 implements Const, Ref, and
Subst structurally (Dispatch is recorded, not selected):

| Encoding | Static side | Runtime side | Mechanism |
|---|---|---|---|
| Const | literal text | — | fold |
| Ref | validated address | pointee | NBT/score indirection |
| Subst | text after substitution | value in arg frame | `$(var)` at a `.mcfunction` boundary |
| Dispatch | per-case command | selector value | branch tree (deferred) |

Core operand: **`Operand<T> = Const(T) | Runtime(ValueId)`** (the honest rename of
`MacroOrStatic<T>`). Encoding is chosen by lowering from the operand's `SyntaxSlot`,
never authored. A slot is `Subst`-forced iff its syntax position has no indirection
form (NBT index, resource id, coordinate, selector fragment, compound key).

## Semantic contract

1. `Operand<T>` distinguishes only compile-time-known (`Const`) from runtime
   (`Runtime(ValueId)`); the physical encoding is a separate lowering decision.
2. A command becomes a macro helper iff, after crossing selection, ≥1 of its slots is
   `Subst`; `is_unusable_inline()` is that derived predicate, not an authored arm.
3. Every `Subst` slot's runtime value is bridged into the argument frame at its key
   from its resolved home before the `FunctionWithStorage` call: a score home via
   `execute store result storage <frame> <key> int 1 run scoreboard players get …`,
   a storage home via `data modify storage <frame> <key> set from storage …`.
4. A `Subst` value's macro serializer owns escaping/range/validation for its slot; a
   raw runtime `String` is never a valid command-syntax fill.
5. Moving a resolution point (`.mcfunction` boundary) within a value's live range is
   behavior-preserving; PS-11 fixes placement at the instruction site and leaves the
   freedom to PS-12.
6. A dynamic NBT path projection (`NbtPathSegment::Index(Runtime(v))`) is a `Subst`
   segment resolved by the same engine — references and macros share one mechanism.

## Representation and IR ownership

### Core IR

- Rename `MacroOrStatic<T>` → `Operand<T>` (`ir/core/macro_or_static.rs` →
  `ir/core/operand.rs`), keeping `Const`/`Runtime` variant names; add `is_const`,
  `as_runtime`, `map` helpers so callers stop pattern-matching raw variants.
- `MinecraftOperationAttributes::BookPage.page_index` stays `Operand<u8>`; the
  `patch_book_page_index` helper (`ir/core/minecraft.rs:218`) loses its `dead_code`
  allow once the real value is threaded.

### Target IR

- Promote `MacroSlot` → **`SyntaxSlot`** (`ir/minecraft/`) carrying, per syntax kind,
  a static serializer, an optional `indirection_form`, and the existing macro
  serializer. Positions without `indirection_form` force `Subst`.
- Generalize `NbtPathSegment::Index(i32)` → `Index(Operand<i32>)`
  (`ir/minecraft/nbt.rs:133`). `Display` renders `Const(n)` as `[n]`; a `Runtime`
  index is only reachable inside a macro line and renders `[$(key)]`.
- Keep `CommandKind::Macro` and `FunctionWithStorage` as **emission encodings**
  produced by the engine, not hand-built IR.

### Lowering — the generic engine

- Add `extract_crossings(command, plan)` (new module under `lower/minecraft/`) doing
  COLLECT → FRAME → BRIDGE → RENDER → CALL as specified in the design note §4, generic
  over `CommandKind`.
- Collapse `SelectedSemanticRecipe`'s `Java26_2*`/`Java26_2Macro*` pairs
  (`preflight.rs:255`) into one slotted recipe; `is_unusable_inline()` becomes derived
  from the presence of a `Subst` slot.
- Reuse the `plan.value_home → score/string_storage/list_storage` chain
  (`lower/minecraft/plan.rs:483-600`) as the shared bridge vocabulary; this is the
  internal reference substrate PS-12 promotes to a first-class `Reference`.

### Frontend

- `HirMinecraftOperationAttributes::BookPageRuntime` carries the argument's checked
  value; `check.rs:5079` stops discarding it (`let _`); `lower.rs:1158` threads the
  real `ValueId` and drops `from_index(0)`.

## Mechanical Minecraft lowering

Runtime `BookPage` is the first real client: read `pages[$(index)].raw` in a helper
whose `index` frame variable is bridged from the argument value's score home. No
new authored recipe per intrinsic — the helper is generated by `extract_crossings`.

## Test architecture

- Core: `Operand<T>` rename round-trips; census neutrality (existing exhaustive
  `CommandKind` matches still compile) with the two variants now engine-produced.
- Target IR: `SyntaxSlot` serializer tests; `NbtPathSegment::Index(Operand)` render
  goldens for `Const` (`[n]`) and, inside a macro line, `Runtime` (`[$(key)]`); the
  `render.rs:170` `function ` keyword regression golden.
- Lowering: `extract_crossings` unit tests over a hand-built command with mixed
  `Const`/`Runtime` slots — asserts frame keys, dedup by `ValueId`, bridge choice by
  home kind, and `$`-prefixing.
- Emit: the test interpreter (`emit.rs:2837`) gains `Macro`/`FunctionWithStorage`
  arms; the bridge seeds the frame with the real value.
- Integration: `tests/programs/` runtime-book-page program that reads a runtime page
  index and produces observable output; four-policy differential + one ignored pinned
  Java 26.2 lifecycle.
- Gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace --all-targets`, pinned server only at the target
  tranche.

## Non-goals (deferred to PS-12+ / recorded in the design note)

- first-class `Reference` value and source-level `DataRef<T>` (Tier B);
- `Operand<T>` across *all* `CommandKind` slots, `CommandTemplate` typed raw-command
  interpolation, comptime language macros (Tier C);
- crossing-*placement* optimization: hoisting, batching, loop-unrolling to eliminate
  macros, callee-vs-caller resolution, profile-guided and cost-directed selection;
- `Dispatch` encoding (bounded decision trees);
- invalidation/alias/effect analysis for references;
- macro cache-cost measurement and the `mdl explain` transparency surface.

## Exit criteria

- `MacroOrStatic` is gone; `Operand<T>` is the single dynamic-operand type and callers
  use its helpers, not raw variant matches;
- `SyntaxSlot` replaces `MacroSlot` with the three serializers, and
  `NbtPathSegment::Index` carries `Operand<i32>`;
- a single `extract_crossings` pass generates the macro helper, frame, bridges, and
  call for any command; the `Java26_2Macro*` pair is gone and `is_unusable_inline()`
  is derived;
- runtime `BookPage` executes correctly end-to-end (no `from_index(0)`, no empty
  frame), proven by the integration fixture under all four policies and on the pinned
  server;
- the three latent defects are fixed with regression tests; and
- the handoff records what Tier B/C inherit and that no placement optimization or
  reference source surface was smuggled in.

## Research basis

- [Value-crossing model — architecture of record](../macro-reference-crossing-model.md)
- [Stage 10 handoff](stage-10-handoff.md) and [Stages 11–12 boundaries](stages-11-12-boundaries.md)
- [`macro-composition.md`](../../mcfunction/macro-composition.md),
  [`dynamic-access.md`](../../mcfunction/dynamic-access.md),
  [`representation-selection.md`](../../mcfunction/representation-selection.md),
  [`native-value-carriers.md`](../../mcfunction/native-value-carriers.md)
