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
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/be1_block_entity_nbt_path.mdl"
);

/// BE-1: a chest-content read against the real pinned Java 26.2 server.
/// Manually verified once already (spinning up the server by hand) that the
/// original design's assumed NBT shape (`components."minecraft:container"`,
/// matched by a lowercase `slot`) was wrong for a *placed* block entity --
/// the real shape is a flat top-level `Items` list matched by a `Byte`-typed
/// `Slot` field, with no nested "item" wrapper. That measured correction is
/// what this fixture and the schema table now encode; this test is the
/// automated regression proving it stays correct.
#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn chest_count_read_matches_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("be1_block_entity_nbt_path.mdl", SOURCE),
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
    let sandbox = ServerSandbox::create(env::var_os("MDL_KEEP_TEST_DIR").is_some()).unwrap();
    sandbox
        .install_datapack(
            "be1_chest",
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
        .command("item replace block 0 4 0 container.0 with minecraft:diamond 5")
        .unwrap();
    server.wait_for_command_log("Replaced").unwrap();

    server.command(&format!("function {entry}")).unwrap();
    server.wait_for_command_log("BE1_STACK").unwrap();

    // Fail-soft: replacing the chest with an ordinary block leaves no
    // Items list at all -- the read must keep its default (0), not error.
    server.command("setblock 0 4 0 minecraft:stone").unwrap();
    let checkpoint = server.log_checkpoint();
    server.command(&format!("function {entry}")).unwrap();
    server.command("say BE1_BARRIER").unwrap();
    server.wait_for_command_log("BE1_BARRIER").unwrap();
    assert!(
        server
            .matching_log_lines_since(checkpoint, "BE1_STACK")
            .unwrap()
            .is_empty(),
        "wrong-block fail-soft fallback retained a stale successful read"
    );

    server.shutdown().unwrap();
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("be1_chest").unwrap(),
        ObjectiveName::new("be1.chest").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("BE-1 block-entity NBT semantics"),
    )
}
