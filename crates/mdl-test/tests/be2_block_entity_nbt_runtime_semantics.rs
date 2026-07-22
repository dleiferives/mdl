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

const SOURCE: &str = include_str!(
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/be2_block_entity_nbt_runtime_slot.mdl"
);

/// BE-1 Slice 2: a genuinely runtime container slot against the real pinned
/// Java 26.2 server. Manually verified once already (spinning up the server
/// by hand) that a macro `$(key)` substitution always renders as bare
/// decimal text regardless of the NBT tag the bridged value was stored
/// with -- the `Byte` type suffix a chest's `Slot` field needs must be a
/// literal character in the command template (`$(i0)b`), not derived from
/// the stored macro-argument value. That measured finding is what
/// `crossings.rs::build_entity_nbt_read_line` now encodes; this test is the
/// automated regression proving it stays correct end to end, including
/// through the ABI (a real function parameter feeding the runtime slot).
#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn runtime_chest_slot_read_matches_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("be2_block_entity_nbt_runtime_slot.mdl", SOURCE),
        &options(),
    )
    .unwrap();
    let source_function = output
        .checked_frontend()
        .function_ids()
        .find(|function| output.source_function_abi(*function).is_some())
        .unwrap();
    let abi = output.source_function_abi(source_function).unwrap();
    let entry = abi.entry_resource().to_string();
    let [(_, slot_slot)] = abi.parameter_homes() else {
        panic!("read_chest_slot must have exactly one parameter");
    };
    let objective = slot_slot.objective().to_string();
    let holder = slot_slot.holder().to_string();

    let sandbox = ServerSandbox::create(env::var_os("MDL_KEEP_TEST_DIR").is_some()).unwrap();
    sandbox
        .install_datapack(
            "be2_chest",
            output
                .emission()
                .pack()
                .files()
                .iter()
                .map(|file| (file.path().as_str(), file.bytes())),
        )
        .unwrap();
    let mut server = sandbox.start(&ServerConfig::new(java, server_jar)).unwrap();

    server.command("forceload add 0 0").unwrap();
    server.wait_for_command_log("Marked chunk [0, 0]").unwrap();
    server.command("setblock 0 4 0 minecraft:chest").unwrap();
    server
        .command("item replace block 0 4 0 container.3 with minecraft:diamond 5")
        .unwrap();
    server.wait_for_command_log("Replaced").unwrap();

    server
        .command(&format!("scoreboard players set {holder} {objective} 3"))
        .unwrap();
    server.command(&format!("function {entry}")).unwrap();
    server.wait_for_command_log("BE2_STACK").unwrap();

    // A different (empty) slot must fail-soft to the default, not retain a
    // stale successful read from the earlier occupied-slot call.
    server
        .command(&format!("scoreboard players set {holder} {objective} 7"))
        .unwrap();
    let checkpoint = server.log_checkpoint();
    server.command(&format!("function {entry}")).unwrap();
    server.command("say BE2_BARRIER").unwrap();
    server.wait_for_command_log("BE2_BARRIER").unwrap();
    assert!(
        server
            .matching_log_lines_since(checkpoint, "BE2_STACK")
            .unwrap()
            .is_empty(),
        "empty-slot fail-soft fallback retained a stale successful read"
    );

    server.shutdown().unwrap();
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("be2_chest").unwrap(),
        ObjectiveName::new("be2.chest").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("BE-1 Slice 2 runtime block-entity NBT semantics"),
    )
}
