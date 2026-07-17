# Execution Context and Coordinate Frames

Status: **Stage 7.5 implemented and measured on Java 26.2**

Implementation plan: [`../compiler/stage-7-5-plan.md`](../compiler/stage-7-5-plan.md).

## Question

How should MDL represent absolute, relative (`~`), and local (`^`) coordinates while
preserving the ordered execution-frame changes made by Minecraft `execute`?

## Target behavior

These commands are not equivalent:

```mcfunction
execute as @e run tp @s ~ ~10 ~
execute as @e at @s run tp @s ~ ~10 ~
```

`as @e` changes only the executor. In the first command each `@s` is a selected
entity, but `~ ~10 ~` is relative to the incoming command position. `at @s` in the
second command additionally changes dimension, position, and rotation to the current
executor, so each entity moves ten blocks above itself.

The correct Java target order is `tp <targets> <location>`:

```mcfunction
execute as @e at @s run tp @s ~ ~10 ~
```

Position transforms are sequential. For example:

```mcfunction
execute positioned ~10 ~ ~ run tp @a ~ ~ ~
```

first moves the execution position ten blocks along X from the incoming frame, then
teleports every selected player to that one transformed position. It does not move
each player ten blocks from their individual positions.

## Selected typed representation

Coordinates are structured semantic values, never pre-rendered command strings:

```text
WorldAxis = Absolute(FiniteNumber) | Relative(FiniteNumber)

WorldPosition = {
  x: WorldAxis,
  y: WorldAxis,
  z: WorldAxis,
}

LocalPosition = {
  left: FiniteNumber,
  up: FiniteNumber,
  forward: FiniteNumber,
}

PositionSpec = World(WorldPosition) | Local(LocalPosition)
```

The provisional source literals are deliberately Minecraft-readable:

```text
.positioned(10, 64, 10)       // all absolute
.positioned(~10, ~, ~)        // relative X; zero-relative Y/Z
.positioned(~10, 64, 10)      // relative X; absolute Y/Z
.positioned(^, ^, ^10)        // local forward movement
```

Bare `~` means `~0`; bare `^` means `^0`. Absolute and `~` components may be mixed
axis by axis. A `^` local vector may not be mixed with absolute or `~` components,
matching the target coordinate parser. These tokens are coordinate constructors in
coordinate-typed positions, not ordinary numeric unary operators.

The implemented components are compile-time finite literals. Dynamic values entering
command syntax require Stage 10's typed Minecraft macro/serialization design or a
different proven lowering; Stage 7.5 does not concatenate runtime numbers into text.

Block positions and continuous positions remain distinct typed constructors because
Minecraft's `block_pos` and `vec3` parsers have different domains and normalization
behavior.

## Ordered frame transformation

A contextual run statement transforms an abstract `ExecutionFrame` in exact source
order:

```text
ExecutionFrame {
  executor
  dimension
  position
  rotation
  anchor
}
```

The context analysis retains provenance needed by later operands, not merely a bit
saying that a component exists. Relevant examples include:

```text
position = Incoming
position = AtExecutor(executor_capture)
position = AtSelectedEntity(query)
position = Positioned(previous_position, position_spec)
position = Absolute(dimension, position)
```

The exact Rust representation may use closed enums and IDs rather than a recursive
tree, but it must answer whether a relative/local operand has the frame components it
requires.

Key transitions are:

- `.as(query)` changes executor and multiplicity, but not position;
- `.at(query)` changes dimension, position, and rotation to each selected entity,
  but does not change executor;
- `.at_executor()` is typed source sugar for `at @s` and records that the resulting
  frame corresponds to the captured executor;
- `.positioned(spec)` interprets `spec` in the immediately preceding frame and
  produces the frame used by later modifiers and commands;
- `.rotated(...)`, `.anchored(...)`, and `.in(...)` update their documented frame
  components in order; and
- local `^` coordinates require suitable position, rotation, and anchor context.

Consequently:

```text
run.positioned(~10, ~, ~).positioned(~10, ~, ~) { ... }
```

is twenty blocks along X from the incoming frame. Optimizers may combine those
steps only after proving exact coordinate equivalence; Stage 7 simply preserves the
ordered transforms.

## Source examples

Self-relative movement is explicit and unambiguous:

```text
run.as(entities).at_executor() |entity| {
    entity.teleport(~, ~10, ~);
}
```

Moving a captured executor to one transformed caller position is different:

```text
run.as(entities).positioned(~10, ~, ~) |worker| {
    worker.teleport(~, ~, ~);
}
```

The implemented receiver-relative spelling is `entity.move_by(0, 10, 0)`. A future
query-wide `players.teleport_to_frame_origin()` would be a distinct possibly-many
operation and is not implied by this slice.

## Direct entity methods

A relative coordinate passed directly to an entity method outside an explicit run
scope still uses the current execution frame:

```text
player.teleport(~, ~10, ~)
```

Minecraft always has an incoming command position even when there is no entity
executor, so “fall back to the entity when there is no frame” has no natural target
boundary. MDL therefore never gives `~` a receiver-dependent alternate meaning.
Receiver-relative movement is expressed as `player.move_by(...)` or by first making
the frame explicit with `run.as(player).at_executor()`. This keeps source spelling,
HIR frame provenance, and emitted command semantics aligned.

## Measured tests

1. Distinct incoming and entity positions distinguish incoming-frame movement from
   self-relative movement.
2. Mixed absolute/relative axes match the server parser and target values.
3. Two sequential `positioned` modifiers use the output frame of the previous one.
4. Local coordinates respond correctly to rotation and anchor changes.
5. `as`, `at`, and `at_executor` preserve/change exactly the documented components.
6. Cross-dimension `at`/`in`/teleport behavior is pinned on the official server.
7. Empty, at-most-one, and many entity queries preserve skip/fork behavior.

The handwritten probes live in
[`../../crates/mdl-test/tests/spatial_command_semantics.rs`](../../crates/mdl-test/tests/spatial_command_semantics.rs).
The compiler-generated four-policy proof lives in
[`../../crates/mdl-test/tests/stage75_server.rs`](../../crates/mdl-test/tests/stage75_server.rs).

## Primary references

- Generated `commands.json` from the official Minecraft Java 26.2 server JAR
- [Minecraft snapshot 18w02a: `as`, `at`, positioned, rotated, dimensions, anchors, and local coordinates](https://feedback.minecraft.net/hc/en-us/articles/360004167991-Minecraft-Java-Edition-Snapshot-18W02A)
- [Typed execute-chain inventory](execute-chains.md)
