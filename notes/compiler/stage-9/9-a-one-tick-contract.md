# Stage 9A — One-Tick Bound Contract

Status: **planned; not started**

## Problem and non-goals

Stage 9's mode 2 (9B, recurring scheduling) and mode 3 (9C, persistent
continuations) both need a checkable fact: "does this function's command
sequence/fork expansion fit inside one tick under the configured server limits?"
9B needs it unconditionally for every scheduled/tick-tag entry (a function that
runs every tick has no cut site to spread work across — it must fit or the pack is
broken). 9A builds the standalone piece of that first: an **opt-in** contract a
user can write on an ordinary exported function today, with no scheduling
machinery at all, that promotes the existing (already-computed, already-reported)
target-cost analysis from an inspectable fact into a hard compile error when
violated.

9A deliberately does **not**:

- add `schedule`/`Ticks`/tick-tag source syntax, or any HIR/Core representation for
  a deferred entry — 9B;
- change what gets *emitted* for a checked function — the contract is purely a
  gate on an already-independent analysis pass; the generated commands for a
  passing function must be byte-identical whether or not it carries the marker;
- make the contract mandatory for any existing function — every program that
  compiles successfully today (including PS-3's Brainfuck capstone, whose three
  public roots are `NoFiniteBoundProven(PositiveCycle)` by design — see
  [`../pre-scheduler/ps-3-handoff.md`](../pre-scheduler/ps-3-handoff.md)) must keep
  compiling successfully, unqualified, after 9A lands;
- solve "prove an arbitrary internal (non-exported) function's standalone cost" —
  see the scope decision below;
- touch `AmbientContextRequirements`/domain 4 (self-rooting) at all — that is 9B's
  concern for scheduled entries specifically; 9A checks *only* the sequence/fork
  bound.

## Current repository boundary

Verified 2026-07-23 against current source; re-check before building on any line.

### Correction to the plan's own wording

[`../stage-9-plan.md`](../stage-9-plan.md) and
[`../stage-9-todo.md`](../stage-9-todo.md) both cite `analysis/minecraft/`'s
`ForkBound` as one of the types 9A wires into. **This type does not exist in that
module.** `ForkBound` (`None | Finite(NonZeroU64) | NoFiniteUpperBound | Unknown`)
is defined once, in `crates/mdl-compiler/src/ir/semantic/behavior.rs:300` — a
**semantic-level**, pre-lowering, per-function conservative summary (see 9.0's own
dossier for its exact shape). `analysis/minecraft/`'s actual **target-level**,
post-lowering fork fact is `RootExecutionSummary::fork_limit_status() ->
CommandLimitStatus` (`cost.rs:862`, `:939` area), derived from a `CountBound` over
`maximum_chain_expansion`/`execute_stages` (`cost.rs:916`,
`CommandLimitStatus::compare_forks`, `cost.rs:845`). 9A uses the target-level fact,
not the semantic one — the plan's citation is stale and this dossier corrects it
per the project's own re-verification discipline.

### The bound-analysis primitive 9A promotes

