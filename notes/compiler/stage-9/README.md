# Stage 9 Design Dossiers

Status: **planned; not started**

The [Stage 9 overview](../stage-9-plan.md) defines the cross-tranche architecture:
one boundary/crossing model with three capabilities layered over it. These dossiers
are the implementation-facing source of truth for each gated tranche and are written
when the tranche's turn comes, not up front.

Dossiers:

1. [`9-0-contracts-and-evidence.md`](9-0-contracts-and-evidence.md) — **measured.**
   Pinned `schedule`/`schedule clear`/`#minecraft:tick` behavior on the pinned
   server, built cross-tick observation into the harness, and froze five of the
   six semantic decisions (the sixth, cut-legality, partially).
2. [`9-a-one-tick-contract.md`](9-a-one-tick-contract.md) — **designed, not
   implemented.** Promotes the existing bound analysis into an opt-in hard
   one-tick contract (capability 1).
3. [`9-b-recurring-scheduling.md`](9-b-recurring-scheduling.md) — **designed, not
   implemented.** Argument-free self-rooting scheduled functions, tick-tag
   registration, self-reschedule-is-not-recursion, reload dedup (capability 2).
4. `9-c-persistent-continuations.md` — resume discriminant, liveness-lifted live
   state, cut/region graph and budget verification, explicit `yield` plus
   schedulable-loop auto-partition, atomic replayable regions, completion and
   cancellation (capability 3). May sub-divide.

Read these with:

- [`../stage-9-plan.md`](../stage-9-plan.md) for the architectural boundary;
- [`../stage-9-todo.md`](../stage-9-todo.md) for execution state;
- [`../pre-scheduler/ps-3-handoff.md`](../pre-scheduler/ps-3-handoff.md) for the
  concrete continuation state the Brainfuck client must preserve;
- [`../stage-8-handoff.md`](../stage-8-handoff.md) for why synchronous frames cannot
  cross a tick; and
- [`../macro-reference-crossing-model.md`](../macro-reference-crossing-model.md) for
  the value-crossing architecture Stage 9 generalizes.

## Document rule

Same as Stage 8: implementation discoveries update the relevant dossier first; the
overview changes only when a cross-tranche invariant or scope boundary changes; the
checklist records completion but is never the only place a decision is explained.

Every dossier has the same required shape:

- problem and non-goals;
- current repository boundary (verified, with a re-check note);
- research applied;
- proposed inputs, outputs, and algorithms;
- invariants and failure behavior;
- diagnostics and public inspection;
- tests, scale/corruption work, and gate; and
- unresolved questions that forbid premature implementation.
