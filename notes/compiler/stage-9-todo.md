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

Status: **designed, not implemented.** Full design in
[`stage-9/9-a-one-tick-contract.md`](stage-9/9-a-one-tick-contract.md), including a
correction to this checklist's own `ForkBound` citation below (that type lives in
`ir/semantic/behavior.rs`, not `analysis/minecraft/`; the actual target-level
primitive is `RootExecutionSummary::fork_limit_status() -> CommandLimitStatus`).

- [ ] Add an opt-in source marker (`export one_tick fn`, new `KeywordOneTick`
      token; scoped to exported functions only — see dossier for why) asserting a
      function completes within one tick.
- [ ] Wire it to the existing bound analysis (`analysis/minecraft/`:
      `CommandLimitStatus`, `RootExecutionSummary`) so a non-`ProvenWithin`
      result under configured limits becomes a hard error with a readable diagnostic
      (which root, which limit, the proven/unknown bound) — new
      `CompilationFailure::TargetContract` variant, inserted between `lowering` and
      `emission` in `compile_package`.
- [ ] Keep the contract purely a checking discipline — no new IR carrier, no lowering
      change to the checked function (dossier specifies a same-output fixture pair
      to prove this directly).
- [ ] Prove the error fires for a runtime-unbounded loop and passes for a bounded
      one; prove a lower configured limit flips a passing function to rejected (via
      `LoweringOptions::with_command_limit_assumptions`, not a fixture — fixtures
      run under fixed default limits).
- [x] Write `stage-9/9-a-one-tick-contract.md`.
- Gate: fast-suite proof of accept/reject at two configured limit settings.

## 9B — Recurring scheduling without continuation (capability 2)

- [ ] Add the `Ticks` type and compile-time-constant delay literals; reject runtime
      delays with a stable "deferred" diagnostic.
- [ ] Add the `schedule` / `schedule clear` source surface and tick-handler
      registration surface; define HIR/Core representation (a scheduled call is a
      deferred entry reference, not a synchronous call).
- [ ] Enforce argument-free scheduled functions; pass required state through external
      stores the function reads.
- [ ] Enforce self-rooting via `AmbientContextRequirements`: reject a scheduled
      function whose `generated_entry_requirement` does not reduce to the default
      root, with a diagnostic pointing at the inherited-context dependency.
- [ ] Recognize self-reschedule as scheduled re-entry, not a recursive call; ensure
      no Stage 8 activation frame is allocated for it.
- [ ] Lower to `schedule function`/`schedule clear` with the schedule-self-first
      ordering; register tick handlers into `#minecraft:tick` in source/module order
      via the `construct.rs` tag mechanism.
- [ ] Apply the resolved reload-dedup lowering (default `replace`, explicit clear if
      9.0 proved it necessary).
- [ ] Four-policy differential + pinned-server proof: a self-rescheduling job and a
      tick-tag job observed across multiple ticks, with correct reload behavior and
      no duplicate chains.
- [ ] Write `stage-9/9-b-recurring-scheduling.md`.
- Gate: pinned-server multi-tick observation under all four policies; `dynamic-lights`-
      shaped and `spawn-animations`-shaped fixtures both expressible.

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
