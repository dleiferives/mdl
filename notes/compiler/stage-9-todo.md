# Stage 9 Implementation Checklist

Status: **planned; not started**

Execution order and gating for [`stage-9-plan.md`](stage-9-plan.md). Each tranche
reaches its applicable gate (server or evaluator) before the next dependent tranche
treats it as established, per the standing PS-2 slice discipline. Detailed per-tranche
design lives in [`stage-9/`](stage-9/README.md); this file records only execution
state.

The three capabilities map to tranches: capability 1 (one-tick contract) is 9A;
capability 2 (recurring scheduling, no continuation) is 9B; capability 3 (persistent
continuations) is 9C. 9.0 is shared foundation; 9D is shared hardening.

## 9.0 — Freeze contracts and pin server evidence

Status: **complete except two named open sub-questions (dimension reconstruction,
player-presence selector leakage), which do not block 9A/9B.**

- [x] Measure `schedule function <id> <time>` on pinned Java 26.2: `replace` vs
      `append` pending-slot behavior, and firing order for multiple schedules landing
      on the same tick. (M5-M9; scheduled-function storage is a per-function set of
      pending target ticks — `replace` clears-and-inserts, `append` inserts-or-no-op.)
- [x] Measure the `/reload` re-arm hazard: does re-running `#minecraft:load` stack a
      duplicate self-reschedule chain, and does `replace` alone prevent it or is an
      explicit `schedule clear` required? (M12-M13b; `replace` alone is sufficient in
      every case tested; the hypothesized `append`+mismatched-delay hazard did not
      reproduce and is recorded as an open, unexplained negative result in
      `semantic-ambiguities.md` A-030 — this does not change the frozen decision.)
- [x] Measure exactly what execution context (`@s`, position, rotation, dimension,
      anchor) a scheduled function receives when it fires. **Partial**: executor loss
      and default ambient position (world spawn point) confirmed (M14). Dimension
      (M15) and rotation/anchor were not resolved — M15's method was invalidated
      (see the dossier and `semantic-ambiguities.md` A-028); rotation/anchor were not
      separately probed this pass. Player-presence leakage (M16) deferred, not run
      (`semantic-ambiguities.md` A-029).
- [x] Measure `#minecraft:tick` handler invocation order and its relationship to
      function-tag declaration order. (M10-M11; fires in declared order, stable
      across `/reload`; tick-tag handlers run before same-tick `schedule` firings.)
- [x] Add deterministic tick-stepping and cross-tick state observation to the server
      harness. Implemented as fixture-local helpers
      (`step_and_settle`/`wait_for_gametime_settled` in
      `crates/mdl-test/tests/stage9_0_schedule_tick_evidence.rs`), not yet promoted
      to a shared `TestServer` method — deferred to whichever of 9B/9C first needs it
      from more than one test file. PS-13 bot infra was not needed (M16 deferred).
- [x] Freeze the six frozen semantic decisions from the plan; record residual
      uncertainty in `semantic-ambiguities.md`. Five decisions frozen without
      qualification; the sixth (cut-legality) frozen for executor/position, open for
      dimension. Residual uncertainty recorded as A-028 through A-030.
- [x] Write `stage-9/9-0-contracts-and-evidence.md`, updated in place with measured
      results (not just the original measurement plan).
- Gate: pinned-server measurements recorded; harness can step ticks and observe
  state deterministically. **Reached.** All 8 test functions in
  `stage9_0_schedule_tick_evidence.rs` pass individually and together against
  `versions/26.2/server-26.2.jar` (via the `tmp/minecraft-server-26.2.jar` bundle)
  and OpenJDK 25.0.3 (125.69s combined run, `--test-threads=1`). The two open
  sub-questions (A-028, A-029) do not block 9A or 9B, which do not depend on
  dimension reconstruction or player-presence leakage; they do block 9C's
  interior-cut context-reconstruction work until resolved.

## 9A — One-tick bound contract (capability 1)

Status: **implemented (2026-07-23).** Full design and implementation notes in
[`stage-9/9-a-one-tick-contract.md`](stage-9/9-a-one-tick-contract.md), including a
correction to this checklist's own `ForkBound` citation below (that type lives in
`ir/semantic/behavior.rs`, not `analysis/minecraft/`; the actual target-level
primitive is `RootExecutionSummary::fork_limit_status() -> CommandLimitStatus`), and
a load-bearing implementation discovery: the cost analysis cannot prove *any* loop
finite under *any* optimization policy (every structured loop lowers to target-level
self-recursion; the analysis's cycle detection is reachability-only, not
trip-count-aware) — see the dossier's "Implementation notes" for what this changed
about the accept/regression fixtures and where the four-policy-disagreement
mechanism actually came from instead (redundant straight-line operation folding).

- [x] Add an opt-in source marker (`export one_tick fn`, new `KeywordOneTick`
      token; scoped to exported functions only — see dossier for why) asserting a
      function completes within one tick.
