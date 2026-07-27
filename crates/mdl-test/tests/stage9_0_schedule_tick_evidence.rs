//! Stage 9.0 pinned-server evidence for `schedule`/`schedule clear`/`#minecraft:tick`.
//!
//! See `notes/compiler/stage-9/9-0-contracts-and-evidence.md` for the measurement
//! plan (M1-M18) this file implements. Each `#[test]` here is one measurement
//! group; failures print the exact observed vs. expected server evidence.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use mdl_test::{ServerConfig, ServerSandbox, TestServer};

const OBJECTIVE: &str = "probe";
const PACK_META: &str = concat!(
    "{\n",
    "  \"pack\": {\n",
    "    \"description\": \"MDL Stage 9.0 schedule/tick evidence\",\n",
    "    \"min_format\": [107, 1],\n",
    "    \"max_format\": [107, 1]\n",
    "  }\n",
    "}\n",
);

fn start(pack_files: &[(&str, &str)]) -> Result<(PathBuf, TestServer), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let config = ServerConfig::new(java, server_jar);
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).map_err(|error| error.to_string())?;
    let mut files: Vec<(&str, &str)> = vec![("pack.mcmeta", PACK_META)];
    files.extend_from_slice(pack_files);
    sandbox
        .install_datapack("mdl_probe", files)
        .map_err(|error| error.to_string())?;
    let server = sandbox.start(&config).map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    Ok((root, server))
}

