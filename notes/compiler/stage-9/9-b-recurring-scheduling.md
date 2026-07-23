# Stage 9B — Recurring Scheduling Without Continuation

Status: **designed, not implemented** (2026-07-23). Depends on 9A
([`9-a-one-tick-contract.md`](9-a-one-tick-contract.md), implemented) for the
`ProvenWithin` check primitive it reuses, and on 9.0
([`9-0-contracts-and-evidence.md`](9-0-contracts-and-evidence.md)) for every
measured `schedule`/`#minecraft:tick` fact it lowers to.

## Problem and non-goals

Capability 2 of Stage 9: a source-level "run this function again in N ticks" and
"run this function every tick," where every invocation runs top-to-bottom and
terminates, and all cross-tick state is external (scoreboard/storage the program
writes and reads itself) — the `dynamic-lights` and `spawn-animations` pattern,
confirmed real and load-bearing by both keystone datapacks
([`../../functionality-keystones/datapacks/dynamic-lights/NOTES.md`](../../functionality-keystones/datapacks/dynamic-lights/NOTES.md),
[`.../spawn-animations/NOTES.md`](../../functionality-keystones/datapacks/spawn-animations/NOTES.md)).
9B builds the second of Stage 9's three capabilities: no persistent continuation
state, no resume discriminant, no live-value materialization across a cut — those
are 9C. 9B is "call this again later," not "suspend and resume mid-function."

9B deliberately does **not**:

- add a resume discriminant, liveness-lifted live-state set, or any generalization
  of the macro crossing engine (`lower/minecraft/crossings.rs`) — there is nothing
  to marshal, because a `schedule` statement's every operand (target function,
  delay, mode) is a compile-time literal. Crossings.rs exists to marshal *runtime*
  values across a forced cut; 9B has no forced cut with runtime state, only a
  target-level command with literal arguments. This is a real scope boundary, not
  an oversight: 9C is what needs the crossing engine, not 9B;
- let a scheduled or `#minecraft:tick`-registered function carry arguments, from
  any call site, ever — confirmed impossible in principle, not merely
  undesirable, by the exact mechanism below;
- auto-insert a "schedule self first" reordering into a function's body — that
  lowering pattern is a documented **author-written** idiom (`dynamic-lights`
  does it by hand: its own first statement is its own re-schedule call), not a
  compiler transformation. See "Schedule-self-first is not compiler-inserted"
  below for why silently reordering statements is rejected;
- add per-tick selector work-budgeting (`limit=N,sort=random`, confirmed load-
  bearing by `spawn-animations`) — that is a selector-grammar gap
  (`mc.entities(...)` needs `limit`/`sort` support), independent of Stage 9
  entirely, tracked separately;
- change anything about `one_tick` (9A) — 9B *reuses* 9A's check primitive
  (function → root → `RootExecutionSummary` → `ProvenWithin`) but does not modify
  `frontend/target_contract.rs` or its tests; and
- resolve 9.0's two open sub-questions (dimension reconstruction, player-presence
  selector leakage, `semantic-ambiguities.md` A-028/A-029) — 9B's self-rooting
  check is a static semantic-model fact independent of those runtime
  measurements (see "Self-rooting and the open dimension question" below for
  exactly why this is safe to build now).

## Current repository boundary

Verified 2026-07-23 against current source (post-9A, commit `eb1197b`);
re-check before building on any line.

### What 9A already shipped that 9B reuses directly

- `crates/mdl-compiler/src/frontend/target_contract.rs` —
  `check_one_tick_contracts(core, lowering, analysis)`, the `ProvenWithin`
  enforcement pattern (root lookup via `LoweringMap::function` →
  `MinecraftProgram::functions()` resource match → `report.roots()` →
  `sequence_limit_status()`/`fork_limit_status()`). 9B adds a **new, sibling**
  module rather than editing this one — see "Reusing, not modifying, 9A's check
  primitive" below.
- `crates/mdl-compiler/src/ir/core/mod.rs:694-709` — `Function` already carries
  `linkage: CoreFunctionLinkage` and `one_tick_contract: bool` as plain sibling
  fields, threaded through `declare_function_with_linkage`. 9B adds two more
  fields the same way (below).
- `crates/mdl-compiler/src/lower/minecraft/api.rs:578-634` —
  `LoweringOutput::analyze_target_execution`'s root list is exactly "every
  `DatapackExport` function, plus the load tag." 9B extends this loop (below);
  this is the exact extension point 9A's own dossier flagged as open
  ("9B needs its own root registration mechanism regardless").
- `crates/mdl-compiler/src/lower/minecraft/construct.rs:77-98` — the
  `#minecraft:load` tag registration (`FunctionTagResourceId::parse` +
  `FunctionTagMerge::Append` + `declare_function_tag`/`begin_function_tag`/
  `FunctionTagEntry::internal`/`tag.finish()`). 9B generalizes this to
  `#minecraft:tick` with N entries instead of exactly one (below).
- `crates/mdl-compiler/src/lower/minecraft/api.rs:244-250,319-334` —
  `LoweredFunction` (`linkage`, `entry_resource`, `generated_entry_requirement`,
  register homes), constructed once per Core function during Minecraft
  lowering. 9B threads two more booleans onto it, mirroring `linkage`.