- [x] Wire it to the existing bound analysis (`analysis/minecraft/`:
      `CommandLimitStatus`, `RootExecutionSummary`) so a non-`ProvenWithin`
      result under configured limits becomes a hard error with a readable diagnostic
      (which root, which limit, the proven/unknown bound) — new
      `CompilationFailure::TargetContract` variant, inserted between `lowering` and
      `emission` in `compile_package`.
- [x] Keep the contract purely a checking discipline — no new IR carrier, no lowering
      change to the checked function (proved directly: a dedicated integration test,
      `one_tick_marker_does_not_change_emitted_commands`, asserts byte-identical
      `MinecraftDebugDumper` output and emitted pack for a marked/unmarked pair under
      all four optimization policies).
- [x] Prove the error fires for a runtime-unbounded loop and passes for a bounded
      one; prove a lower configured limit flips a passing function to rejected (via
      `LoweringOptions::with_command_limit_assumptions`, not a fixture — fixtures
      run under fixed default limits).
- [x] Write `stage-9/9-a-one-tick-contract.md`.
- Gate: fast-suite proof of accept/reject at two configured limit settings.
  **Reached.** `cargo test -p mdl-compiler --test source_fixtures` (4 new
  `stage9/9a_one_tick_*` fixtures plus every pre-existing fixture) and
  `cargo test -p mdl-compiler --test stage9a_one_tick_contract` (4 dedicated
  tests: two-configured-limits, four-policy disagreement, byte-identical
  emission, multiple-violations-all-reported) are both green, alongside the
  full pre-existing `mdl-compiler` suite (789 lib tests unaffected).

## 9B — Recurring scheduling without continuation (capability 2)

Status: **implemented (2026-07-23).** Full design and implementation notes in
[`stage-9/9-b-recurring-scheduling.md`](stage-9/9-b-recurring-scheduling.md),
including the anticipated load-bearing discovery (`CoreOp::function_references()`
feeds both inter-function reachability and the Stage 8 recursion/call-graph
classification undifferentiated by `FunctionReferenceKind`, so `Schedule`/
`ScheduleClear` Core operations simply do not implement it) plus several more
found during implementation: `TargetExecutionRoot::FunctionTag` resolves to one
independent `RootExecutionSummary` *per tag entry*, not one pre-aggregated
summary, so the aggregate tick-tag budget check sums entries itself; any
`tick`/schedule-target function containing an unsafe minecraft command can
never be proven self-rooted under the current ambient model (not an edge
case — the default outcome); and `ItemReplaceBlock`'s conservative `Unknown`
native-outcome cost (predating Stage 9B) rules out block-entity writes as an
observable side effect for cost-provable handlers too. See the dossier's
"Implementation notes" for the full account.

