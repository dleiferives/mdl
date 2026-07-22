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
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps12e_entity_nbt_scalar_fields.mdl"
);

/// PS-12E: the `Bool`/`Int32` entity-NBT scalar route (`.resolved`, `.count`
/// -- neither behind a runtime list index, so both always take the inline
/// scratch/score-conversion shape: `execute store result score ... run data
/// get storage <scratch>`) exercised against the real pinned Java 26.2
/// server. This route was previously only checked structurally
/// (`ps12c_entity_nbt_path_lowering.rs`); it had never been proven live,
/// which is exactly the gap that let the sibling macro/runtime String
/// route's leading-dot bug ship unnoticed. Three states in one lifecycle:
/// a real `resolved=true`/`count=1` book, a real `resolved=false`/`count=5`
/// book (forcing a stack size real survival wouldn't produce, to exercise
/// the `count > 1` branch), and a fail-soft wrong-item fallback.
#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn entity_nbt_scalar_fields_match_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("ps12e_entity_nbt_scalar_fields.mdl", SOURCE),
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
            "mdl_ps12e_scalar",
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

    // Stage 1: a real resolved book, count 1 -> resolved-only branch.
    server.command("summon minecraft:armor_stand 0 4 0 {Tags:['mdl_ps12e_scalar'],equipment:{mainhand:{id:'minecraft:written_book',count:1,components:{'minecraft:written_book_content':{pages:[{raw:{text:'x'}}],title:{raw:'t'},author:'a',generation:0,resolved:true}}}}}").unwrap();
    server
        .wait_for_command_log("Summoned new Armor Stand")
        .unwrap();
    server.command(&format!("function {entry}")).unwrap();
    server
        .wait_for_command_log("MDL_PS12E_SCALAR_RESOLVED_ONLY")
        .unwrap();

    // Stage 2: an unresolved book stacked past 1 -> count-only branch.
    server.command("item replace entity @e[type=minecraft:armor_stand,tag=mdl_ps12e_scalar,limit=1] weapon.mainhand with minecraft:written_book 5").unwrap();
    server.command("data merge entity @e[type=minecraft:armor_stand,tag=mdl_ps12e_scalar,limit=1] {equipment:{mainhand:{components:{'minecraft:written_book_content':{pages:[{raw:{text:'x'}}],title:{raw:'t'},author:'a',generation:0,resolved:false}}}}}").unwrap();
    let checkpoint = server.log_checkpoint();
    server.command(&format!("function {entry}")).unwrap();
    server.command("say MDL_PS12E_SCALAR_BARRIER_1").unwrap();
    server
        .wait_for_command_log("MDL_PS12E_SCALAR_BARRIER_1")
        .unwrap();
    let stage2 = server
        .matching_log_lines_since(checkpoint, "MDL_PS12E_SCALAR_")
        .unwrap();
    assert!(
        stage2.iter().any(|line| line.contains("COUNT_ONLY")),
        "stage 2 (unresolved, count=5) did not hit the count-only branch: {stage2:?}"
    );

    // Stage 3: fail-soft wrong-item fallback -> both defaults, neither branch.
    server.command("item replace entity @e[type=minecraft:armor_stand,tag=mdl_ps12e_scalar,limit=1] weapon.mainhand with minecraft:stone").unwrap();
    server.command("data remove entity @e[type=minecraft:armor_stand,tag=mdl_ps12e_scalar,limit=1] equipment.mainhand.components").unwrap();
    let checkpoint = server.log_checkpoint();
    server.command(&format!("function {entry}")).unwrap();
    server.command("say MDL_PS12E_SCALAR_BARRIER_2").unwrap();
    server
        .wait_for_command_log("MDL_PS12E_SCALAR_BARRIER_2")
        .unwrap();
    let stage3 = server
        .matching_log_lines_since(checkpoint, "MDL_PS12E_SCALAR_")
        .unwrap();
    assert!(
        stage3.iter().any(|line| line.contains("NEITHER")),
        "stage 3 (wrong item) did not fail-soft to the neither branch: {stage3:?}"
    );

    server.shutdown().unwrap();
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl_ps12e_scalar").unwrap(),
        ObjectiveName::new("mps12e.scalar").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL PS-12E entity-NBT scalar fields"),
    )
}
