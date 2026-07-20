# The Value-Crossing Model for Macros and References

Date: 2026-07-20
Status: **architecture of record — supersedes the generalization sketches in
[`pre-scheduler/stages-11-12-boundaries.md`](pre-scheduler/stages-11-12-boundaries.md)**

This note describes one compiler intrinsic that unifies Minecraft function macros and
references. It is a structural model, not an implementation plan; §10 records how it stages
onto Stages 11–12 and §9 names the concrete code anchors a later stage would touch.

## Why this note exists

Stage 10 (see [`pre-scheduler/stage-10-handoff.md`](pre-scheduler/stage-10-handoff.md))
shipped the *seed* of a macro system but not a system:

- `MacroOrStatic<T>` (`ir/core/macro_or_static.rs`) captures the static/dynamic duality but is
  wired into exactly **one** field — `MinecraftOperationAttributes::BookPage.page_index`.
- Macro support is **per-intrinsic**: each dynamic op needs its own hand-authored
  `Java26_2Macro*` recipe (`lower/minecraft/preflight.rs:255`), its own `command_kind()`
  template, and its own `is_unusable_inline()` arm. `BookPage` is the only one, and the pattern
  does not scale to the ~10 `CommandKind` variants × the 55 Brigadier argument domains
  inventoried in [`../mcfunction/native-value-carriers.md`](../mcfunction/native-value-carriers.md).
- The runtime bridge is **unwired**: `frontend/lower.rs:1158` hardcodes `ValueId::from_index(0)`,
  `frontend/check.rs:5079` discards the argument's SSA value (`let _`), and `emit.rs:313-334`
  seeds an **empty** argument compound and never writes the runtime value into it. A runtime
  `BookPage` therefore cannot execute correctly today.
- **References** — the static/indirect half of the duality described in
  [`../mcfunction/nbt/references-identity-and-ownership.md`](../mcfunction/nbt/references-identity-and-ownership.md),
  [`../mcfunction/dynamic-access.md`](../mcfunction/dynamic-access.md), and
  [`../mcfunction/representation-selection.md`](../mcfunction/representation-selection.md) —
  have no first-class representation. The physical "home" concept exists only as
  lowering-private plumbing (`lower/minecraft/plan.rs` `Home`/`HomeId`/`HomeRole`).

Three latent defects are symptoms of the un-generalized design and are subsumed by it:
`ir/minecraft/render.rs:170` (`FunctionWithStorage` omits the `function ` keyword), the
empty-frame bridge bug above, and the test interpreter at `emit.rs:2837` missing
`Macro`/`FunctionWithStorage` arms.

**Goal:** one abstraction — a *value-crossing edge* — that makes macros and references two
encodings of the same thing, derives macro-ness generically for every command instead of
authoring it per-intrinsic, and turns "where does the runtime value become text" into a free
*placement* choice the optimizer owns.

## 1. The core idea: every operand is a value-crossing edge

Every operand of every command must ultimately become **text** in a `.mcfunction` line. A
runtime value can reach a syntax position in exactly four ways. These are not four features;
they are four **encodings of one abstract edge** between a runtime value and a static
syntactic context:

| Encoding | Static side | Runtime side | Crossing mechanism | Chosen when |
|---|---|---|---|---|
| **Const** | literal text | — | constant fold | value is compile-time known |
| **Ref** (reference) | a validated *address* (path / score ref) | the pointee value | NBT/score **indirection** (`... from storage`, `scoreboard players operation`, `execute store … run`) | the position has an indirection form |
| **Subst** (macro) | command text *after* substitution | value in an argument frame | `$(var)` substitution at a `.mcfunction` boundary | the position has **no** indirection form |
| **Dispatch** | one static command per case | a selector value | a branch/decision tree | a small finite domain, or cost wins |

The duality the whole note turns on:

```text
Reference:  static NBT path   ──indirection──→   runtime value
Macro:      runtime value      ──$(var) at a .mcfunction boundary──→   static command text
```

Both have one static side and one runtime side. **References bridge inward** (a fixed address
pulls a runtime value in); **macros bridge outward** (a runtime value is pushed into fixed
text). They are mirror images, and the compiler picks the direction per operand by which
syntax position it is filling and what it costs.

