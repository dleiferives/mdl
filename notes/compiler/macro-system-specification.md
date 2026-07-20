# Macro System — Full Specification

Date: 2026-07-20
Status: **specification. §1–§3 describe the shipped PS-11A–C mechanism (with two confirmed bugs,
see [`entity-nbt-path-composability.md`](entity-nbt-path-composability.md) Part 1); §4 onward is
design for PS-11's Tier C generalization — not yet implemented.**

`macro-reference-crossing-model.md` states the macro half of the value-crossing duality as a
short conceptual sketch. This note is the operational specification: every syntax position in
every `CommandKind` classified by whether it can be indirected (`Ref`) or must be substituted
(`Subst`), the exact macro ABI (frame layout, key naming, escaping), the safety invariants that
make substitution sound, and the cost/placement model worked out concretely enough to implement
against — not just gestured at in a table.

## 1. What exists today (PS-11A–C, commit `7654bc0`)

- `Operand<T> = Const(T) | Runtime(ValueId)` (`ir/core/operand.rs`) — the honest rename of
  `MacroOrStatic<T>`, used today only on `MinecraftOperationAttributes::BookPage.page_index` and
  `NbtPathSegment::Index`.
- `SyntaxSlot` (`ir/minecraft/macro_command.rs`) — eight variants (`Int`, `Float`, `Snbt`,
  `NbtKey`, `NbtIndex`, `ResourceId`, `SelectorFragment`, `CommandFragment`), each with a stubbed
  `indirection_form() -> Option<IndirectRecipe>` that returns `None` for every variant today
  (`IndirectRecipe` is `pub enum IndirectRecipe {}` — uninhabited, honestly marked unimplemented,
  not faked).
- `crossings.rs`'s COLLECT→FRAME→BRIDGE→RENDER→CALL engine, generic over `CommandKind` in
  principle but with exactly one populated match arm today: `CommandKind::Data(DataCommand::Modify
  { source: DataSource::Entity { path, .. }, .. })`, walking `NbtPathSegment::Index(Operand::Runtime(_))`
  entries in `path`. Every other `CommandKind` variant, and every other `DataSource` variant, is
  unhandled (`collect_runtime_operands` returns `vec![]` for anything else; `render_as_macro`
  panics on anything else).

