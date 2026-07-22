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
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps12e_entity_nbt_loop_index.mdl"
);

/// PS-12E: a genuinely loop-carried runtime index against the real pinned
/// Java 26.2 server -- the entity-NBT read instruction fires three times in
/// one function execution, each time with a different runtime `i`, summing
/// three real pages' lengths (2 + 2 + 2 = 6). Every earlier runtime-index
/// server test (`ps12_entity_nbt_path_semantics.rs`) exercises exactly one
/// occurrence per run; this is the strongest available live proof that the
/// macro/crossings engine is correct across repeated per-iteration calls
/// with distinct operand values, not just a single call site.
#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn entity_nbt_loop_index_matches_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("ps12e_entity_nbt_loop_index.mdl", SOURCE),
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
            "mdl_ps12e_loop",
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
    server.command("summon minecraft:armor_stand 0 4 0 {Tags:['mdl_ps12e_loop'],equipment:{mainhand:{id:'minecraft:written_book',count:1,components:{'minecraft:written_book_content':{pages:[{raw:{text:'ab'}},{raw:{text:'cd'}},{raw:{text:'ef'}}],title:{raw:'test'},author:'mdl',generation:0,resolved:true}}}}}").unwrap();
    server
        .wait_for_command_log("Summoned new Armor Stand")
        .unwrap();

    server.command(&format!("function {entry}")).unwrap();
    server.wait_for_command_log("MDL_PS12E_LOOP_OK").unwrap();

    server.shutdown().unwrap();
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl_ps12e_loop").unwrap(),
        ObjectiveName::new("mps12el.loop").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL PS-12E entity-NBT loop-carried index"),
    )
}
