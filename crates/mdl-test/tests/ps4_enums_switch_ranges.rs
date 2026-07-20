use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, CompilationOutput, FrontendLimits, FunctionVisibility, SourceInput,
    compile_source,
};
use mdl_compiler::ir::core::CoreType;
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel, RegisterSlot};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer, sha256_file};
use serde::Deserialize;

const SOURCE: &str = include_str!("../../../tests/programs/enums-switch-ranges/src/main.mdl");
const CASES: &str = include_str!("../../../tests/programs/enums-switch-ranges/cases.json");
const SERVER_SHA256: &str = "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";

#[derive(Deserialize)]
struct Case {
    input: i32,
    expected: i32,
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn enum_switch_range_products_match_vanilla_in_one_lifecycle() {
    if let Err(error) = run_server_test() {
        panic!("{error}");
    }
}

fn run_server_test() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let hash = sha256_file(&server_jar).map_err(|error| error.to_string())?;
    if ![SERVER_SHA256, EXTRACTED_SERVER_SHA256].contains(&hash.as_str()) {
        return Err(format!("unexpected server SHA-256: {hash}"));
    }
    let policies = [
        (
            CoreOptimizationLevel::None,
            MinecraftOptimizationLevel::None,
        ),
        (
            CoreOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ),
        (
            CoreOptimizationLevel::Baseline,
            MinecraftOptimizationLevel::None,
        ),
        (
            CoreOptimizationLevel::Baseline,
            MinecraftOptimizationLevel::Baseline,
        ),
    ];
    let compiled = policies
        .into_iter()
        .enumerate()
        .map(|(index, (core, minecraft))| compile_policy(index, core, minecraft))
        .collect::<Result<Vec<_>, _>>()?;
    let sandbox = ServerSandbox::create(env::var_os("MDL_KEEP_TEST_DIR").is_some())
        .map_err(|error| error.to_string())?;
    for (namespace, output) in &compiled {
        sandbox
            .install_datapack(
                namespace,
                output
                    .emission()
                    .pack()
                    .files()
                    .iter()
                    .map(|file| (file.path().as_str(), file.bytes())),
            )
            .map_err(|error| error.to_string())?;
    }
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let mut server = sandbox
        .start(&ServerConfig::new(java, server_jar))
        .map_err(|error| error.to_string())?;
    server
        .command("forceload add 0 0")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Marked chunk [0, 0]")
        .map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    if let Err(error) = exercise(&mut server, &compiled) {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nPS-4 sandbox preserved at {}",
            root.display()
        ));
    }
    server
        .command("forceload remove 0 0")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Unmarked chunk [0, 0]")
        .map_err(|error| error.to_string())?;
    server.shutdown().map_err(|error| error.to_string())
}

fn compile_policy(
    index: usize,
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
) -> Result<(String, CompilationOutput), String> {
    let namespace = format!("mdl_ps4_{index}");
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(&namespace).unwrap(),
        ObjectiveName::new(&format!("mps4.{index}")).unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    let options = CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 1_000_000, 1_000_000),
        EmissionOptions::new("PS-4 vanilla enum/switch/range evidence"),
    );
    compile_source(SourceInput::new("ps4.mdl", SOURCE), &options)
        .map(|output| (namespace, output))
        .map_err(|error| error.to_string())
}

fn exercise(
    server: &mut TestServer,
    compiled: &[(String, CompilationOutput)],
) -> Result<(), String> {
    let cases: Vec<Case> = serde_json::from_str(CASES).map_err(|error| error.to_string())?;
    for (policy, (_, output)) in compiled.iter().enumerate() {
        let source = output
            .checked_frontend()
            .function_ids()
            .find(|function| {
                output.checked_frontend().function_visibility(*function)
                    == Some(FunctionVisibility::DatapackExport)
            })
            .ok_or_else(|| format!("policy {policy} has no export"))?;
        let abi = output
            .source_function_abi(source)
            .ok_or_else(|| format!("policy {policy} has no ABI"))?;
        let ([(parameter_ty, parameter)], [(result_ty, result)]) =
            (abi.parameter_homes(), abi.result_homes())
        else {
            return Err(format!("policy {policy} has unexpected ABI shape"));
        };
        if *parameter_ty != CoreType::I32 || *result_ty != CoreType::I32 {
            return Err(format!("policy {policy} has non-i32 ABI"));
        }
        for (case_index, case) in cases.iter().enumerate() {
            set_score(server, parameter, case.input)?;
            server
                .command(&format!("function {}", abi.entry_resource()))
                .map_err(|error| error.to_string())?;
            server
                .wait_for_command_log("Running function")
                .map_err(|error| error.to_string())?;
            let marker = format!("MDL_PS4_{policy}_{case_index}");
            server
                .command(&format!(
                    "execute if score {} {} matches {} run say {marker}",
                    result.holder(),
                    result.objective(),
                    case.expected
                ))
                .map_err(|error| error.to_string())?;
            server
                .wait_for_command_log(&marker)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn set_score(server: &mut TestServer, slot: &RegisterSlot, value: i32) -> Result<(), String> {
    server
        .command(&format!(
            "scoreboard players set {} {} {value}",
            slot.holder(),
            slot.objective()
        ))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Set")
        .map(|_| ())
        .map_err(|error| error.to_string())
}
