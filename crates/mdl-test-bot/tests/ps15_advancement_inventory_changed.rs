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
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps15_advancement_inventory_changed.mdl"
);

/// PS-15: this milestone's own calibrated evidence -- the first push-model
/// event this compiler has ever proven against a real connected player.
/// `CoreEvaluator` has no player or advancement-state concept at all, so
/// there is no honest Core-evaluator differential for this feature (unlike
/// every earlier PS milestone); the pinned server is the only real gate.
///
/// A real bot connects and is `/give`n the item Slice 1's `inventory_changed`
/// criterion is watching. The reward function is not called directly (there
/// is no source-level entry point to invoke) -- it only ever runs because
/// vanilla's own advancement system reacts to the criterion becoming
/// satisfied, exactly the "push, not pull" shift this milestone adds.
///
/// The entire point of this test is the *second* give: every advancement
/// fires its reward exactly once by default, revoke or not, so a test that
/// only checked the first fire would validate nothing beyond vanilla's own
/// baseline behavior. Only a working `advancement revoke @s only <id>`
/// auto-revoke (the compiler-injected first command of the reward body)
/// lets the same criterion satisfy and fire again.
///
/// Also live-checks `advancement-triggers.md`'s "omitting `display` makes it
/// fully hidden" claim, which the design notes cite from wiki research, not
/// a direct measurement: no advancement chat broadcast must appear.
#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and nightly Rust"]
fn inventory_changed_advancement_reward_refires_after_auto_revoke() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("ps15_advancement_inventory_changed.mdl", SOURCE),
        &options(),
    )
    .unwrap();

    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).expect("create sandbox");
    sandbox
        .install_datapack(
            "ps15_advancement",
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

    // Stage 1: give the watched item once. The criterion fires and the
    // reward's `say` marker must reach the console log. `minecraft:diamond`
    // is also the trigger item of vanilla's own real, visible "Diamonds!"
    // advancement (`minecraft:story/mine_diamond`, confirmed live below by
    // its broadcast) -- this first give's log window is confounded by that
    // unrelated, one-shot-forever vanilla advancement, so the hidden-ness
    // check happens in Stage 2 instead, where vanilla's own copy is already
    // permanently earned and can never broadcast again.
    server
        .command("give mdl-test-bot minecraft:diamond 1")
        .expect("give diamond");
    server
        .wait_for_command_log("MDL_GOT_DIAMOND")
        .expect("first inventory_changed criterion should fire the reward");

    // Stage 2 -- the actual non-negotiable proof: repeat the identical
    // action and confirm the reward fires a SECOND time. This is the only
    // thing that distinguishes a working auto-revoke from vanilla's default
    // one-shot-forever behavior.
    let checkpoint = server.log_checkpoint();
    server
        .command("give mdl-test-bot minecraft:diamond 1")
        .expect("give a second diamond");
    server
        .wait_for_command_log("MDL_GOT_DIAMOND")
        .expect("auto-revoke should let the criterion re-fire the reward a second time");

    // Live-check the hidden-advancement claim on this second, uncontaminated
    // window: vanilla's own "Diamonds!" was already permanently earned in
    // Stage 1 and cannot broadcast again, so any advancement-completion
    // broadcast seen here could only come from MDL's own hidden advancement
    // re-firing -- which must never happen, since it has no `display` key.
    let broadcast_lines = server
        .matching_log_lines_since(checkpoint, "has made the advancement")
        .unwrap();
    assert!(
        broadcast_lines.is_empty(),
        "a hidden advancement (no display key) must never broadcast a chat/toast line, saw: {broadcast_lines:?}"
    );

    // `BotHandle::shutdown` can legitimately time out after any real
    // interaction (PS-13's own measured, still-unroot-caused rough edge) --
    // logged, not a hard failure, matching `ps14_player_equipment_read.rs`'s
    // established pattern.
    if let Err(error) = bot.shutdown() {
        eprintln!(
            "[ps15_advancement_inventory_changed] bot shutdown did not complete cleanly: {error}"
        );
    }
    server.shutdown().expect("shutdown server");
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps15_advancement").unwrap(),
        ObjectiveName::new("ps15.advancement").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-15 advancement inventory_changed"),
    )
}