**Consequence:** `MacroOrStatic<T>` was only ever modelling two of the four encodings (Const,
Subst). The clean model separates *what the frontend knows* from *how lowering encodes it*:

- **Core IR operand:** `Operand<T> = Const(T) | Runtime(ValueId)` — the frontend only ever
  knows "constant" vs "a runtime SSA value." (This is `MacroOrStatic<T>` renamed and demoted to
  its honest meaning.)
- **Encoding (`Ref` / `Subst` / `Dispatch`)** is chosen by a lowering pass from the operand's
  *syntax slot*, never authored by hand.

## 2. Macro resolution is a placement problem (the live-range insight)

The pivotal realization: **any `.mcfunction` call boundary can be the macro resolution point**
— the place where a `Runtime` operand crosses into static text via `$(var)`.

A macro-typed value therefore has a **live range**: from its definition/update site in Core IR,
down through the call graph, to every syntax use site. *Any block within that live range that
sits at (or can be split into) a `.mcfunction` boundary is a legal resolution point.* Moving
the resolution point never changes observable behavior — it is a **structural choice, not a
semantic constraint.**

This makes macro lowering **isomorphic to register allocation / instruction scheduling**:

| Macro placement move | Classic-compiler analogue |
|---|---|
| Choose resolution block within a live range | spill/rematerialize placement |
| Hoist resolution to a dominating block | loop-invariant code motion |
| Batch several `Subst` uses into one helper + one frame | coalescing / bundling |
| Unroll a constant-bounded loop → `Subst` becomes `Const` per iteration | constant propagation after unrolling |
| Resolve callee-side vs caller-side | inline vs out-of-line boundary selection |
| Eliminate via layout change / captured context | dead-crossing elimination |

The same is true of references: a `Ref` crossing (the indirection bridge) also has a live range
and a placement — you can hoist a repeated read, sink a write, or coalesce reads of the same
address. **Crossing placement is the single optimization surface that owns hoisting, batching,
unrolling, and cost-directed encoding selection for both macros and references.**

## 3. The slot taxonomy: generalize `MacroSlot` to every syntax position

Today `MacroSlot` (`ir/minecraft/macro_command.rs`) enumerates *macro* positions only (`Int,
Float, Snbt, NbtKey, NbtIndex, ResourceId, SelectorFragment, CommandFragment`). Promote it to a
**`SyntaxSlot`** taxonomy that describes *every* place a value can sit in a command, backed by
the 55 Brigadier argument domains in
[`../mcfunction/native-value-carriers.md`](../mcfunction/native-value-carriers.md). Each slot
declares up to three serializers:

```text
SyntaxSlot {
    syntax_kind,                        // Int, Float, NbtIndex, NbtKey, ResourceId<K>,
                                        //   Coord, SelectorArg, Snbt<T>, Message, Objective,
                                        //   ScoreHolder, FunctionId, ...  (closed; Brigadier-derived)
    static_serializer:   T -> text,                  // Const encoding
    indirection_form:    Option<IndirectRecipe>,     // Ref encoding, if the position supports it
    macro_serializer:    MacroSlot,                  // Subst encoding: T -> $(key) + escaping/validation
}
```

The presence/absence of `indirection_form` is the structural fact that decides Ref-vs-Subst:

- **Indirectable positions** (prefer `Ref`, never force a macro): a *data source* (`… from
  storage <path>`), a *score operand* (`scoreboard players operation`), a *store target*
  (`execute store result …`), a *text component* (`{"storage":…,"nbt":…}`). These are why
  ordinary arithmetic, copies, and even runtime `Text` are **not** macros.
- **Non-indirectable positions** (force `Subst`, or `Dispatch`): an NBT list **index** `[N]`, a
  **resource id**, a **coordinate literal**, a **selector fragment**, a **compound key** in a
  path. Brigadier parses these as syntax with no `from`/`store` form, so the only runtime
  encodings are macro substitution or a static dispatch tree.

Safety invariant (from [`../mcfunction/macro-composition.md`](../mcfunction/macro-composition.md)
and [`stage-6-effects-plan.md`](stage-6-effects-plan.md)): a raw runtime `String` is **never** a
valid `SyntaxSlot` fill for command syntax. `CommandFragment` remains unsafe/opt-in only. Every
slot owns its escaping, range, and NBT/token rules.

