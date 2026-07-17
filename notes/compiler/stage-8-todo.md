# Stage 8 Implementation Checklist

Status: **complete for the frozen scalar synchronous scope**

Authoritative design: [`stage-8-plan.md`](stage-8-plan.md).

Stage 8 is now limited to scalar physical realizations and synchronous calling
conventions. Structs, lists, deferred conditions, runtime spatial values, and
persistent entity references are not exit requirements.

## 8.0 — Freeze contracts and pin Java evidence

Design dossier: [`stage-8/8-0-contract-and-evidence.md`](stage-8/8-0-contract-and-evidence.md).

- [x] Write a handwritten Java 26.2 fixture where a many-context `execute as` body
      uses values before/after nested function calls and records exact child order.
- [x] Test zero, one, and several contexts; nested forks; ordinary completion;
      `return`; command failure; and command-sequence interruption.
- [x] Prove whether one child invocation fully unwinds before the next begins.
- [x] Measure command-storage tail-frame append, `[-1]`/`[-2]` access, typed scalar
      copies, nested push/pop, and residual state after limit abortion.
- [x] Record exact local command/fork costs for static and recursive prototypes.
- [x] Decide whether recursive exported entries are root-only/resetting or require an
      explicit recovery entry after abnormal termination.
- [x] Define `CallerBounded` recursion depth without inventing truncation, a trap, or
      a false static bound.
- [x] Record unresolved behavior in `semantic-ambiguities.md`.

Gate: the activation model is backed by pinned server behavior; no current legality
gate has been removed.

## 8A — Separate physical domains

Design dossier: [`stage-8/8-a-realization-model.md`](stage-8/8-a-realization-model.md).

- [x] Add typed IDs and closed data for physical storage, realizations, use
      requirements, materialization occurrences, ABI modes, and activation plans.
- [x] Keep compiler facts separate from physical storage and realizations.
- [x] Define only `ScoreBool`, `ScoreI32`, `ActivationNbtBool`, and
      `ActivationNbtI32` storage classes.
- [x] Let one `ValueId` own zero, one, or several realization occurrences.
- [x] Give every realization an exact semantic owner, storage identity, type,
      definition site, live region, and provenance.
- [x] Make storage reuse end the previous realization before a new one begins.
- [x] Define exact per-use accepted storage classes rather than one global value
      representation.
- [x] Add `ElidedKnown`, `DirectScore`, and `ActivationFrameField` ABI modes without
      changing semantic signatures.
- [x] Add `SerialStatic` and `RecursiveStack` activation disciplines separately from
      ABI modes.
- [x] Add deterministic table/work limits and all-or-nothing limit failure.
- [x] Add dumps and public read-only inspection without exposing mutable planning
      internals.
- [x] Independently verify density, ownership, type compatibility, dominance/live
      reachability, storage non-overlap, and complete use satisfaction.
- [x] Add representative independent corruption tests across every owned physical
      table relation.
- [x] Add a 20,000-value sparse physical-plan scale proof.

Gate: existing Core receives complete verified physical-domain plans without target
resource allocation or carrier identities leaking into Core.

## 8B — Migrate the fixed-score compatibility path

Design dossier: [`stage-8/8-b-score-compatibility.md`](stage-8/8-b-score-compatibility.md).

- [x] Derive use requirements from every retained scalar instruction operand/result,
      CFG transfer, call parameter/result, return, and export boundary.
- [x] Express existing assigned homes as score storage and existing value/home
      intervals as score realizations.
- [x] Express current function ABI and call-result copies through indexed ABI modes
      and occurrence plans.
- [x] Integrate `InstructionPlan`, `FunctionAbi`, call destinations, and edge
      transfers as a bidirectionally verified fixed-score compatibility projection,
      rather than a second representation authority.
- [x] Keep constants as facts separately from the explicit score materialization
      selected for a definition retained by the current compatibility policy.
- [x] Add explicit constant-to-score recipes and compose physical type/cost
      contracts with structured-target effect, outcome, path, and final-length
      verification.
- [x] Split existing semantic `TargetPreflight` from a new `PhysicalPreflight` that
      validates materialization/ABI recipes before resources.
- [x] Make `None` preserve current nonrecursive target output as an exact
      compatibility oracle.
- [x] Let `Baseline` reuse/coalesce realizations only through existing verified
      liveness and home rules.
- [x] Reconcile every planned realization/materialization with fixed-score placement
      and the existing constructed-command/target verifier chain.

Gate: all existing programs remain semantically equivalent, and the nonrecursive
`None` artifact stays byte-compatible except for explicitly reviewed metadata/report
additions.

## 8C — Admit serial many-context run bodies

Design dossier: [`stage-8/8-c-serial-contexts.md`](stage-8/8-c-serial-contexts.md).

- [x] Add an activation-overlap analysis distinct from invocation multiplicity and
      command/fork cost.
- [x] Model several measured child invocations as serial, not simultaneously live,
      only if 8.0 proved complete unwind order.
- [x] Replace `lower.unsupported-run-cardinality` with an overlap-safety decision;
      retain independent minimum-fork and hard-limit checks.
- [x] Verify that outlined run-body locals, fixed call parameters/results, recipe
      temporaries, and edge temporaries are reset/redefined before each child use.