Everything here was independently verified during 9.0's own codebase pass
(9.0's dossier, "Bound/cost analysis" section) and re-confirmed now with the
specific call graph 9A hooks into:

- `CommandLimitStatus` (`cost.rs:812`): `ProvenWithin | MayExceed | ProvenExceeds |
  NoFiniteBoundProven(NoFiniteBoundReason) | Unknown(UnknownCostReason)`. 9A's
  contract is satisfied only by `ProvenWithin`.
- `RootExecutionSummary` (`cost.rs:862`) carries `sequence_limit_status()` and
  `fork_limit_status()` **per root**, already computed by
  `analyze_verified_target_execution` (`analyze.rs:41`) against whichever
  `CommandLimitAssumptions` (configured `max_command_sequence_length`/
  `max_command_forks`) the compilation used.
- `TargetExecutionRoot` (`cost.rs:777`) is `Function(McFunctionId) |
  FunctionTag(FunctionTagId)` — analysis is generic over root *kind*, but **which
  functions become roots at all is decided by the caller**, not by the analysis
  itself.

### Where roots actually come from today — the load-bearing discovery for scope

`LoweringOutput::analyze_target_execution` (`lower/minecraft/api.rs:578-634`, the
convenience method `frontend/compile.rs:945` actually calls) builds its `roots`
list as: **every function whose `CoreFunctionLinkage` is `DatapackExport`, plus the
generated `#minecraft:load` tag** (`api.rs:595-631`). Nothing else is analyzed as
an independent root today. This is the fact that decides 9A's scope (below): a
`one_tick`-marked function's bound can only be *checked as an independent unit* if
it is already, or becomes, a root in this list.

### `Function`/`CoreFunctionLinkage` — the natural plumbing site

`ir::core::Function` (`ir/core/mod.rs:696`) has `linkage: CoreFunctionLinkage`
(`Internal | DatapackExport`, `:127-132`) as a plain field alongside
`parameters`/`results`/`origin`. A `one_tick_contract: bool` (plus enough origin
information for diagnostics — likely reusing `origin: OriginId` already on the
struct, or a dedicated span if the marker's own source location must be
distinguished from the function declaration's) is the same category of fact,
threaded the same way. This is **not** a new IR carrier in the sense the plan's
"no new IR carrier" rule means (a new Core instruction/opcode or target IR node) —
it is a semantic side-fact on a function declaration, exactly like `linkage`
already is.

### Source syntax — currently no attribute/modifier system exists at all

Checked directly, not assumed: `AstFunction` (`frontend/ast.rs:122-131`) has
exactly one modifier slot, `visibility: AstFunctionVisibility` (`Private | Public |
Export`, `:135-139`), populated by `parse_function` (`frontend/parser.rs:412-417`)
via a single-token dispatch (`KeywordPub` / `KeywordExport` / else `Private`).
`KeywordUnsafe` exists in the lexer (`lexer.rs:526`) but is **not** a function
modifier — it introduces `unsafe { ... }` *statement* blocks
(`parser.rs:924`, `parse_unsafe_minecraft_statement`). There is no `#[...]`,
`@...`, or any other attribute grammar anywhere in the frontend. 9A is the first
feature to need a function-level modifier beyond visibility, and must add real
(small) grammar, not slot into an existing mechanism.

The single-token dispatch at `parser.rs:412-417` also has **eight other call
sites** across the parser that enumerate the function-start token set for
lookahead/recovery purposes (`parser.rs:195, 1895, 1910-1913, 2223-2224,
2311-2312, 2326-2327, 2372-2375, 2392-2395, 2416-2419`) — every one of these needs
the new keyword added to its lookahead set, or a function opening with the new
modifier will silently fail to be recognized as the start of a declaration in some
contexts (e.g. module-item parsing, error recovery sync points). This is
mechanical but not optional, and is exactly the kind of thing a grep-only pass
would miss if the new keyword were added only to the primary `parse_function`
dispatch.

### Diagnostic and compile-pipeline shape

`Diagnostic::new(code: &'static str, message, origin)` (`diagnostic.rs:56`) is the
uniform constructor; existing codes use a dotted namespace by owning subsystem
(`lower.unbounded-command-forks`, `lower.recursive-call-abi` —
`lower/minecraft/audit.rs`; `target-cost.root-capacity-overflow` —
`lower/minecraft/api.rs:585`). **`lower/minecraft/audit.rs`'s `audit_legality` is a
different, earlier check** — it runs during Core-to-target lowering *legality*
(pre-construction, semantic-level `ForkBound`/run-scope prefix analysis) and is not
the mechanism 9A extends; 9A's diagnostic fires later, after the physical
`MinecraftProgram` and its `TargetExecutionCostReport` already exist.

`compile_package` (`frontend/compile.rs:900-969`) currently treats
`target_analysis` (line 945) as **purely advisory** — `lowering.analyze_target_execution(...)`'s
`Result` is stored directly on `CompilationOutput` and never inspected to decide
whether `compile_package` itself returns `Err`. This is the exact fact that lets
PS-3's `NoFiniteBoundProven` roots compile successfully today, and 9A must not
change that for unmarked functions. Every existing `CompilationFailure` variant
(`:541` onward) follows the same shape: accumulated context from every prior
successful phase, plus a phase-specific failure payload (e.g. `DatapackEmission {
sources, checked_frontend, source_to_core, core_optimization, lowering,
diagnostics }`, the closest structural precedent since it also fires after
`lowering` succeeds).

`LoweringOptions` (`lower/minecraft/mod.rs:66-95`) already exposes
`with_command_limit_assumptions(CommandLimitAssumptions)` (`mod.rs:134`), and
`CommandLimitAssumptions::new(max_command_sequence_length, max_command_forks)`
(`analysis/minecraft/cost.rs:216`) is a public, checked constructor. This is the
existing surface the todo's "prove a lower configured limit flips a passing
function to rejected" gate needs — it is configuration a test can already set, not
something 9A must invent.

## Research applied

No new research threads — 9A is architecture-internal plumbing work, not a design
question the Esterel/async/Fiber/etc. synthesis in
[`../stage-9-plan.md`](../stage-9-plan.md) speaks to. The relevant precedent is
internal: Stage 8's own rule ("never remove a compiler rejection until a complete
verified replacement exists") runs in reverse here — 9A *adds* a rejection, and the
matching discipline is "never make an existing passing program start failing
unless it opts in," which is why the scope and default-off design below are load-
bearing, not incidental.

## Proposed inputs, outputs, and algorithms

### Scope decision: `one_tick` requires `export`

The marker is accepted only on a function whose visibility is already `Export`.
Rationale, stated plainly because it is a real fork, not a forced conclusion:

- Today, only `DatapackExport`-linked functions (plus the compiler-generated load
  tag) are analyzed as independent roots at all (see above). Checking a
  `one_tick`-marked *internal* function would require inventing a way to add an
  ad-hoc extra root for a non-export function — genuinely new plumbing the todo's
  "no new IR carrier" instruction argues against taking on now.
- 9B's scheduled/tick-tag entries will need their *own* root registration
  mechanism regardless (parallel to how `#minecraft:load`'s tag is registered in
  `lower/minecraft/construct.rs:77-98` — the direct parallel `#minecraft:tick`
  registration 9B adds). When 9B lands, it can and should reuse 9A's underlying
  **check primitive** (function → root → `RootExecutionSummary` →
  `ProvenWithin`-or-error) by registering its own roots and calling the same
  helper — it does not need 9A's *source-syntax* plumbing to do that. This
  dossier's "unresolved questions" section says this explicitly rather than
  presupposing 9B's design.
- A `one_tick` marker on a `Private`/`Public`-but-not-exported function is
  therefore a **hard, early, semantic-phase rejection** (fails during the existing
  `SemanticChecking` frontend phase, well before Core generation, target lowering,
  or target analysis run at all) — cheap, fast, and explains the requirement
  directly rather than surfacing a confusing absence deep in target analysis.

### Source syntax

A new keyword modifier, accepted **after** visibility and **before** `fn`:

```mdl
export one_tick fn tick_handler() {
    // ...
}
```

Concretely:

- New token `TokenKind::KeywordOneTick` (lexer keyword table entry, `"one_tick" =>
  TokenKind::KeywordOneTick`, alongside the existing table at `lexer.rs:520-543`).
- `AstFunction` gains `one_tick: bool` and `one_tick_span: Option<Span>` (mirroring
  `visibility`/`visibility_span`'s existing shape at `ast.rs:123-124`).
- `parse_function` (`parser.rs:412`) becomes a small fixed-order modifier
  accumulation (visibility token, then optional `KeywordOneTick`) rather than a
  single match — a mechanical, bounded change, not a general modifier-list parser.
  Canonical order is fixed (`export one_tick fn`, not the reverse) to keep the
  grammar closed-form rather than permissively reorderable, matching this
  project's general preference elsewhere (e.g. fixed dossier section order, fixed
  diagnostic precedence order in `cost.rs`'s own doc comments).
- Every one of the eight other function-start lookahead sites listed above gets
  `TokenKind::KeywordOneTick` added alongside `KeywordExport`/`KeywordPub` — this
  is the mechanical-but-mandatory part flagged in the repository-boundary section.
- `one_tick` on a non-`Export` function is rejected in the existing semantic-
  checking phase with a new diagnostic (proposed code:
  `frontend.one-tick-requires-export`), naming the function and pointing at both
  the `one_tick` token's span and the (missing-`export`) visibility.

The exact keyword spelling (`one_tick`) is a naming proposal, not a load-bearing
decision — see "Unresolved questions."

### Core/lowering plumbing

- `ir::core::Function` gains `pub(crate) one_tick_contract: bool` beside
  `linkage`. Set from the checked HIR function record during Core generation
  (`lower_hir`), the same phase that already sets `linkage` from
  `AstFunctionVisibility`.
- No change to `CoreFunctionLinkage` itself, no new Core instruction, no new
  target IR node, and — this is the falsifiable claim 9A's own tests must prove —
  **no change to the emitted commands** for the checked function itself, marked
  or not. The field is read exactly once, late, by the new check pass described
  below; it participates in no lowering decision.

### The check pass

A new function, e.g. `check_one_tick_contracts(program: &CoreProgram,
lowering_map: &LoweringMap, report: &TargetExecutionCostReport) ->
Diagnostics`, called from `compile_package` immediately after
`lowering.analyze_target_execution(...)` returns `Ok(report)` (`compile.rs:945`),
before `emit_datapack` is attempted (saving that work on a doomed compilation):

1. Collect every Core function with `one_tick_contract == true` (already export-
   only by construction, enforced earlier in semantic checking — this pass does
   not need to re-check that invariant, only rely on it).
2. For each, resolve its `McFunctionId` via the same `lowering_map`
   function-resource lookup `LoweringOutput::analyze_target_execution` already
   performs internally (`api.rs:590-611`), then find its
   `ResolvedTargetExecutionRoot::Function(id)` entry in `report.roots()`
   (`report.rs:429`).
3. Read that root's `RootExecutionSummary::sequence_limit_status()` and
   `fork_limit_status()`. Both must be `CommandLimitStatus::ProvenWithin`.
4. For any root failing either check, emit one `Diagnostic` (proposed code:
   `target-cost.one-tick-not-proven`) naming: the function's source name, which
   limit failed (sequence vs. fork), the exact `CommandLimitStatus` variant
   observed (interpolating `{status:?}` directly rather than re-deriving a
   friendlier-but-lossier summary — `ProvenExceeds`, `MayExceed`,
   `NoFiniteBoundProven(reason)`, or `Unknown(reason)` all carry distinct,
   already-meaningful information worth preserving verbatim), and the exact
   configured `CommandLimitAssumptions` value it was checked against.
5. If `analyze_target_execution` itself returned `Err(TargetExecutionAnalysisFailure)`
   and at least one `one_tick`-marked function exists in the program, that is
   *also* a hard failure under 9A (the contract cannot be proven if the analysis
   that would prove it did not complete) — surfaced as a distinct diagnostic
   (proposed code: `target-cost.one-tick-analysis-incomplete`) rather than silently
   passing. If no `one_tick`-marked function exists, an analysis failure remains
   exactly as advisory as it is today — 9A must not change that path.

A non-empty diagnostic set becomes a new `CompilationFailure::TargetContract {
sources, checked_frontend, source_to_core, core_optimization, lowering,
diagnostics }` variant (mirroring `DatapackEmission`'s shape, the closest existing
precedent since it also fires after `lowering` succeeds), returned instead of
proceeding to `emit_datapack`. An empty diagnostic set changes nothing — `compile_package`
proceeds exactly as today.

### Interaction with the four optimization policies

`RootExecutionSummary`'s bounds are computed against the *actual* lowered target
program for whichever `CoreOptimizationOptions`/`MinecraftOptimizationLevel` this
specific compilation used — optimization can shrink a command sequence enough to
flip `MayExceed`/`NoFiniteBoundProven` into `ProvenWithin` (or, more likely in
practice, prove a tighter bound). **The contract is therefore evaluated per
compilation, not required to hold identically across all four policies.** A
function may legitimately pass `one_tick` under `Baseline|Baseline` and fail under
`None|None` — that is real, useful signal (the unoptimized build genuinely might
not fit), not a bug to paper over. This must be stated explicitly because the
project's fixture harness auto-runs all four policy combinations and expects them
to agree on most properties; 9A is a case where policy-*dependent* acceptance is
the intended, correct behavior, and the test plan below must prove that
deliberately rather than let an accidental "policies disagree" failure look like a
harness bug.

## Invariants and failure behavior

- **No regression for unmarked functions.** Every program that compiles
  successfully today (in particular PS-3's Brainfuck capstone and its
  `NoFiniteBoundProven(PositiveCycle)` public roots) must keep compiling
  successfully, byte-for-byte identically, after 9A lands. This is the single most
  important invariant and needs its own regression fixture, not just new-feature
  tests.
- **No emission change for a passing marked function.** A `one_tick`-marked,
  provably-within-budget function must emit identical commands to the same
  function unmarked. Proven by a source-fixture pair (marked vs. unmarked,
  otherwise identical body) with an exact target-output diff assertion.
- **`one_tick` without `export` fails early and cheaply.** Semantic-phase
  rejection, not a confusing absence three phases later.
- **The contract is evaluated, not assumed, per compilation.** No caching or
  cross-invocation memoization of a "passed before" result — a changed
  `CommandLimitAssumptions` or a changed optimization policy must be able to flip
  the verdict on an unchanged source function, and the test plan proves this in
  both directions (todo's explicit gate).
- **Diagnostic completeness, not first-failure-only.** If multiple `one_tick`-
  marked functions in one program fail, all of them are reported in one
  `CompilationFailure::TargetContract`, not just the first — matching the existing
  `Diagnostics` (plural) convention used everywhere else in this pipeline.

## Diagnostics and public inspection

New diagnostic codes (proposed, namespaced to match existing convention):

- `frontend.one-tick-requires-export` — semantic-phase; `one_tick` on a
  non-exported function.
- `target-cost.one-tick-not-proven` — post-lowering; a marked root's sequence or
  fork bound is not `ProvenWithin`. Message includes the exact
  `CommandLimitStatus` variant and the configured assumption checked against.
- `target-cost.one-tick-analysis-incomplete` — post-lowering; target-execution
  analysis itself failed while at least one `one_tick` marker was present.

No new public inspection surface is required beyond what already exists:
`TargetExecutionCostReport::dump()` (`report.rs:435`) already renders every root's
`sequence_limit_status`/`fork_limit_status` in the existing target-cost report
text — a `one_tick`-marked function's proof reasoning is already inspectable
before 9A; 9A only adds the *enforcement*, not new inspectability. `mdl explain`-
shaped tooling (referenced conceptually in
[`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md) §8
for crossing decisions) is out of scope for 9A specifically.

## Tests, scale/corruption work, and gate

Fast-suite source fixtures (`crates/mdl-compiler/tests/source-fixtures/`, the
`// MDL: success` / `// MDL: failure <boundary> <code>` convention from
[`../testing-harness.md`](../testing-harness.md)):

- **Accept**: `export one_tick fn` with a statically bounded loop (fixed iteration
  count) — compiles successfully under default `CommandLimitAssumptions`, and
  emits identical commands to the same function without the `one_tick` marker
  (exact target diff, both directions of the pair).
- **Reject — unbounded**: `export one_tick fn` with a runtime-data-dependent loop
  (no static CFG bound, `NoFiniteBoundProven(PositiveCycle)`-shaped) — fails with
  `target-cost.one-tick-not-proven`, message names the function and the observed
  `NoFiniteBoundProven` reason.
- **Reject — export-less**: `one_tick fn` (no `export`) — fails with
  `frontend.one-tick-requires-export` during semantic checking, before Core
  generation.
- **Regression**: an existing `NoFiniteBoundProven`-shaped exported function
  *without* the marker still compiles successfully (guards the "no regression for
  unmarked functions" invariant directly, not just by absence of a failing test).
- **Multiple violations**: two independently `one_tick`-marked, independently
  failing functions in one program both appear in the single
  `CompilationFailure::TargetContract`'s diagnostics.

Dedicated Rust integration test (not fixture-shaped, since fixtures run under
fixed default limits) for the todo's explicit gate:

- **Two configured limit settings**: a single fixed source function whose command
  sequence is close to, but under, the *default* `max_command_sequence_length`.
  Compile once with `LoweringOptions::with_command_limit_assumptions` left at the
  target default (passes, `ProvenWithin`) and once with it lowered via
  `CommandLimitAssumptions::new(lower_value, ...)` below that function's actual
  sequence count (fails, `ProvenExceeds` or `MayExceed`) — proving the contract is
  genuinely limit-sensitive, not a static pass/fail baked into the fixture.

Four-policy-specific test (separate from the standard fixture auto-run, since this
one's point is that policies are allowed to *disagree*):

- A function whose command sequence is `ProvenWithin` under `Baseline|Baseline`
  but not under `None|None` (constructed deliberately, likely via a moderately
  large fixed-iteration loop that optimization collapses/shrinks) — asserts the
  contract's verdict differs by policy as designed, not as an unnoticed bug.

Scale/corruption work: not applicable — 9A adds no new persistent state format,
only a checking pass over already-verified data.

Gate (from [`../stage-9-todo.md`](../stage-9-todo.md)): fast-suite proof of
accept/reject at two configured limit settings. **Satisfied by** the dedicated
Rust integration test above plus the fixture set; both must be green, along with
the regression and no-emission-change fixtures, before 9A is considered complete.

## Unresolved questions that forbid premature implementation

- **Exact keyword spelling** (`one_tick` proposed) is a naming choice, not
  architecture — should be confirmed (or bikeshedded) before the lexer change
  lands, but does not block writing the rest of the plumbing.
- **Whether 9B's scheduled/tick-tag functions reuse `one_tick`'s source syntax**
  (e.g. implicitly requiring/implying it) **or only its underlying check
  primitive** is explicitly left to 9B's own dossier. This dossier's design
  (export-only scope, root list decided by the caller) is compatible with either
  answer, but does not presuppose one — 9B should read this section before
  assuming either shape.
- **Whether `pub` (non-export) functions should ever be eligible** once/if a
  future stage gives them independent-root status is out of scope; the current
  `export`-only restriction is a scope decision tied to *today's* root-
  construction logic, not a permanent language rule.
- **Diagnostic code names** (`frontend.one-tick-requires-export`,
  `target-cost.one-tick-not-proven`, `target-cost.one-tick-analysis-incomplete`)
  are proposals matching existing dotted-namespace convention; final names should
  be confirmed against whatever the implementing session finds cleanest once the
  actual diagnostic call sites are written, same as any other code review.
- **Whether rotation/anchor context ever matters to 9A** — it does not; domain 4
  (self-rooting) is untouched by this tranche. Restated here only because the
  plan's six frozen decisions table (9.0's dossier) already flagged dimension
  reconstruction as open, and a reader might wonder whether that blocks 9A. It
  does not: 9A checks only sequence/fork bounds, never ambient context.

## References

Internal:

- [`../stage-9-plan.md`](../stage-9-plan.md) — capability 1's one-paragraph
  description and its (corrected, see above) `ForkBound` citation
- [`../stage-9-todo.md`](../stage-9-todo.md) — 9A's checklist and gate
- [`stage-9/README.md`](README.md) — required dossier shape
- [`9-0-contracts-and-evidence.md`](9-0-contracts-and-evidence.md) — sibling
  tranche; establishes the `analysis/minecraft/` vocabulary this dossier reuses
- [`../pre-scheduler/ps-3-handoff.md`](../pre-scheduler/ps-3-handoff.md) — the
  concrete `NoFiniteBoundProven` roots the no-regression invariant protects
- [`../testing-harness.md`](../testing-harness.md) — source-fixture convention
  9A's fast-suite tests follow

Codebase anchors (verified 2026-07-23; re-check before building on any line):

- `crates/mdl-compiler/src/analysis/minecraft/cost.rs:206,777,812,862` —
  `CommandLimitAssumptions::new`, `TargetExecutionRoot`, `CommandLimitStatus`,
  `RootExecutionSummary`
- `crates/mdl-compiler/src/analysis/minecraft/analyze.rs:28,41` —
  `analyze_target_execution`/`analyze_verified_target_execution`
- `crates/mdl-compiler/src/analysis/minecraft/report.rs:429,435` —
  `TargetExecutionCostReport::roots()`/`dump()`
- `crates/mdl-compiler/src/lower/minecraft/api.rs:578-634` —
  `LoweringOutput::analyze_target_execution`, the current export-plus-load-tag
  root-construction logic 9A's scope decision is built on
- `crates/mdl-compiler/src/lower/minecraft/mod.rs:66-134` — `LoweringOptions`,
  `with_command_limit_assumptions`
- `crates/mdl-compiler/src/lower/minecraft/audit.rs` — the *different*, earlier
  semantic-level legality check (`lower.unbounded-command-forks` etc.); not what
  9A extends
- `crates/mdl-compiler/src/ir/core/mod.rs:127-141,694-703` —
  `CoreFunctionLinkage`, `Function` (the plumbing site for `one_tick_contract`)
- `crates/mdl-compiler/src/frontend/ast.rs:120-139` — `AstFunction`,
  `AstFunctionVisibility` (the plumbing site for `one_tick`/`one_tick_span`)
- `crates/mdl-compiler/src/frontend/lexer.rs:518-543` — keyword table
- `crates/mdl-compiler/src/frontend/parser.rs:412-417` plus the eight lookahead
  sites listed in "Current repository boundary" — `parse_function` and every
  function-start token-set check that needs the new keyword added
- `crates/mdl-compiler/src/frontend/compile.rs:541-969` — `CompilationFailure`,
  `compile_package`, the exact insertion point for the new check between
  `lowering` (line 928) and `emission` (line 946)
- `crates/mdl-compiler/src/diagnostic.rs:44-70` — `Diagnostic::new`