## 4. The generic macro engine: derive macro-ness, never author it

With operands as `Operand<T>` over typed `SyntaxSlot`s, macro helpers become **generated**, not
written. A single pass over *any* structured command produces everything:

```text
extract_crossings(command, plan):
    1. COLLECT   walk the command's operand tree; for each Runtime(ValueId) operand,
                 read its SyntaxSlot. If the slot is indirectable and cost favors it,
                 mark it Ref; else mark it Subst (or Dispatch).
    2. FRAME     for the Subst set, synthesize a MacroArguments frame: dedup by ValueId,
                 stable key naming, one MacroVariable per (value, slot).
    3. BRIDGE    for each frame variable, resolve ValueId -> HomeId (plan.value_home) ->
                 physical home, and emit the bridge that writes it into the arg compound:
                   score home:   execute store result storage <frame> <key> int 1
                                     run scoreboard players get <holder> <obj>
                   storage home: data modify storage <frame> <key> set from storage <src>
    4. RENDER    emit the command as MacroLine(s): literal segments verbatim, Subst slots as
                 $(key); a line with any variable is $-prefixed. Ref slots were already
                 filled by their indirection bridge, so they render as ordinary text.
    5. CALL      seed the frame, then FunctionWithStorage(helper, frame).
```

This is generic over **all** `CommandKind` variants. Adding a new intrinsic never touches the
macro engine — you declare its command with typed slots and macro-ness falls out.

**The structural payoff — "there is no macro command."** Conceptually, *every* command is
potentially a macro; macro-ness is *derived* from whether any slot resolved to `Subst`.
`CommandKind::Macro` and `FunctionWithStorage` stop being hand-authored IR you build; they
become **emission encodings** the engine produces. `SelectedSemanticRecipe`'s
`Java26_2*`/`Java26_2Macro*` pairs collapse into one slotted recipe, and `is_unusable_inline()`
stops being a hand-written arm — it becomes the derived predicate "this command has ≥1 `Subst`
slot after crossing selection."

## 5. References as first-class, and source-level `DataRef<T>`

The reference side is the mirror of the macro engine and reuses its machinery.

### 5.1 Promote the home model to a reference substrate

`plan.rs`'s `Home`/`HomeId`/`HomeRole` already *is* the physical-address concept, but it is
lowering-private. Promote a **`Reference`** value visible earlier in lowering:

```text
Reference {
    target_kind:    Storage | Entity | Block | Score,   // where it lives
    target_identity,                                     // StorageId / selector / block pos / holder
    path:           NbtPath-with-slots,                  // validated address (may contain Subst segments!)
    ty:             CoreType,
    invalidation:   facts,                               // see 5.3
}
```

- **read(ref) -> slot** and **write(slot, ref)** lower to *indirection crossings* (Ref) — the
  exact same `from`/`store` bridges the macro engine's step 3 emits. Refs and macros share one
  bridge vocabulary.
- **project(ref, field/index)** extends `path`. If the index/key is `Const`, it is a static path
  segment. **If it is `Runtime`, the projection is a `Subst` segment in the path** — i.e. *a
  dynamic reference projection is literally a macro on the path*. This is the composition that
  makes the model beautiful: references and macros are not two subsystems that interoperate,
  they are the **same slot machinery** applied to a path vs. to a command operand. This is
  exactly the `NbtPathSegment::Index(i32)` → `NbtPathSegment::Index(Operand<i32>)`
  generalization, seen from the reference side.

### 5.2 The default stays copy; references are opt-in

Deep-copy assignment remains the default aggregate value semantics (frozen PS-2 decision).
`DataRef<T>` is the **source-language** opt-in for aliasing / in-place mutation. Its type carries
`{target_kind, identity, validated_path, ty, invalidation}` — a *capability*, never an ordinary
`String`
([`../mcfunction/nbt/references-identity-and-ownership.md`](../mcfunction/nbt/references-identity-and-ownership.md)).
Reads/writes lower to indirection crossings; dynamic projections lower to macro crossings;
escape/lifetime is checked in the frontend.

### 5.3 Invalidation, aliasing, and effects

References carry the invalidation facts from the notes and feed effect/alias analysis:

- compound-field ref survives unrelated field edits, dies if the field/ancestor is
  removed/replaced;
