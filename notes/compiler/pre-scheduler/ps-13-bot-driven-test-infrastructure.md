# PS-13 — Bot-Driven Player Test Infrastructure

Status: **planned, not yet implemented.**

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

Extend `mdl-test` (`crates/mdl-test/src/lib.rs`'s `ServerSandbox`, or a small sibling
module alongside it — see "Open questions") with an Azalea-backed bot client that can,
against the exact same pinned server jar `ServerSandbox` already spins up:

- connect with offline auth and disconnect cleanly, including as part of test teardown
  even on assertion failure (mirroring `ServerSandbox::shutdown`'s existing discipline);
- walk to a position (simple point-to-point movement is enough — no pathfinding
  sophistication is required by anything downstream in this roadmap, since chests in
  these tests are reachable in a straight line by construction);
- open a container at a known block position and read its current contents;
- click a specific slot in an open container;
- send/observe chat messages (advancement reward functions can be asserted via the
  existing `say`-marker pattern every other PS milestone already uses, so bot-side chat
  observation, not just server-log observation, may be redundant — confirm during
  implementation whether `ServerSandbox`'s existing log-line waiting already covers
  this before building a second observation path).

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

## Verify against current implementation before starting

`crates/mdl-test/src/lib.rs`'s `ServerSandbox` writes `server.properties` with
`server-port=0` (confirmed at `lib.rs:331`, asserted by its own test at `lib.rs:972`).
Port `0` tells the OS to bind an arbitrary free ephemeral port — deliberate, and
correct for the harness's existing job, since it avoids port collisions between
parallel test runs that never needed a second client to find the server. But it means
**the real listening port is not known in advance**, and nothing in `ServerSandbox`
today parses it back out of the server's own startup log (confirmed: no
`bound_port`/`actual_port`/log-scraping-for-port code exists anywhere in the file).
A bot has to connect over a real TCP socket, unlike the console/RCON-style command
driving `ServerSandbox` already does — so PS-13's first real piece of work is teaching
the harness to recover the actual bound port (Minecraft logs it at startup; the
existing log-line-waiting infrastructle already used for `wait_for_command_log` is the
natural place to extract it from) and expose it to whatever constructs the bot client.
This is a small, concrete gap, not a blocker, but it's real work this note's earlier
draft didn't call out and should be the first thing checked, not assumed solved.

## Not yet determined

- Crate placement: extend `mdl-test` directly, or add a new sibling crate (e.g.
  `mdl-test-bot`) that `mdl-test` test files depend on. Leans toward extending
  `mdl-test` directly unless Azalea's dependency tree turns out to meaningfully slow
  down compilation of tests that don't need it — check this empirically (a clean
  `cargo build` timing comparison) before deciding, not by intuition.
- Whether bot-side assertions (reading the bot's own observed inventory/world state)
  are needed at all, or whether every PS-14 through PS-17 test can get by on the
  existing log-line-driven assertion style plus the bot purely as an actor (not an
  observer). Decide this once PS-14's actual test shapes are drafted, not preemptively.
- Whether the bot needs to survive across multiple commands/assertions within one test
  (stay connected, same session) or whether a fresh connect-act-disconnect cycle per
  assertion is acceptable/preferable for isolation. PS-17's multi-screen walkthrough
  almost certainly wants one persistent connection across a whole test; simpler
  PS-14/15 tests might not care either way — don't over-build persistence machinery
  until PS-17 actually needs it.

## Structural ideas to carry forward

- Give the bot connection the same teardown discipline `ServerSandbox::shutdown`
  already has — guaranteed disconnect on drop/failure, not just on the happy path,
  so a failed assertion mid-test can't leak a connected bot process/socket into the
  next test.
- Whatever exposes the discovered port should live next to `ServerConfig`/`start`'s
  existing API shape (`lib.rs:47,245`), not bolted on separately — a caller that
  already has a `TestServer` handle should be able to ask it for a bot-connectable
  address with no extra wiring.
