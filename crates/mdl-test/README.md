# mdl-test

`mdl-test` runs generated datapacks on a real vanilla Minecraft dedicated server.
It is intentionally separate from fast compiler unit tests.

The Stage 1 smoke test creates a disposable flat world, installs a data-pack format
107.1 pack, waits for the server's readiness event, invokes a function over server
stdin, observes a score-backed success marker, stops the server, and removes the
sandbox. Server stdout and stderr are drained concurrently into `harness.log` so the
Java process cannot block on a full output pipe.

## Run the smoke test

Pass the official Minecraft 26.2 server JAR explicitly:

```sh
cargo run -p mdl-test -- smoke \
  --server-jar /path/to/server.jar \
  --java /path/to/java
```

On this development machine, Java is currently:

```text
/opt/homebrew/opt/openjdk@25/bin/java
```

The CLI also accepts environment variables:

```sh
MDL_SERVER_JAR=/path/to/server.jar \
MDL_JAVA=/path/to/java \
cargo run -p mdl-test -- smoke
```

Use `--keep` or set `MDL_KEEP_TEST_DIR=1` to retain a successful sandbox. Failed
server runs are always retained, and the error prints the sandbox path. Every
retained directory contains `harness.log`, the generated world, and the exact pack
that ran.

## Test layers

Fast tests do not start Java:

```sh
cargo test --workspace
```

The real-server integration test is ignored by default:

```sh
MDL_SERVER_JAR=/path/to/server.jar \
MDL_JAVA=/path/to/java \
cargo test -p mdl-test --test vanilla_smoke -- --ignored --nocapture
```

The ordinary server harness is the initial path because compiler tests need direct
function and command invocation. Mojang's dedicated GameTest entry point remains a
useful later layer for tick-sensitive block and entity tests with JUnit-like XML
reports.

## Primary references

- [Minecraft Java 26.2, data-pack format 107.1, and official server JAR](https://www.minecraft.net/en-us/article/minecraft-java-edition-26-2)
- [Mojang's dedicated GameTest entry point and report options](https://www.minecraft.net/en-us/article/minecraft-snapshot-25w03a)
- [Rust child-process and piped-I/O behavior](https://doc.rust-lang.org/stable/std/process/index.html)