- list-index ref may silently re-target after insert/remove/sort/reverse;
- entity ref: temporarily unresolvable while unloaded, permanently invalid when killed;
- block ref depends on dimension/loaded-chunk/block-entity-type;
- storage ref is the cleanest compiler-owned reference, but raw commands are observation
  barriers.

UUID handling follows the measured fixture: keep `[I; four ints]` for storage/equality/copy,
carry an execution-context/selector proof while the entity is selected, and convert to the
canonical UUID string only for a command domain that demands it.

## 6. Applying the model to *all* commands — worked examples

The point of the unification is that one pipeline explains every command. Each row is the *same*
`extract_crossings` pass choosing an encoding per slot by the slot's `indirection_form`:

| Source intent | Slot(s) | Chosen encoding | Emitted form |
|---|---|---|---|
| set NBT int from a runtime score | data value (indirectable) | **Ref** | `execute store result storage … int 1 run scoreboard players get …` — *no macro* |
| `say` a runtime string | message (indirectable as a text component) | **Ref** | `tellraw @a {"storage":"…","nbt":"…"}` — *no macro* |
| read `values[i]`, `i` runtime | NBT index (not indirectable) | **Subst** | `$data modify … from storage … values[$(i)]` |
| `tp @s x y z`, coords runtime | coordinate literals (not indirectable) | **Subst** | `$tp @s $(x) $(y) $(z)` |
| `scoreboard players get h o`, holder/obj runtime | name positions (not indirectable) | **Subst** | `$scoreboard players get $(h) $(o)` |
| call `ns:path`, id runtime, small domain | function id (not indirectable) | **Dispatch** or **Subst** | branch tree of static calls, or `$function $(ns):$(path)` |
| decrement a hot counter each tick | score operand (indirectable) | **Ref** | `scoreboard players remove …` — *never a macro* |

Two teaching cases fall out:

- **One command, mixed encodings.** `scoreboard players operation` with a runtime *value* but
  static objective uses `Ref` for the value; the same command with a runtime *objective name*
  forces `Subst` for that slot. The decision is per-slot, not per-command.
- **Prefer the cheaper direction.** Runtime `say`/`Text` becomes a `Ref` (text component reading
  storage), *not* a macro — the model automatically avoids a needless reparse because the
  message position is indirectable. `dynamic-access.md`'s "the best dynamic index is sometimes an
  index the compiler eliminated" is the same principle one tier up.

## 7. What makes it a beautiful, composable intrinsic

The invariants that keep it clean — the acceptance criteria for the design:

1. **One operand type everywhere.** Every dynamic command field is `Operand<T>` over a typed
   `SyntaxSlot`. No special-case fields.
2. **Derived, not authored.** Macro helpers, frames, bridges, and `is_unusable_inline` are
   *generated* by `extract_crossings`. Adding an intrinsic never edits the macro engine.
3. **Placement ⟂ semantics.** The resolution point is chosen by a scheduler-like pass; moving it
   is provably behavior-preserving (the live-range invariant), which is what unlocks
   hoist/batch/unroll/inline for free.
4. **Refs ∘ Macros compose.** A dynamic reference projection *is* a `Subst` path segment; one
   slot mechanism serves both. There is no macro↔reference interop layer because there are no two
   things.
5. **Closed and typed.** Slots are a closed, Brigadier-derived enum; a raw `String` never becomes
   command syntax; `CommandFragment` stays unsafe-opt-in.
6. **Cost-visible.** Every crossing has a cost, and every choice is explainable.

## 8. Cost model and transparency

Per [`../mcfunction/representation-selection.md`](../mcfunction/representation-selection.md),
every operand's chosen encoding is a *plan* with setup, steady-state, and code-size costs,
compared across: (1) ordinary commands + a score/storage bridge (Ref), (2) one macro command +
existing compound (Subst), (3) value specialization into pre-parsed functions, (4) bounded
dispatch tree, (5) a layout change that removes the dynamic position. Cost dimensions come from
[`../mcfunction/README.md`](../mcfunction/README.md) (reparsed macro lines, execute stages, fork
contexts, function invocations, cache hit/miss, score/NBT reads-writes, bridge temporaries, pack
size). `mdl explain` must show, per operand: the selected encoding, the reason, inserted
conversions, and the rejected alternatives — "aggressive optimization understandable rather than
magical."