This section is not aspirational — it is what to read if the goal is "what actually runs
today." Everything below §2 is either filling in the stubs already present in the shipped code
(`SyntaxSlot::indirection_form`, more `crossings.rs` match arms) or new structure the shipped
code has no trace of yet (the ABI's formal frame-naming rule, hoisting/batching).

## 2. The macro ABI — exact frame and naming contract

A macro helper `.mcfunction` receives its arguments through one NBT compound in command storage.
The shipped engine already fixes the physical location:

```text
storage: mdl:__mdl/macro
compound key: args
```

with each `MacroVariable.key` becoming one field of that compound (`args.<key>`). The shipped
`build_frame` (`crossings.rs`) assigns keys `i0`, `i1`, ... in first-encounter order, deduplicated
by `ValueId` (the same runtime value referenced twice gets one frame slot, reused). This section
promotes that from "an implementation detail of one function" to a **stated ABI contract** other
code is allowed to depend on:

1. **One frame per macro call, not per macro-typed value.** If a command needs three runtime
   values substituted, they share one compound and one `function ... with storage` call — never
   three separate calls. (Already true today because `build_frame` processes the whole operand
   set for one command at once; stated here so it stays true as `collect_runtime_operands` grows
   more match arms.)
2. **Keys are stable and content-addressed by `ValueId`, not by source position.** Two uses of
   the same `ValueId` in one command's operand tree always get the same key. This is required for
   §5's hoisting/batching to be sound — batching two commands into one helper call is only valid
   if identical values collapse to identical keys.
3. **The frame is seeded to `{}` immediately before every call, never reused across calls.**
   Matches the shipped `seed_macro_frame`. A future batching optimization (§5) that reuses one
   seeded frame across multiple calls in the same block must re-justify this per-call reset, not
   silently drop it — Mojang's own cache-key behavior is keyed on the *complete* parameter tuple
   (`macro-composition.md`), so a stale leftover field is not merely wasted, it can change cache
   hit/miss behavior in ways the compiler did not intend.
4. **Every field actually used by the macro body is present; no unused field is added.**
   (`macro-composition.md`: "include only actually referenced values in a macro argument tuple" —
   this is a stated cost requirement, not just tidiness; extra fields widen the effective cache
   key.)

## 3. The full `SyntaxSlot` taxonomy — every command surface, classified

This is the promised completion of `macro-reference-crossing-model.md` §3's table, worked
through the *actual* closed `CommandKind`/`ExecuteModifierKind` enums (`ir/minecraft/command.rs`,
`ir/minecraft/execute.rs`), not a representative sample. "Indirectable" means a future
`IndirectRecipe` should exist for this position (an operand here can be filled by `... from
storage`/`execute store`/`scoreboard players operation` instead of macro substitution);
"Subst-only" means Brigadier parses this position as inline syntax with no indirection form, so
`Runtime` here is **always** a macro crossing.

| `CommandKind` variant | Operand position | `SyntaxSlot` | Indirectable? |
| --- | --- | --- | --- |
| `Score(ScoreCommand::PlayersSet)` | `value: i32` | `Int` | **Indirectable** — score value already has a native indirection form: `execute store result score <target> run ...` |
| `Score(PlayersAdd/PlayersRemove)` | `amount` | `Int` | Indirectable, same as above |
| `Score(PlayersOperation)` | `op` operand score | (operand is itself a `ScoreRef`, always static holder/objective) | Indirectable — this is literally what `scoreboard players operation` *is* |
| `Score(*)` | holder / objective **names** | `NbtKey`-shaped (name, not value) | **Subst-only** — Brigadier's `score_holder`/`objective` argument parsers accept literal tokens, no `from`/`store` form exists for "which objective" |
| `Data(Get/Modify)` | `target`/`source` **path segments** (keys, indices) | `NbtKey` (compound key) / `NbtIndex` (list index) | **Subst-only for the path shape itself** — see NbtPath row below. The **value being written** (`DataSource::Value`) is Indirectable (`... set from <path>` already exists as the non-macro form) |
| `Data(Modify)` | `NbtPathSegment::Index` | `NbtIndex` | **Subst-only** — `[N]` is Brigadier path syntax, no indirection exists for "which index" (this is the one already shipped) |
| `Data(Modify)` | `NbtPathSegment::Key` | `NbtKey` | **Subst-only**, and additionally restricted to compile-time-known keys only — see `references-design.md` §6 and the schema system; MDL does not attempt indirection *or* substitution for a truly dynamic key today (`dynamic-keys-and-path-safety.md`) |
| `Say(SayCommand)` | `message: SayMessage` | `Snbt`/text | **Indirectable** — a runtime message should become a text component reading storage (`tellraw`-shaped), not a macro; see `macro-reference-crossing-model.md` §6's worked example |
| `Teleport(TeleportCommand)` | `destination` coordinates | `Float`/`Int` per axis | **Subst-only** — coordinate literals are inline Brigadier syntax (`~`, `^`, plain numbers), no `from storage` form for "the coordinate itself" |
| `Execute` / `ExecuteModifierKind::As, At` | `Selector` | `SelectorFragment` | **Subst-only** — selector arguments are inline syntax |
| `Execute` / `In` | `DimensionId` | `ResourceId` | **Subst-only** — dimension is a resource-id literal position |
| `Execute` / `Positioned, Rotated` | coordinates | `Float` | **Subst-only**, same as `Teleport` |
| `Execute` / `Anchored` | `TargetAnchor` (closed enum: eyes/feet) | — | **Never dynamic** — this is a closed compile-time enum in the type system already; not a macro/indirection question at all |
| `Execute` / `Align` | `TargetAxes` | — | Same — closed compile-time value |
| `Execute` / `If, Unless` | `Condition` (recursive: `ScoreMatches`, `ScoreCompare`, `Function`, ...) | mixed, recurse into the condition's own operands | Each leaf follows its own row (e.g. `ScoreMatches`'s range bounds are `Int`, Indirectable in the sense the *value being tested* can come from a score, but the *comparison syntax itself* is inline) |
| `Execute` / `Store` | `StoreDestination` (`Storage{target,...}` / `Score{...}`) | — | The destination **path** follows `Data`'s rules above; this modifier *is itself* one of the two general indirection bridges (§7 names it explicitly) |
| `Function(FunctionCall)` | `CallableRef` (function/tag resource id) | `ResourceId` | **Subst-only** — `function <id>` has no indirection form; "which function to call" cannot be expressed as `from storage` |
| `Return(ReturnCommand::Value)` | `i32` | `Int` | **Indirectable** — `return run scoreboard players get ...` already exists as the non-literal form |
| `Raw(UnsafeRawCommand)` | entire text | `CommandFragment` | **Unsafe, opt-in only** — never produced by automatic lowering, per the existing safety invariant |
| `Macro`, `FunctionWithStorage` | — | — | These are **emission encodings**, not source-level operand positions — see `macro-reference-crossing-model.md` §4's "there is no macro command" |

