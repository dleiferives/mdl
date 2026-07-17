# Stage 7.5: Execution Frames and Static Spatial Semantics

Status: **design frozen for implementation planning**

Stage 7 proved one complete typed Minecraft command path. Stage 7.5 deliberately
broadens the semantic pressure on that path before Stage 8 chooses runtime value
representations and calling conventions. It is not a general command library and it
does not make coordinates or entities into ordinary storable values.

The implementation checklist is [`stage-7-5-todo.md`](stage-7-5-todo.md).

## Exit boundary

The representative program is:

```mdl
export fn move_worker() {
    run
        .as(mc.entities(ArmorStand).with_tag("worker").limit(1))
        .at_executor()
        .positioned(~10, ~, ~)
        .rotated(~90, ~)
        .in(mc.dimension.overworld)
        .anchored(mc.anchor.eyes)
        .align(mc.axes.xz)
        |worker|
    {
        worker.say("moving");
        worker.teleport(~, ~1, ~);
        worker.move_by(0, 2, 0);
    }
}
```

The spelling is provisional. The semantic distinctions are not:

- every modifier is preserved and applied in exact source order;
- `as` changes the executor but does not move the frame;
- `at` changes dimension, position, and rotation but not the executor;
- `at_executor` requires the current typed executor and is the exact-one `at @s`
  form;
- relative and local coordinates use the immediately preceding execution frame;
- `teleport` uses that current frame even when its receiver is elsewhere;
- `move_by` is explicitly receiver-relative and establishes the receiver's frame in
  its selected recipe; and
- neither teleporting an entity nor moving it mutates the execution frame inherited
  by later statements in the outlined function.

## Why this is Stage 7.5 rather than Stage 8

The new values are immutable compiler-known attributes. They are analogous to
immediate operands or operation attributes, not SSA runtime operands. They cannot be
bound to locals, passed to ordinary functions, returned, stored in a scoreboard/NBT,
or constructed from runtime arithmetic.

This follows a useful common compiler separation:

- MLIR distinguishes runtime operands from compile-time attributes/properties and
  requires full conversion to leave no illegal operation behind;
- Cranelift distinguishes immediate operand types from ordinary SSA value types;
- rustc's typed high-level IR makes method normalization and implicit adjustments
  explicit before lower IR construction;
- Zig's AIR is analyzed per-function IR consumed by code generation rather than a
  copy of source syntax; and
- GHC roles warn that nominal semantic identity and shared runtime representation
  are separate facts.

Stage 8 begins when one of these semantic values must cross a function/run boundary
as runtime data or choose among physical carriers.

## Frozen Stage 7.5 scope

### Run modifiers

Stage 7.5 supports this closed source set:

| Modifier | Operand | Context action | Multiplicity |
| --- | --- | --- | --- |
| `.as(query)` | `EntityQuery<T, C>` | replace executor with `T` | query cardinality |
| `.at(query)` | `EntityQuery<T, C>` | replace dimension, position, rotation | query cardinality |
| `.at_executor()` | current `Executor<T>` proof | replace dimension, position, rotation from `@s` | exactly once |
| `.positioned(position)` | static `PositionSpec` | replace position | exactly once |
| `.rotated(rotation)` | static `RotationSpec` | replace rotation | exactly once |
| `.in(dimension)` | static `DimensionKey` | replace dimension and rescale position | exactly once |
| `.anchored(anchor)` | static `EntityAnchor` | replace anchor | exactly once |
| `.align(axes)` | nonempty static `Axes` | floor selected position axes | exactly once |

`.at(query)` does not create a capture. A final capture always denotes the current
executor, so in `run.as(a).at(b) |executor|`, `executor` is selected by `a`, not `b`.
`at_executor` is rejected unless the exact current prefix proves an entity executor.

The following Java 26.2 branches remain deferred: `facing`, `positioned as`,
`positioned over`, `rotated as`, `on`, `summon`, `if`, `unless`, and `store`. They
have additional cardinality, derived-context, world-effect, or nested-outcome
contracts and should not be smuggled into a context-only tranche.

### Static spatial attributes

The semantic vocabulary is closed and target-independent:

