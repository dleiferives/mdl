use std::env;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use azalea::BlockPos;
use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox};
use mdl_test_bot::BotHandle;

const SOURCE: &str = include_str!(
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps17_chest_menu_capstone.mdl"
);

const CHEST_POS: BlockPos = BlockPos { x: 0, y: 4, z: 0 };

/// PS-17: the capstone the entire PS-13-17 sequence was built toward. A real
/// bot opens a real placed chest and clicks through both of its "screens":
/// slot 0 (emerald) teleports the player, slot 1 (gold ingot) rewrites the
/// chest to reveal screen 2's only button, and clicking that new button (a
/// redstone block that did not exist anywhere in the world until PS-16's
/// write created it) grants a reward item. Every assertion below is read
/// back through the server console, not the bot's own view of what
/// happened, matching this project's established evidence discipline
/// (BE-1/BE-2, PS-13's `chest_click.rs`, PS-14/PS-15's own bot tests).
///
/// No Core-evaluator differential -- same reasoning as PS-15: `CoreEvaluator`
/// has no player/advancement/block-write state to simulate honestly.
#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and nightly Rust"]
fn chest_menu_capstone_walkthrough_matches_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("ps17_chest_menu_capstone.mdl", SOURCE),
        &options(),
    )
    .unwrap();

    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).expect("create sandbox");
    sandbox
        .install_datapack(
            "ps17_chest_menu",
            output
                .emission()
                .pack()
                .files()
                .iter()
                .map(|file| (file.path().as_str(), file.bytes())),
        )
        .expect("install datapack");
    let mut server = sandbox
        .start(&ServerConfig::new(java, server_jar))
        .expect("start server");

    server.command("forceload add 0 0").expect("forceload");
    server
        .wait_for_command_log("Marked chunk [0, 0]")
        .expect("forceload confirmation");
    server
        .command("setblock 0 4 0 minecraft:chest")
        .expect("place chest");

    // Screen 1: slot 0 = emerald (teleport button), slot 1 = gold ingot
    // (advance-to-screen-2 button).
    server
        .command("item replace block 0 4 0 container.0 with minecraft:emerald 1")
        .expect("stock emerald button");
    server
        .wait_for_command_log("Replaced")
        .expect("item replace confirmation");
    server
        .command("item replace block 0 4 0 container.1 with minecraft:gold_ingot 1")
        .expect("stock gold ingot button");
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

    // Button 1 (emerald): teleport. `open_container_and_click` only sends
    // the click packet -- it doesn't wait for the server to actually
    // process it, so a short settle delay before asserting is required
    // (measured directly as a real, reproducible race in `chest_click.rs`,
    // this milestone's own precedent for the click mechanism).
    let opened = bot
        .open_container_and_click(CHEST_POS, 0)
        .expect("open and click should not error");
    assert!(opened, "the bot must have found the chest to open");
    thread::sleep(Duration::from_millis(500));
    server
        .wait_for_command_log("MDL_TELEPORTED")
        .expect("emerald click should fire the teleport handler");
    server
        .command(
            "execute positioned 10 -60 10 if entity @a[name=mdl-test-bot,distance=..0.01] run say MDL_POS_OK",
        )
        .expect("query bot position");
    server
        .wait_for_command_log("MDL_POS_OK")
        .expect("the teleport must have actually moved the player, not just fired the handler");

    // The clicked emerald is still sitting in the bot's own inventory (this
    // milestone's own non-goals explicitly skip clearing it as cosmetic).
    // Measured directly: with the emerald still held, the very next `tp`
    // unexpectedly re-satisfies the auto-revoked `inventory_changed`
    // criterion and re-runs the handler mid-reposition, which destabilized
    // the bot's connection in an earlier run of this exact test. Clearing it
    // first removes that spurious retrigger before repositioning.
    server
        .command("clear mdl-test-bot minecraft:emerald")
        .expect("clear the emerald so repositioning cannot spuriously retrigger the handler");
    server.command("say MDL_CLEAR_BARRIER").unwrap();
    server
        .wait_for_command_log("MDL_CLEAR_BARRIER")
        .expect("clear barrier");

    // The handler's own teleport just moved the bot away from the chest --
    // walk it back into reach before the next click, the same way it was
    // placed there initially. A real player would move back on their own;
    // this just automates that same physical step.
    server
        .command("tp mdl-test-bot 1 4 0 facing 0 4 0")
        .expect("teleport bot back to the chest");
    server
        .wait_for_command_log("Teleported")
        .expect("teleport confirmation");

    // Button 2 (gold ingot): rewrite the chest to reveal screen 2. This is
    // the write PS-16 exists for -- the real proof is the re-read after,
    // not just that the handler ran.
    let checkpoint = server.log_checkpoint();
    let opened = bot
        .open_container_and_click(CHEST_POS, 1)
        .expect("open and click should not error");
    assert!(opened, "the bot must have found the chest to open");
    thread::sleep(Duration::from_millis(500));
    server
        .wait_for_command_log("MDL_SCREEN_2")
        .expect("gold ingot click should fire the screen-transition handler");
    server
        .command("data get block 0 4 0 Items")
        .expect("query chest contents");
    server
        .wait_for_command_log("redstone_block")
        .expect("slot 1 must now hold the redstone block the write created");
    assert!(
        server
            .matching_log_lines_since(checkpoint, "Serialization errors")
            .unwrap()
            .is_empty(),
        "the screen-transition write must not trip a Serialization errors warning \
         (this write lands on a slot click processing just emptied -- BE-2's own \
         confirmed-safe unoccupied-slot case)"
    );
    assert!(
        server
            .matching_log_lines_since(checkpoint, "minecraft:emerald")
            .unwrap()
            .is_empty(),
        "slot 0 must have been cleared to air by the write, not merely emptied by the earlier click"
    );

    // Button on screen 2 (redstone block, did not exist until the write
    // above created it): the reward. Clicking it succeeding is real,
    // end-to-end proof the write worked -- not just that it compiled.
    let opened = bot
        .open_container_and_click(CHEST_POS, 1)
        .expect("open and click should not error");
    assert!(
        opened,
        "the bot must have found the chest to open, now showing the redstone block"
    );
    thread::sleep(Duration::from_millis(500));
    server
        .wait_for_command_log("MDL_REWARD")
        .expect("redstone block click should fire the reward handler");
    server
        .command("data get entity mdl-test-bot Inventory")
        .expect("query bot inventory");
    server
        .wait_for_command_log("stick")
        .expect("the reward stick must have actually landed in the bot's inventory");

    // `BotHandle::shutdown` can legitimately time out after any real
    // container interaction (PS-13's own measured, still-unroot-caused
    // rough edge) -- logged, not a hard failure, matching every other
    // PS-13+ bot test's established pattern.
    if let Err(error) = bot.shutdown() {
        eprintln!("[ps17_chest_menu_capstone] bot shutdown did not complete cleanly: {error}");
    }
    server.shutdown().expect("shutdown server");
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps17_chest_menu").unwrap(),
        ObjectiveName::new("ps17.chest_menu").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-17 chest-menu capstone"),
    )
}