### 3.1 What this table is for

It is the answer to "apply this structure to all commands, not just NBT," worked out concretely
rather than asserted. Two consequences fall out directly:

- **Most command surfaces are Subst-only.** Coordinates, selectors, resource ids, objective/holder
  names, and NBT path shape have no Brigadier indirection form — a runtime value reaching any of
  these positions is *necessarily* a macro crossing, no matter how the crossing-selection cost
  model is tuned. The engine's job for these is purely mechanical (COLLECT/FRAME/BRIDGE/RENDER),
  never a choice.
- **A few high-traffic surfaces are genuinely Indirectable**, and getting those right is where
  the model earns its keep: score values, storage values, and `say`/text messages. These are
  exactly the positions where a naive "always macro when runtime" implementation would burn a
  reparse and a cache-key slot for something that should have been an ordinary `execute store`/
  `from storage` line instead. `IndirectRecipe` (§4) exists specifically to make this choice
  structural instead of ad hoc.

## 4. Filling in `IndirectRecipe` — what an indirection recipe actually is

`SyntaxSlot::indirection_form() -> Option<IndirectRecipe>` is stubbed to always return `None`
today. This section specifies what fills it in, per the Indirectable rows in §3's table:

```text
IndirectRecipe {
    ScoreValue,       // fills a numeric position via a prior
                       //   `execute store result score <target> run <compute>`
    StorageValue,     // fills a numeric/string/snbt position via a prior
                       //   `execute store result storage <target> ... run <compute>`
                       //   or plain `... set from storage <path>` when no store/compute is
                       //   needed (the value already lives in storage)
    ScoreOperand,     // fills a score-vs-score position directly:
                       //   `scoreboard players operation <target> <op> <source>`
    TextComponent,     // fills a message/text position via
                       //   {"storage":"<id>","nbt":"<path>"} instead of literal text
}
```

Each variant is a **command template with one hole**, not a general code generator — this keeps
the recipe closed and reviewable, matching the existing `MacroSlot`/`SyntaxSlot` closed-enum
discipline (`macro-reference-crossing-model.md` §7's "closed and typed" invariant). Selecting
`Some(IndirectRecipe::ScoreValue)` for `SyntaxSlot::Int` on a `Score(PlayersSet)` value position,
for example, is what makes `set target = some_runtime_score;` lower to a plain
`scoreboard players operation`/`execute store` chain instead of ever considering a macro — no
cost comparison is even needed for this case, because there is only one sane implementation and
it was always available.

## 5. Cost, cache, and placement — worked through, not just tabled

`macro-reference-crossing-model.md` §8 lists cost dimensions; this section works one concrete
scenario through them, because a list of dimension names alone does not answer "what does the
optimizer actually do."

**Scenario:** a loop body reads `reader.equipment.mainhand.components."minecraft:written_book_content".pages[i].raw`
once per iteration, where `i` is the loop induction variable, trip count not statically known.

- Every iteration's `i` is a distinct SSA value in Core IR terms, but the **macro helper function
  itself** is the same `.mcfunction` across all iterations (it's a static declaration, not
  regenerated per iteration) — so the cost that matters is *per-call* reparse/cache cost, not
  code size.
- Mojang's documented cache behavior (`macro-composition.md`): repeated identical parameter
  tuples may hit a cache; novel tuples reparse. Since `i` varies every iteration, **every call is
  a cache miss** under this loop shape — there is no argument-tuple reuse to exploit here, unlike
  a loop that re-reads the *same* page repeatedly.
- **Placement choice available today:** none beyond "call from the block containing the read" —
  Stage 10's instruction-site placement, which is what's shipped. **Placement choices this note
  specifies for later work (not yet built):**
  - *Batching*: if the loop body also does a second unrelated runtime-indexed read in the same
    iteration, one helper call with a two-field frame beats two calls with two one-field frames
    (§2's frame-per-call-not-per-value ABI rule already allows this; the optimizer pass to detect
    and merge the two `collect_runtime_operands` results into one `build_frame` call does not
    exist yet).
  - *Loop unrolling elimination*: if the trip count *is* provably bounded (SCCP-proven), unroll
    N times and substitute `Runtime(i)` → `Const(0), Const(1), ..., Const(N-1)` per copy — this
    removes every macro call in the loop entirely, replacing them with N ordinary inline reads.
    This is strictly better whenever code-size growth (N copies) is acceptable, and is the
    concrete instance of `macro-reference-crossing-model.md` §2's "unroll a constant-bounded
    loop → `Subst` becomes `Const`" row.
  - *Specialization*: if `i`'s runtime domain is provably small and finite (e.g., a 3-way `switch`
    feeding the index), emit 3 pre-parsed specialized reads plus a dispatch, instead of one macro
    — `dynamic-access.md`'s "Candidate 2" and `macro-composition.md`'s "four pre-parsed functions
    plus dispatch" example, applied to this exact call site.