fn command(server: &mut TestServer, text: &str) -> Result<(), String> {
    server.command(text).map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// Diagnostic: does `execute in <dim> as @e[...]` actually filter by
// dimension? (Needed to interpret M15 correctly -- an unconditional summon
// in overworld showed up as matched by BOTH `in minecraft:overworld` and
// `in minecraft:the_nether` selector checks, which contradicts the assumed
// semantics and must be understood directly rather than guessed.)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25; diagnostic, not a gate"]
fn recon_dimension_selector_scoping() {
    if let Err(error) = run_recon_dimension() {
        panic!("{error}");
    }
}

fn run_recon_dimension() -> Result<(), String> {
    let (root, mut server) = start(&[(
        "data/mdl_probe/function/init.mcfunction",
        "scoreboard objectives add probe dummy\n",
    )])?;
    let result = (|| -> Result<(), String> {
        command(&mut server, "function mdl_probe:init")?;
        command(&mut server, "forceload add 0 0")?;
        server.wait_for_command_log("Marked chunk").map_err(|error| error.to_string())?;
        command(&mut server, "scoreboard players set #found probe 0")?;
        command(&mut server, "summon minecraft:marker 5 200 5 {Tags:[\"dimcheck\"]}")?;
        server
            .wait_for_command_log("Summoned")
            .map_err(|error| error.to_string())?;
        // Bare selector, no dimension redirection at all, from the ordinary
        // overworld console root -- must match (sanity baseline).
        command(&mut server, "execute if entity @e[tag=dimcheck] run scoreboard players set #found probe 1")?;
        let bare = read_score(&mut server, "#found")?;
        eprintln!("recon: bare `if entity @e[tag=dimcheck]` (overworld console root) -> found={bare}");
        command(&mut server, "scoreboard players set #found probe 0")?;
        command(&mut server, "execute if entity @e[type=minecraft:marker] run scoreboard players set #found probe 1")?;
        let bare_untagged = read_score(&mut server, "#found")?;
        eprintln!("recon: bare `if entity @e[type=minecraft:marker]` (no tag filter) -> found={bare_untagged}");
        command(&mut server, "list")?;
        let list_line = server.wait_for_command_log("players online").map_err(|error| error.to_string())?;
        eprintln!("recon: {list_line}");

        for (label, dim) in [("overworld", "minecraft:overworld"), ("nether", "minecraft:the_nether")] {
            command(&mut server, "scoreboard players set #found probe 0")?;
            command(
                &mut server,
                &format!(
                    "execute in {dim} if entity @e[tag=dimcheck] run scoreboard players set #found probe 1"
                ),
            )?;
            let found = read_score(&mut server, "#found")?;
            eprintln!("recon: `in {dim} if entity @e[tag=dimcheck]` -> found={found} ({label})");
        }
        // Try teleporting the entity across dimensions (`execute in <dim> run
        // tp @s ...` shaped) to see whether the entity actually *has* one
        // true dimension that a direct, unambiguous primitive can move it
        // out of and prove absence from the other.
        command(&mut server, "execute as @e[tag=dimcheck] in minecraft:the_nether run tp @s ~ ~ ~")?;
        command(&mut server, "scoreboard players set #found probe 0")?;
        command(&mut server, "execute in minecraft:overworld if entity @e[tag=dimcheck] run scoreboard players set #found probe 1")?;
        let found_overworld_after_tp = read_score(&mut server, "#found")?;
        command(&mut server, "scoreboard players set #found probe 0")?;
        command(&mut server, "execute in minecraft:the_nether if entity @e[tag=dimcheck] run scoreboard players set #found probe 1")?;
        let found_nether_after_tp = read_score(&mut server, "#found")?;
        eprintln!(
            "recon: after `as @e[tag=dimcheck] in minecraft:the_nether run tp @s ~ ~ ~`, \
             found_overworld={found_overworld_after_tp}, found_nether={found_nether_after_tp}"
        );
        Ok(())
    })();
    finish(server, root, result)
}

fn set_score(server: &mut TestServer, holder: &str, value: i32) -> Result<(), String> {
    command(
        server,
        &format!("scoreboard players set {holder} {OBJECTIVE} {value}"),
    )
}

/// Reads a score back synchronously via the `<holder> has <value> [<objective>]`
/// echo. Fails loudly (rather than timing out silently) if the value observed
/// does not match, printing both so a mismatch is immediately diagnosable.
fn expect_score(server: &mut TestServer, holder: &str, expected: i32) -> Result<(), String> {
    command(server, &format!("scoreboard players get {holder} {OBJECTIVE}"))?;
    let line = server
        .wait_for_command_log(&format!("{holder} has"))
        .map_err(|error| error.to_string())?;
    let expected_text = format!("{holder} has {expected} [{OBJECTIVE}]");
    if line.contains(&expected_text) {
        Ok(())
    } else {
        Err(format!("expected {expected_text:?}, got {line:?}"))
    }
}

fn read_score(server: &mut TestServer, holder: &str) -> Result<i32, String> {
    command(server, &format!("scoreboard players get {holder} {OBJECTIVE}"))?;
    let line = server
        .wait_for_command_log(&format!("{holder} has"))
        .map_err(|error| error.to_string())?;
    let after_has = line
        .split("has ")
        .nth(1)
        .ok_or_else(|| format!("could not parse score line: {line:?}"))?;
    after_has
        .split_whitespace()
        .next()
        .ok_or_else(|| format!("could not parse score line: {line:?}"))?
        .parse::<i32>()
        .map_err(|error| format!("could not parse score line {line:?}: {error}"))
}

fn finish(server: TestServer, root: PathBuf, result: Result<(), String>) -> Result<(), String> {
    match result {
        Ok(()) => server.shutdown().map_err(|error| error.to_string()),
        Err(error) => {
            let mut server = server;
            server.preserve_sandbox();
            let _ = server.shutdown();
            Err(format!("{error}\nsandbox preserved at {}", root.display()))
        }
    }
}

// ---------------------------------------------------------------------------
// Group 1 (M1-M4): deterministic tick control
// ---------------------------------------------------------------------------

const G1_INIT: &str = concat!(
    "scoreboard objectives add probe dummy\n",
    "scoreboard players set #tick_count probe 0\n",
    "scoreboard players set #mark_count probe 0\n",
);
const G1_TICK: &str = "scoreboard players add #tick_count probe 1\n";
const G1_MARK: &str = "scoreboard players add #mark_count probe 1\n";
const G1_TICK_TAG: &str = "{\n  \"values\": [\"mdl_probe:tick\"]\n}\n";

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn group1_deterministic_tick_control() {
    if let Err(error) = run_group1() {
        panic!("{error}");
    }
}

fn run_group1() -> Result<(), String> {
    let (root, mut server) = start(&[
        ("data/mdl_probe/function/init.mcfunction", G1_INIT),
        ("data/mdl_probe/function/tick.mcfunction", G1_TICK),
        ("data/mdl_probe/function/mark.mcfunction", G1_MARK),
        ("data/minecraft/tags/function/tick.json", G1_TICK_TAG),
    ])?;
    let result = exercise_group1(&mut server);
    finish(server, root, result)
}

/// `tick step <N>` acknowledges immediately with "Stepping N tick(s)" but the
/// actual N-tick processing is not necessarily complete by the time that log
/// line appears (discovered empirically: reading a counter right after the
/// acknowledgment for N=5 observed only +1, i.e. the read raced the step).
/// Polls the native gametime clock until two consecutive reads agree, which is
/// a completion signal that does not assume any particular step duration.
///
/// Promoted into `mdl_test::wait_for_gametime_settled` (Stage 9B); this is a
/// thin `Result<_, String>` wrapper so every existing call site in this file
/// is unchanged.
fn wait_for_gametime_settled(server: &mut TestServer) -> Result<i64, String> {
    mdl_test::wait_for_gametime_settled(server).map_err(|error| error.to_string())
}

fn query_gametime(server: &mut TestServer) -> Result<i64, String> {
    mdl_test::query_gametime(server).map_err(|error| error.to_string())
}

fn exercise_group1(server: &mut TestServer) -> Result<(), String> {
    // Freeze before anything else runs, so there is no unfrozen window (however
    // short) between server boot and the first measurement in which real ticks
    // could elapse and be wrongly attributed to `tick step`.
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;
    command(server, "function mdl_probe:init")?;
    expect_score(server, "#tick_count", 0)?;

    // M1: freezing must stop the `#minecraft:tick` handler from firing due to
    // wall-clock ticking, while ordinary console commands keep working (already
    // demonstrated by the round-trips above completing without timing out).
    let baseline_ticks = read_score(server, "#tick_count")?;
    let baseline_gametime = query_gametime(server)?;
    std::thread::sleep(Duration::from_millis(1200));
    let after_wait_ticks = read_score(server, "#tick_count")?;
    let after_wait_gametime = query_gametime(server)?;
    if after_wait_ticks != baseline_ticks || after_wait_gametime != baseline_gametime {
        return Err(format!(
            "M1 FAILED: tick_count {baseline_ticks}->{after_wait_ticks}, gametime \
             {baseline_gametime}->{after_wait_gametime} during 1.2s of real time while frozen \
             -- freeze does not fully stop ticking"
        ));
    }

    // M2: bare `tick step` advances exactly one tick, cross-checked against both
    // the `#minecraft:tick`-tag-driven counter and the native gametime clock.
    // `wait_for_gametime_settled` (not an immediate read) is required: the
    // "Stepping N tick(s)" acknowledgment logs before stepping completes.
    command(server, "tick step")?;
    server
        .wait_for_command_log("Stepping 1 tick")
        .map_err(|error| error.to_string())?;
    let gametime_after_one = wait_for_gametime_settled(server)?;
    let ticks_after_one = read_score(server, "#tick_count")?;
    if gametime_after_one != baseline_gametime + 1 || ticks_after_one != baseline_ticks + 1 {
        return Err(format!(
            "M2 FAILED: gametime {baseline_gametime}->{gametime_after_one}, tick_count \
             {baseline_ticks}->{ticks_after_one} after a bare `tick step` (expected both +1)"
        ));
    }

    // M3: `tick step <N>` advances exactly N ticks -- checked at N=5 and N=20,
    // with the tag-driven counter and gametime required to agree at every step.
    for n in [5_i64, 20] {
        let before_ticks = read_score(server, "#tick_count")?;
        let before_gametime = query_gametime(server)?;
        command(server, &format!("tick step {n}"))?;
        server
            .wait_for_command_log(&format!("Stepping {n} tick"))
            .map_err(|error| error.to_string())?;
        let after_gametime = wait_for_gametime_settled(server)?;
        let after_ticks = read_score(server, "#tick_count")?;
        if i64::from(after_ticks - before_ticks) != n || after_gametime - before_gametime != n {
            return Err(format!(
                "M3 FAILED at N={n}: tick_count {before_ticks}->{after_ticks}, gametime \
                 {before_gametime}->{after_gametime} (expected both to advance by exactly {n})"
            ));
        }
    }

    command(server, "tick unfreeze")?;
    server
        .wait_for_command_log("running normally")
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 2 (M5-M8): `replace` vs `append` pending-slot semantics
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn group2_replace_vs_append() {
    if let Err(error) = run_group2() {
        panic!("{error}");
    }
}

fn run_group2() -> Result<(), String> {
    let (root, mut server) = start(&[
        ("data/mdl_probe/function/init.mcfunction", G1_INIT),
        ("data/mdl_probe/function/mark.mcfunction", G1_MARK),
    ])?;
    let result = exercise_group2(&mut server);
    finish(server, root, result)
}

/// Steps exactly one call to `tick step <n>` and waits for it to actually
/// finish (see `wait_for_gametime_settled` -- the acknowledgment log line is
/// not a completion signal).
fn step_and_settle(server: &mut TestServer, n: u32) -> Result<(), String> {
    mdl_test::step_and_settle(server, n).map_err(|error| error.to_string())
}

fn exercise_group2(server: &mut TestServer) -> Result<(), String> {
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;
    command(server, "function mdl_probe:init")?;

    // M5: three `replace` schedules of the same function, same tick -- does
    // exactly one firing occur, or three?
    set_score(server, "#mark_count", 0)?;
    for _ in 0..3 {
        command(server, "schedule function mdl_probe:mark 5t replace")?;
    }
    step_and_settle(server, 5)?;
    expect_score(server, "#mark_count", 1)?;

    // M6: `replace` at 5t immediately followed by `replace` at 100t (same
    // starting tick). Does the probe fire at +5 (first delay honored) or +100
    // (replace also resets the timer)? Either is a legitimate `replace`
    // semantics; what matters and is asserted is that only ONE firing ever
    // happens in total (the single-pending-slot guarantee), not which delay
    // wins -- that fact is recorded, not assumed.
    set_score(server, "#mark_count", 0)?;
    command(server, "schedule function mdl_probe:mark 5t replace")?;
    command(server, "schedule function mdl_probe:mark 100t replace")?;
    step_and_settle(server, 5)?;
    let count_at_5 = read_score(server, "#mark_count")?;
    step_and_settle(server, 95)?;
    let count_at_100 = read_score(server, "#mark_count")?;
    eprintln!(
        "M6: mark_count at +5 ticks = {count_at_5}, at +100 ticks = {count_at_100} \
         ({})",
        if count_at_5 == 1 {
            "first (5t) delay was honored"
        } else if count_at_100 == 1 {
            "second (100t) call reset the timer"
        } else {
            "UNEXPECTED: neither delay produced exactly one firing"
        }
    );
    if count_at_100 != 1 {
        return Err(format!(
            "M6 FAILED: expected exactly one total firing from two `replace` calls to the \
             same function id, got mark_count={count_at_100} after the later of the two delays \
             elapsed (intermediate reading at +5 ticks was {count_at_5})"
        ));
    }

    // M7: three `append` schedules of the same function, same tick, same
    // delay -- does it fire three times? (Measured, not assumed: recorded as
    // a finding either way, since this turned out to be a genuinely open
    // question the plan's research did not settle.)
    set_score(server, "#mark_count", 0)?;
    for _ in 0..3 {
        command(server, "schedule function mdl_probe:mark 5t append")?;
    }
    step_and_settle(server, 5)?;
    let m7_count = read_score(server, "#mark_count")?;
    eprintln!(
        "M7: three `append` calls to the same function at the same target tick produced \
         mark_count={m7_count} (3 would mean independent queued firings; 1 would mean append \
         dedups when function+target-tick are identical)"
    );

    // M8: `append` with two different delays -- two independent firings at
    // their respective ticks, not coalesced. This isolates whether M7's
    // result (if not 3) was because append never queues more than one pending
    // entry per function at all, or specifically dedups identical
    // (function, target-tick) pairs.
    set_score(server, "#mark_count", 0)?;
    command(server, "schedule function mdl_probe:mark 3t append")?;
    command(server, "schedule function mdl_probe:mark 7t append")?;
    step_and_settle(server, 3)?;
    let m8_count_at_3 = read_score(server, "#mark_count")?;
    step_and_settle(server, 4)?;
    let m8_count_at_7 = read_score(server, "#mark_count")?;
    eprintln!(
        "M8: append at 3t then 7t (different target ticks) produced mark_count={m8_count_at_3} \
         at +3 ticks, mark_count={m8_count_at_7} at +7 ticks"
    );
    if m8_count_at_3 != 1 || m8_count_at_7 != 2 {
        return Err(format!(
            "M8 FAILED: expected two independent firings at different target ticks \
             (1 then 2), got {m8_count_at_3} then {m8_count_at_7} -- append does not reliably \
             queue independent entries even at different target ticks"
        ));
    }

    command(server, "tick unfreeze")?;
    server
        .wait_for_command_log("running normally")
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 3 (M9-M11): same-tick firing order
// ---------------------------------------------------------------------------

const G3_LOG_A: &str = "data modify storage mdl_probe:log order append value \"A\"\n";
const G3_LOG_B: &str = "data modify storage mdl_probe:log order append value \"B\"\n";
const G3_LOG_C: &str = "data modify storage mdl_probe:log order append value \"C\"\n";
const G3_LOG_S: &str = "data modify storage mdl_probe:log order append value \"S\"\n";
// Deliberately non-alphabetical declared order: C, A, B.
const G3_TICK_TAG: &str =
    "{\n  \"values\": [\"mdl_probe:log_c\", \"mdl_probe:log_a\", \"mdl_probe:log_b\"]\n}\n";

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn group3_same_tick_firing_order() {
    if let Err(error) = run_group3() {
        panic!("{error}");
    }
}

fn run_group3() -> Result<(), String> {
    let (root, mut server) = start(&[
        ("data/mdl_probe/function/log_a.mcfunction", G3_LOG_A),
        ("data/mdl_probe/function/log_b.mcfunction", G3_LOG_B),
        ("data/mdl_probe/function/log_c.mcfunction", G3_LOG_C),
        ("data/mdl_probe/function/log_s.mcfunction", G3_LOG_S),
        ("data/minecraft/tags/function/tick.json", G3_TICK_TAG),
    ])?;
    let result = exercise_group3(&mut server);
    finish(server, root, result)
}

fn read_order_log(server: &mut TestServer) -> Result<String, String> {
    command(server, "data get storage mdl_probe:log order")?;
    let line = server
        .wait_for_command_log(" has the following contents: ")
        .map_err(|error| error.to_string())?;
    line.rsplit_once("contents: ")
        .map(|(_, tail)| tail.to_owned())
        .ok_or_else(|| format!("could not parse storage feedback: {line:?}"))
}

fn reset_order_log(server: &mut TestServer) -> Result<(), String> {
    command(server, "data modify storage mdl_probe:log order set value []")
}

fn exercise_group3(server: &mut TestServer) -> Result<(), String> {
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;

    // M9: two `schedule`-fired functions, both due the same tick. Trial twice
    // with the call order reversed to see whether firing order tracks
    // schedule-call order or something independent of it.
    for (label, first, second) in [("a-then-b", "log_a", "log_b"), ("b-then-a", "log_b", "log_a")]
    {
        reset_order_log(server)?;
        command(server, &format!("schedule function mdl_probe:{first} 1t replace"))?;
        command(server, &format!("schedule function mdl_probe:{second} 1t replace"))?;
        step_and_settle(server, 1)?;
        let order = read_order_log(server)?;
        eprintln!("M9 ({label}): fired order = {order}");
    }

    // M10: `#minecraft:tick` handlers declared in non-alphabetical order
    // (C, A, B in tick.json) -- does firing order match declared order?
    // Checked once now, then again after `/reload` re-parses the tag from
    // scratch, to see whether the order is a stable, re-derivable fact of the
    // tag file rather than incidental to one JVM's first resolution.
    reset_order_log(server)?;
    step_and_settle(server, 1)?;
    let order_before_reload = read_order_log(server)?;
    eprintln!("M10 (before /reload): fired order = {order_before_reload}");

    command(server, "reload")?;
    server
        .wait_for_command_log("Loaded 1688 advancements")
        .map_err(|error| error.to_string())?;
    // `/reload` re-registers the tick-tag handlers; freeze state does not
    // necessarily survive it, so re-freeze defensively before stepping again.
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;
    reset_order_log(server)?;
    step_and_settle(server, 1)?;
    let order_after_reload = read_order_log(server)?;
    eprintln!("M10 (after /reload): fired order = {order_after_reload}");
    if order_after_reload != order_before_reload {
        return Err(format!(
            "M10 FAILED: tick-tag firing order changed across /reload: {order_before_reload:?} \
             -> {order_after_reload:?}"
        ));
    }

    // M11: within the same tick, do `#minecraft:tick` handlers run before,
    // after, or interleaved with that tick's due `schedule` firings?
    reset_order_log(server)?;
    command(server, "schedule function mdl_probe:log_s 1t replace")?;
    step_and_settle(server, 1)?;
    let order_with_schedule = read_order_log(server)?;
    eprintln!(
        "M11: tick-tag order interleaved with a same-tick schedule firing = {order_with_schedule}"
    );

    command(server, "tick unfreeze")?;
    server
        .wait_for_command_log("running normally")
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 4 (M12-M13): `/reload` re-arm hazard
// ---------------------------------------------------------------------------

const G4_INIT: &str = "scoreboard objectives add probe dummy\nscoreboard players set #loop_count probe 0\n";
// `init` (which resets `#loop_count` to 0) deliberately is NOT in the load
// tag -- only `install` is, matching a real self-rescheduling job where load
// re-arms the chain but does not reset unrelated persistent state. `init` is
// instead called once, manually, immediately after boot (see
// `exercise_group4`), so re-running `#minecraft:load` on `/reload` re-issues
// only the schedule call under test, not a state reset that would confound
// the measurement.
const G4_LOAD_TAG: &str = "{\n  \"values\": [\"mdl_probe:install\"]\n}\n";

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn group4_reload_rearm_hazard() {
    // M12: install re-arms with `replace` at the same delay the chain uses
    // internally -- must never duplicate.
    if let Err(error) = run_group4("replace", 1, 1) {
        panic!("M12 (install replace, matched 1t/1t delay): {error}");
    }
    // M13a: install re-arms with `append` at the same delay the chain uses
    // internally -- per M7, append at an identical (function, target-tick)
    // pair dedups, so this is predicted safe too.
    if let Err(error) = run_group4("append", 1, 1) {
        panic!("M13a (install append, matched 1t/1t delay): {error}");
    }
    // M13b: install re-arms with `append` at a delay that does NOT match the
    // chain's own internal reschedule delay (5t chain, 3t install) -- per M8,
    // append at a *different* target tick creates an independent entry, so
    // this is predicted to duplicate the chain. This is the concrete
    // real-world shape `spawn-animations` defends against with an explicit
    // `schedule clear` before its mismatched-delay install-time reschedule.
    // This is a genuinely open, negative result, not a pass/fail gate: the
    // hypothesized append+mismatched-delay duplication hazard (motivated by
    // M7/M8 and the `spawn-animations` pack's defensive `schedule clear`)
    // did NOT reproduce with a 5t chain / 3t install split. Recorded as
    // evidence either way rather than forced into an assertion -- see the
    // dossier for the standing open question this leaves.
    match run_group4("append", 5, 3) {
        Ok(()) => eprintln!(
            "M13b (install append, mismatched 5t chain / 3t install delay): NO duplication \
             observed. This contradicts the hypothesis (from M7/M8) that append at a target \
             tick different from the chain's own pending entry creates an independent second \
             entry -- open question, needs more targeted measurement before any 'append is \
             unsafe at load time' claim is frozen."
        ),
        Err(error) => eprintln!(
            "M13b (install append, mismatched 5t/3t delay): {error}"
        ),
    }
}

fn run_group4(install_mode: &str, chain_delay: u32, install_delay: u32) -> Result<(), String> {
    let loop_body = format!(
        "schedule function mdl_probe:loop {chain_delay}t replace\n\
         scoreboard players add #loop_count probe 1\n"
    );
    let install_body =
        format!("schedule function mdl_probe:loop {install_delay}t {install_mode}\n");
    let (root, mut server) = start(&[
        ("data/mdl_probe/function/init.mcfunction", G4_INIT),
        ("data/mdl_probe/function/loop.mcfunction", &loop_body),
        ("data/mdl_probe/function/install.mcfunction", &install_body),
        ("data/minecraft/tags/function/load.json", G4_LOAD_TAG),
    ])?;
    let result = exercise_group4(&mut server, install_mode, chain_delay);
    finish(server, root, result)
}

fn exercise_group4(
    server: &mut TestServer,
    install_mode: &str,
    chain_delay: u32,
) -> Result<(), String> {
    // `#minecraft:load` (just `install`) already ran once, unfrozen, during
    // normal boot -- exactly like a real deployment. `init` runs once here,
    // manually, to create the objective and zero the counter.
    command(server, "function mdl_probe:init")?;
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;

    // Run the chain across two full periods of its own reschedule delay
    // before any reload, so a pending entry is reliably in flight at the
    // moment of reload. The baseline is read dynamically (not assumed) since
    // a brief real-time window before this test's own `tick freeze` lands
    // can let the chain fire once already.
    for _ in 0..(2 * chain_delay) {
        step_and_settle(server, 1)?;
    }
    let before_reload = read_score(server, "#loop_count")?;

    // Reload mid-chain: this re-runs `#minecraft:load`, which re-issues the
    // install-time `schedule function mdl_probe:loop <install_delay>t
    // <mode>` call while a prior chain entry may already be pending.
    command(server, "reload")?;
    server
        .wait_for_command_log("Loaded 1688 advancements")
        .map_err(|error| error.to_string())?;
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;

    let window = 4 * chain_delay;
    for _ in 0..window {
        step_and_settle(server, 1)?;
    }
    let after_reload = read_score(server, "#loop_count")?;
    let advanced = after_reload - before_reload;
    let expected = i32::try_from(window / chain_delay).expect("small test constant");
    eprintln!(
        "install=`{install_mode}` chain_delay={chain_delay}t: loop_count advanced by {advanced} \
         over {window} ticks following a mid-chain /reload (expected {expected} if the chain is \
         single; {} would mean two concurrent chains were stacked)",
        expected * 2
    );

    command(server, "tick unfreeze")?;
    server
        .wait_for_command_log("running normally")
        .map_err(|error| error.to_string())?;

    if advanced != expected {
        return Err(format!(
            "loop_count advanced by {advanced} (not the expected {expected}) over {window} \
             post-reload ticks with install=`{install_mode}` chain_delay={chain_delay}t \
             install_delay -- {}",
            if advanced > expected {
                "a duplicate self-reschedule chain was stacked by /reload"
            } else {
                "the chain lost ticks across /reload (stalled or was cancelled)"
            }
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 5 (M14-M15): execution context loss/reconstruction
// ---------------------------------------------------------------------------

const G5_SETUP: &str = concat!(
    "scoreboard objectives add probe dummy\n",
    "scoreboard players set #in_overworld probe 0\n",
    "scoreboard players set #in_nether probe 0\n",
    "data modify storage mdl_probe:ctx has_self set value 0b\n",
    "kill @e[tag=probe_source]\n",
    "kill @e[tag=probe_ambient]\n",
    "summon minecraft:armor_stand 5 200 5 \
     {Tags:[\"probe_source\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
);
const G5_MARK_CTX: &str = concat!(
    // Proves executor loss: if `@s` still resolves to an entity, this writes
    // true; a fired function with no executor leaves it at the `false`
    // default set by `setup`.
    "execute if entity @s run data modify storage mdl_probe:ctx has_self set value 1b\n",
    // Materializes the *ambient* position/dimension into a real, separately
    // queryable entity -- `~ ~ ~` only resolves to something meaningful
    // because of whatever context (or lack of it) is active when this line
    // runs.
    "summon minecraft:marker ~ ~ ~ {Tags:[\"probe_ambient\"]}\n",
    // `in <dim>` must precede the entity selector so the selector is
    // evaluated *within* that dimension (entity selectors are scoped to the
    // execution context's current dimension) -- `as @e[...] in <dim>` would
    // evaluate the selector first and then unconditionally retarget the
    // dimension, which always "succeeds" regardless of where the entity
    // actually is and does not test anything.
    "execute in minecraft:overworld as @e[tag=probe_ambient] run \
     scoreboard players set #in_overworld probe 1\n",
    "execute in minecraft:the_nether as @e[tag=probe_ambient] run \
     scoreboard players set #in_nether probe 1\n",
);

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn group5_execution_context_loss() {
    if let Err(error) = run_group5() {
        panic!("{error}");
    }
}

fn run_group5() -> Result<(), String> {
    let (root, mut server) = start(&[
        ("data/mdl_probe/function/setup_ctx.mcfunction", G5_SETUP),
        ("data/mdl_probe/function/mark_ctx.mcfunction", G5_MARK_CTX),
    ])?;
    let result = exercise_group5(&mut server);
    finish(server, root, result)
}

fn read_bool_storage(server: &mut TestServer, path: &str) -> Result<String, String> {
    command(server, &format!("data get storage mdl_probe:ctx {path}"))?;
    let line = server
        .wait_for_command_log(" has the following contents: ")
        .map_err(|error| error.to_string())?;
    line.rsplit_once("contents: ")
        .map(|(_, tail)| tail.to_owned())
        .ok_or_else(|| format!("could not parse storage feedback: {line:?}"))
}

fn read_ambient_marker_position(server: &mut TestServer) -> Result<String, String> {
    command(server, "data get entity @e[tag=probe_ambient,limit=1] Pos")?;
    let line = server
        .wait_for_command_log(" has the following entity data: ")
        .map_err(|error| error.to_string())?;
    line.rsplit_once("entity data: ")
        .map(|(_, tail)| tail.to_owned())
        .ok_or_else(|| format!("could not parse entity feedback: {line:?}"))
}

fn exercise_group5(server: &mut TestServer) -> Result<(), String> {
    command(server, "forceload add 0 0")?;
    server
        .wait_for_command_log("Marked chunk [0, 0]")
        .map_err(|error| error.to_string())?;
    command(server, "function mdl_probe:setup_ctx")?;
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;

    // M14: schedule `as`/`at` a real entity positioned away from the origin
    // (5, 200, 5), then observe what the fired callback's ambient context
    // actually is.
    command(
        server,
        "execute as @e[tag=probe_source] at @s run schedule function mdl_probe:mark_ctx 1t replace",
    )?;
    step_and_settle(server, 1)?;
    let has_self = read_bool_storage(server, "has_self")?;
    let position = read_ambient_marker_position(server)?;
    let in_overworld = read_score(server, "#in_overworld")?;
    eprintln!(
        "M14: has_self={has_self}, ambient position={position}, in_overworld={in_overworld} \
         (source entity was at [5.0d, 200.0d, 5.0d] in overworld)"
    );
    if has_self != "0b" {
        return Err(format!(
            "M14 FAILED: expected the fired callback to have no executor (has_self=0b), got \
             has_self={has_self}"
        ));
    }

    // M15: attempted but NOT trusted -- see `recon_dimension_selector_scoping`.
    // `execute in <dim> if entity @e[...]`/`in <dim> as @e[...]` were found
    // to not reliably filter by dimension in this harness's test conditions
    // (both overworld and nether checks matched the same, definitely-single,
    // overworld-resident entity; a subsequent attempt to force a genuine
    // cross-dimension relocation via `tp` then made BOTH checks report
    // absence instead, most likely because the destination nether chunk was
    // never `forceload`ed and the entity became untracked). This means the
    // in_overworld/in_nether readings below are NOT reliable evidence of
    // which dimension the fired callback actually executed in, and M15 is
    // left as an open measurement needing a redesigned method (forceload the
    // target chunk in BOTH dimensions before testing) rather than reported
    // as a settled fact.
    command(server, "kill @e[tag=probe_ambient]")?;
    set_score(server, "#in_overworld", 0)?;
    set_score(server, "#in_nether", 0)?;
    command(
        server,
        "execute in minecraft:the_nether run schedule function mdl_probe:mark_ctx 1t replace",
    )?;
    step_and_settle(server, 1)?;
    let in_overworld_after_nether_schedule = read_score(server, "#in_overworld")?;
    let in_nether_after_nether_schedule = read_score(server, "#in_nether")?;
    eprintln!(
        "M15 (UNRELIABLE, see comment above): after scheduling from `in minecraft:the_nether`, \
         raw dimension-check readings were overworld={in_overworld_after_nether_schedule}, \
         nether={in_nether_after_nether_schedule} -- not a trustworthy answer"
    );

    command(server, "tick unfreeze")?;
    server
        .wait_for_command_log("running normally")
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 6 (M17): schedule-self-first robustness under a mid-tick abort
// ---------------------------------------------------------------------------

const G6_INIT: &str = concat!(
    "scoreboard objectives add probe dummy\n",
    "scoreboard players set #abort_fire_count probe 0\n",
    "scoreboard players set #abort_reached_end probe 0\n",
);

fn g6_selfabort_body() -> String {
    let mut body = String::new();
    body.push_str("schedule function mdl_probe:selfabort 1t replace\n");
    body.push_str("scoreboard players add #abort_fire_count probe 1\n");
    // Padding so a low `max_command_sequence_length` stops execution well
    // before the final line, proving the abort is genuinely mid-function.
    for _ in 0..20 {
        body.push_str("scoreboard players add #abort_padding probe 1\n");
    }
    body.push_str("scoreboard players add #abort_reached_end probe 1\n");
    body
}

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn group6_schedule_self_first_survives_abort() {
    if let Err(error) = run_group6() {
        panic!("{error}");
    }
}

fn run_group6() -> Result<(), String> {
    let selfabort_body = g6_selfabort_body();
    let (root, mut server) = start(&[
        ("data/mdl_probe/function/init.mcfunction", G6_INIT),
        ("data/mdl_probe/function/selfabort.mcfunction", &selfabort_body),
    ])?;
    let result = exercise_group6(&mut server);
    finish(server, root, result)
}

fn exercise_group6(server: &mut TestServer) -> Result<(), String> {
    command(server, "function mdl_probe:init")?;
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;
    // Low enough that `schedule` (op1) and the fire-count increment (op2)
    // execute but the function is cut off long before its final line.
    command(server, "gamerule minecraft:max_command_sequence_length 3")?;

    command(server, "schedule function mdl_probe:selfabort 1t replace")?;
    for expected_fires in 1..=3 {
        step_and_settle(server, 1)?;
        let fire_count = read_score(server, "#abort_fire_count")?;
        let reached_end = read_score(server, "#abort_reached_end")?;
        eprintln!(
            "M17 step {expected_fires}: abort_fire_count={fire_count}, \
             abort_reached_end={reached_end}"
        );
        if fire_count != expected_fires {
            return Err(format!(
                "M17 FAILED: expected abort_fire_count={expected_fires} after {expected_fires} \
                 self-rescheduling firings under a low command-sequence limit, got {fire_count} \
                 -- the reschedule call (issued before the abort) did not survive the abort, so \
                 the chain died"
            ));
        }
        if reached_end != 0 {
            return Err(
                "M17 test invalid: the function reached its final line, so the configured \
                 command-sequence limit did not actually abort it mid-function -- this proves \
                 nothing about abort survival"
                    .to_owned(),
            );
        }
    }

    command(server, "gamerule minecraft:max_command_sequence_length 65536")?;
    command(server, "tick unfreeze")?;
    server
        .wait_for_command_log("running normally")
        .map_err(|error| error.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Group 7 (M18): `schedule clear` cleanliness
// ---------------------------------------------------------------------------

const G7_INIT: &str = "scoreboard objectives add probe dummy\nscoreboard players set #clear_count probe 0\n";
const G7_CLEARME: &str = concat!(
    "schedule function mdl_probe:clearme 1t replace\n",
    "scoreboard players add #clear_count probe 1\n",
);

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn group7_schedule_clear_cleanliness() {
    if let Err(error) = run_group7() {
        panic!("{error}");
    }
}

fn run_group7() -> Result<(), String> {
    let (root, mut server) = start(&[
        ("data/mdl_probe/function/init.mcfunction", G7_INIT),
        ("data/mdl_probe/function/clearme.mcfunction", G7_CLEARME),
    ])?;
    let result = exercise_group7(&mut server);
    finish(server, root, result)
}

fn exercise_group7(server: &mut TestServer) -> Result<(), String> {
    command(server, "function mdl_probe:init")?;
    command(server, "tick freeze")?;
    server
        .wait_for_command_log("The game is frozen")
        .map_err(|error| error.to_string())?;

    command(server, "schedule function mdl_probe:clearme 1t replace")?;
    step_and_settle(server, 1)?;
    step_and_settle(server, 1)?;
    let before_clear = read_score(server, "#clear_count")?;
    if before_clear < 1 {
        return Err(format!(
            "M18 test invalid: expected clearme to have fired at least once before `schedule \
             clear`, got clear_count={before_clear}"
        ));
    }

    let checkpoint = server.log_checkpoint();
    command(server, "schedule clear mdl_probe:clearme")?;
    server
        .wait_for_command_log("Removed")
        .map_err(|error| error.to_string())?;
    for _ in 0..5 {
        step_and_settle(server, 1)?;
    }
    let after_clear = read_score(server, "#clear_count")?;
    let noise = server
        .matching_log_lines_since(checkpoint, "xception")
        .map_err(|error| error.to_string())?;
    eprintln!(
        "M18: clear_count before /clear={before_clear}, after 5 more steps={after_clear}; \
         exception-ish log lines since clear: {noise:?}"
    );
    if after_clear != before_clear {
        return Err(format!(
            "M18 FAILED: clear_count changed from {before_clear} to {after_clear} after \
             `schedule clear` with no further schedule call -- a firing occurred that should \
             not have"
        ));
    }
    if !noise.is_empty() {
        return Err(format!(
            "M18 FAILED: unexpected exception-like log output after `schedule clear`: {noise:?}"
        ));
    }

    command(server, "tick unfreeze")?;
    server
        .wait_for_command_log("running normally")
        .map_err(|error| error.to_string())?;
    Ok(())
}
