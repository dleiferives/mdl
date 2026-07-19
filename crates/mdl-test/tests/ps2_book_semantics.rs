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
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps2_written_book_page.mdl"
);

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn written_book_literal_page_shape_and_string_slices_match_java_26_2() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("ps2_written_book_page.mdl", SOURCE),
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
            "mdl_ps2_book",
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
    server.command("summon minecraft:armor_stand 0 4 0 {Tags:['mdl_ps2_book'],equipment:{mainhand:{id:'minecraft:written_book',count:1,components:{'minecraft:written_book_content':{pages:[{raw:{text:'ab😀+'}}],title:{raw:'test'},author:'mdl',generation:0,resolved:true}}}}}").unwrap();
    server
        .wait_for_command_log("Summoned new Armor Stand")
        .unwrap();
    server.command("data modify storage mdl:ps2 whole set from entity @e[type=minecraft:armor_stand,tag=mdl_ps2_book,limit=1] equipment.mainhand.components.'minecraft:written_book_content'.pages[0].raw").unwrap();
    server
        .wait_for_command_log("Modified storage mdl:ps2")
        .unwrap();
    server
        .command("data modify storage mdl:ps2 tail set string storage mdl:ps2 whole -1")
        .unwrap();
    server
        .wait_for_command_log("Modified storage mdl:ps2")
        .unwrap();
    server
        .command("data modify storage mdl:ps2 prefix set string storage mdl:ps2 whole 0 -1")
        .unwrap();
    server
        .wait_for_command_log("Modified storage mdl:ps2")
        .unwrap();
    server.command("data get storage mdl:ps2").unwrap();
    let line = server
        .wait_for_command_log("mdl:ps2 has the following contents")
        .unwrap();
    assert!(line.contains("tail: \"+\""), "{line}");
    assert!(line.contains("prefix: \"ab😀\""), "{line}");
    server.command(&format!("function {entry}")).unwrap();
    server.wait_for_command_log("MDL_PS2_BOOK_OK").unwrap();
    server
        .command("item replace entity @e[type=minecraft:armor_stand,tag=mdl_ps2_book,limit=1] weapon.mainhand with minecraft:stone")
        .unwrap();
    server.wait_for_command_log("Replaced").unwrap();
    let checkpoint = server.log_checkpoint();
    server.command(&format!("function {entry}")).unwrap();
    server.command("say MDL_PS2_BOOK_BARRIER").unwrap();
    server.wait_for_command_log("MDL_PS2_BOOK_BARRIER").unwrap();
    assert!(
        server
            .matching_log_lines_since(checkpoint, "MDL_PS2_BOOK_OK")
            .unwrap()
            .is_empty(),
        "wrong-item fallback retained a stale successful page"
    );
    server.shutdown().unwrap();
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl_ps2_book").unwrap(),
        ObjectiveName::new("mps2.book").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL PS-2 written-book intrinsic"),
    )
}
