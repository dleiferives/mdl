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

const SOURCE: &str =
    include_str!("../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps12_entity_nbt_path.mdl");

/// PS-12D: the general schema-typed entity-NBT path, exercised with a
/// genuinely runtime page index, against the real pinned Java 26.2 server --
/// the same live-server bar `ps2_book_semantics.rs` holds the retired
/// hardcoded intrinsic to, now proving the composable replacement produces
/// identical real-server behavior (correct page text, and the fail-soft
/// empty-string fallback when the main-hand item stops being a book).
#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn entity_nbt_path_runtime_index_matches_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("ps12_entity_nbt_path.mdl", SOURCE),
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
            "mdl_ps12_book",
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
    server.command("summon minecraft:armor_stand 0 4 0 {Tags:['mdl_ps12_book'],equipment:{mainhand:{id:'minecraft:written_book',count:1,components:{'minecraft:written_book_content':{pages:[{raw:{text:'ab😀+'}}],title:{raw:'test'},author:'mdl',generation:0,resolved:true}}}}}").unwrap();
    server
        .wait_for_command_log("Summoned new Armor Stand")
        .unwrap();

    server.command(&format!("function {entry}")).unwrap();
    server.wait_for_command_log("MDL_PS12_BOOK_OK").unwrap();

    server
        .command("item replace entity @e[type=minecraft:armor_stand,tag=mdl_ps12_book,limit=1] weapon.mainhand with minecraft:stone")
        .unwrap();
    server.wait_for_command_log("Replaced").unwrap();
    let checkpoint = server.log_checkpoint();
    server.command(&format!("function {entry}")).unwrap();
    server.command("say MDL_PS12_BOOK_BARRIER").unwrap();
    server
        .wait_for_command_log("MDL_PS12_BOOK_BARRIER")
        .unwrap();
    assert!(
        server
            .matching_log_lines_since(checkpoint, "MDL_PS12_BOOK_OK")
            .unwrap()
            .is_empty(),
        "wrong-item fallback retained a stale successful page"
    );
    server.shutdown().unwrap();
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl_ps12_book").unwrap(),
        ObjectiveName::new("mps12.book").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL PS-12 entity-NBT path intrinsic"),
    )
}
