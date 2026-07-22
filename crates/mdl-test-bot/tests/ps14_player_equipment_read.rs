use std::env;
use std::path::PathBuf;

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
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps14_player_equipment_read.mdl"
);

/// PS-14: this milestone's own calibrated evidence -- the first entity-NBT
/// read this compiler has ever proven against a genuinely connected player
/// rather than an ArmorStand. A real bot connects, is given a diamond
/// chestplate through the same `item replace entity ... armor.chest with
/// ...` console command the design doc's own live measurement used, and the
/// compiled `mc.entities(Player)` program reads `.equipment.chest.count`
/// back. Success is read from the console log (`MDL_PS14_PLAYER_CHEST_OK`),
/// not the bot's own view of what happened, matching this project's
/// established evidence discipline (BE-1, PS-12E, PS-13's `chest_click.rs`).
/// A second stage removes the chestplate and re-invokes to prove the
/// fail-soft default (count defaults to 0, so the `count > 0` branch does not
/// fire) still holds for a player receiver, not just an ArmorStand one.
#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and nightly Rust"]
fn player_chest_equipment_read_matches_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("ps14_player_equipment_read.mdl", SOURCE),
        &options(),
    )
    .unwrap();
    let source_function = output
        .checked_frontend()
        .function_ids()
        .find(|function| output.source_function_abi(*function).is_some())
        .unwrap();
    let entry = output
        .source_function_abi(source_function)
        .unwrap()
        .entry_resource()
        .to_string();

    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).expect("create sandbox");
    sandbox
        .install_datapack(
            "ps14_player",
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

    let bot = BotHandle::connect(&server, "mdl-test-bot").expect("bot should connect and spawn");
    server
        .command("tp mdl-test-bot 0 4 0")
        .expect("teleport bot into the forceloaded chunk");
    server
        .wait_for_command_log("Teleported")
        .expect("teleport confirmation");

    // Stage 1: a real chestplate -> count=1, the success branch fires.
    server
        .command("item replace entity mdl-test-bot armor.chest with minecraft:diamond_chestplate")
        .expect("give chestplate");
    server
        .wait_for_command_log("Replaced")
        .expect("item replace confirmation");
    server.command(&format!("function {entry}")).unwrap();
    server
        .wait_for_command_log("MDL_PS14_PLAYER_CHEST_OK")
        .expect("chest-equipped read should reach the success branch");

    // Stage 2: fail-soft -- an empty chest slot must yield the type-
    // appropriate default (0), not a stale or crashing read.
    server
        .command("item replace entity mdl-test-bot armor.chest with minecraft:air")
        .expect("clear chestplate");
    server
        .wait_for_command_log("Replaced")
        .expect("item replace confirmation");
    let checkpoint = server.log_checkpoint();
    server.command(&format!("function {entry}")).unwrap();
    server.command("say MDL_PS14_PLAYER_BARRIER").unwrap();
    server
        .wait_for_command_log("MDL_PS14_PLAYER_BARRIER")
        .unwrap();
    assert!(
        server
            .matching_log_lines_since(checkpoint, "MDL_PS14_PLAYER_CHEST_OK")
            .unwrap()
            .is_empty(),
        "empty chest slot fail-soft fallback retained a stale successful read"
    );

    // `BotHandle::shutdown` can legitimately time out after any container/
    // entity-state interaction (PS-13's own measured, still-unroot-caused
    // rough edge) -- logged, not treated as a hard failure, matching
    // `chest_click.rs`'s established pattern.
    if let Err(error) = bot.shutdown() {
        eprintln!("[ps14_player_equipment_read] bot shutdown did not complete cleanly: {error}");
    }
    server.shutdown().expect("shutdown server");
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps14_player").unwrap(),
        ObjectiveName::new("ps14.player").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-14 player equipment read"),
    )
}
