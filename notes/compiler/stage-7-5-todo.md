# Stage 7.5 Implementation Checklist

Status: **complete and gated**

Authoritative design: [`stage-7-5-plan.md`](stage-7-5-plan.md).

Implement these tranches in order. Each gate must leave a verified repository state;
do not add source syntax before the selected target structures and preflight legality
exist.

## 7.5.0 — Freeze scope and budgets

- [x] Freeze the supported modifier set: `as`, `at`, `at_executor`, `positioned`,
      `rotated`, `in`, `anchored`, and `align`.
- [x] Freeze `Executor.teleport(PositionSpec)` as frame-relative and
      `Executor.move_by(RelativeWorldOffset)` as receiver-relative.
- [x] Keep all spatial arguments compiler-known attributes; reject binding, passing,
      returning, runtime construction, and interpolation.
- [x] Add explicit limits for decimal bytes/digits, modifiers per scope,
      package-wide modifier occurrences, and selected modifier recipes.
- [x] Record `facing`, entity-as-position/rotation, heightmaps, relations, summon,
      filters, stores, block positions, and runtime values as deferred.

Gate: accepted examples and rejection examples cannot be mistaken for Stage 8
first-class value support.

## 7.5A — Pin Java 26.2 evidence

- [x] Extend the official command-report test to assert the selected `execute`
      branches, parsers, redirects, selector multiplicity, and the `teleport` tree.
- [x] Measure exact context changes for every selected modifier on the pinned server.
- [x] Measure `in` scaling and order relative to `positioned`; retain unmeasured
      boundary behavior conservatively.
- [x] Measure local-coordinate behavior under rotation and feet/eyes anchors.
- [x] Measure teleport native success/result, failure cases, cross-dimension behavior,
      and whether later commands retain the original execution frame.
- [x] Record unresolved behavior in `semantic-ambiguities.md`; do not encode an
      optimistic descriptor fact.

Gate: every target contract used below is backed by command-tree evidence, server
measurement, or an explicit conservative `Unknown`.

## 7.5B — Exact static spatial semantics

- [x] Add bounded `FiniteDecimal` with canonical numeric equality, retained source
      provenance, deterministic printing, and no host-float semantic dependency.
- [x] Add `WorldAxis`, `WorldPosition`, `LocalPosition`, `PositionSpec`,
      `RotationAxis`, `RotationSpec`, and `RelativeWorldOffset`.
- [x] Add `DimensionKey`, `EntityAnchor`, and nonempty canonical `Axes`.
- [x] Enforce all-local or all-world position components and reject `^` rotation.
- [x] Keep block positions separate and absent.
- [x] Add linear validation and scale tests for long/adversarial decimal input.

Gate: semantic attributes are owned, hash/equality stable, target-independent, and
cannot enter `RuntimeValueType` or `CoreType`.

## 7.5C — Structured target spatial vocabulary

- [x] Add Java-validated target numeric atoms and structured world/local positions,
      rotations, anchors, axes, and dimensions.
- [x] Extend `ExecuteModifierKind` with positioned, rotated, anchored, and align;
      retain existing structured as/at/in variants.
- [x] Add structured `TeleportCommand` for the selected `@s <position>` form.
- [x] Add `ENTITY_WRITE` to target effects and classify teleport without weakening to
      unknown raw behavior.
- [x] Update target builder, verifier, renderer, dump, command depth, context/effect
      contract, syntax census, local/global cost, outcomes, and exhaustive matches.
- [x] Add parser-bound, UTF-16 command-length, corruption, deterministic rendering,
      and command-cost tests independently.

Gate: every selected target atom and command round-trips through structured IR; no
new path accepts a rendered fragment or unsafe text.

## 7.5D — Instance contracts and retained preflight recipes

- [x] Generalize semantic behavior queries so verified attributes may refine ambient
      requirements without copying facts into operation declarations.
- [x] Add closed Java 26.2 recipe IDs for every selected run modifier, teleport, and
      move-by.
- [x] Extend `TargetPreflight` with dense ordered run-scope modifier recipe slots.
- [x] Migrate the existing `as` query conversion into retained preflight selection.
- [x] Validate decimals, queries, dimensions, complete rendered length, reachability,
      and target compatibility before resource allocation.
- [x] Reconcile semantic instance contracts, selected arguments, structured target
      contracts, native outcomes, and local costs through one authoritative
      projection per recipe.
- [x] Independently reconstruct and compare the retained modifier recipe table;
      retain the existing semantic recipe corruption tests for
      key/argument/context/effect/cost and detached-slot drift.

Gate: construction cannot parse, reselect, or revalidate a source modifier or spatial
operation; it consumes a verified selected recipe.

## 7.5E — Source grammar and checked modifier chains

- [x] Add bounded decimal, `~`, and `^` token/AST support only in compiler-known
      spatial argument positions.
- [x] Preserve tuple/component/modifier/member/call origins independently and recover
      at the next modifier, block, or statement boundary.
- [x] Add provisional namespace constants for built-in dimensions, anchors, and axes.
- [x] Extend the closed source run-modifier registry and reject unsupported overloads
      without a stringly extension map.
- [x] Type-check `at(query)` and invocation bounds without creating an executor
      capture.
- [x] Require an exact current executor proof for `at_executor()`.
- [x] Preserve final capture identity across context-only modifiers and reject stale
      captures after executor replacement.
