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
- [Stage 7 typed Minecraft programming-model plan](stage-7-plan.md)
- [Stage 7 audited remainder implementation plan](stage-7-remainder-plan.md)
- [Stage 7 implementation checklist](stage-7-todo.md)
- [Stage 7 to Stage 8 handoff](stage-7-handoff.md)
- [Stage 7.5 execution-frame and spatial-semantics plan](stage-7-5-plan.md)
- [Stage 7.5 implementation checklist](stage-7-5-todo.md)
- [Stage 7.5 to Stage 8 handoff](stage-7-5-handoff.md)
- [Stage 8 calling-convention and representation plan](stage-8-plan.md)
- [Stage 8 implementation checklist](stage-8-todo.md)
- [Stage 8 per-substep design dossiers](stage-8/README.md)
- [Stage 8 completion audit](stage-8-completion-audit.md)
- [Stage 8 handoff](stage-8-handoff.md)
- [Stage 8.5 pre-scheduler capability-validation index](pre-scheduler/README.md)
- [Stage 8.5 roadmap](pre-scheduler/roadmap.md)
- [PS-1 semantic and vanilla testing plan](pre-scheduler/ps-1-testing-plan.md)
- [PS-1 implementation checklist](pre-scheduler/ps-1-testing-todo.md)
- [PS-1 to PS-2 handoff](pre-scheduler/ps-1-handoff.md)
- [PS-2 Brainfuck-driven capability-expansion plan](pre-scheduler/ps-2-capability-expansion-plan.md)
- [PS-2.0 frozen semantics and ownership decisions](pre-scheduler/ps-2-0-decisions.md)
- [PS-2 implementation checklist](pre-scheduler/ps-2-capability-expansion-todo.md)
- [PS-2 language/standard-library/intrinsic/runtime boundary](pre-scheduler/ps-2/standard-library-boundary.md)
- [PS-3 Brainfuck capstone plan](pre-scheduler/ps-3-brainfuck-capstone-plan.md)
- [PS-3 implementation checklist](pre-scheduler/ps-3-brainfuck-capstone-todo.md)
- [PS-3 to Stage 9 handoff](pre-scheduler/ps-3-handoff.md)
- [Post-Stage-8 aggregate value client plan](aggregate-values-plan.md)
- [Historical Stage 7A module handoff](stage-6-modules-plan.md)
- [Historical Stage 7B effect/raw-command handoff](stage-6-effects-plan.md)
- [Compiler semantic ambiguities and unknowns](semantic-ambiguities.md)
- [Compiler test harness and fixture workflow](testing-harness.md)
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
`mdl` CLI, and both source/CLI Java 26.2 server proofs are gated. Stage 7 is complete:
the Zig-like rooted module graph, conservative external/raw boundary, typed static
entity queries and run scopes, retained fork deployment evidence, captured
`Executor.say`, independent HIR/Core context proofs, instance-aware Java 26.2 recipe,
structured target command, exact source-to-command correlation, and clientless
official-server differential are all verified. Stage 7.5 is also complete: ordered
frame modifiers, exact static spatial attributes, distinct frame-relative teleport
and receiver-relative movement, retained target recipes, exact cross-layer
correlation, and clientless Java 26.2 differentials are gated. Stage 8 is complete
for the bounded scalar-realization/calling-convention boundary: serial many-context
reuse, recursive-SCC-only typed spill frames, explicit recovery, physical recipe
accounting, determinism/corruption gates, and clientless Java 26.2 execution are
verified. Stage 8.5 is complete through PS-3. Its testing foundation, synchronous
language/value capabilities, and ordinary seven-module Brainfuck application all
pass. The 30-case complete-state reference differential runs under all four policy
products; the full 0..99-page typed book adapter runs the split-page `A` showcase on
pinned Java 26.2 under every policy, with wrong-item and malformed-book cases kept
distinct. Exact artifact/cost evidence is pinned and sequence-cycle uncertainty is
retained honestly. Persistent continuations and multi-tick scheduling are now Stage
9; general language metaprogramming and broader runtime interpolation remain later
work.