- [x] Add the `tick` function modifier (tick-tag registration, does **not** require
      `export` — diverges from 9A's `one_tick` deliberately, see dossier) and the
      `schedule` / `schedule clear` statements (bare function-name target, not a
      call expression — see dossier for why reusing `AstCall` was rejected). Delay
      stays a bare compile-time integer literal (no new `Ticks` literal suffix
      proposed; flagged open in the dossier); mode (`append`/`replace`) is a plain
      identifier validated against a closed vocabulary, mirroring
      `EventTrigger::from_source_name`.
- [x] Define HIR/Core representation: `HirStatementKind::Schedule`/`ScheduleClear`
      siblings of `Call`; `CoreOp::Schedule`/`ScheduleClear` siblings of `Call` that
      deliberately return no `function_references()` reference (the load-bearing
      discovery above — this is what makes self-reschedule allocate no Stage 8
      activation frame, with no change to `audit.rs` needed).
- [x] Enforce argument-free `tick`/schedule-target functions (new check, not free
      from ordinary call-site arity checking given the bare-name grammar); pass
      required state through external stores the function reads.
- [x] Enforce self-rooting via `AmbientContextRequirements`/`generated_entry_requirement()`
      (currently read only by one test assertion, never enforced): reject a
      `tick`/schedule-target function whose entry requirement is not `NONE`, with a
      diagnostic naming the non-`None` component. Safe to build now despite 9.0's
      open dimension question (A-028) — this is a static semantic-model check, not
      a runtime-reconstruction claim; see dossier.
- [x] Extend `LoweringOutput::analyze_target_execution`'s root list: every
      schedule-target function becomes its own independent `Function` root; the
      `#minecraft:tick` tag (constructed only if non-empty) becomes one aggregate
      `FunctionTag` root covering all handlers together, matching vanilla's actual
      per-tick combined-budget semantics.
- [x] Lower to `schedule function`/`schedule clear` (new minimal `CommandKind`
      variants mirroring `AdvancementRevokeCommand` — no crossings.rs involvement,
      no runtime operands to marshal); register tick handlers into `#minecraft:tick`
      in `FunctionId` order (already deterministic) via the `construct.rs` tag
      mechanism, generalized from one entry to N. Schedule-self-first stays a
      documented authoring pattern, not a compiler-inserted reordering.
- [x] Apply the resolved reload-dedup lowering: `replace` unconditionally, no
      explicit `schedule clear` inserted by the compiler (9.0 M13a/M13b already
      showed `replace` alone is sufficient).
- [x] Four-policy differential + pinned-server proof: a self-rescheduling job
      (`dynamic-lights` shape) and a tick-tag job (`spawn-animations` shape, tick tag
      plus a separate self-rescheduling watchdog) observed across multiple ticks,
      with correct reload behavior and no duplicate chains. Promoted 9.0's
      `wait_for_gametime_settled`/`step_and_settle` out of the single fixture file
      into a shared `mdl-test` helper (`crates/mdl-test/src/tick.rs`) — 9.0
      explicitly deferred this to whichever of 9B/9C needed it from more than one
      file first; that was 9B.
- [x] Write `stage-9/9-b-recurring-scheduling.md`.
- Gate: pinned-server multi-tick observation under all four policies; `dynamic-lights`-
      shaped and `spawn-animations`-shaped fixtures both expressible. **Reached.**
      `cargo test -p mdl-compiler --lib` (789 tests, unaffected),
      `cargo test -p mdl-compiler --test source_fixtures` (all `9b_*` fixtures:
      3 accept, 6 reject, 1 regression, 1 schedule-clear-only-target, alongside
      every pre-existing fixture), `cargo test -p mdl-compiler --test
      stage9b_recurring_scheduling` (4 dedicated tests: aggregate tick-tag
      budget, self-reschedule-is-not-recursion, four-policy disagreement,
      multiple violations), and `cargo test -p mdl-test --test
      stage9b_recurring_scheduling_server -- --ignored --nocapture` (both
      keystone shapes, all four policies, reload-no-duplication) are all green
      against `tmp/minecraft-server-26.2.jar` and OpenJDK 25.0.3.

## 9C — Persistent continuations (capability 3)

- [ ] Add the resume-discriminant model (one objective per resumable function) and the
      entry-dispatch lowering (`execute if score $resume … matches N run function …`).
- [ ] Compute the live-state materialization set per cut from existing liveness; lift
      only live locals; bridge them to scoreboard (scalars) / storage (aggregates)
      through the generalized crossing engine.
- [ ] Build the internal cut/region graph over the structured target IR (not a public
      IR level); verify each inter-cut region fits the per-tick budget.
- [ ] Add the explicit `yield` statement and the schedulable-loop marker; auto-
      partition at loop iteration boundaries (Fiber unit) to fit the budget.
- [ ] Reject any inter-cut region exceeding the budget with an "add a yield / mark
      loop schedulable" diagnostic — never silently split straight-line code.
- [ ] Enforce the cut-legality rule at every interior cut (post-cut context
      self-rooting or reconstructible).
- [ ] Implement atomic replayable regions: advance the continuation frame only at a
      successful cut; re-run an interrupted region from the last committed frame;
      defer or make idempotent any in-region side effects.
- [ ] Implement completion (stored typed result + completion signal, poll or scheduled
      handler) and cancellation (clear pending + terminal discriminant).
- [ ] Preserve the abnormal-interruption rule: command-limit abort routes to the
      generated recovery entry and is never a semantic result.
- [ ] Run the Brainfuck package (`tests/programs/brainfuck/`) as a tick-spanning job
      preserving the full `ps-3-handoff.md` state set; prove semantic equivalence to
      the synchronous run.
- [ ] Write `stage-9/9-c-persistent-continuations.md` (may sub-divide internally:
      discriminant+state, region graph+budget, yield+auto-partition, recovery+
      completion).
- Gate: pinned-server tick-spanning Brainfuck run; four-policy differential showing
      identical final state regardless of tick spread.

## 9D — Hardening and handoff

- [ ] Public reports: every cut, materialized value, resume dispatch, region budget
      decision, and completion/cancellation contract explained.
- [ ] Four-policy differential proving partitioning changes *when*, never *what*.
- [ ] Corruption tests over the persistent continuation frame (torn/aborted writes),
      determinism tests over multi-handler tick ordering and same-tick schedule firing,
      and a scale proof.
- [ ] Pinned-server regressions for every capability at configured limit settings.
- [ ] Write `stage-9-completion-audit.md` and `stage-9-handoff.md` naming what Stage
      10 (general macro interpolation) and Stage 11 (global cut-minimizing fusion
      optimizer) inherit.
- Gate: all fast, MSRV, scale, corruption, determinism, and pinned-server gates pass.

## Cross-tranche rules

- No tranche removes a rejection until it has a complete verified replacement (the
  Stage 8 discipline: recursion rejection was removed only once a full stack plan
  existed).
- Every capability crosses the full Stage 8.5 boundary set (semantics → HIR/Core →
  verify/print → evaluator where target-independent → None/Baseline equivalence →
  physical/target lowering → source fixture → pinned server → cost/limit → handoff).
- Unsupported pieces fail at an owned boundary with a stable diagnostic; nothing falls
  through to a raw command or a test inspecting compiler-private storage.
