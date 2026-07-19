# PS-1 to PS-2 Handoff

Status: **PS-1 complete; PS-2 is ready to begin at PS-2.0**

PS-1 now supplies three deliberately separate authorities: bounded execution of
target-independent Core, typed compiler/target structure, and state-based execution
on the pinned vanilla server. PS-2 can add vertical language capabilities without
inventing another test harness.

## Delivered Core oracle

`ir::core::eval` executes verified Core with explicit step and call-depth budgets.
The supported closed subset includes scalar constants and pure scalar operations,
block parameters, jumps, branches, return/unreachable, ordinary calls, multiple
scalar results, and direct/mutual recursion. Unsupported Minecraft/external
operations and exhausted budgets return owned structured outcomes rather than
panicking or pretending to be MDL values.

PS-2 extends this evaluator when it freezes arithmetic, cyclic loop, aggregate,
list, or string semantics. It must not teach the evaluator Minecraft command
behavior.

## Delivered vanilla scenario surface

The typed runner provides:

- semantic policy differentials across all four Core/Minecraft `None|Baseline`
  products, including up to 64 deterministic caller-ordered cases per server
  lifecycle;
- exact sequence/fork-limit roots without semantic observation wrappers;
- explicit executor, dimension, position, rotation, and anchor invocation frames;
- test-owned objectives, storage projections, entity tags, and forced chunks;
- score and bounded exact-type SNBT initial state;
- score, storage NBT, and cardinality-only entity set/multiset observations;
- independent optional success/result channels and continuation/completion state;
- condition-polled entity readiness across scheduled roots;
- bounded observations, NBT input/depth/node count, entity count, differences, and
  in-memory attributed logs; and
- generated compiler artifacts, complete datapacks, `harness.log`, and a normalized
  `scenario-failure.txt` in every preserved failure sandbox.

The complete server log is streamed to disk. In-memory checkpoint matching retains
the latest 16,384 attributed lines and fails explicitly if a checkpoint expires.

## Deliberately unsupported runner surface

PS-1 does not provide a Minecraft emulator or arbitrary scenario command language.
The declarative runner does not yet initialize/observe blocks, project entity NBT
fields, normalize public message effects, own bossbars/gamerules, or automate a
connected player. Add one of these only with the PS-2 capability that needs it.

Storage ownership is path-scoped in practice: Minecraft requires a path for `data
remove storage`, so reset/cleanup clears every storage path declared by the
scenario's initial or expected projection. A storage resource with no declared path
is rejected rather than issuing invalid vanilla syntax.

## Calibrated evidence

The pinned Java 26.2 gates establish:

- Core-evaluated scalar call/branch behavior agrees across four emitted policies;
- exact-type storage NBT round-trips through bounded SNBT parsing;
- zero/one/many and nested many-context execution preserve expected multiplicity;
- no-result/no-context store callbacks are absent, failure writes zero, successful
  zero and `return 0` report success one/result zero, and nonzero return is distinct;
- exact sequence interruption prevents the measured terminal completion write;
- a redirect whose output size equals the configured fork limit executes no body
  while its containing function continues;
- incoming execution frames and executor-relative spatial frames remain distinct;
- direct/mutual recursion balances compiler-private frames in normal execution; and
- hard-limit abort may leave frame residue, while the explicit load entry recovers.

The reusable vanilla facts discovered while closing PS-1 are recorded in
[`../../mcfunction/command-outcomes.md`](../../mcfunction/command-outcomes.md) and
[`../../mcfunction/entity-activation.md`](../../mcfunction/entity-activation.md).

## PS-2 working rule

For each PS-2 capability, use the narrowest applicable evidence:

1. freeze semantics and diagnostics;
2. add typed HIR/Core plus verifier/printer/editor integration;
3. extend Core evaluation for target-independent behavior;
4. prove `None|Baseline` semantic equivalence;
5. add physical/lowering evidence without snapshotting incidental pack layout;
6. add a typed vanilla scenario only for Minecraft-defined behavior; and
7. add a bare exact-limit case when legality depends on a hard boundary.

The default workspace suite remains clientless and Java-free because every server
gate is ignored unless explicitly selected.

## First PS-2 action

Begin with PS-2.0, not implementation of syntax. Freeze the Brainfuck dialect,
program/fuel bounds, connected-player requirement, and standard-library/intrinsic
ownership decisions in
[`ps-2-capability-expansion-todo.md`](ps-2-capability-expansion-todo.md). Once those
decisions are closed, the first implementation tranche is scalar arithmetic,
mutation, and same-tick loop control in PS-2A.
