use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, CompilationOutput, FrontendLimits, FunctionVisibility, SourceInput,
    compile_source,
};
use mdl_compiler::ir::core::{CanonicalPrinter, CoreType};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    ActivationDepthContract, LoweredFunction, LoweringOptions, MinecraftOptimizationLevel,
    RegisterSlot,
};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer, sha256_file};

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const JAVA_26_2_EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";

const SOURCE: &str = r#"
export fn preserve(flag: Bool, preserved: Bool) -> Bool {
    if (flag) {
        var child: Bool;
        child = preserve(false, false);
        if (preserved) { return true; }
        return child;
    }
    return false;
}

fn mutual_left(flag: Bool) -> Bool {
    if (flag) { return mutual_right(false); }
    return false;
}

fn mutual_right(flag: Bool) -> Bool {
    if (flag) { return mutual_left(false); }
    return true;
}

export fn mutual(flag: Bool) -> Bool {
    return mutual_left(flag);
}

fn context_nested() {
    unsafe minecraft("scoreboard players add @s mdl8ctx 100");
}

export fn serial_contexts() {
    run.as(mc.entities(ArmorStand).with_tag("mdl8_absent").limit(3)) {
        unsafe minecraft("scoreboard players add @s mdl8ctx 1000");
    }
    run.as(mc.entities(ArmorStand).with_tag("mdl8_context").limit(1)) {
        unsafe minecraft("scoreboard players add @s mdl8ctx 1");
    }
    run.as(mc.entities(ArmorStand).with_tag("mdl8_context").limit(3)) |worker| {
        unsafe minecraft("scoreboard players add @s mdl8ctx 10");
        context_nested();
        var recursive: Bool;
        recursive = preserve(true, true);
        if (recursive) {
            unsafe minecraft("scoreboard players add @s mdl8ctx 1000");
        }
        worker.say("serial context");
    }
}
"#;

const POLICIES: [(CoreOptimizationLevel, MinecraftOptimizationLevel); 4] = [
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

#[test]
fn recursive_compiler_output_has_balanced_structured_frames_under_all_policies() {
    for index in 0..POLICIES.len() {
        let (_, output) = compile_policy(index).unwrap_or_else(|error| panic!("{error}"));
        let text = emitted_functions(&output);
        assert_eq!(
            output
                .lowering()
                .map()
                .execution_contract()
                .activation_depth(),
            ActivationDepthContract::CallerBounded
        );
        let statistics = output.lowering().report().statistics();
        assert_eq!(
            statistics.physical_storages(),
            statistics.homes() + statistics.recursive_spill_bridges(),
            "policy {index} must declare every score home and recursive frame field exactly once"
        );
        let report = output.lowering().report().dump();
        assert!(report.contains("class=ActivationNbtBool binding=RecursiveSpill"));
        assert!(report.contains("activation=RecursiveStack"));
        assert!(report.contains("physical-call "));
        assert!(report.contains("spills=[(AssignedHomeId("));
        assert!(text.contains("execute as @e[type=minecraft:armor_stand,tag=mdl8_context"));
        assert!(text.contains("limit=3"));
        assert!(text.contains("say serial context"));
        assert!(text.contains("\"frames\" append value {}"));
        assert!(text.contains("\"frames\"[-1]"));
        assert!(text.contains("\"frames\" set value []"));
        assert_eq!(
            text.matches("\"frames\" append value {}").count(),
            text.matches("data remove storage").count(),
            "policy {index} emitted statically unbalanced recursive edges"
        );
        assert_eq!(
            text.matches("execute store result storage").count(),
            1,
            "policy {index} must spill only the one caller value live across recursion"
        );
        assert!(text.contains(" byte 1 run scoreboard players get "));
        assert_eq!(
            text.matches("execute store result score").count(),
            1,
            "policy {index} must restore exactly the selected live spill"
        );
    }
}

#[test]
fn recursive_and_serial_stage8_output_is_deterministic_under_all_policies() {
    for index in 0..POLICIES.len() {
        let (_, first) = compile_policy(index).unwrap_or_else(|error| panic!("{error}"));
        let (_, second) = compile_policy(index).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            first.checked_frontend().dump(first.sources()),
            second.checked_frontend().dump(second.sources())
        );
        assert_eq!(
            CanonicalPrinter::new(first.core_optimization().program(), first.sources())
                .unwrap()
                .render(),
            CanonicalPrinter::new(second.core_optimization().program(), second.sources())
                .unwrap()
                .render()
        );
        assert_eq!(
            first.core_optimization().report(),
            second.core_optimization().report()
        );
        assert_eq!(first.lowering().report(), second.lowering().report());
        assert_eq!(first.lowering().map(), second.lowering().map());
        assert_eq!(
            first.lowering().dump_lowering(),
            second.lowering().dump_lowering()
        );
        assert_eq!(
            MinecraftDebugDumper::program(first.lowering().program()),
            MinecraftDebugDumper::program(second.lowering().program())
        );
        assert_eq!(first.target_analysis(), second.target_analysis());
        assert_eq!(first.emission().pack(), second.emission().pack());
        assert_eq!(first.emission().footprint(), second.emission().footprint());
        assert_eq!(
            first.emission().trace().records().collect::<Vec<_>>(),
            second.emission().trace().records().collect::<Vec<_>>()
        );
    }
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn compiler_generated_recursive_frames_execute_under_all_policies() {
    if let Err(error) = run_server_test() {
        panic!("{error}");
    }
}

fn compile_policy(index: usize) -> Result<(String, CompilationOutput), String> {
    let (core, minecraft) = POLICIES[index];
    let namespace = format!("mdl8_recursive_{index}");
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(&namespace).unwrap(),
        ObjectiveName::new(&format!("mdl8.r{index}")).unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    let options = CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL Stage 8 recursive compiler conformance"),
    );
    let output = compile_source(SourceInput::new("stage8_recursive.mdl", SOURCE), &options)
        .map_err(|error| format!("policy {index} compilation failed: {error}"))?;
    Ok((namespace, output))
}

