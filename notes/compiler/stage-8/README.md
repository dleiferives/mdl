# Stage 8 Design Dossiers

Status: **complete for the frozen scalar synchronous scope**

The Stage 8 overview defines the cross-tranche architecture. These dossiers are the
implementation-facing source of truth for each gated substep:

1. [`8-0-contract-and-evidence.md`](8-0-contract-and-evidence.md) — freeze the
   activation/failure contracts and measure Java 26.2 before changing legality.
2. [`8-a-realization-model.md`](8-a-realization-model.md) — separate facts, storage,
   realizations, use requirements, materializations, ABI modes, and activation.
3. [`8-b-score-compatibility.md`](8-b-score-compatibility.md) — migrate today's
   fixed-score lowering into the new model without changing nonrecursive output.
4. [`8-c-serial-contexts.md`](8-c-serial-contexts.md) — replace cardinality rejection
   with measured activation-overlap reasoning for synchronous many-context bodies.
5. [`8-d-recursive-frames.md`](8-d-recursive-frames.md) — add recursive-SCC-only
   activation frames, exact spills, call transfers, cleanup, and recovery.
6. [`8-e-hardening-and-handoff.md`](8-e-hardening-and-handoff.md) — public reports,
   corruption/scale/differential/server gates, and the next-client handoff.

Read these with:

- [`../stage-8-plan.md`](../stage-8-plan.md) for the architectural boundary;
- [`../stage-8-todo.md`](../stage-8-todo.md) for execution state;
- [`../stage-8-completion-audit.md`](../stage-8-completion-audit.md) for final
  implementation and evidence reconciliation;
- [`../stage-8-handoff.md`](../stage-8-handoff.md) for the downstream contract;
- [`../stage-7-5-handoff.md`](../stage-7-5-handoff.md) for inherited guarantees; and
- [`../../mcfunction/representation-selection.md`](../../mcfunction/representation-selection.md)
  for the broader backend research inventory.

## Document rule

Implementation discoveries update the relevant dossier first. The overview changes
only when a cross-tranche invariant or scope boundary changes. The checklist records
completion; it must not become the only place where an implementation decision is
explained.

Every dossier has the same required shape:

- problem and non-goals;
- current repository boundary;
- research applied;
- proposed inputs, outputs, and algorithms;
- invariants and failure behavior;
- diagnostics and public inspection;
- tests, scale/corruption work, and gate; and
- unresolved questions that forbid premature implementation.