None of the three bullets above exist in code today; they are the concrete work items this
section hands to whichever later PS milestone builds the crossing-placement pass
(`macro-reference-crossing-model.md` §2, §10 Tier B/C).

## 6. Safety invariants — restated as enforceable rules, not prose

Collected from `macro-composition.md`, `dynamic-keys-and-path-safety.md`, and
`stage-6-effects-plan.md`, stated as rules a verifier can check:

1. **A `SyntaxSlot::CommandFragment` value is never produced by any automatic lowering path.**
   The only constructor is the explicit unsafe-raw-command surface. A verifier check: every
   `MacroVariable` in a compiler-generated `MacroCommand` has `slot != CommandFragment`.
2. **A raw runtime `String` is never accepted where a `SyntaxSlot` expects a validated type.**
   There is no implicit `String -> MacroSlot` conversion anywhere; every slot's macro serializer
   takes its own typed source value (`u8` for `NbtIndex`, a validated resource-id type for
   `ResourceId`, etc.), never a bare `String`.
3. **Every macro variable key is unique within its frame and matches `[a-zA-Z0-9_]+`.** Already
   enforced by `MacroArguments::new` (`macro_command.rs`) — restated here as a cross-referenced
   invariant, not a new rule.
4. **A macro command's assembled line (literals + substituted `$(key)` placeholders) passes the
   same physical-line shape validation as any other command line** — no leading/trailing
   whitespace, no embedded newline, no reserved-prefix collision — checked on the *assembled*
   text, not per-literal-fragment (this was PS-11's fixed bug; restated here as the permanent
   rule going forward, not a one-time patch).
5. **A dynamic (`Runtime`) NBT path segment is only ever an `Index`, never a `Key`.** Per
   `dynamic-keys-and-path-safety.md`, safely encoding an arbitrary runtime string into a path
   segment is unsolved; until a `MacroNbtKey` encoder exists and is separately specified and
   tested, `NbtPathSegment::Key` accepts only `Operand::Const` — enforced at the type level by
   `NbtPathKey` never being parameterized by `Operand<...>` (unlike `Index`, which is), not by a
   runtime check.

## 7. Relationship to references

Restated for cross-navigation (full treatment in [`references-design.md`](references-design.md)
§7): a reference read/write-through is a **Ref crossing**, filled by exactly the `IndirectRecipe`
forms this note specifies in §4 — there is no separate reference-lowering mechanism. A dynamic
step inside a reference's path is a **Subst crossing** on that path segment, going through this
note's engine unchanged. The two notes describe one mechanism from two entry points (value-first
here, reference-first there) on purpose.

## 8. What "done" looks like for Tier C (§3–§6 of this note)

1. Every row in §3's table with "Indirectable" has a real `IndirectRecipe` variant (§4) wired
   into `SyntaxSlot::indirection_form`, replacing the current always-`None` stub.
2. `crossings.rs`'s `collect_runtime_operands`/`render_as_macro` match arms cover every
   `CommandKind` variant in §3's table, not just `Data(Modify)` — verified by a census test
   (mirroring the project's existing "exhaustive match" test discipline) that fails if a new
   `CommandKind` variant is added without a corresponding crossing-engine arm.
3. §2's ABI rules (stable content-addressed keys, one frame per call, no unused fields) hold as
   *tested* invariants, not just documented intent — a property test generating random operand
   sets and asserting frame-key stability/dedup is a natural fit here.
4. §5's three placement optimizations (batching, unroll-elimination, specialization) each have at
   least one integration test proving the *cheaper* command shape is what's actually emitted, not
   just that the naive macro shape is emitted correctly (which is already tested today).
5. §6's five safety invariants each have a corruption/verifier test, matching the project's
   existing "independently recompute every invariant, don't just trust the constructor" pattern
   from PS-4/PS-5's HIR verification discipline.