```text
FiniteDecimal

WorldAxis = Absolute(FiniteDecimal) | Relative(FiniteDecimal)
WorldPosition = { x: WorldAxis, y: WorldAxis, z: WorldAxis }
LocalPosition = { left: FiniteDecimal, up: FiniteDecimal, forward: FiniteDecimal }
PositionSpec = World(WorldPosition) | Local(LocalPosition)

RotationAxis = Absolute(FiniteDecimal) | Relative(FiniteDecimal)
RotationSpec = { yaw: RotationAxis, pitch: RotationAxis }

RelativeWorldOffset = { x: FiniteDecimal, y: FiniteDecimal, z: FiniteDecimal }
DimensionKey
EntityAnchor = Feet | Eyes
Axes = nonempty subset of { X, Y, Z }
```

`FiniteDecimal` is an exact bounded decimal literal, not an `f64` and not a runtime
string. Stage 7.5 accepts a deliberately small grammar: optional sign, required
integer digits, and an optional fractional part. Exponents, NaN, infinities, and
runtime interpolation are rejected. The representation owns normalized numeric
components while provenance retains the original lexeme.

Target preflight converts the exact semantic decimal into a Java-26.2-validated
command numeric atom and proves the rendered value round-trips through the selected
parser. This avoids making host floating-point formatting part of source semantics.

Bare `~` and `^` mean zero. Absolute and relative world axes may mix. Any local `^`
component requires all three components to be local; mixing `^` with absolute or `~`
is invalid. Rotation permits absolute and relative components but never `^`.

`DimensionKey` internally supports a validated namespaced resource identity. The
first source surface exposes the three built-in constants; arbitrary resource
construction remains deferred until resource literals have a package-wide design.
`Axes` is stored as a nonempty bitset and rendered in canonical `xyz` order.

Block positions remain distinct and deferred. Stage 7.5 implements the Java
`minecraft:vec3` family only; it does not pretend that block coordinates and
continuous positions are interchangeable.

### First spatial operations

Two methods intentionally have different frame meaning:

```mdl
executor.teleport(~, ~10, ~); // destination relative to current execution frame
executor.move_by(0, 10, 0);   // destination relative to executor's own position
```

They normalize to separate semantic keys:

```text
TeleportCurrentExecutor(PositionSpec)
MoveCurrentExecutorBy(RelativeWorldOffset)
```

Both require the exact lexical current-executor proof, return source `Void`, perform
a world write, do not fork, and have finite work. The generic Core operation remains
`Unknown/Never/Opaque` to generic optimizers. Their native success/result behavior is
measured and retained separately from source `Void` and local command continuation.

`TeleportCurrentExecutor` selects structured `teleport @s <position>`. Its ambient
requirements depend on its attribute instance:

- absolute world coordinates require executor and dimension;
- relative world coordinates additionally require current position; and
- local coordinates additionally require rotation and anchor.

`MoveCurrentExecutorBy` requires only the typed executor from its caller. Its Java
recipe establishes `at @s` internally before `teleport @s ~x ~y ~z`, so incoming
position/dimension/rotation are not part of the source operation's requirement.

## Ordered execution-frame model

The existing five-component frame remains the semantic state:

```text
ExecutionContext {
  executor,
  position,
  rotation,
  dimension,
  anchor,
}
```

Non-executor components continue to store `Inherited`, `Unavailable`, or
`Established { by: exact modifier step }`. The value is owned by the modifier; the
context stores exact provenance rather than a recursively copied transformation
tree. Stage 7.5 performs no coordinate-folding optimization, so this is sufficient
and avoids quadratic nested state.

Each modifier instance exposes one independently verifiable transfer contract:

| Instance | Reads before the step | Writes for its body |
| --- | --- | --- |
| `as(query)` | query-defined position + dimension | executor |
| `at(query)` | query-defined position + dimension | position + rotation + dimension |
| `at_executor` | executor | position + rotation + dimension |
| positioned absolute | none | position |
| positioned with `~` | position | position |
| positioned with `^` | position + rotation + anchor | position |
| rotated absolute | none | rotation |
| rotated with `~` | rotation | rotation |
| `in` | position + dimension | position + dimension |
| `anchored` | none | anchor |
| `align` | position | position |

