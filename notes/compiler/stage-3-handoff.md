# Stage 3 to Stage 4 Handoff

Status: **Complete for the Java 26.2 vertical slice**

Stage 3 owns a verified, immutable, serialization-near Minecraft program. Stage 4
must decide *which* legal target program to build; it does not need to change Stage
3 serialization.

## Proven public handoff shapes

`crates/mdl-compiler/tests/stage3_handoff.rs` constructs and golden-tests, using only
public Stage 3 APIs:

1. a one-command conditional gate;
2. a stable dual-guard `if`/`unless` branch;
3. a return-based single-evaluation dispatcher;
4. an ordinary call followed by resumed caller work; and
5. a `return run function` tail call.

The artifact contains only the functions explicitly declared by the fixture. The
emitter creates no helpers and selects no branch or call policy.

## Stage 4 ownership

Stage 4 remains responsible for:

- choosing target function boundaries and stable declaration order;
- selecting conditional-gate, proven dual-guard, snapshotted-Boolean, or
  return-dispatcher lowering;
- proving when a condition is stable across an arm;
- assigning physical scoreboard and storage homes;
- allocating deterministic objective, holder, storage, function, and tag names;
- generating initialization and choosing load/tick entry points;
- choosing ordinary versus tail calls and the result/success convention; and
- retaining source origins while constructing nested target commands.

These choices are semantic lowering and layout. None belongs in datapack emission.

## Frozen Stage 3 boundary

The handoff audit found no missing typed operation for the first Stage 4 vertical
slice (`Int`, `Bool`, constants, scoreboard state, comparisons, calls, branches,
returns, and raw barriers). New language features may require new closed target
variants later, but they must be added from a concrete lowering need.

Stage 3 deliberately contains no Core values, blocks, or terminators; generated-name
allocator; mutable final-IR editor; scheduler; macro ABI; branch optimizer; selector
filter framework; or filesystem writer. Pending builder state cannot enter
`MinecraftProgram`. `UnsafeRawCommand` remains the explicit unknown semantic barrier.

## Conformance evidence

The ignored `mdl-test/tests/minecraft_ir.rs` test directly constructs Stage 3 IR,
emits it twice, validates trace-to-physical-line correspondence, installs the generic
artifact before startup, and executes it on the official Minecraft Java 26.2 server
under Java 25. It covers every initial structured command family, load/tick and
nested/optional tags, exact scoreboard/storage outcomes, return result versus
success, empty functions, raw execution, and the 2,000,000-UTF-16-unit function-line
boundary.

The successful pinned inputs used to close Stage 3 were:

```text
Minecraft server: official 26.2 bundler/server JAR
Java: OpenJDK 25.0.3
Data-pack format: [107, 1]
```