fn emitted_functions(output: &CompilationOutput) -> String {
    output
        .emission()
        .pack()
        .files()
        .iter()
        .filter(|file| file.path().as_str().ends_with(".mcfunction"))
        .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

fn exports(output: &CompilationOutput) -> Vec<&LoweredFunction> {
    output
        .checked_frontend()
        .function_ids()
        .filter(|function| {
            output.checked_frontend().function_visibility(*function)
                == Some(FunctionVisibility::DatapackExport)
        })
        .map(|function| output.source_function_abi(function).unwrap())
        .collect()
}

fn run_server_test() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let actual_hash = sha256_file(&server_jar).map_err(|error| error.to_string())?;
    if ![JAVA_26_2_SERVER_SHA256, JAVA_26_2_EXTRACTED_SERVER_SHA256].contains(&actual_hash.as_str())
    {
        return Err(format!(
            "expected the pinned Java 26.2 bundle or extracted-server SHA-256, got {actual_hash}"
        ));
    }
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let compiled = (0..POLICIES.len())
        .map(compile_policy)
        .collect::<Result<Vec<_>, _>>()?;
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).map_err(|error| error.to_string())?;
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
    let mut server = sandbox
        .start(&ServerConfig::new(java, server_jar))
        .map_err(|error| error.to_string())?;
    server
        .command("forceload add 0 0")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Marked chunk [0, 0]")
        .map_err(|error| error.to_string())?;
    server
        .command("scoreboard objectives add mdl8ctx dummy")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Created new objective")
        .map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    if let Err(error) = exercise(&mut server, &compiled) {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 8 compiler sandbox preserved at {}",
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

fn exercise(
    server: &mut TestServer,
    compiled: &[(String, CompilationOutput)],
) -> Result<(), String> {
    for (index, (namespace, output)) in compiled.iter().enumerate() {
        let exported = exports(output);
        if exported.len() != 3 {
            return Err(format!(
                "policy {index} published {} exports",
                exported.len()
            ));
        }
        server
            .command(&format!(
                "data modify storage {namespace}:__mdl/runtime/v0 frames set value [{{poison:1b}}]"
            ))
            .map_err(|error| error.to_string())?;
        server
            .wait_for_command_log("Modified storage")
            .map_err(|error| error.to_string())?;
        server
            .command(&format!("function {namespace}:__mdl/load"))
            .map_err(|error| error.to_string())?;
        server
            .wait_for_command_log("Running function")
            .map_err(|error| error.to_string())?;
        assert_frames_empty(server, namespace, &format!("RECOVERY_EMPTY_{index}"))?;
        invoke(
            server,
            exported[0],
            &[1, 1],
            1,
            &format!("PRESERVE_{index}"),
        )?;
        assert_frames_empty(server, namespace, &format!("PRESERVE_EMPTY_{index}"))?;
        invoke(server, exported[0], &[1, 0], 0, &format!("CHILD_{index}"))?;
        assert_frames_empty(server, namespace, &format!("CHILD_EMPTY_{index}"))?;
        invoke(server, exported[1], &[1], 1, &format!("MUTUAL_{index}"))?;
        assert_frames_empty(server, namespace, &format!("MUTUAL_EMPTY_{index}"))?;
        exercise_serial_contexts(server, exported[2], index)?;
    }
    Ok(())
}

fn exercise_serial_contexts(
    server: &mut TestServer,
    function: &LoweredFunction,
    index: usize,
) -> Result<(), String> {
    if !function.parameter_homes().is_empty() || !function.result_homes().is_empty() {
        return Err(format!("policy {index} serial context export is not Void"));
    }
    for x in 0..3 {
        server
            .command(&format!(
                "summon minecraft:armor_stand {x} 80 0 {{Tags:[\"mdl8_context\"],Invisible:1b,Marker:1b,NoGravity:1b}}"
            ))
            .map_err(|error| error.to_string())?;
        server
            .wait_for_command_log("Summoned new Armor Stand")
            .map_err(|error| error.to_string())?;
    }
    server
        .command(&format!("function {}", function.entry_resource()))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Running function")
        .map_err(|error| error.to_string())?;
    expect_entity_count(server, 1111, 1, &format!("CONTEXT_ONE_{index}"))?;
    expect_entity_count(server, 1110, 2, &format!("CONTEXT_TWO_{index}"))?;
    server
        .command("kill @e[type=minecraft:armor_stand,tag=mdl8_context]")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Killed 3 entities")
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn expect_entity_count(
    server: &mut TestServer,
    entity_score: i32,
    expected: i32,
    marker: &str,
) -> Result<(), String> {
    server
        .command(&format!(
            "execute store result score #count mdl8ctx run execute if entity @e[type=minecraft:armor_stand,tag=mdl8_context,scores={{mdl8ctx={entity_score}}}]"
        ))
        .map_err(|error| error.to_string())?;
    server
        .command(&format!(
            "execute if score #count mdl8ctx matches {expected} run say {marker}"
        ))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(marker)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn invoke(
    server: &mut TestServer,
    function: &LoweredFunction,
    arguments: &[i32],
    expected: i32,
    marker: &str,
) -> Result<(), String> {
    if function.parameter_homes().len() != arguments.len() || function.result_homes().len() != 1 {
        return Err(format!("{marker} disagrees with generated scalar ABI"));
    }
    for ((ty, slot), value) in function.parameter_homes().iter().zip(arguments) {
        if *ty != CoreType::Bool || !matches!(value, 0 | 1) {
            return Err(format!("{marker} has a non-Boolean argument"));
        }
        set_score(server, slot, *value)?;
    }
    server
        .command(&format!("function {}", function.entry_resource()))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Running function")
        .map_err(|error| error.to_string())?;
    let (ty, result) = &function.result_homes()[0];
    if *ty != CoreType::Bool {
        return Err(format!("{marker} result is not Boolean"));
    }
    expect_score(server, result, expected, marker)
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

fn expect_score(
    server: &mut TestServer,
    slot: &RegisterSlot,
    expected: i32,
    marker: &str,
) -> Result<(), String> {
    server
        .command(&format!(
            "execute if score {} {} matches {expected} run say {marker}",
            slot.holder(),
            slot.objective()
        ))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(marker)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn assert_frames_empty(
    server: &mut TestServer,
    namespace: &str,
    marker: &str,
) -> Result<(), String> {
    server
        .command(&format!(
            "execute unless data storage {namespace}:__mdl/runtime/v0 frames[0] run say {marker}"
        ))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(marker)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
