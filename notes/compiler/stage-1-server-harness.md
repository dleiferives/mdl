# Stage 1: Rust Workspace and Vanilla Server Harness

Status: **Complete for the first vertical slice**

Stage 1 is implemented by the `mdl-test` workspace crate. It deliberately provides
one narrow capability: execute a generated datapack on the pinned vanilla server and
make the result observable to Rust tests.

## What exists

- a Rust 2024 workspace with strict unsafe and Clippy policy;
- a dependency-free server harness using `std::process`;
- unique disposable server directories outside the repository;
- generated EULA and low-overhead flat-world server configuration;
- installation of generated datapack files before world creation;
- explicit Java and server-JAR configuration;
- concurrent stdout and stderr draining;
- readiness detection from the server output rather than a fixed sleep;
- one-line console command submission;
- bounded startup, command, and graceful-shutdown waits;
- forced process termination if graceful shutdown does not complete;
- a combined `harness.log` and recent-log context in errors;
- automatic preservation of failed sandboxes and optional preservation of successful
  sandboxes;
- fast unit tests and an ignored real-server integration test;
- a small CLI for local smoke tests.

The smoke datapack uses data-pack format `[107, 1]`, the singular 26.2 `function`
resource directory, and a scoreboard-backed assertion. Rust invokes
`function mdl_test:smoke` and waits for a marker emitted only when the score equals
42.

## Verified result

The smoke test was run on 2026-07-11 with:

```text
Minecraft Java server: 26.2
Java: OpenJDK 25.0.3
host: macOS arm64
```

The server:

1. discovered and loaded `mdl_smoke`;
2. created the disposable flat world;
3. reported ready;
4. ran `mdl_test:smoke`;
5. emitted `MDL_SMOKE_RESULT_42`;
6. handled `stop` and saved all dimensions;
7. exited cleanly.

The configured flat generator was also verified to start without the fallback error
produced by an empty `generator-settings` object.

## Deliberate limits

This stage does not yet provide:

- compiler IR or datapack emission APIs;
- a structured scoreboard/NBT result protocol;
- Mojang GameTest structure generation;
- server-JAR downloading or version resolution;
- parallel vanilla servers;
- client automation;
- performance benchmarking statistics.

Those should be added only when a compiler milestone needs them. The current public
surface—sandbox creation, datapack file installation, server startup, command input,
log waiting, and shutdown—is sufficient for the Stage 2 and Stage 3 work to build on.

## Commands

See the [`mdl-test` README](../../crates/mdl-test/README.md) for local and integration
test commands.