- [x] Reject any path whose values genuinely escape one child activation.
- [x] Test nested ordinary calls and typed Minecraft operations inside zero/one/many
      source run scopes under all optimization policies.
- [x] Add a compiler-generated clientless Java 26.2 differential with distinct
      per-entity inputs/results so cross-child contamination is observable.

Gate: synchronous many-context bodies compile through static homes only when serial
reuse is proven, without adding an NBT frame merely because multiplicity exceeds one.

## 8D — Add recursive SCC frames

Design dossier: [`stage-8/8-d-recursive-frames.md`](stage-8/8-d-recursive-frames.md).

- [x] Reuse the existing iterative call graph/SCC analysis to identify only reachable
      recursive components.
- [x] Keep acyclic functions and edges on `SerialStatic`.
- [x] Build canonical typed spill-frame schemas for recursive SCC entry/internal
      edges.
- [x] Add score-to-frame and frame-to-score materialization recipes with exact widths,
      paths, effects, outcomes, costs, origins, and command lengths.
- [x] Compute exact score realizations live across each recursive call and spill only
      those values.
- [x] Select the direct-score parameter/result ABI, under which no caller-frame-
      resident parameter/result needs a `[-2]` transfer in Stage 8.
- [x] Select a complete physical parameter/result mode for every recursive call
      occurrence.
- [x] Prove the ordered direct-score restore/result protocol equivalent to a
      simultaneous boundary transfer through immutable frame sources and explicit
      destination-separation checks.
- [x] Reject any coalescing/placement where a result overwrites a different caller
      value still live after the call.
- [x] Keep restore/result/pop in the caller continuation so an ordinary semantic
      return from the callee cannot bypass cleanup; reserve wrappers for a future
      frame-field parameter/result ABI.
- [x] Route internal calls to exported functions through internal activation entries,
      never public root reset/recovery wrappers.
- [x] Implement the selected root reset/recovery contract and publish its external
      nesting restriction.
- [x] Publish recursion depth as `CallerBounded` alongside command-limit evidence.
- [x] Remove `lower.recursive-call-abi` only when every recursive SCC has a complete
      verified frame and physical-preflight plan.
- [x] Prohibit scheduling/yielding with a live synchronous frame by keeping typed
      scheduling outside Core until Stage 9 persistent-continuation semantics exist.
- [x] Test direct recursion, mutual recursion, multiple results, branches, early
      returns, dead results, nested run scopes, and limit-abort recovery.
- [x] Run the generated recursive cases on the pinned server without a client.

Gate: direct and mutual scalar recursion work through frames confined to recursive
SCCs; nonrecursive programs retain static storage.

## 8E — Reports, hardening, and exit

Design dossier: [`stage-8/8-e-hardening-and-handoff.md`](stage-8/8-e-hardening-and-handoff.md).

- [x] Report semantic value, fact, storage, realization, use requirement,
      materialization, physical ABI, activation discipline, spill, and exact
      provenance as separate domains.
- [x] Explain why each frame/wrapper/transfer/resource exists and why static storage
      was illegal where a stack was selected.
- [x] Prove unused frames, wrappers, materializations, objectives, and storage paths
      are absent.
- [x] Assert deterministic Core, preflights, realization/activation plans, lowering
      plan, target IR, artifact bytes, traces, maps, and reports.
- [x] Differentially run every valid/invalid fixture under all four optimization
      combinations.
- [x] Retain every Stage 7/7.5 typed-command, unsafe-boundary, command-limit, and
      spatial regression.
- [x] Independently mutate missing/detached/mistyped realization, ABI, frame, spill,
      recipe, resource, and command-correlation entries.
- [x] Run formatting, diff hygiene, warnings-denied Clippy/rustdoc, workspace tests,
      Rust 1.85 MSRV, 20,000-node scale, and deterministic rebuild gates.
- [x] Run the pinned official-server fork-order, recursive-frame, generated
      differential, and recovery suites.
- [x] Update the roadmap, compiler/mcfunction research, ambiguity ledger, testing
      guide, CLI/public API docs, and Stage 8 handoff.
- [x] Plan the next aggregate/value-feature stage from actual Stage 8 realization and
      ABI evidence.

## Explicitly deferred

- [x] Deferred conditions and native command success need a separate effect/stability
      aware computation-form plan.
- [x] Structs/lists need source value, copying, mutation, ownership, and operation
      semantics before physical layouts.
- [x] Persistent entity references wait for exact-one acquisition and lifecycle
      semantics.
- [x] Runtime spatial values wait for numeric and command-materialization semantics.
- [x] Strings/text, floats/fixed point, enums, maps, general references, and
      Minecraft-backed schemas are later vertical slices.
- [x] Higher-order and indirect calls need callable identity and callback ABI work.
- [x] Tail-recursion elimination is an optimization after correct recursive frames.
- [x] Scheduling and persistent continuation frames belong to Stage 9.
- [x] Function macros and arbitrary dynamic paths belong to Stage 10.
- [x] Global/profile-guided representation search belongs to Stage 11.
- [x] Stable cross-package ABI and nested external entry belong to Stage 12.
- [x] Frame-field parameters/results, typed `[-2]` forwarding, and generalized
      frame-source parallel copies wait for an ABI client that actually selects
      frame fields.
