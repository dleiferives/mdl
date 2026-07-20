# Stages 11 and 12 — Conceptual Boundaries

Date: 2026-07-20
Status: **superseded by [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md)**

> This file was an early boundary sketch. The concrete structural model — the
> value-crossing edge, the `SyntaxSlot` taxonomy, the derived `extract_crossings`
> engine, and first-class references — now lives in the architecture of record
> [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md), with
> the first executable slice planned in
> [`ps-11-macro-reference-crossing-plan.md`](ps-11-macro-reference-crossing-plan.md).
> The sketch below is retained for provenance.

## Stage 11: Macro Generalization and Optimization

Stage 11 takes the `MacroOrStatic<T>` pattern proven on one intrinsic (book page)
and generalizes it across the lowering surface, then applies the optimizations
that the live-range model enables. No new concepts — everything here flows from
the architecture shipped in Stage 10.

### Runtime ValueId Bridging

The `ValueId::from_index(0)` placeholder in `HirMinecraftOperationAttributes::BookPageRuntime` → `MacroOrStatic::Macro(ValueId)` lowering needs to become the actual I32 from the source expression. This requires:

- Resolving the `ValueId` to its `HomeId` in the plan (scoreboard home)
- Emitting `execute store result storage ... run scoreboard players get` to bridge from score to storage before the `function ... with storage` call
- Placing the bridged value into the macro argument compound at the correct key

The plan-level infrastructure (`InstructionPlan`, `FunctionLoweringCx`, score/storage home lookups) already exists — this is a focused wire-up, not new architecture.

### NbtPath Generalization

Move `MacroOrStatic<i32>` into `NbtPathSegment::Index`. Every intrinsic that constructs an NBT path with an index (book pages, list access, entity NBT reads) gets macro support without per-intrinsic recipes.

The render path for `NbtPath` already knows how to emit `[N]` syntax. A `Macro(v)` index renders as `[$(nbt_index)]` in the macro body context. The preflight recipe that produces the `DataCommand` just passes through — the `MacroOrStatic` in the path segment triggers helper creation automatically.

This collapses the per-intrinsic `Java26_2Macro*` recipe pattern into one generic path-level mechanism.

### Core Evaluator Macro Support

The Core evaluator needs to handle `CommandKind::Macro` and `CommandKind::FunctionWithStorage`. For differential testing, the evaluator should recognize macro commands and report them as un-evaluable (like external ops already do).

### Loop Unrolling for Macro Elimination

When a loop has a known iteration count at compile time (SCCP-proven bounds) and a macro-typed induction variable, the optimizer unrolls the loop body N times, substituting `Macro(value)` → `Static(0)`, `Static(1)`, ..., `Static(N-1)` for each iteration. Each unrolled copy uses the existing static lowering path — zero macro calls, zero reparse cost. This is a Core IR transformation that runs before lowering.

### Macro Hoisting and Batching

**Hoisting:** When the same `Macro(ValueId)` feeds multiple externals in different blocks, call the macro helper once at the earliest dominating block, store the result, and reuse it.

**Batching:** When multiple macro-typed externals appear in the same block, emit one helper function with multiple `$` lines and one argument compound. One `function ... with storage` call instead of multiple.

Both are plan-level optimizations (recipe and instruction reordering), not Core IR changes.

### Scoreboard Generalization

Just as NbtPath gets `MacroOrStatic<i32>` for indices, scoreboard operations get macro support for dynamic objective names, holder names, and score values. The pattern is the same — the renderer produces `$scoreboard players ... $(param)` syntax.

### Cost-Directed Boundary Selection

Stage 10 uses instruction-site placement: the macro helper is called from the block containing the `External` instruction. Stage 11 can choose different placement based on cost — earlier resolution (dominating block) avoids repeated calls, later resolution (callee-side) pushes work into callers, inlined resolution (SCCP-proven constant) eliminates the macro entirely.

---

## Stage 12: Full Command-General Macro System and Language Metaprogramming

Stage 12 takes the macro infrastructure through its final generalization and adds compile-time language-level metaprogramming.

### All Commands Consume Macro-Typed Values

Any `CommandKind` variant that carries a static string, identifier, or literal gets a `MacroOrStatic<T>` wrapper. The render pipeline emits `$(key)` instead of the literal text. This covers say messages, teleport coordinates, selector arguments, resource IDs, SNBT values — the `MacroSlot` taxonomy provides the serializer for each syntax position.

### Language-Level Macros (Comptime)

The other half of the original Stage 10 scope. Language macros operate on typed HIR, produce HIR, and participate in hygiene/name resolution. They never emit `$`-prefixed lines.

A `comptime` annotation on a function or block means: evaluate this at compile time. The Core evaluator serves as the execution engine. Pure functions can be fully evaluated. Functions with external ops become partial — constant-fold what they can, leave the rest.

### Typed Raw-Command Interpolation

Implements the `CommandTemplate` design from `notes/compiler/stage-6-effects-plan.md` — a parsed sequence of literal and typed placeholder segments with compiler-owned serializers. This generalizes `MacroSegment::Literal` / `MacroSegment::Variable` to track which typed source values go into which syntax positions.

### Macro Cache Optimization

Constant-fold compile-time values out of macro templates, separate high-variance parameters from low-variance ones, hoist repeated calls. The Stage 10 `macro-composition.md` research describes the strategy — Stage 12 implements it.

### Dynamic Frame Allocation

For recursive functions with macro calls, static frames (one per signature) are insufficient. Stage 12 adds stack/list frames for recursive macro use, with spill/restore at call boundaries.

---

## Boundary Between Stages

| Stage 11 | Stage 12 |
|----------|----------|
| Generalizing macros to existing lowering (NbtPath, scoreboard) | Generalizing to ALL commands |
| Loop unrolling, hoisting, batching — Core IR and plan optimizations | Macro cache optimization — backend optimization |
| Core evaluator macro awareness | Comptime language macros |
| Runtime value bridging (wire the placeholder) | Typed raw-command interpolation |
| Cost-directed boundary selection | Dynamic frames, recursive macro support |

Stage 11 makes macros fast and broad within the existing compilation model.
Stage 12 makes macros a first-class language feature with compile-time metaprogramming
and typed interpolation across the entire command surface.