Requirement transfer runs in reverse: discharge downstream requirements satisfied
by the step's writes, then join the step's reads. HIR and Core independently replay
this algorithm and compare their entry requirements before optimization. Optimized
Core requirements remain the generated contract used by lowering.

`in` must be modeled as both a dimension and position transform because Java rescales
and clamps the current position between dimensions. Its order relative to
`positioned` is therefore observable and must be measured on the pinned server.

Effects remain separate from context transfer. Resolving `as(query)` or `at(query)`
is a world read with query-owned work/fork bounds. `at_executor` reads the current
entity's frame but never forks. Literal positioned/rotated/in/anchored/align steps
have no ordinary world-state effect even though they transform the nested execution
context. Teleport and move-by are world writes; move-by also reads the receiver frame.
None of these modifiers produces output merely by transforming context.

## IR and lowering architecture

### Source and HIR

- Add contextual coordinate tokens/AST nodes without making `~` or `^` general
  arithmetic operators.
- Preserve component, tuple, method, and modifier origins independently.
- Extend the compiler-owned run-modifier and Minecraft-method registries with closed
  rules; do not scatter string matches across the checker.
- Store typed static attributes and exact executor proofs in HIR.
- HIR verification replays modifier order, context proofs, literal validity,
  capture identity, instance behavior, and dense occurrence inventories.

### Core

- Static coordinates/modifier arguments are program-owned declarations/attributes,
  never `CoreType`, SSA operands, or scoreboard homes.
- Extend `RunModifierInstance` and `CoreExecutionContext` with the selected closed
  variants and exact origins.
- Extend `MinecraftOperationAttributes` for teleport and move-by while erasing the
  source-only lexical executor proof.
- Update Core builders, printers, verifiers, declaration cloning/editing, ambient
  analysis, function-reference consumers, and every exhaustive optimizer match even
  where the correct behavior is to remain opaque.
- Derive operation behavior from the verified operation instance, not just its key:
  coordinate form changes ambient requirements.
- Keep generic Core optimization conservative until a later pass understands these
  semantic operations explicitly.

### Target preflight and structured Minecraft IR

Stage 7.5 moves all source run-modifier target selection into `TargetPreflight`.
Preflight retains an ordered selected recipe for every reachable Core run-scope
modifier, including the existing `as` recipe. Construction consumes selected target
arguments; it does not parse decimals, convert queries, or rediscover recipes.

Add closed recipe identities for the selected Java 26.2 modifiers and operations.
Recipe projections reconcile:

- semantic instance reads/writes/effects/forks/work/outcome;
- selected target argument representation;
- structured target command/modifier contract;
- local cost and maximum chain expansion; and
- exact provenance and physical placement.

Add structured target types for coordinate atoms, positions, rotations, anchors,
axes, and teleport. Extend every verifier, renderer, syntax classifier, effect/context
census, cost analysis, global solver, debug dump, and corruption fixture. Add an
`ENTITY_WRITE` target effect category rather than treating teleport as an unknown
world mutation.

`TeleportCurrentExecutor` lowers directly in the containing function.
`MoveCurrentExecutorBy` lowers to one structured nested execute command, not raw text
and not an isolation helper.

### Correlation and reports

Retain exact modifier correlation:

```text
SourceRunId + modifier index
  -> Core RunScopeId + modifier index
  -> selected Java26_2 modifier recipe
  -> target McFunctionId + CommandId + execute-modifier index
```

The existing semantic-operation correlation extends naturally to teleport and
move-by. Reports distinguish source semantic requirements, optimized generated-entry
requirements, modifier-local reads/writes, selected recipes, invocation bounds,
physical cost, and actual target placement. Preconstruction failures never claim
target command IDs.

## Implementation order

1. Pin Java 26.2 command-tree and server behavior evidence.
2. Add bounded exact decimal and spatial semantic attributes with no source syntax.
3. Add structured target spatial arguments, execute modifiers, and teleport.
4. Generalize semantic instance contracts and target preflight recipe retention.
5. Add source grammar/checking for static coordinate forms and modifier chains.
6. Extend HIR/Core context transfer and independent ambient differential proof.
7. Add teleport and move-by source methods through the closed registry.
8. Complete construction reconciliation, mappings, reports, and support matrices.
9. Run four-policy differentials, corruption/scale suites, and one clientless
   Java 26.2 conformance lifecycle.

