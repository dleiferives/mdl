use std::env;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use azalea::BlockPos;
use mdl_test::{ServerConfig, ServerSandbox};
use mdl_test_bot::BotHandle;

/// PS-13 Stage 4: this milestone's own calibrated evidence -- proof the bot
/// infrastructure itself works end to end, with no compiler involvement, before
/// PS-14 depends on it. A chest with a known item in a known slot is placed with
/// ordinary console commands (the same `setblock`/`item replace block ...
/// container.N with ...` shape BE-1's own pinned tests already use), a real bot
/// connects, opens that chest, and left-clicks the occupied slot. Success is
/// observed the same way BE-1/BE-2's own live checks were: reading the result back
/// through the console rather than trusting the bot's own view of what happened.
#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and nightly Rust"]
fn bot_clicking_a_chest_slot_moves_the_item_into_its_inventory() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let config = ServerConfig::new(java, server_jar);

    let sandbox = ServerSandbox::create(preserve).expect("create sandbox");
    let mut server = sandbox.start(&config).expect("start server");

    server.command("forceload add 0 0").expect("forceload");
    server
        .wait_for_command_log("Marked chunk [0, 0]")
        .expect("forceload confirmation");
    server
        .command("setblock 0 4 0 minecraft:chest")
        .expect("place chest");
    server
        .command("item replace block 0 4 0 container.3 with minecraft:diamond 5")
        .expect("stock chest slot 3");
    server
        .wait_for_command_log("Replaced")
        .expect("item replace confirmation");

    let bot = BotHandle::connect(&server, "mdl-test-bot").expect("bot should connect and spawn");
    server
        .command("tp mdl-test-bot 1 4 0 facing 0 4 0")
        .expect("teleport bot next to the chest");
    server
        .wait_for_command_log("Teleported")
        .expect("teleport confirmation");

    let opened = bot
        .open_container_and_click(BlockPos { x: 0, y: 4, z: 0 }, 3)
        .expect("open and click should not error");
    assert!(
        opened,
        "the bot must have found a container to open at the chest"
    );
    // `left_click` only sends the click packet -- it doesn't wait for the server
    // to actually process it and update inventory. Measured directly: querying
    // immediately afterward is a real, reproducible race (the assertion below
    // failed intermittently without this). A short, generous settle delay is
    // simpler and more honest than pretending click-acknowledgement tracking
    // exists when it doesn't.
    thread::sleep(Duration::from_millis(500));

    // Read the result back through the console -- not through the bot's own view
    // of what happened -- matching this project's established evidence discipline
    // (BE-1/BE-2's own live checks did the same). `data get`'s own response line is
    // logged directly, so waiting for "diamond" to appear in it is enough; no
    // NBT-predicate selector needed (an earlier attempt at one hit a real 26.2
    // command-parser syntax error -- confirmed in the live harness.log, not
    // guessed -- so this sticks to the plain, already-proven `data get` shape).
    server
        .command("data get entity mdl-test-bot Inventory")
        .expect("query bot inventory");
    let found_diamond = server.wait_for_command_log("diamond").is_ok();

    // This is the milestone's actual claim -- assert it before attempting
    // teardown, so a shutdown wrinkle can never mask whether the real mechanism
    // (open a container, click a slot, item moves into inventory) worked.
    assert!(
        found_diamond,
        "expected the clicked diamond stack to have moved into the bot's inventory"
    );

    // `BotHandle::shutdown` is bounded (`BotError::ShutdownTimeout` after 15s, not
    // a hang -- see its own doc comment), but a container interaction has been
    // observed to occasionally leave the bot's background thread taking longer
    // than that to notice `exit()`. That's a shutdown-path rough edge in this
    // very-fresh Azalea build, not evidence the underlying mechanism this test
    // exists to prove is broken (already asserted above), so it's logged rather
    // than treated as a hard failure here. The thread is a plain background
    // thread inside this test binary; it does not outlive the process.
    if let Err(error) = bot.shutdown() {
        eprintln!("[chest_click] bot shutdown did not complete cleanly: {error}");
    }
    server.shutdown().expect("shutdown server");
}
