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
- using the mandatory single-evaluation return dispatcher for its correctness
  baseline;
- assigning physical scoreboard and storage homes;
- allocating deterministic objective, holder, storage, function, and tag names;
- generating initialization and choosing load/tick entry points;
- choosing ordinary versus tail calls and the result/success convention; and
- retaining source origins while constructing nested target commands.

These choices are semantic lowering and layout. None belongs in datapack emission.
Snapshot, proven dual-guard, and other profitable branch selection starts in Stage
5 after the baseline exists; Stage 4 does not expose a strategy option with one
implemented choice.

## Closed Stage 3 boundary and the completed concrete extension

The original handoff audit found no missing typed operation for ordinary Core
computation. The later Stage 4 initialization audit found one concrete target-level
gap: collision-safe objective creation needs internal `execute if function`, which
Stage 3 does not currently represent. Stage 4 task 4A.1 adds one closed internal
function condition. Stage 4A added `Condition::Function(McFunctionId)`, threaded
program-aware rendering and internal-reference verification through execute
modifiers, and pinned positive, zero, and failed `return run` behavior on vanilla
before construction began. Its command contract remains conservatively unknown like
an ordinary function call. This was a narrow extension from a demonstrated consumer,
not a general reopening of Stage 3.

Stage 4 now uses that condition only for collision-safe initialization. Its ordinary
branch baseline is the fixed two-command dispatcher: one guarded
`return run function` for the true edge followed by one unconditional false tail.
Edges with simultaneous block-argument moves route through preplanned helpers; empty
edges call their destination block directly.

Core still has no scoreboard-backed mutable state, selector/context operation, or raw
operation. Stage 3's score, storage, selector, and `UnsafeRawCommand` forms are target
vocabulary, not evidence that those features already exist at the semantic Core
level. Source mutable state and raw commands still require target-neutral
effect/import semantics; adding a Minecraft raw variant to Core would collapse the
layer boundary Stage 4 just proved.

Stage 3 deliberately contains no Core values, blocks, or terminators; generated-name
allocator; mutable final-IR editor; scheduler; macro ABI; branch optimizer; selector
filter framework; or filesystem writer. Pending builder state cannot enter
`MinecraftProgram`. `UnsafeRawCommand` remains the explicit unknown semantic barrier.

## Conformance evidence

The ignored `mdl-test/tests/minecraft_ir.rs` test directly constructs Stage 3 IR,
emits it twice, validates trace-to-physical-line correspondence, installs the generic
artifact before startup, and executes it on the official Minecraft Java 26.2 server
under Java 25. Stage 4A extended the same one-startup run instead of adding a second
server fixture. It covers every initial structured command family, load/tick and
nested/optional tags, exact scoreboard/storage outcomes, return result versus
success, the internal function condition, signed-boundary scoreboard behavior,
current command-limit gamerules, empty functions, raw execution, and the
2,000,000-UTF-16-unit function-line boundary.

The successful pinned inputs used to close Stage 3 were:

```text
Minecraft server: official 26.2 bundler/server JAR
Java: OpenJDK 25.0.3
Data-pack format: [107, 1]
```
