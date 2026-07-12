# Automated Minecraft Datapack Testing

## Current target

As of 2026-07-11, the current stable Java Edition release is Minecraft 26.2 and the data-pack format is 107.1.

OpenJDK 25.0.3 is installed through Homebrew at:

```sh
/opt/homebrew/opt/openjdk@25/bin/java
```

The formula is keg-only, so `java` is not automatically added ahead of macOS's
system wrapper on `PATH`. Test commands can use the absolute path or export
`PATH="/opt/homebrew/opt/openjdk@25/bin:$PATH"` for the current shell.

Most datapack behavior can be tested using a headless Java dedicated server. A Minecraft client does not need to connect for functions, commands, scoreboards, command storage, macros, scheduling, blocks, entities, loot, predicates, or most other server-authoritative behavior.

## Mojang GameTest runner

The official server JAR contains a dedicated GameTest entry point:

```sh
java \
  -DbundlerMainClass=net.minecraft.gametest.Main \
  -jar server.jar \
  --packs build/test-packs \
  --tests 'mdl:*' \
  --report build/gametest-results.xml \
  --universe build/gametest-world
```

The runner creates a disposable test world, loads datapacks, runs the selected tests, writes a JUnit-like XML report, and exits automatically. Minecraft 26.2 also exposes `--repeatCount` and `--verify` options.

Vanilla datapack-defined GameTests are primarily block-based. Test structures can contain start, accept, and fail test blocks along with command blocks or redstone that invoke generated functions and assert world state. Direct programmatic `GameTestHelper` assertions require Java test functions registered by a mod, so they are optional rather than a requirement for the initial compiler test system.

Official reference: <https://www.minecraft.net/en-us/article/minecraft-snapshot-25w03a>

## Ordinary headless server tests

The compiler can also be tested against an ordinary disposable server:

1. Compile MDL into a datapack.
2. Install the generated pack into a fresh test world's `datapacks` directory.
3. Start the dedicated server with `java -jar server.jar --nogui`.
4. Wait for successful startup and datapack loading.
5. Send commands through standard input, RCON, or the server-management protocol.
6. Invoke generated functions and macros.
7. Query function results, scoreboards, command storage, blocks, entities, or NBT.
8. Compare the observed state with the expected result.
9. Stop the server and return a machine-readable pass/fail result.

This approach is useful for compiler integration tests because the generated datapack is parsed and executed by the real game. It can detect invalid `pack.mcmeta`, obsolete directories, invalid JSON resources, malformed commands, macro-instantiation errors, incorrect execution context, bad return propagation, and incorrect scoreboard/storage operations.

## When a client is necessary

A connected client is only required for genuinely client-side or real-player scenarios, including:

- Resource-pack rendering, models, textures, and shaders
- Visual verification of particles or UI
- Perceived sound behavior
- Advancement, dialog, or other client UI
- Keyboard, mouse, and interaction flows
- Semantics that specifically require a real connected player

Most compiler and datapack-runtime behavior should remain in the faster headless test suites. Client automation can be added later as a separate end-to-end layer.

## Recommended test layers

1. Compiler unit tests for parsing, typing, lowering, and command generation.
2. Pack-load tests using the official server to validate every generated resource.
3. Headless function tests for macros, returns, scoreboards, storage, and execution context.
4. GameTests for deterministic tick-sensitive block and entity behavior, with JUnit output for CI.
5. Optional client-driven tests only where server-side observation is insufficient.

The repository's existing implementation does not need to be preserved. This testing strategy should be designed around the new compiler and the real Minecraft 26.2 runtime.
