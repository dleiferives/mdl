# Compiler Design Notes

- [Stage 0 foundational decisions](stage-0-decisions.md)
- [Stage 1 server harness](stage-1-server-harness.md)
- [Stage 2 typed SSA implementation plan](stage-2-ssa-plan.md)
- [Stage 2 implementation record](stage-2-ssa-implementation.md)
- [Stage 3 structured Minecraft IR and emission plan](stage-3-minecraft-ir-plan.md)
- [Stage 3 implementation checklist](stage-3-todo.md)
- [Stage 3 to Stage 4 handoff](stage-3-handoff.md)
- [Stage 4 first Core-to-Minecraft lowering plan](stage-4-first-lowering-plan.md)
- [Stage 4 implementation checklist](stage-4-todo.md)
- [Stage 5 baseline optimization and cost plan](stage-5-baseline-optimization-plan.md)
- [Stage 5 implementation checklist](stage-5-todo.md)
- [Stage 5 Core pass design plans](stage-5-passes/README.md)
- [Stage 5 to Stage 6 handoff](stage-5-handoff.md)
- [Stage 6 minimal typed frontend plan](stage-6-minimal-frontend-plan.md)
- [Stage 6 implementation checklist](stage-6-todo.md)
- [Stage 7A whole-package module and import plan](stage-6-modules-plan.md)
- [Stage 7B typed Minecraft effect and unsafe raw-command plan](stage-6-effects-plan.md)
- [Compiler semantic ambiguities and unknowns](semantic-ambiguities.md)
- [Rough implementation stages](implementation-stages.md)
- [mcfunction backend research](../mcfunction/README.md)

Stages 0 through 4 are complete for the first vertical slice. Verified Core now
lowers through a deterministic physical plan into the structured Minecraft IR,
emits an exact datapack and trace, and passes the official Java 26.2 server
conformance gate. Stage 5A's exact artifact footprint, structured execution-cost
analysis, command-limit assumptions, vanilla boundary fixture, and scale gates are
complete. The owned Core optimizer, atomic editor substrate, canonicalization, SCCP,
DCE, straight-line fusion, local CSE, closed reporting pipeline, generated semantic
differentials, and fast lowered-target differential are complete through Stage 5D.
Stage 5E's corrected runtime-demand and immutable physical-planning tranche is
complete and gated: the final plan is independently checked structurally and by
sparse symbolic home-content dataflow, generated Core/target differentials cover
both physical policies, and both policies pass the official Java 26.2 server. Stage
5F's sparse liveness and conservative coalescing tranche is also complete and gated:
bounded all-or-nothing analyses, exact access contracts, sparse scale fixtures,
independent home-content verification, emitted-command differentials, and the updated
official-server regressions pass. Stage 5G's closed target-recipe accounting,
explicit block placement, placement-aware resources, independent verifier,
constructed-command reconciliation, exact decision reporting, and official-server
`None`/`Baseline` differential are complete and gated. Stage 5H completes public
report ownership, the raw measurement protocol and private timing hooks, generated
reproduction context, the final corruption audit, the optimized-Core one-startup
vanilla proof, and the Stage 6 handoff. Stage 5 is complete. Stage 6 is also complete:
the bounded scalar lexer/parser, typed HIR, sparse flow checking and SSA lowering,
owned compilation facade, deterministic four-policy source differential, minimal
`mdl` CLI, and both source/CLI Java 26.2 server proofs are gated. Stage 6I fixes the
next ownership boundaries: whole-package modules/exports are Stage 7A, typed
Minecraft operations and static unsafe effects Stage 7B, representation bridging
Stage 8, scheduling consumption Stage 9, and runtime interpolation Stage 10.
