# Stage 7.5 to Stage 8 Handoff

Status: **Stage 7.5 complete and gated**

Stage 7.5 proves ordered Minecraft execution frames and exact compiler-known spatial
arguments end to end. It deliberately does not choose runtime carriers for positions
or persistent entity identities; those are now concrete Stage 8 design pressures
rather than speculative requirements.

## Completed source slice

```mdl
export fn move_worker() {
    run
        .as(mc.entities(ArmorStand).with_tag("worker").limit(1))
        .at_executor()
        .positioned(~10, ~, ~)
        .rotated(~90, ~)
        .in(mc.dimension.overworld)
        .anchored(mc.anchor.eyes)
        .align(mc.axes.xz) |worker| {
            worker.say("moving");
            worker.teleport(~, ~1, ~);
            worker.move_by(0, 2, 0);
        }
}
```

The selected modifier set is `as`, `at`, `at_executor`, `positioned`, `rotated`,
`in`, `anchored`, and `align`. Coordinates are exact bounded decimal attributes;
absolute and `~` world axes may mix, while a position containing `^` must be entirely
local. These attributes cannot be assigned, passed, returned, interpolated, or stored
as Core SSA values.

`Executor.teleport(position)` interprets relative/local coordinates in the current
execution frame. `Executor.move_by(dx, dy, dz)` is receiver-relative and therefore
lowers through `execute at @s run teleport @s ~dx ~dy ~dz`. A teleport command changes
the target entity but does not mutate the execution frame inherited by later
commands.

## Preserved compiler contracts

- HIR and Core independently replay modifier order and infer ambient frame
  requirements. Position, rotation, dimension, anchor, and executor facts retain
  exact modifier-step provenance.
- Target preflight owns a dense, verified Java 26.2 recipe for every modifier and
  semantic operation before construction. Construction consumes structured selected
  arguments and never reparses source spellings.
- Every source modifier maps through `SourceRunId` and `RunScopeId` to its actual
  target function, command, modifier index, and `RunModifierRecipeId`.
- Teleport and move-by use closed semantic keys and structured target commands; no
  raw Minecraft command is introduced by the typed path.
- Decimal, per-scope modifier, package modifier, target command-length, and target
  parser bounds are explicit. The same modifier budgets guard source checking, Core
  construction, and target preflight.
- All four Core/Minecraft optimization-policy combinations emit deterministic,
  semantically equivalent packs.

Public inspection uses `CheckedFrontendOutput::run_scope_ids`,
`SourceToCoreMap::run_scope`, `CompilationOutput::source_run_modifier`, and
`LoweringMap::run_modifier`. Callers do not need to parse dumps or infer allocation
order.

## Verification evidence

Fast workspace tests, warnings-denied Clippy and rustdoc, Rust 1.85 MSRV checking,
formatting, and diff hygiene form the normal gate. The opt-in tests use the official
Java 26.2 bundle without a client:

- `official_command_report` regenerates and audits Mojang's Brigadier command tree;
- `spatial_command_semantics` measures each selected frame transform, modifier order,
  local rotation and anchors, cross-dimension behavior, and teleport outcomes; and
- `stage75_server` compiles the representative source under all four policies and
  verifies frame-relative teleport, receiver-relative movement, and frame
  immutability in one server lifecycle.

The server bundle and extracted server hashes remain recorded in
[`../../versions/README.md`](../../versions/README.md). Test commands and environment
variables are documented in [`../../crates/mdl-test/README.md`](../../crates/mdl-test/README.md).

## Stage 8 representation pressure

Stage 8 and its follow-on representation clients should address these concrete
missing capabilities in dependency order:

1. runtime positions and rotations crossing ordinary calls and outlined run scopes;
2. exact-one entity references that survive context changes without becoming an
   implicit fork;
3. ownership and lifetime of scoreboard/NBT homes under recursion, reentrancy, and
   possibly-many execution; and
4. explicit conversions between compile-time attributes, scoreboard values, NBT
   values, and active execution context.

Do not generalize static coordinates into a universal value carrier by accident.
The revised Stage 8 first proves scalar realizations, serial many-context calls, and
recursive activation frames. Runtime positions, persistent entity references, and
aggregates become separately planned clients of that verified foundation. Conditions
and stores remain coupled to future outcome/value representation. Scheduling remains
Stage 9, macros/interpolation Stage 10, aggressive frame folding Stage 11, and
multi-version/stable package ABI Stage 12.

## References

- [Stage 7.5 design](stage-7-5-plan.md)
- [Stage 7.5 completed checklist](stage-7-5-todo.md)
- [Coordinate-frame research](../mcfunction/coordinate-frames.md)
- [Typed execute-chain research](../mcfunction/execute-chains.md)
- [Semantic ambiguity ledger](semantic-ambiguities.md)
- [Testing harness](testing-harness.md)