## 9. How the current code maps onto this design

Refactor guidance, not this note's work. Concrete anchors so a later stage is mechanical:

- `MacroOrStatic<T>` (`ir/core/macro_or_static.rs`) → **`Operand<T> = Const | Runtime`**; used by
  every dynamic command field, not just `BookPage`.
- `NbtPathSegment::Index(i32)` (`ir/minecraft/nbt.rs:133`) → `Index(Operand<i32>)`; the renderer's
  `[N]` path already exists, `[$(key)]` is the `Subst` case. This single change gives every
  path-building intrinsic macro/reference support (the Stage 11 "NbtPath generalization").
- `MacroSlot` → **`SyntaxSlot`** with static/indirection/macro serializers (§3).
- `CommandKind::Macro` / `FunctionWithStorage` → **emission encodings** produced by
  `extract_crossings`, not authored IR. `SelectedSemanticRecipe`'s `Java26_2*`/`Java26_2Macro*`
  pairs (`preflight.rs:255`) collapse to one slotted recipe; `is_unusable_inline()`
  (`preflight.rs:500`) becomes derived.
- `plan.rs` `Home`/`HomeId`/`HomeRole` → promoted **`Reference`** substrate (§5.1); the
  `value_home → score/string_storage/list_storage` chain becomes the shared bridge vocabulary.
- New **crossing-placement pass** (live-range over the call graph) is the home of hoisting,
  batching, unrolling, and cost-directed encoding selection (§2).
- Wiring the generic bridge (§4 step 3) subsumes the three latent defects: it replaces the
  empty-frame seed (`emit.rs:313`), needs the argument `ValueId` retained (fix `check.rs:5079`
  `let _` and `lower.rs:1158` `from_index(0)` — thread the real value, drop the stub), and the
  derived renderer fixes the missing `function ` keyword (`render.rs:170`) and the test
  interpreter arms (`emit.rs:2837`).

## 10. Staging

- **Tier A — generic macro engine + bridge** (≈ Stage 11): `Operand<T>`, `SyntaxSlot`,
  `NbtPathSegment::Index(Operand)`, `extract_crossings`, wired bridge, recipe collapse, bugs.
- **Tier B — first-class references** (≈ Stage 11/12 boundary): `Reference` substrate,
  `DataRef<T>` source type, refs∘macros path composition, invalidation/alias/effect analysis,
  cost-directed encoding selection.
- **Tier C — total generalization** (≈ Stage 12): `Operand<T>` across *all* `CommandKind` slots,
  typed `CommandTemplate` raw-command interpolation ([`stage-6-effects-plan.md`](stage-6-effects-plan.md)),
  comptime language macros, dynamic/recursive frame allocation, profile-guided selection.

## Further reading

This note is the conceptual overview. Three later notes work out full, implementation-grade
specifications for the pieces sketched here:

- [`macro-system-specification.md`](macro-system-specification.md) — the complete `SyntaxSlot`
  taxonomy across every `CommandKind`/`ExecuteModifierKind`, the macro ABI's exact frame/naming
  contract, and the placement/cost model worked through a concrete scenario.
- [`references-design.md`](references-design.md) — the full Tier B reference design: `DataRef<T>`
  syntax, invalidation rules per target kind, alias/effect analysis requirements, and the
  `mut`/`ref` relationship.
- [`nbt-schema-system.md`](nbt-schema-system.md) — the general compiler-known-dictionary
  mechanism that gives entity-NBT paths (and any future schema-typed access) their typing,
  designed so a new field is a data row, never a code change.
- [`entity-nbt-path-composability.md`](entity-nbt-path-composability.md) — the first concrete
  client of all three: replacing the hardcoded written-book-page intrinsic with a general,
  composable path expression, including two confirmed bugs found in the shipped PS-11A–C engine.

## 11. Open questions (need server measurement, not decision)

- 26.2 macro cache capacity and exact replacement/escaping rules (source inspection / server).
- Whether forked execution contexts can share one static scratch frame for a given lowering.
- Break-even points: macro reparse vs. dispatch tree vs. layout change, by index distribution.
- Frame liveness across nested/recursive macro calls (static frame vs. stack/list frame).
