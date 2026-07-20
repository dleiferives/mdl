# Stage 10: Typed Minecraft Function Macro Lowering — Handoff

Date: 2026-07-20
Status: **shipped — infrastructure complete, runtime value bridging deferred**

## What Was Built

### `MacroOrStatic<T>` — The Reference-Macro Duality

A single Core IR type that captures the duality between references (static NBT path
→ indirects to runtime value) and macro access (runtime value → crosses function
call boundary → becomes static command text via `$(variable)` substitution).

```rust
pub enum MacroOrStatic<T> {
    Static(T),       // compile-time constant — existing static lowering
    Macro(ValueId),  // runtime value — requires macro helper function
}
```

Lives in `ir/core/macro_or_static.rs`. Currently used by `MinecraftOperationAttributes::BookPage`
but designed for any attribute that has a static/dynamic split.

### Typed Macro IR (`macro_command.rs`)

Fully validated IR types for representing macro function bodies:

- `MacroSlot` — closed enum of safe syntax positions (Int, Float, Snbt, NbtKey, NbtIndex, ResourceId, SelectorFragment, CommandFragment)
- `MacroLine` — one command line, optionally `$`-prefixed based on `has_variables()`
- `MacroSegment` — `Literal(String)` or `Variable(MacroVariableId)`
- `MacroArguments` — validated ordered key set with duplicate detection
- `MacroCommand` — multi-line macro body with argument frame definition

### Command IR Extensions

- `CommandKind::Macro(MacroCommand)` — a macro function body, rendered with `$` prefix for lines with variables
- `CommandKind::FunctionWithStorage(FunctionWithStorage)` — calling convention for passing macro arguments via `function <id> with storage <path>`

All 15+ exhaustive match arms updated across the codebase (solve, report, contract, emit, local, syntax, reconcile, verify, etc.).

### Preflight Recipe Routing

`SelectedSemanticRecipe::is_unusable_inline()` gates whether a recipe needs a helper `.mcfunction`.
Macro recipes (`Java26_2MacroBookPage`) return `true`, which propagates through `external_requires_helper()`
to allocate a `PlannedFunctionId` and route to `InstructionPlan::External { helper }`.

The helper body is created via `define_external_helper` with the recipe's `command_kind()`,
which produces a `CommandKind::Macro(...)` for macro recipes.

### Book Page Intrinsic

- `MinecraftOperationAttributes::BookPage { page_index: MacroOrStatic<u8> }` — the single attribute
- Preflight matches: `Static(n)` → `Java26_2BookPage` (existing inline data command), `Macro(v)` → `Java26_2MacroBookPage` (macro helper)
- `Java26_2MacroBookPage` produces a two-line `MacroCommand`: empty init (`data modify ... set value ""`) + macro entity read (`$data modify ... pages[$(index)].raw`)

### Frontend

- HIR: `HirMinecraftOperationAttributes::BookPageRuntime { page_origin }` — runtime book page reads
- Checker: accepts runtime I32 expressions and creates `BookPageRuntime` HIR
- Lowering: maps `BookPageRuntime` → `MinecraftOperationAttributes::BookPage { page_index: MacroOrStatic::Macro(ValueId(0)) }` (placeholder, see deferred items)

## How to Add a New Macro-Backed Intrinsic

1. Change the attribute's static field to `MacroOrStatic<T>` in `ir/core/minecraft.rs`
2. Add a `Static`/`Macro` match arm in preflight recipe selection (`preflight.rs`)
3. Add a `Java26_2Macro*` variant to `SelectedSemanticRecipe` with `is_unusable_inline() = true`
4. Implement `command_kind()` returning `CommandKind::Macro(...)` with the command template
5. Add a `BookPageRuntime`-equivalent HIR variant if needed
6. Wire the checker to accept runtime expressions

## Deferred to Stage 11

### Runtime Value Bridging
The `ValueId::from_index(0)` in the HIR→Core lowering for `BookPageRuntime` is a placeholder.
The actual runtime I32 from the expression needs to be:
1. Resolved to its HomeId in the plan
2. Bridged from scoreboard to storage via `execute store result storage ... run scoreboard players get`
3. Placed into the macro argument compound before the `function ... with storage` call

This requires plan-level HomeId resolution in the emit phase, which needs access to
the `InstructionPlan` and `FunctionLoweringCx` infrastructure already available.

### Core Evaluator Macro Support
The Core evaluator (`ir/core/eval.rs`) cannot execute `CommandKind::Macro` or
`CommandKind::FunctionWithStorage` operations. Stage 11 should add support so that
differential testing covers macro-based compilation.

### NbtPath Generalization
Move `MacroOrStatic<i32>` into `NbtPathSegment::Index` so that any intrinsic producing
an NBT path with an index gets macro support without per-intrinsic recipe code.

### Loop Unrolling for Macro Elimination
Constant-bounded loops with macro-typed induction variables can be unrolled at the Core IR
level, converting `Macro(value)` → `Static(constant)` for each iteration. Eliminates
macro calls entirely for the book-page-loop case.

### Macro Hoisting and Batching
When the same `Macro(ValueId)` feeds multiple external ops, call the macro helper once
at the earliest dominating block and store the result. When multiple macro-typed
externals appear in the same block, batch them into one helper function.

## Explicitly NOT in Scope

These were originally part of Stage 10 but are deferred to Stage 12+:

- **Language-level macros (comptime)** — compile-time AST→AST metaprogramming
- **MacroCommandFragment** — unsafe raw command text from runtime strings
- **Macro cache optimization** — constant folding, variance separation, hoisting
- **Dynamic frame allocation** — stack/list frames for recursive macro use
- **Selector fragment macros** — no client yet

## Files Changed

23 files, 365 insertions, 83 deletions:

**New:**
- `ir/core/macro_or_static.rs` — `MacroOrStatic<T>` type
- `ir/minecraft/macro_command.rs` — `MacroSlot`, `MacroCommand`, `MacroLine`, `MacroSegment`, `MacroArguments`, `MacroVariable`

**Modified (exhaustive matches):**
- `analysis/minecraft/{local,solve,report,syntax}.rs` — `CommandKind` matches
- `ir/minecraft/{command,contract,dump,render,verify,mod}.rs` — command IR
- `ir/core/{minecraft,print,verify,mod}.rs` — Core IR types
- `lower/minecraft/{emit,preflight,resources,plan/assemble,plan/reconcile}.rs` — lowering
- `frontend/{check,hir,behavior,lower}.rs` — frontend