- `crates/mdl-compiler/src/frontend/compile.rs:1692` — the only existing reader
  of `generated_entry_requirement()` today is a **test assertion**; nothing in
  the compiler pipeline currently enforces self-rooting on anything. 9B's
  self-rooting check (below) is the first real consumer.

### The load-bearing discovery: `function_references()` feeds both reachability and the recursion/call graph, undifferentiated

This is the fact 9B's Core representation is built around, and it was not
obvious going in.

`CoreOp::function_references()` (`ir/core/mod.rs:623-679`) is the **single**
query that reports non-SSA function references out of an operation. It is
consumed in exactly one place that matters here:
`FunctionSemanticInventory::new` (`lower/minecraft/analysis.rs:277-288`) walks
every instruction's `function_references()` and pushes each into
`call_sites: Vec<CallSite>`. `CallGraph::new`
(`lower/minecraft/audit.rs:154-182`) then builds its forward/reverse adjacency
from `emitted.call_sites()` **unconditionally — with no filtering by
`FunctionReferenceKind`.** `ActivationOverlapAnalysis::analyze`
(`audit.rs:56-131`) runs strongly-connected-components over that graph and
marks every SCC-recursive function/call-site, which is what drives Stage 8's
activation-frame allocation (`8-d-recursive-frames.md`).

`FunctionReferenceKind` (`ir/core/mod.rs:279-289`) already has three variants —
`DirectCall`, `RunScopeModifier`, `RunScopeBody` — and **all three** are
legitimately treated as call-graph edges today, because all three really are
synchronous invocations. There is no existing precedent for a function
reference that must be visible to *something* but invisible to the call graph.

Separately: whole-program Core function **emission** does not depend on
inter-function reachability at all. `plan/assemble.rs:195`,
`for (function, declaration) in core.functions()`, plans every declared Core
function unconditionally — there is no dead-function tree-shaking pass in this
compiler today. So a function referenced only by a `schedule` statement is
never at risk of being pruned; nothing needs to make it "reachable" for
emission purposes.