This order gives target atoms and contract evidence somewhere verified to land before
the source surface can produce them. No phase accepts an operation that a later phase
cannot fully legalize.

## Real-server proof

One force-loaded, clientless Java 26.2 lifecycle should distinguish:

1. `as` without `at_executor` from `as(...).at_executor()`;
2. two sequential relative `positioned` steps from one step;
3. modifier order across `positioned` and `in`;
4. absolute, mixed-relative, and local positions;
5. rotation and feet/eyes anchors for local coordinates;
6. `at(query)` changing the frame without changing the captured executor;
7. frame-relative `teleport` from receiver-relative `move_by`;
8. teleporting the executor without mutating the frame seen by the next statement;
9. empty, at-most-one, and rejected-many modifier prefixes; and
10. native teleport success/result behavior separately from source `Void`.

The official generated `commands.json` gate asserts the selected `execute` branches,
argument parser kinds, redirects, selector multiplicity, and `teleport` tree. Syntax
evidence does not substitute for behavioral server measurements.

## Resource and correctness gates

- Bound decimal bytes/digits, modifier count per scope, package-wide modifier count,
  and selected-recipe slots before allocation.
- Make all source/HIR/Core transfers iterative over modifier vectors.
- Add 20,000-step analysis/verification tests without constructing an overlong target
  command; target preflight separately diagnoses command-length overflow.
- Test every new exhaustive match and mutation: wrong variant/key, mixed local/world
  axes, empty axes, stale executor proof, wrong reads/writes, reordered recipes,
  target-argument drift, cost drift, placement drift, and detached correlations.
- Preserve deterministic IDs, diagnostics, reports, target IR, packs, and traces over
  repeated builds and package input permutations.

## Explicitly deferred

- first-class `Position`, `Rotation`, `Dimension`, `EntityRef`, or query values;
- dynamic coordinate arithmetic or runtime interpolation;
- arbitrary resource literals and custom-dimension source construction;
- block positions, regions, facing, heightmaps, and entity-relative rotation forms;
- `execute on`, `summon`, conditions, stores, and command-result blocks;
- teleport destinations that are entity references, optional facing/rotation forms,
  or possibly-many receiver methods;
- fork-safe/reentrant call frames, scheduling, macros, and multi-version recipes; and
- coordinate/frame optimization beyond preserving exact order.

## References

- [Stage 7 to Stage 8 handoff](stage-7-handoff.md)
- [Execution context and coordinate research](../mcfunction/coordinate-frames.md)
- [Typed execute-chain inventory](../mcfunction/execute-chains.md)
- [Minecraft Java 26.2](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Minecraft snapshot 18w02a: frame transforms, teleport, and local coordinates](https://feedback.minecraft.net/hc/en-us/articles/360004167991-Minecraft-Java-Edition-Snapshot-18W02A)
- [Minecraft Java 1.19.4: newer execute branches](https://feedback.minecraft.net/hc/en-us/articles/13987663727757-Minecraft-Java-Edition-1-19-4)
- [MLIR operation operands versus compile-time attributes/properties](https://mlir.llvm.org/docs/DefiningDialects/Operations/)
- [MLIR dialect conversion and full legality](https://mlir.llvm.org/docs/DialectConversion/)
- [MLIR side effects and instance-dependent speculation](https://mlir.llvm.org/docs/Rationale/SideEffectsAndSpeculation/)
- [Cranelift IR value and immediate operand types](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md)
- [rustc typed high-level IR](https://rustc-dev-guide.rust-lang.org/thir.html)
- [Zig analyzed IR](https://raw.githubusercontent.com/ziglang/zig/master/src/Air.zig)
- [OCaml compiler backend and representation boundary](https://ocaml.org/docs/compiler-backend)
- [GHC nominal versus representational roles](https://ghc.gitlab.haskell.org/ghc/doc/users_guide/exts/roles.html)
