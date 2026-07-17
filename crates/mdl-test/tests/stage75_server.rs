use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, FrontendLimits, FunctionVisibility, SourceInput, compile_source,
};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer, sha256_file};

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";

const SOURCE: &str = r#"
export fn frame_relative() {
    run.as(mc.entities(ArmorStand).with_tag("mdl75_frame").limit(1))
        .at_executor().positioned(~10, ~, ~) |worker| {
        worker.teleport(~, ~1, ~);
    }
}

export fn receiver_relative() {
    run.as(mc.entities(ArmorStand).with_tag("mdl75_move").limit(1))
        .positioned(100, 20, 100) |worker| {
        worker.move_by(0, 2, 0);
    }
}

export fn frame_is_immutable() {
    run.as(mc.entities(ArmorStand).with_tag("mdl75_immutable").limit(1))
        .at_executor() |worker| {
        worker.teleport(~, ~1, ~);
        worker.teleport(~1, ~, ~);
    }
}
"#;

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn compiled_stage75_spatial_semantics_run_under_all_optimization_policies() {
    if let Err(error) = run_test() {
        panic!("{error}");
    }
}

fn run_test() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let hash = sha256_file(&server_jar).map_err(|error| error.to_string())?;
    if hash != JAVA_26_2_SERVER_SHA256 {
        return Err(format!("official server bundle hash changed: {hash}"));
    }
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
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
    let mut compiled = Vec::new();
    for (index, (core, minecraft)) in policies.into_iter().enumerate() {
        let namespace = format!("mdl75_{index}");
        let lowering = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new(&namespace).unwrap(),
            ObjectiveName::new(&format!("mdl75.{index}")).unwrap(),
        )
        .unwrap()
        .with_optimization_level(minecraft);
        let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
        let options = CompilationOptions::new(
            FrontendLimits::DEFAULT,
            CoreOptimizationOptions::new(core),
            lowering,
            TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
            EmissionOptions::new("MDL Stage 7.5 server conformance"),
        );
        let output = compile_source(SourceInput::new("stage75_server.mdl", SOURCE), &options)
            .map_err(|error| format!("policy {index} compilation failed: {error}"))?;
        let exports = output
            .checked_frontend()
            .function_ids()
            .filter(|function| {
                output.checked_frontend().function_visibility(*function)
                    == Some(FunctionVisibility::DatapackExport)
            })
            .map(|function| {
                output
                    .source_function_abi(function)
                    .unwrap()
                    .entry_resource()
                    .to_string()
            })
            .collect::<Vec<_>>();
        if exports.len() != 3 {
            return Err(format!(
                "policy {index} published {} exports",
                exports.len()
            ));
        }
        compiled.push((namespace, output, exports));
    }

    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).map_err(|error| error.to_string())?;
    for (namespace, output, _) in &compiled {
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
    let mut server = sandbox
        .start(&ServerConfig::new(java, server_jar))
        .map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    if let Err(error) = exercise(&mut server, &compiled) {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 7.5 compiler sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn exercise(
    server: &mut TestServer,
    compiled: &[(
        String,
        mdl_compiler::frontend::CompilationOutput,
        Vec<String>,
    )],
) -> Result<(), String> {
    command_wait(
        server,
        "execute in minecraft:overworld run forceload add 0 0",
        "force loaded",
    )?;
    command_wait(
        server,
        "execute in minecraft:overworld run summon minecraft:armor_stand 10 5 10 {Tags:[\"mdl75_frame\"],Marker:1b,NoGravity:1b}",
        "Summoned new",
    )?;
    command_wait(
        server,
        "execute in minecraft:overworld run summon minecraft:armor_stand 10 5 10 {Tags:[\"mdl75_move\"],Marker:1b,NoGravity:1b}",
        "Summoned new",
    )?;
    command_wait(
        server,
        "execute in minecraft:overworld run summon minecraft:armor_stand 10 5 10 {Tags:[\"mdl75_immutable\"],Marker:1b,NoGravity:1b}",
        "Summoned new",
    )?;

    for (index, (_, _, exports)) in compiled.iter().enumerate() {
        reset(server, "mdl75_frame")?;
        reset(server, "mdl75_move")?;
        reset(server, "mdl75_immutable")?;
        for export in exports {
            server
                .command(&format!("function {export}"))
                .map_err(|error| error.to_string())?;
            server
                .wait_for_command_log("Running function")
                .map_err(|error| error.to_string())?;
        }
        expect_position(
            server,
            "mdl75_frame",
            "20 6 10",
            &format!("MDL75_FRAME_{index}"),
        )?;
        expect_position(
            server,
            "mdl75_move",
            "10 7 10",
            &format!("MDL75_MOVE_{index}"),
        )?;
        expect_position(
            server,
            "mdl75_immutable",
            "11 5 10",
            &format!("MDL75_IMMUTABLE_{index}"),
        )?;
    }
    Ok(())
}

fn reset(server: &mut TestServer, tag: &str) -> Result<(), String> {
    command_wait(
        server,
        &format!("execute in minecraft:overworld run teleport @e[tag={tag},limit=1] 10 5 10"),
        "Teleported",
    )
}

fn expect_position(
    server: &mut TestServer,
    tag: &str,
    position: &str,
    marker: &str,
) -> Result<(), String> {
    server
        .command(&format!("execute in minecraft:overworld positioned {position} if entity @e[tag={tag},distance=..0.01] run say {marker}"))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(marker)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn command_wait(server: &mut TestServer, command: &str, expected: &str) -> Result<(), String> {
    server.command(command).map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(expected)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
