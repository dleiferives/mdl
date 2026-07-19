# PS-1 Testing Foundation Checklist

Status: **complete (2026-07-18)**

Authoritative design: [`ps-1-testing-plan.md`](ps-1-testing-plan.md).

Implement in order. Every gate must leave the existing fast and ignored-server
suites working.

## PS-1.0 — Freeze contracts

- [x] Inventory the current unit, source-fixture, exact-golden, structural server,
      generated server, command-report, limit, and measurement suites.
- [x] Freeze the three authorities: Core evaluator, compiler structure, and pinned
      vanilla execution.
- [x] Define typed evaluator outcomes and resource-limit failures.
- [x] Define the normalized world-state projection and comparison rules.
- [x] Define semantic, exact-limit, and measurement scenario modes.
- [x] Decide the smallest supported scenario data surface; retain Rust escape cases
      for target research instead of arbitrary shell directives.
- [x] Record unresolved target observations in `semantic-ambiguities.md`.

Gate: a test author can identify one correct oracle for every planned assertion.

## PS-1A — Core evaluator

- [x] Add bounded evaluator configuration and deterministic failure types.
- [x] Execute verified scalar constants and pure instructions.
- [x] Execute block parameters, jumps, branches, returns, and unreachable.
- [x] Execute ordinary calls and multiple scalar results.
- [x] Support direct/mutual recursion under explicit step and depth limits.
- [x] Reject external/Minecraft operations with an owned structured reason.
- [x] Keep evaluation independent from Core optimization and Minecraft lowering.
- [x] Add malformed-input boundary tests even though public evaluation requires
      verified Core.
- [x] Add evaluator determinism, deep-CFG, recursion-limit, and scale tests.

Gate: the existing target-independent scalar corpus has executable semantic results.

## PS-1B — Typed scenario and observation model

- [x] Add typed scenario IDs, policies, invocation frames, setup declarations,
      observation paths, outcomes, and completion expectations.
- [x] Add normalized score, NBT, block, entity-set/multiset, and log-effect values.
- [x] Reject duplicate paths, unsafe resource names, undeclared cleanup, and
      contradictory expectations before server startup.
- [x] Add deterministic scenario rendering for failure reports.
- [x] Unit-test comparison and concise nested-state diffs without Java.
- [x] Add generation nonces and stable test-owned result roots.

Gate: scenarios and expected observations validate entirely in the fast test suite.

## PS-1C — One-startup policy differential

- [x] Compile the first scalar source under all four Core/Minecraft policy products.
- [x] Rewrite only deployment-owned namespaces/objectives through supported compiler
      options; never patch emitted command strings.
- [x] Install all policy packs plus one harness driver before server startup.
- [x] Reset supported declared score state before each policy invocation.
- [x] Query and normalize score observations after each invocation.
- [x] Compare each observation to its explicit expectation and to the other policies.
- [x] Preserve the sandbox, compiler artifacts, observations, and log checkpoint on
      the first failure.
- [x] Give every policy and generation an independent completion marker.

Implementation note: the scalar differential and the batched NBT/entity calibration
pass against the pinned vanilla server. Block, projected entity-field, and public-log
adapters remain capability-driven extensions rather than PS-1 requirements.

Gate: one server process proves semantic equality across all four policies.

## PS-1D — Outcome and completion channels

- [x] Store command success and result independently as optional NBT channels in
      semantic-mode wrappers.
- [x] Record whether the first outer continuation line executed before ABI result
      publication.
- [x] Distinguish no child context, failed command, successful result zero, returned
      zero, returned nonzero, and sequence interruption.
- [x] Query synchronous completion with an ordered generation/policy console barrier,
      without a timing sleep.
- [x] Reserve generation-specific result roots and the nonce/done protocol for later
      multi-tick scenarios without adding
      a scheduler.

Gate: every currently known command completion class has a non-ambiguous observation.

Implementation note: the complete synchronous matrix passes on vanilla. In
particular, unavailable store callbacks remain distinct from numeric zero and
`return 0` reports success one/result zero.

## PS-1E — Instrumentation isolation

- [x] Prevent semantic observation wrappers from being used by exact-limit cases.
- [x] Add bare-root invocation and post-root observation for sequence/fork probes.
- [x] Account explicitly for any harness command included in a measured contract;
      the root-owned terminal completion write is typed and counted, while setup,
      barrier, publication, and queries are separate roots.
- [x] Keep wall-time and macro-cache measurements in the measurement protocol rather
      than ordinary semantic assertions.
- [x] Add a regression proving that added observation commands cannot silently shift
      a hard-limit boundary.

Gate: semantic instrumentation cannot invalidate cost evidence.

Implementation note: typed bare-root sequence and fork fixtures pass on vanilla;
setup/readiness, barriers, publication, and observations remain outside the measured
root.

## PS-1F — Calibration corpus

- [x] Add a scalar internal-call plus branch case with Core-evaluator expectations
      across all four compiler policy products.
- [x] Add direct and mutual recursion cases plus frame cleanup.
- [x] Add zero/one/many and nested `execute as` scenarios.
- [x] Add incoming-frame versus `.at_executor()` movement scenarios.
- [x] Add return/failure/result-zero/no-context contrasts.
- [x] Port or wrap exact sequence and fork boundary probes.
- [x] Add abort-residue and load/recovery behavior.
- [x] Compare new evidence with existing Stage 7.5/8 tests before removing any
      duplicate handwritten fixture.

Gate: PS-1's infrastructure is calibrated against independently established vanilla
behavior and existing compiler contracts.

## PS-1G — Workflow and hardening

- [x] Add focused local commands and opt-in server commands to the testing notes.
- [x] Document server/JDK/hash requirements and preserved failure artifacts.
- [x] Ensure the default workspace suite never requires Java or a server JAR.
- [x] Use Cargo test filtering plus deterministic caller-ordered batched scenarios.
- [x] Bound scenario counts, observation sizes, NBT depth, log retention, and diff
      rendering.
- [x] Run formatting, lints, fast workspace tests, and the pinned ignored-server gate.
- [x] Write the PS-1 to PS-2 handoff around supported evaluator operations and
      scenario observation capabilities.

Gate: PS-2 can add one vertical feature test without designing another harness.