Putting these together: **`CoreOp::Schedule`/`CoreOp::ScheduleClear` must not
implement `function_references()` at all** — they return no reference, exactly
like `BoolConstant`/`I32Constant`/every other non-`Call` operation already
does. This is not a special-case filter bolted onto `CallGraph::new` (which
would be exactly the kind of thing that bit-rots the next time someone adds a
`FunctionReferenceKind` variant and forgets to re-check the filter); it is the
absence of a reference at the one place references are minted, which the call
graph cannot see. "Self-reschedule is not recursion" is therefore true **by
construction**, and costs nothing on the reachability side because reachability
was never a real constraint here. This falsifiable claim ("a function
self-scheduling its own re-entry gets no activation frame, and this holds
without any change to `audit.rs`") is exactly what the test plan below proves
directly.

### `HirStatementKind`, `HirCall`, and where `Schedule`/`ScheduleClear` sit

`HirStatementKind` (`frontend/hir.rs:753-775`) already lists `Call(HirCall)` as
one of eleven statement kinds; `HirCall { callee: SourceFunctionId, arguments:
Box<[HirExpression]>, origin }` (`hir.rs:953-957`). `Schedule`/`ScheduleClear`
are proposed as two more sibling `HirStatementKind` variants — not a payload
carried on `HirCall` — because their checked semantics genuinely differ (no
arguments ever, a literal delay, a literal mode, no return value, and — per the
discovery above — a different Core lowering that must not touch
`function_references()`).

### Why the `on` event-handler mechanism (PS-15) was considered and rejected as 9B's registration surface

`AstEventHandler` (`frontend/ast.rs:325-333`, `on <trigger>(...) |binding| {
...}`) is the closest existing "declare a function the compiler auto-registers
as an entry point" precedent, and was checked directly:
`check_event_handler`/`check_event_handler_criterion`
(`frontend/check.rs:1485-1554`) compile every handler to a hidden advancement
with a criterion and an auto-revoke reward, and unconditionally establish a
`Player` executor/position/rotation/dimension context
(`check.rs:1516-1521`) for the handler body. This is the structural *opposite*
of what a tick handler needs (self-rooting, no executor at all, no criterion,
no revoke, fires unconditionally forever). Reusing `on` for `tick` would mean
either forcing a fake advancement/criterion for something that isn't an
advancement event, or forking the mechanism enough that nothing is actually
shared. 9B does not reuse `on`; see "Source syntax" below for the mechanism
actually proposed (a function modifier, parallel to 9A's `one_tick`).

### `CommandKind` and why 9B needs no crossings.rs involvement

`CommandKind` (`ir/minecraft/command.rs:89-114`) already has 10 variants, none
of which is a scheduling command. The closest structural precedent for a new,
minimal, no-runtime-operand command is `AdvancementRevokeCommand`
(`ir/minecraft/advancement.rs:10-19`, wired at
`construct.rs:163`): a command that names one resource and carries no runtime
data at all — exactly the shape `ScheduleCommand`/`ScheduleClearCommand` need,
since a `schedule` statement's target/delay/mode are all compile-time literals
by the frozen decision (9.0: "no indirection form exists for schedule's time
argument," confirmed by grammar). This is why 9B never touches
`crossings.rs`'s `COLLECT → FRAME → BRIDGE → RENDER → CALL` pipeline: there are
no runtime operands to collect or bridge.

## Research applied

No new external research threads beyond what `stage-9-plan.md` already
synthesized (Esterel/async/Fiber/etc.) — 9B is the plan's own capability 2,
already scoped there as architecture-internal plumbing plus one real design
fork (the `function_references()` discovery above, which the plan did not
anticipate at this level of detail). The two keystone datapacks
(`dynamic-lights`, `spawn-animations`) are 9B's primary applied evidence: they
are read, not hypothesized, real shipped packs confirmed to run on the pinned
26.2 target, and 9B's fixture/gate plan below is built to reproduce both
shapes exactly.

## Proposed inputs, outputs, and algorithms

### Source syntax

**Tick-tag registration** — a new function modifier, `tick`, in the same
fixed-order modifier slot 9A's `one_tick` already occupies:

```mdl
tick fn light_tick() {
    // runs every tick; registered into #minecraft:tick
}
```

Unlike `one_tick`, **`tick` does not require `export`** — this is a deliberate
divergence, justified by the same repository fact that motivated `one_tick`'s
restriction in the other direction: `one_tick` needed `export` because export
was the *only* existing way for a function to become an independent root at
all. A `tick`-marked function creates its **own** root (the `#minecraft:tick`
tag, see "Root registration" below), so it never needed export's root-granting
property in the first place. `Private` is expected to be the common case for a
tick handler (it is invoked only by the game engine via the tag, never by name
from other MDL code). `tick` and `one_tick` together on the same function are
proposed as a **rejected combination** (new diagnostic, "Diagnostics" below):
a tick handler's contract is checked through the tag's *aggregate* root (all
handlers' combined per-invocation cost, matching how the tag actually fires —
see "Root registration"), not the function's own independent root, so writing
both would assert two different, easily-confused things about the same
function.

**Scheduling** — two new statements, closely tracking vanilla's own
`schedule function <id> <time> [append|replace]` / `schedule clear <id>`
naming (the source vocabulary this whole tranche measures against):

```mdl
schedule light_tick, 5;
schedule light_tick, 5, append;
schedule clear light_tick;
```

Concretely:

- New token `TokenKind::KeywordSchedule`; new token `TokenKind::KeywordClear`
  (reserved, not contextual — see "the `clear` grammar-ambiguity fork" below
  for why this one *is* a reserved keyword when `append`/`replace` are not).
- The scheduling target is a **bare function name, not a call expression** —
  `schedule light_tick, 5;`, never `schedule light_tick(), 5;`. This was a
  real fork, considered and rejected the other way: reusing `AstCall` (as
  `one_tick`'s dossier reused it nowhere, but as `AstCallStatement` already
  does elsewhere) would let a user write `schedule some_fn(1, 2), 5;` naming a
  two-parameter function with matching call-site arity — which ordinary
  arity-checking would *accept*, silently defeating the argument-free
  requirement, because arity-checking only ever compares a call site's
  argument count against the callee's declared parameter count, and both
  would agree here. A bare name reference has no argument-list syntax to get
  this wrong in, and makes the visual distinction between "invoking" and
  "naming for later scheduling" explicit rather than implicit. This reuses
  whichever existing sub-path already resolves an `AstCall`'s callee against
  the function namespace when the callee expression is a bare identifier —
  the same resolution, minus the argument list.
- Delay is a bare `DecimalInteger` literal (`lexer.rs:199-219`) in a narrow,
  non-general grammar position — the same shape `AstSignedInteger`
  (`ast.rs:227-232`) already uses for switch-statement patterns, not a general
  expression. **No literal suffix (`5t`) is proposed** — see "Unresolved
  questions" for why this is left open rather than settled; the recommendation
  here is grammar-minimal (no new lexer suffix-scanning machinery) since the
  frozen decision already establishes there is no indirection form to
  distinguish a suffixed literal from anyway.
- Mode (`append`/`replace`, optional, defaults to `replace` per the frozen
  decision) is a **plain identifier**, validated semantically against a
  closed vocabulary — directly reusing the exact pattern
  `EventTrigger::from_source_name` already established for event-handler
  trigger names (`check.rs:1491-1502`): not a reserved keyword, checked after
  parsing, with a friendly "not a recognized schedule mode" diagnostic on
  mismatch. A variable or function can still be named `append` or `replace`.

**The `clear` grammar-ambiguity fork.** If `clear` were a plain identifier like
`append`/`replace`, `schedule clear;` and `schedule <name-that-happens-to-be-
spelled-clear>, 5;` would need lookahead past the point where the parser must
already have committed to one shape, and a user naming a function `clear`
would silently change what `schedule clear` means depending on context — a
sharp edge, not a convenience. Reserving `clear` as a real keyword (costing
one name out of the function/variable vocabulary, the same tax `unsafe`/`run`/
`on` already impose) resolves this with zero lookahead: right after
`KeywordSchedule`, `KeywordClear` present-or-absent is the entire dispatch.

### HIR/Core representation

- `HirStatementKind` (`hir.rs:753-775`) gains two variants:
  `Schedule(HirSchedule)`, `ScheduleClear(HirScheduleClear)`.
  `HirSchedule { callee: SourceFunctionId, delay_ticks: u32, mode:
  ScheduleMode, origin }`; `HirScheduleClear { callee: SourceFunctionId,
  origin }`. `ScheduleMode` is a plain two-variant enum (`Append | Replace`),
  the Hir/Core-level fact the frozen decision's "dedicated type" language is
  actually satisfied by (see "Unresolved questions" — the delay itself stays a
  plain `u32`, not a distinct nameable `Ticks` type, for the same
  grammar-minimalism reason as above).
- `ir::core::mod.rs::CoreOp` gains `Schedule(FunctionId, u32, ScheduleMode)` /
  `ScheduleClear(FunctionId)`, siblings of `Call(FunctionId)` at
  `mod.rs:369`. Both implement `function_references()` returning **no**
  reference (the load-bearing discovery above) — this is the one line in the
  whole design that must not be "fixed" later by someone noticing the operand
  looks like a call and adding a reference for symmetry.
- `ir::core::Function` (`mod.rs:694-709`) gains two more sibling booleans,
  threaded through `declare_function_with_linkage` exactly as
  `one_tick_contract` already is:
  - `tick_handler: bool` — set when the source function carries the `tick`
    modifier.
  - `is_schedule_target: bool` — set when **any** `schedule` (arm) statement
    anywhere in the whole package names this function. Computed during HIR-to-
    Core lowering by a first pass over every `schedule` statement in the
    package (collecting the referenced-target `SourceFunctionId` set) before
    the existing per-function declaration pass that already sets `linkage`/
    `one_tick_contract` — the same lowering phase, one extra small pass.
    **Only `schedule` (arm) statements set this, not `schedule clear`
    statements** — a function referenced only by `schedule clear` is never
    actually invoked by *this* program's own schedule calls (it may be
    defensively clearing a schedule armed by an older version of the pack, or
    simply be a no-op-safe clear of something never armed at all), so it needs
    no root registration, no `ProvenWithin` check, and no self-rooting check —
    only ordinary function-name resolution, which it already gets for free.

### Root registration — generalizing `analyze_target_execution`

`LoweringOutput::analyze_target_execution` (`api.rs:578-634`) gains two more
root-construction loops, alongside its existing export loop
(`api.rs:595-612`) and load-tag push (`api.rs:613-631`):

1. For each `LoweredFunction` with `is_schedule_target == true` (mirrored onto
   `LoweredFunction` from Core `Function::is_schedule_target`, the same way
   `linkage` already is), push `TargetExecutionRoot::Function` — parallel to
   the export loop, independent of visibility. Each scheduled target is its
   **own** independent root: when the scheduler fires it, that is a wholly
   separate synchronous run from its own entry point, exactly like an export
   root's invocation.
2. If any `LoweredFunction` has `tick_handler == true`, resolve the generated
   `#minecraft:tick` tag (constructed conditionally — see below) the same way
   the load tag is resolved (`api.rs:620-630`, matching by
   `FunctionTagResourceId::parse("minecraft:tick")`) and push **one**
   `TargetExecutionRoot::FunctionTag` for it — not one root per handler. This
   is not a simplification; it is the correct model of vanilla's own
   semantics: `#minecraft:tick` fires **every** entry function within one
   tick's command budget, all together, so it is the tag's *aggregate*
   sequence/fork cost that must be `ProvenWithin`, not each handler's cost
   independently. A program with two tick handlers, each individually cheap
   but whose sum exceeds the configured limit, must be rejected — and is,
   automatically, because `TargetExecutionRoot::FunctionTag` already walks all
   of a tag's entries as one execution graph (the same mechanism the load tag
   root already exercises today, just previously only ever with one entry).

### `construct.rs` — generalizing the load-tag pattern to N tick handlers

`TargetConstruction::declare` (`construct.rs:60-134`) gains a conditional
block, structurally parallel to the unconditional load-tag block
(`construct.rs:77-98`) but:

- **conditional** on at least one Core function having `tick_handler == true`
  — a program that never uses `tick` gets no `#minecraft:tick` tag at all,
  preserving 9A's own no-regression discipline extended to 9B: a program with
  zero 9B usage must emit byte-identically to how it emits today;
- iterates **every** `tick_handler`-marked function (not exactly one, unlike
  load), in `FunctionId` index order — already the deterministic order the
  frozen decision requires ("multiple `#minecraft:tick` handlers run in
  source/module declaration order," confirmed by measurement M10), since Core
  `FunctionId`s are already assigned in a single deterministic whole-package
  walk;
- pushes one `FunctionTagEntry::internal(...)` per handler into the tag,
  generalizing the load tag's single `tag.push()` call
  (`construct.rs:94-97`) into a loop.

### Lowering the `schedule`/`schedule clear` commands

- New `ir::minecraft::command.rs` variants: `CommandKind::Schedule(ScheduleCommand)`,
  `CommandKind::ScheduleClear(ScheduleClearCommand)`. Shape mirrors
  `AdvancementRevokeCommand` (`advancement.rs:10-19`) — a target resource plus,
  for `Schedule`, the literal delay and mode; no `StoragePath`, no
  `CallableRef`-with-macro-arguments shape, since there is nothing to marshal.
- Target resolution reuses the same `DeclarationMap`/`LoweringMap`
  function-resource lookup an ordinary `Call` already uses to turn its callee
  `FunctionId` into a resource id — no new resolution machinery.
- New `render.rs` arms render `schedule function <resource> <delay>t
  [append|replace]` and `schedule clear <resource>`, following the existing
  per-`CommandKind` arm pattern (e.g. `AdvancementRevoke`'s arm at
  `render.rs:180`).

### Schedule-self-first is not compiler-inserted

The frozen decision's recommended lowering ("re-arm the next invocation before
doing work, so a mid-tick abort does not break the chain") is a **documented
authoring pattern**, confirmed by M17 (a self-rescheduling function surviving
three consecutive command-limit aborts because its own re-schedule call was
its first command) and by `dynamic-lights`'s own hand-written source
(`internal/main.mcfunction`'s literal first line is its own re-schedule call).
9B does not reorder an author's statements to enforce this — silently moving a
`schedule` statement to the top of a function body would be exactly the kind
of surprising compiler-inserted transformation this project avoids elsewhere.
The pattern is instead documentation (and, as a genuinely optional future
addition outside this gate, a non-blocking advisory lint suggesting it when a
function schedules itself anywhere other than its first statement) — left as
the author's responsibility, matching the real pack's own authorship.

### Enforcement: argument-free and self-rooted, at the right pipeline points

Two checks, applied to the union of `tick_handler` functions and
`is_schedule_target` functions (a function can be both, or either):

1. **Argument-free.** The named function's own declared parameter count must
   be exactly zero. This is a **new** check (not free from arity-checking,
   given the bare-name-reference grammar decision above deliberately removes
   the call-site arity signal that would have made it seem free but actually
   wasn't — see the rejected-`AstCall` reasoning). Checked in the existing
   semantic-checking frontend phase, the same phase 9A's `one_tick`-without-
   `export` check already runs in, against whichever function declaration the
   bare name resolves to.
2. **Self-rooted.** `generated_entry_requirement()` (`api.rs:354-355`,
   post-Core-optimization, currently read only by one test assertion —
   `compile.rs:1692`) must equal `AmbientContextRequirements::NONE`. This is
   necessarily a **post-lowering** check — `generated_entry_requirement` does
   not exist until after Core optimization and construction — so it runs at
   the same insertion point 9A's `check_one_tick_contracts` already runs at
   (`compile_package`, after `lowering`/`analyze_target_execution`, before
   `emit_datapack`), not in semantic checking.

### Self-rooting and the open dimension question

9.0 left dimension reconstruction genuinely open (M15 invalidated, not
answered; `semantic-ambiguities.md` A-028). This does **not** block the
self-rooting check above: the check is a static fact about the closed semantic
model (`AmbientContextRequirements::dimension()`'s `ContextRequirement`
lattice value for this function, computed the same way it already is for every
function today — `ir/core/ambient.rs`) — it asks "does this function's body
consume a dimension requirement at all," not "what dimension does the
scheduler actually deliver." A function that touches dimension gets rejected
either way, self-rooted or not; what M15 being open actually blocks is a
*different*, deferred capability — reconstructing a lost dimension context
rather than merely rejecting its absence, which is explicitly 9C's job
(interior-cut context reconstruction), not 9B's. 9B never reconstructs
anything; it only proves NONE or rejects.

### Reusing, not modifying, 9A's check primitive

The `ProvenWithin` check itself (root lookup, `sequence_limit_status()`/
`fork_limit_status()`, message formatting) is structurally near-identical to
`check_one_tick_contracts` (`target_contract.rs:33-127`). The proposal is a
**new, sibling module** (`frontend/schedule_contract.rs`,
`check_schedule_contracts`) rather than editing 9A's shipped, tested pass —
deliberately conservative, at the cost of some duplicated logic. Whether the
implementing session should instead factor out a shared helper both passes
call is left open (below) rather than presupposed, since it is a real
judgment call once the second call site's exact shape exists to compare
against the first, not an architectural fork this dossier needs to settle.

## Invariants and failure behavior

- **No regression for programs that use no 9B feature.** No `#minecraft:tick`
  tag is constructed, no new root is registered, and no new diagnostic can
  fire, for a program with zero `tick`-marked functions and zero `schedule`/
  `schedule clear` statements — byte-identical emission to today, mirroring
  9A's own headline invariant and needing its own regression fixture, not just
  absence of a failing test.
- **`tick`'s aggregate cost, not per-handler cost, is what must fit one
  tick.** Two individually-cheap tick handlers whose combined sequence/fork
  cost exceeds the configured limit are rejected together via the tag's one
  aggregate root — this must be proven by a dedicated two-handler fixture, not
  assumed from the single-handler case.
- **Self-reschedule allocates no Stage 8 activation frame**, provable directly
  (not just by absence of a rejection) by inspecting `ActivationOverlapAnalysis`
  output for a self-scheduling function and asserting
  `function_is_recursive` is `false` for it, alongside the positive control
  (an ordinary recursive function still correctly classified `true`).
- **Argument-free and self-rooted are enforced for every `tick`/schedule-
  target function, regardless of whether it is also independently reachable
  by an ordinary call** — the check walks the flag, not the call graph.
- **`schedule clear`-only references impose no contract.** A function named
  only by `schedule clear` (never by any `schedule` arm statement in the same
  program) is not required to be argument-free, self-rooted, or independently
  `ProvenWithin` — only to exist and resolve as an ordinary function name.
- **`tick` and `one_tick` together is a rejected combination**, not silently
  one-or-the-other.
- **The default schedule mode is `replace`**, and MDL's own generated
  `schedule` commands use `replace` unconditionally when the author omits an
  explicit mode — per the frozen decision and M5/M6/M12.

## Diagnostics and public inspection

New diagnostic codes (proposed, namespaced to match existing convention —
final names confirmed at implementation time, same as every prior tranche):

- `frontend.check.tick-and-one-tick-conflict` — a function marked both `tick`
  and `one_tick`.
- `frontend.check.schedule-target-not-argument-free` — a `tick`-marked
  function, or a function named by any `schedule` (arm) statement, declares a
  nonzero parameter count.
- `frontend.check.unknown-schedule-mode` — a trailing schedule-statement
  identifier is not `append` or `replace` (mirrors `UNKNOWN_EVENT_TRIGGER`'s
  existing shape at `check.rs:1494-1501`).
- `frontend.check.schedule-target-unresolved` — a `schedule`/`schedule clear`
  bare name does not resolve to any declared function (reuses whichever
  diagnostic already fires for an ordinary unresolved call-target name, via
  the same resolution sub-path the bare-name reference reuses; not a new
  mechanism, name to be confirmed against that existing diagnostic at
  implementation time).
- `target-cost.tick-tag-not-proven` / `target-cost.schedule-target-not-proven`
  — post-lowering; a `#minecraft:tick`-tag aggregate root, or an independent
  scheduled-target root, is not `ProvenWithin`. Message shape mirrors
  `target-cost.one-tick-not-proven` exactly (exact `CommandLimitStatus`
  variant, configured assumptions).
- `target-cost.schedule-analysis-incomplete` — mirrors
  `target-cost.one-tick-analysis-incomplete`: target-execution analysis itself
  failed while at least one `tick_handler`/`is_schedule_target` function
  exists.
- `lower.schedule-entry-not-self-rooted` — post-lowering; a `tick_handler`/
  `is_schedule_target` function's `generated_entry_requirement()` is not
  `AmbientContextRequirements::NONE`. Message names which component
  (executor/position/rotation/dimension/anchor) is non-`None`.

No new public inspection surface beyond what 9A already established:
`TargetExecutionCostReport::dump()` already renders every root's status,
including the new `FunctionTag`-kind aggregate root for `#minecraft:tick` once
one exists (the report format is already generic over root kind — nothing new
to build here, only new roots to feed it).

## Tests, scale/corruption work, and gate

Fast-suite source fixtures (`crates/mdl-compiler/tests/source-fixtures/stage9/`,
proposed `9b_*` naming alongside the existing `9a_*` fixtures):

- **Accept — tick handler**: a self-rooted, zero-arg `tick fn`, straight-line
  body, registered into `#minecraft:tick`.
- **Accept — dynamic-lights shape**: a self-rescheduling function (its own
  `schedule` statement targeting itself, authored schedule-self-first as its
  own first statement) with no tick tag at all — reproducing
  `dynamic-lights`'s exact structure (pure `schedule`, no `#minecraft:tick`).
- **Accept — spawn-animations shape**: one `tick fn` main handler *and* a
  separately self-rescheduling slower watchdog function in the same program —
  reproducing `spawn-animations`'s exact structure (both mechanisms present
  together).
- **Reject — tick/one_tick conflict**: `tick one_tick fn` (or either order) —
  `frontend.check.tick-and-one-tick-conflict`.
- **Reject — non-argument-free tick handler**: `tick fn f(x: Int32) {}` —
  `frontend.check.schedule-target-not-argument-free`.
- **Reject — non-argument-free schedule target**: `schedule f, 5;` where `f`
  declares a parameter — same code, at the `schedule`-statement call site.
- **Reject — unknown mode**: `schedule f, 5, backwards;` —
  `frontend.check.unknown-schedule-mode`.
- **Reject — not self-rooted**: a `tick fn` whose body establishes an executor
  capture (e.g. via a `run as ...` block) — `lower.schedule-entry-not-self-
  rooted`, naming the executor component.
- **Regression**: an ordinary program using neither `tick` nor `schedule`
  compiles identically before/after 9B, byte-for-byte (the no-regression
  invariant, proven directly, not by absence of a failing test).
- **`schedule clear`-only target**: a function referenced only by
  `schedule clear`, never by any `schedule` arm — compiles successfully with
  no contract enforced on it (proves the asymmetry documented above).

Dedicated Rust integration tests (`crates/mdl-compiler/tests/
stage9b_recurring_scheduling.rs`, mirroring `stage9a_one_tick_contract.rs`'s
shape):

- **Aggregate tick-tag budget**: two `tick`-marked functions, each
  individually `ProvenWithin` under the default limit, whose *combined*
  sequence cost exceeds a deliberately lowered configured limit
  (`LoweringOptions::with_command_limit_assumptions`) — proves the tag's
  aggregate root, not a per-handler sum the compiler would have to compute
  itself, is what the existing `RootExecutionSummary` machinery already
  reports correctly.
- **Self-reschedule is not recursion**: direct inspection of
  `ActivationOverlapAnalysis::function_is_recursive` for a self-scheduling
  function (`false`) alongside an ordinary self-recursive function in the same
  program (`true`), proving the `function_references()` omission holds without
  any change to `audit.rs`.
- **Four-policy disagreement analog**: reusing 9A's own discovered mechanism
  (straight-line redundant-operation folding differs by optimization policy,
  not loop elimination — no loop is ever provably finite under any policy) to
  construct a tick handler that is `ProvenWithin` under `Baseline|Baseline`
  but not under `None|None`, at a fixed configured limit — same falsifiable
  shape as 9A's own test, now for the aggregate tag root.
- **Multiple violations**: two independently failing `tick`/schedule-target
  functions in one program both appear in the single
  `CompilationFailure`'s diagnostics (whatever variant/shape 9B's new checks
  surface through — see "Unresolved questions").

Pinned-server proof (new file, `crates/mdl-test/tests/
stage9b_recurring_scheduling_server.rs`), reusing 9.0's tick-stepping
primitive: 9.0's own todo explicitly deferred promoting
`wait_for_gametime_settled`/`step_and_settle`
(`stage9_0_schedule_tick_evidence.rs`) out of that fixture file "to whichever
of 9B/9C first needs it from more than one test file" — 9B is that first need,
so this promotion (into a shared `mdl-test` helper, exact location TBD at
implementation time) is due now, not optional.

- Compile and deploy programs reproducing both keystone shapes (dynamic-lights:
  pure self-reschedule; spawn-animations: tick-tag plus self-reschedule),
  under all four optimization policies (`None|None` through
  `Baseline|Baseline`), step several ticks, and observe via scoreboard that
  each fires the expected number of times.
- Observe `/reload` mid-chain does not duplicate either pattern, extending
  9.0's M12/M13 observations to compiler-*generated* (not hand-authored)
  `schedule` commands specifically.
- This is the todo's explicit gate line: "pinned-server multi-tick observation
  under all four policies; `dynamic-lights`-shaped and `spawn-animations`-
  shaped fixtures both expressible."

Scale/corruption work: not applicable, for the same reason as 9A — 9B adds no
new persistent state *format* the compiler owns. The external scoreboard/
storage state a scheduled program reads and writes is ordinary program state,
not a compiler-owned continuation frame; that format doesn't exist until 9C.

Gate (proposed, refining `stage-9-todo.md`'s existing line): fast-suite
accept/reject fixtures above, all green; the three dedicated Rust integration
tests above, all green; the pinned-server four-policy, both-keystone-shapes
proof above, all green; alongside the full pre-existing `mdl-compiler`/
`mdl-test` suites with zero regressions.

## Unresolved questions that forbid premature implementation

- **Whether the schedule delay should be a distinct literal-suffix `Ticks`
  type (`5t`) or stay a bare integer literal with the type distinction living
  only at the Hir/Core level (`ScheduleMode`-adjacent `u32`, not a
  user-nameable type).** This dossier recommends the latter (no new lexer
  suffix-scanning machinery, consistent with the grammar-minimal precedent
  `AstSignedInteger` already sets for switch patterns) but this is a real,
  reasonable fork the plan's "a dedicated `Ticks` type" language does not
  fully resolve either way — confirm before the lexer change lands, exactly
  as 9A left its own keyword spelling open.
- **Exact new keyword spellings** (`tick`, `schedule`, `clear`) are naming
  proposals, not architecture, same status as 9A's `one_tick` spelling.
- **Whether `check_schedule_contracts` should be factored to share code with
  `check_one_tick_contracts`, or stay a deliberately separate sibling
  module.** This dossier recommends staying separate (conservative, does not
  touch 9A's tested pass) but flags the duplication as a real, deferrable
  judgment call for the implementing session once both passes' actual shapes
  can be compared.
- **Which `CompilationFailure` variant carries 9B's new diagnostics** — a new
  `CompilationFailure::ScheduleContract` variant (mirroring 9A's
  `TargetContract`) is the natural default, but whether the argument-free/
  unknown-mode semantic-phase diagnostics belong in the existing semantic-
  checking failure variant (parallel to how `one_tick`-without-`export`
  landed in ordinary semantic checking, not a dedicated variant) while only
  the post-lowering `ProvenWithin`/self-rooting diagnostics get the new
  variant, is left to implementation-time judgment, not presupposed here.
- **Whether an advisory (non-blocking) lint for "schedule-self-first" is worth
  building now or deferred entirely** — explicitly out of this gate either
  way; noted only so a reader does not mistake its absence for an oversight.
- **Whether `tick`-marked-but-never-independently-checked functions should
  also be checked for the ordinary Stage 8 recursion ABI** (i.e., can a
  `tick`-marked function still be called synchronously, by name, from
  elsewhere in the program, in addition to firing every tick?) — nothing in
  this design forbids it (the `tick` modifier and ordinary callability are
  orthogonal), but whether that combination is a realistic, useful pattern
  worth a dedicated fixture, or an edge case not worth spending gate budget
  on, is left open.

## References

Internal:

- [`../stage-9-plan.md`](../stage-9-plan.md) — capability 2's one-paragraph
  description and the six frozen semantic decisions 9B lowers to
- [`../stage-9-todo.md`](../stage-9-todo.md) — 9B's checklist and gate
- [`stage-9/README.md`](README.md) — required dossier shape
- [`9-0-contracts-and-evidence.md`](9-0-contracts-and-evidence.md) — the
  measured `schedule`/`#minecraft:tick` facts this dossier lowers directly
  (M5-M12, M17-M18)
- [`9-a-one-tick-contract.md`](9-a-one-tick-contract.md) — the check primitive
  reused, and the "9B needs its own root registration" question this dossier
  answers
- [`../semantic-ambiguities.md`](../semantic-ambiguities.md) A-028/A-029 — the
  open dimension/player-presence questions this dossier explains it does not
  need resolved
- [`../../functionality-keystones/datapacks/dynamic-lights/NOTES.md`](../../functionality-keystones/datapacks/dynamic-lights/NOTES.md)
  and
  [`.../spawn-animations/NOTES.md`](../../functionality-keystones/datapacks/spawn-animations/NOTES.md)
  — the two real-datapack shapes the gate reproduces
- [`../testing-harness.md`](../testing-harness.md) — source-fixture convention

Codebase anchors (verified 2026-07-23; re-check before building on any line):

- `crates/mdl-compiler/src/frontend/target_contract.rs` — 9A's check
  primitive, reused not modified
- `crates/mdl-compiler/src/ir/core/mod.rs:279-289,369,623-679,694-709` —
  `FunctionReferenceKind`, `CoreOp::Call`, `function_references()`, `Function`
  (the plumbing site for `tick_handler`/`is_schedule_target`)
- `crates/mdl-compiler/src/lower/minecraft/analysis.rs:220-289` —
  `FunctionSemanticInventory::new`, where `call_sites` is populated from
  `function_references()`
- `crates/mdl-compiler/src/lower/minecraft/audit.rs:56-131,154-182` —
  `ActivationOverlapAnalysis::analyze`, `CallGraph::new` (no kind-based
  filtering — the load-bearing fact this design is built on)
- `crates/mdl-compiler/src/lower/minecraft/plan/assemble.rs:195` — every
  declared Core function is planned/emitted unconditionally (no whole-program
  dead-function elimination exists)
- `crates/mdl-compiler/src/lower/minecraft/api.rs:244-250,319-334,354-355,
  578-634` — `LoweredFunction`, `generated_entry_requirement()`,
  `analyze_target_execution` (the root-construction extension point)
- `crates/mdl-compiler/src/lower/minecraft/construct.rs:60-134` —
  `TargetConstruction::declare`, the load-tag pattern generalized to
  `#minecraft:tick`
- `crates/mdl-compiler/src/ir/minecraft/command.rs:89-114` — `CommandKind`
- `crates/mdl-compiler/src/ir/minecraft/advancement.rs:10-19` —
  `AdvancementRevokeCommand`, the structural precedent for
  `ScheduleCommand`/`ScheduleClearCommand`
- `crates/mdl-compiler/src/frontend/ast.rs:120-143,325-333,337-352,356-360,
  388-393` — `AstFunction`, `AstEventHandler` (considered and rejected as the
  tick-registration mechanism), `AstStatement`, `AstCall`, `AstCallStatement`
- `crates/mdl-compiler/src/frontend/hir.rs:753-775,953-957` —
  `HirStatementKind`, `HirCall` (the plumbing site for `HirSchedule`/
  `HirScheduleClear`)
- `crates/mdl-compiler/src/frontend/check.rs:1485-1554` —
  `check_event_handler`, `EventTrigger::from_source_name` (the pattern
  `ScheduleMode`'s contextual-identifier validation reuses)
- `crates/mdl-compiler/src/frontend/lexer.rs:199-219` — `DecimalInteger`
  scanning (no suffix machinery today)
- `crates/mdl-compiler/src/frontend/compile.rs:1692` — the only existing
  reader of `generated_entry_requirement()` (a test assertion, not
  enforcement)
- `crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs` — the
  tick-stepping primitive due for promotion into a shared helper
- `crates/mdl-compiler/tests/stage9a_one_tick_contract.rs` — the dedicated-
  Rust-integration-test pattern 9B's own tests mirror