- [x] Add positive/negative source fixtures across all four optimization policies.

Gate: source dumps contain normalized typed attributes and exact origins, while
ordinary expressions still cannot construct these values.

## 7.5F — HIR/Core ordered frame transfer

- [x] Extend `ExecutionContext` establishment methods for position, rotation,
      dimension, and anchor with exact modifier-step provenance.
- [x] Give every HIR modifier instance exhaustive reads/writes, invocation bounds,
      effects, and reverse requirement transfer.
- [x] Make HIR verification replay exact modifier order, context state, captures,
      attribute validity, and derived function behavior.
- [x] Extend program-owned Core run modifier declarations and exact source/Core
      correlations without introducing SSA spatial values.
- [x] Update Core builders, printers, verifiers, declaration cloning/editing,
      ambient analysis, function-reference consumers, and exhaustive optimizer
      matches for every new closed attribute/modifier variant.
- [x] Independently replay Core frame and ambient-requirement transfer, including
      calls and nested zero-modifier scopes.
- [x] Differentially compare unoptimized HIR/Core entry requirements and retain only
      independently recomputed optimized Core requirements.
- [x] Add corruption, recursion, unreachable-scope, repeated/nested modifier,
      determinism, bounded-chain, and package-budget tests; retain the existing
      20,000-function iterative ambient-analysis scale proof.

Gate: modifier order and frame provenance survive source → HIR → Core independently,
and no recursive transformation tree or quadratic table is retained.

## 7.5G — Teleport and move-by vertical slices

- [x] Add semantic keys, signatures, validators, effects, context rules, fork/work,
      outcomes, and documentation for teleport and move-by.
- [x] Add closed current-executor method rules with precise wrong-receiver, arity,
      literal, stale-proof, and `Void` diagnostics.
- [x] Store exact lexical executor proofs in HIR and erase them at the Core boundary.
- [x] Add Core attributes/provenance and instance-dependent ambient analysis.
- [x] Select Java 26.2 target recipes in preflight and lower teleport directly.
- [x] Lower move-by to one structured `execute at @s run teleport @s ~...` command,
      never raw text or an isolation helper.
- [x] Reconcile world/entity effects, no-fork behavior, native outcome, local cost,
      complete message/coordinate attributes, and actual placement.
- [x] Prove teleport/move-by do not mutate the execution frame for later source
      statements.

Gate: both methods compile to distinct structured recipes whose server behavior
matches their source meanings.

## 7.5H — Correlation, reports, and public inspection

- [x] Correlate every source run modifier to Core run-scope step, selected recipe,
      and actual target function/command/modifier index.
- [x] Extend semantic-operation source/Core/target command maps to teleport and
      move-by, rejecting unsafe or detached occurrences.
- [x] Report source behavior, optimized generated requirements, invocation bounds,
      recipe, physical placement, and exact provenance without conflating domains.
- [x] Generate the supported source-method matrix from registry/recipe data and
      publish the closed run-modifier recipe identities through exact correlations.
- [x] Publish configured assumptions, target defaults, and derived fork minimums
      unchanged through the expanded modifier set.
- [x] Update CLI ABI reporting without exposing internal outlined helpers as exports.

Gate: public APIs answer exact identity questions without parsing dumps or inferring
IDs from allocation order.

## 7.5I — Differential and real-server exit

- [x] Add valid/invalid fixtures for every selected coordinate family, modifier,
      operation, capture rule, representative target limit, and contextual misuse.
- [x] Assert deterministic HIR, Core, preflight, plan, target IR, pack bytes, trace,
      maps, and reports under all four optimization-policy combinations.
- [x] Run clientless Java 26.2 suites covering incoming-frame versus executor-frame
      behavior, sequential transforms, dimension order, local rotation/anchor, `at`
      capture preservation, teleport versus move-by, frame immutability, empty
      match, and fork rejection.
- [x] Retain the Stage 7 typed-say and unsafe-command server regressions.
- [x] Verify official bundle hash and preserve the sandbox on any behavioral drift.

Gate: the representative Stage 7.5 program and all semantic contrasts execute as
specified on the pinned server without a client.

## Completion audit

- [x] Run `cargo fmt --all --check` and `git diff --check`.
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Run `cargo test --workspace`.
- [x] Run warnings-denied rustdoc and the pinned Rust 1.85 MSRV gate.
- [x] Run Stage 7.5 budget, generated differential, command-report, and
      official-server suites.
- [x] Review every new exhaustive match and verifier mutation independently.
- [x] Update compiler README, roadmap, handoff, testing notes, mcfunction research,
      ambiguity ledger, and references.
- [x] Write the Stage 7.5 → Stage 8 handoff around concrete representation pressure:
      runtime positions, persistent entity references, and function/run boundaries.

## Explicitly deferred beyond Stage 7.5

- [x] First-class/runtime spatial and entity values belong to Stage 8.
- [x] Fork-safe/reentrant calling conventions belong to Stage 8.
- [x] Conditions/stores and value-producing command blocks wait for outcome/value
      representation work.
- [x] Scheduling remains Stage 9; interpolation/macros remain Stage 10.
- [x] Aggressive context/coordinate folding remains Stage 11.
- [x] Multi-version recipes and stable package/resource ABI remain Stage 12.
