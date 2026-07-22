# PS-13 — Bot-Driven Player Test Infrastructure

Status: **implemented (2026-07-22).** `crates/mdl-test-bot/` — a deliberately
non-workspace crate, see below — provides `BotHandle`/`BotHandle::connect`, exercised
by two live pinned-server tests: `tests/bot_handle_lifecycle.rs` (connect/spawn/exit
round trip) and `tests/chest_click.rs` (this milestone's own gate: a real bot opens a
placed chest and left-clicks a slot, and the item is confirmed, via the console — not
the bot's own view — to have moved into its inventory).

## Why this exists

PS-14 (`Player` entity kind) and PS-15 (advancement-triggered events) both need a real
`minecraft:player` entity to test against. Vanilla has no way to summon or fake one —
`/summon minecraft:player` is rejected server-side — and there is no server-side trick
that gets around it, because the thing being tested (a player's own inventory changing,
an advancement criterion evaluating) is specifically player-shaped game logic. This
project has twice already, deliberately, deferred solving that:

- `ps-1-handoff.md`: "does not... automate a connected player. Add one of these only
  with the PS-2 capability that needs it."
- `ps-2-0-decisions.md`: "A real player's main-hand book remains an opt-in later
  boundary; it does not justify implementing a Minecraft network client inside
  `mdl-test`."

PS-13 is that capability finally arriving, in the same role PS-1's server harness
played for PS-2: infrastructure the next milestones build on, with no compiler changes
of its own.

## Library research (2026-07-22)

Three approaches were evaluated for getting a real, scriptable player onto the pinned
26.2 server:

**Mineflayer** (Node.js, PrismarineJS) — the most mature general-purpose bot library,
but confirmed non-functional on our target version. Connecting to a 26.1.2 server (one
release behind ours) throws `unsupported protocol version` outright
([mineflayer#3893](https://github.com/PrismarineJS/mineflayer/issues/3893), still open).
The community's ViaVersion/ViaBackwards workaround gets a bot to spawn, but container
click packets — the exact operation this whole roadmap needs — "just does not work...
packets sent fail silently" per the same thread. Ruled out on function, not maturity.

**HeadlessMC** ([headlesshq/headlessmc](https://github.com/headlesshq/headlessmc)) —
runs the real Minecraft Java client headlessly (LWJGL calls stubbed to no-ops) instead
of reimplementing the protocol, so it can't diverge from real client behavior by
construction. Confirmed 26.2 support (release `2.10.0`, 2026-07-13,
`fix(26.2): Fixed lwjgl headless launching for 26.2`). Ships a companion "HMC-Specifics"
in-client mod exposing inventory slot IDs/contents and a `click <id>` command, plus a
dedicated sibling project ([`mc-runtime-test`](https://github.com/headlesshq/mc-runtime-test))
purpose-built for driving the real client from CI. Not selected: the integration shape
is a Java launcher process plus an in-client companion mod, talked to over a
console/JSON protocol — a materially heavier subprocess-orchestration layer than a
native library call, for a fidelity guarantee this project's use case doesn't need
(we aren't testing rendering or input timing, just inventory/advancement game logic).

**Azalea** ([azalea-rs/azalea](https://github.com/azalea-rs/azalea)) — **selected.**
Rust-native protocol reimplementation, drops into `mdl-test` as an ordinary Cargo
dependency with `.await` calls inline in async tests, no subprocess/console-parsing
layer at all. Confirmed 26.2 support (commits dated 2026-07-13, e.g. "fix incorrect
particle order for 26.2"). Its own example
[`examples/steal.rs`](https://github.com/azalea-rs/azalea/blob/main/azalea/examples/steal.rs)
already does almost exactly what PS-15 needs to test:

```rust
let chest = bot.open_container_at(chest_block).await?.unwrap();
for (index, slot) in chest.contents().unwrap_or_default().iter().enumerate() {
    if let ItemStack::Present(item) = slot {
        if item.kind == ItemKind::Diamond {
            chest.left_click(index);
        }
    }
}
```

Offline-mode auth (`Account::offline("bot")`) matches the harness's existing
`online-mode=false` local servers — no real Microsoft account needed anywhere in CI.

Accepted risk: Azalea has a single primary maintainer, ~37% doc coverage on
docs.rs, and its own README says "many parts of Azalea are still unfinished and will
receive breaking changes." That risk is "some interaction might have a rough edge we
have to work around," which is qualitatively different from Mineflayer's "does not run
on our version at all" — the disqualifying failure mode.

## Scope

Delivered via `crates/mdl-test-bot/` (a non-workspace crate depending on `mdl-test` by
path — see "Real findings" for why it isn't inside `mdl-test` itself), an
Azalea-backed `BotHandle` that, against the exact same pinned server jar
`ServerSandbox` already spins up:

- connects with offline auth and disconnects (bounded — see `BotError::
  ShutdownTimeout`), including as part of test teardown even on assertion failure
  (mirroring `ServerSandbox::shutdown`'s existing discipline);
- opens a container at a known block position and left-clicks a specific slot
  (`open_container_and_click`) — proven end to end against a real chest.

Movement was scoped down further than originally planned: `chest_click.rs` teleports
the bot into range with an ordinary console `tp` command rather than using Azalea's
own pathfinder, since nothing in this milestone's own gate needed bot-driven
movement and console teleport is simpler, deterministic, and already available.
`BotHandle::client()` exposes the raw `azalea::Client` for anything a future
milestone needs that isn't wrapped yet (movement included).

## Non-goals

- Nothing compiler-side. No `Player` `EntityKind` (PS-14), no advancement IR (PS-15).
- No pathfinding, combat, crafting, or any bot capability beyond what PS-14 through
  PS-17's tests actually exercise.
- No CI/headless-runner packaging concerns beyond what already applies to running the
  pinned server jar today (`MDL_SERVER_JAR`/`MDL_JAVA`-gated, `#[ignore]`d tests).

## Dependencies

None beyond what PS-1 already built (`ServerSandbox`). This is the first milestone in
the sequence for exactly that reason.

## Downstream

PS-14 (proving `Player` behaves correctly against a real logged-in entity, not just
structurally), PS-15 (the bot is the only way to satisfy `minecraft:inventory_changed`
for real), PS-17 (the capstone's full walkthrough is bot-driven end to end).

## Verify against current implementation before starting — updated 2026-07-22, measured

`crates/mdl-test/src/lib.rs`'s `ServerSandbox` writes `server.properties` with
`server-port=0` (confirmed at `lib.rs:331`, asserted by its own test at `lib.rs:972`).
This note originally proposed recovering the real port by scraping it from the
server's startup log. **That's wrong, and was checked directly against the real
pinned server rather than left as an assumption:** the log prints

```
[Server thread/INFO]: Starting Minecraft server on 127.0.0.1:0
```

— it echoes the *configured* value (`0`) back verbatim, not the OS-resolved port.
Cross-checked the real bound port with `ss -tlnp` at the same moment: `33127`,
nothing like what the log claims. Log-scraping cannot work here; this was caught
before writing any integration code by spinning up the real server jar and looking,
same discipline BE-1's two real bugs were caught by.

The correct fix inverts the problem: **reserve the port on the Rust side before the
server ever starts**, instead of trying to discover it after. Bind a `TcpListener` to
`127.0.0.1:0`, read the OS-assigned port back via `.local_addr()?.port()`, drop the
listener, and write that concrete port number into `server.properties` as
`server-port=<N>` before spawning `java`. This needs a small, real change to
`ServerSandbox::write_server_files` (`lib.rs:312`, currently hardcodes
`"server-port=0\n"` with no parameter) and `TestServer::start` (`lib.rs:245`) to
plumb the resolved port through and expose it publicly (e.g. `TestServer::game_port()
-> u16`) for whatever builds the bot's connection address. There is a small,
accepted TOCTOU race between closing the probe listener and Java binding the same
port — the same race every "reserve a free port for a subprocess" utility accepts,
and collision odds are negligible in this harness's already-isolated, single-server-
at-a-time sandbox.

## Verified against the real client library, not just its docs

Two more things this note originally left as open questions, now resolved by reading
Azalea's actual source (`azalea/src/client_impl/mod.rs`) rather than doc summaries:

- **`Client::disconnect()` vs `Client::exit()` are genuinely different, and the
  difference matters for test teardown.** `disconnect()`'s own doc comment: "Note that
  this will not return from your client builder. If you need that, consider using
  [`Self::exit`] instead." `exit()`'s doc comment: "End the entire client or swarm, and
  return from [`ClientBuilder::start`]." For a test that needs to regain control after
  the bot is done acting, teardown must call `exit()`, not `disconnect()` — confirmed
  directly in source, including a documented usage example matching this exact need.
- **`Event::Spawn`** ("Fired when the player fully spawns into the world (is in a
  loaded chunk) and is ready to interact with it... This is usually the event you
  should listen for when waiting for the bot to be ready") is the correct hook for
  handing the live `Client` out of the event-handler callback to the test-driving code
  — `Client` is cheaply `Clone` (backed by `Arc<RwLock<World>>`), so the handler can
  send it through a `tokio::sync::oneshot` channel the first time `Event::Spawn`
  fires, and the test-driving code awaits receiving it before issuing any actions.
- **`ClientBuilder::start(...)` is a run-forever call by design** — per Azalea's own
  maintainer guidance, disabling auto-reconnect still "will not make `ClientBuilder::
  start` return on disconnect, because Azalea will keep the internal swarm around
  forever until it's forcibly exited." It must be driven from a background task, never
  awaited directly in the test's main flow. (Originally planned as a plain
  `tokio::spawn` — turned out not to work; see "Real findings" below for why it needs
  a dedicated OS thread instead, and why even the teardown call needs its own bound.)

`mdl-test` has zero async/tokio dependency today (confirmed: no `tokio` anywhere in
the workspace, `ServerSandbox`/`TestServer` are 100% thread-and-channel based). Azalea
requires tokio plus `bevy_app`/`bevy_ecs`/`bevy_tasks` (confirmed from `azalea`'s own
`Cargo.toml`) — real, non-trivial new dependency weight. It belongs under
`[dev-dependencies]` in `crates/mdl-test/Cargo.toml`, mirroring how `mdl-compiler`
itself is already a dev-dependency there — the shipped `mdl-test` library/binary
should not need to compile Azalea at all, only test binaries that actually use a bot.
The natural shape for test authors: a synchronous-facing `BotHandle` (returned by a
new `TestServer::connect_bot(&self, name: &str) -> Result<BotHandle>`) that owns one
`tokio::runtime::Runtime` internally and `.block_on()`s each of its own methods — so
bot-driven tests keep reading top-to-bottom like every existing pinned-server test,
freely interleaving ordinary `TestServer::command()`/`wait_for_log()` calls with bot
actions, with no `async`/`.await` visible in test bodies at all.

## Real findings from implementation (2026-07-22)

Every one of these was measured against the real toolchain/library/server, not
assumed — several directly overturned this note's own earlier plan.

- **Azalea's build script hard-requires a nightly Rust toolchain** (`panic!` in
  `azalea/build.rs` if `RUSTUP_TOOLCHAIN` doesn't contain `"nightly"`). Nothing in the
  original library research surfaced this. Folding Azalea into `mdl-test` directly
  would have forced `cargo test --workspace` onto nightly for every crate, for every
  contributor, whether or not they touch bot tests — Cargo resolves one toolchain per
  invocation across a whole workspace. Fixed by moving everything Azalea-dependent
  into **`crates/mdl-test-bot/`, a crate deliberately excluded from the root
  workspace** (its own empty `[workspace]` table stops Cargo walking up and treating
  it as an unlisted member of the root `Cargo.toml`) — invoked separately with `cd
  crates/mdl-test-bot && cargo +nightly test -- --ignored`, matching how these tests
  were already opt-in (`#[ignore]`, env-var-gated). `TestServer::game_port()`
  (Stage 1's fix, landed in `mdl-test` itself) is the only thing `mdl-test-bot`
  actually needs from `mdl-test` beyond the ordinary `ServerSandbox`/`TestServer` API.
- **crates.io's published `azalea` is stale.** Newest published version is
  `0.16.0+mc26.1` (2026-03-28) — Cargo's semver resolution ignores build-metadata
  suffixes for matching, so a plain `version = "0.16.0+mc26.2"` requirement silently
  resolved to the stale `+mc26.1` release when tried. Real 26.2 fixes only exist on
  the unreleased git `main` branch. `mdl-test-bot/Cargo.toml` pins a specific commit
  via a `git`/`rev` dependency instead, with a comment explaining why and a note to
  re-pin if it goes stale.
- **`ClientBuilder::start(...)`'s future is not `Send`** — confirmed by trying
  `tokio::spawn` on it first and reading the resulting `*const () cannot be shared
  between threads` compiler error (it internally uses a `tokio::task::LocalSet`). It
  runs on a dedicated OS thread with its own single-threaded runtime and `LocalSet`,
  entirely separate from the runtime `BotHandle` uses afterward for ordinary `Client`
  method calls.
- **Auto-reconnect must be disabled (`.reconnect_after(None)`), or a real disconnect
  hangs shutdown forever.** `BotHandle::open_container_and_click` triggered a real
  mid-test disconnect once (a `StacklessClosedChannelException` while the server was
  delivering a chat packet, cause not fully root-caused). Without
  `reconnect_after(None)`, Azalea's default behavior after that disconnect is to
  retry forever, so `Client::exit()` afterward never actually let
  `ClientBuilder::start` return — measured as a genuine multi-minute hang, not a
  theoretical risk.
- **`Client::exit()` itself can block indefinitely on its own.** It's a plain
  synchronous call (`self.ecs.write().write_message(...)`) with no timeout of its
  own. After a container interaction, the bot's background tick loop was observed
  holding that same ECS write lock long enough that `exit()` blocked past any
  reasonable bound — even with auto-reconnect disabled. `BotHandle::stop` now calls
  `exit()` on its own short-lived detached thread and separately waits, with a bound
  (`BotError::ShutdownTimeout` after 15s via a `std::sync::mpsc::Receiver::
  recv_timeout` — `std::thread::JoinHandle::join` has no timeout of its own, so a
  plain join can't provide this bound), rather than calling `exit()` inline on the
  caller's own thread.
- **`ContainerHandle::left_click` only sends the click packet — it does not wait for
  the server to process it.** Querying the bot's inventory immediately afterward is a
  real, reproducible race (confirmed: the assertion failed intermittently without a
  settle delay, passed reliably with one). `chest_click.rs` adds a 500ms sleep after
  the click before querying; there's no click-acknowledgement API to await instead.
- **A "Got `SetContainerContentEvent` for container with ID 1, but the current
  container ID is 0" warning appears on every container interaction**, whether or not
  the container is explicitly closed afterward (tested both ways — closing turned out
  not to be the cause of anything). Appears benign (the click and inventory update
  both still land correctly), but unresolved — left as a known, harmless-so-far
  cosmetic wrinkle in this very-fresh Azalea build rather than chased further, since
  `open_container_and_click`'s actual claim is independently verified through the
  console on every test run.
- **`data get entity <name> Inventory`'s response is a plain logged line** — no
  NBT-predicate selector (`[nbt={...}]`) needed to check its contents. An initial
  attempt at one hit a real Minecraft 26.2 command-parser syntax error (confirmed
  directly in `harness.log`: `Expected whitespace to end one argument, but found
  trailing data`), abandoned in favor of just waiting for a substring in the `data
  get` response itself.

## Still open

- The container-ID-mismatch warning's root cause. Not blocking (see above), but worth
  a closer look if it ever starts correlating with an actual assertion failure rather
  than just shutdown slowness.
- Whether bot-side assertions (reading the bot's own observed inventory/world state)
  are ever needed, or whether every PS-14 through PS-17 test can keep using the
  existing log-line-driven console assertion style with the bot purely as an actor.
  `chest_click.rs` only needed the latter.
- Whether a bot needs to survive across multiple commands/assertions within one test
  (stay connected) or whether a fresh connect-per-assertion cycle is preferable.
  PS-17's multi-screen walkthrough will likely want one persistent connection; decide
  when that test is actually drafted, not preemptively.
- Azalea's git pin (`crates/mdl-test-bot/Cargo.toml`) will need periodic re-pinning as
  its `main` branch moves; no automation for this exists yet.
