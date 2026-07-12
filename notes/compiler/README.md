# Compiler Design Notes

- [Stage 0 foundational decisions](stage-0-decisions.md)
- [Stage 1 server harness](stage-1-server-harness.md)
- [Stage 2 typed SSA implementation plan](stage-2-ssa-plan.md)
- [Stage 2 implementation record](stage-2-ssa-implementation.md)
- [Stage 3 structured Minecraft IR and emission plan](stage-3-minecraft-ir-plan.md)
- [Stage 3 implementation checklist](stage-3-todo.md)
- [Stage 3 to Stage 4 handoff](stage-3-handoff.md)
- [Rough implementation stages](implementation-stages.md)
- [mcfunction backend research](../mcfunction/README.md)

Stages 0 through 3 are complete for the first vertical slice. Stage 3's structured
Minecraft IR and deterministic emitter pass both the server-free proof corpus and
the official Java 26.2 server conformance gate. Stage 4 lowering and layout is next.
