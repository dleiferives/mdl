use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{
    AnalysisArithmeticCaps, TargetExecutionAnalysisLimits, TargetExecutionCostReport,
};
use mdl_compiler::datapack::{EmissionOptions, TraceMap};
use mdl_compiler::diagnostic::render_diagnostics;
use mdl_compiler::frontend::{
    CompilationFailure, CompilationOptions, CompilationOutput, FrontendLimits, FrontendLimitsError,
    SourceFunctionId, SourceInput, compile_source,
};
use mdl_compiler::ir::core::{CanonicalPrinter, CoreType};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    LoweredFunction, LoweringOptions, LoweringOptionsError, LoweringPhase,
    MinecraftOptimizationLevel, RegisterSlot,
};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer};

const SCALAR_SOURCE_NAME: &str = "stage6_scalar.mdl";
const SCALAR_SOURCE: &str = include_str!("fixtures/stage6_scalar.mdl");

#[derive(Clone, Copy, Debug)]
struct SourceConfiguration {
    label: &'static str,
    pack_name: &'static str,
    namespace: &'static str,
    objective: &'static str,
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
}

const CONFIGURATIONS: [SourceConfiguration; 4] = [
    SourceConfiguration {
        label: "core-none-minecraft-none",
        pack_name: "mdl_stage6_nn",
        namespace: "mdl_stage6_nn",
        objective: "mdl6.nn",
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::None,
    },
    SourceConfiguration {
        label: "core-none-minecraft-baseline",
        pack_name: "mdl_stage6_nb",
        namespace: "mdl_stage6_nb",
        objective: "mdl6.nb",
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
    SourceConfiguration {
        label: "core-baseline-minecraft-none",
        pack_name: "mdl_stage6_bn",
        namespace: "mdl_stage6_bn",
        objective: "mdl6.bn",
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::None,
    },
    SourceConfiguration {
        label: "core-baseline-minecraft-baseline",
        pack_name: "mdl_stage6_bb",
        namespace: "mdl_stage6_bb",
        objective: "mdl6.bb",
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
];

const CHOOSE: usize = 0;
const CHOOSE_FOR_SIGN: usize = 2;
const INVERT: usize = 3;
const SAME: usize = 4;
const EXERCISE_VOID: usize = 5;
const FUNCTION_COUNT: usize = 6;

#[derive(Debug, Eq, PartialEq)]
struct CompilationSnapshot {
    hir: String,
    source_to_core: String,
    core: String,
    core_report: String,
    lowering_report: String,
    target: String,
    target_analysis: String,
    pack: String,
    trace: String,
    abi: String,
}

#[test]
fn source_compilation_is_deterministic_under_all_four_policies() {
    for configuration in CONFIGURATIONS {
        let first = compile_fixture(configuration).unwrap_or_else(|error| {
            panic!("{} first compilation failed: {error}", configuration.label)
        });
        let second = compile_fixture(configuration).unwrap_or_else(|error| {
            panic!("{} second compilation failed: {error}", configuration.label)
        });

        assert_eq!(
            snapshot(&first),
            snapshot(&second),
            "{} was not byte-stable",
            configuration.label
        );
        assert_eq!(first.emission().pack(), second.emission().pack());
        assert_eq!(
            first.core_optimization().report().level(),
            configuration.core
        );
        assert_eq!(
            first.lowering().report().optimization_level(),
            configuration.minecraft
        );
        assert_eq!(
            first.lowering().report().namespace().as_str(),
            configuration.namespace
        );
        assert_eq!(
            first.lowering().report().register_objective().as_str(),
            configuration.objective
        );
        assert_complete_maps(&first);
        assert_source_origin_continuity(&first);
    }
}

#[test]
fn invalid_sources_stop_at_the_expected_owned_boundary() {
    let cases = [
        (
            "malformed.mdl",
            "fn broken(value: Int32 -> Int32 { return value; }",
            FailureBoundary::Syntax,
            "frontend.parse.expected-token",
        ),
        (
            "unknown.mdl",
            "fn broken() -> Int32 { return missing; }",
            FailureBoundary::Semantic,
            "frontend.check.unknown-name",
        ),
        (
            "ill_typed.mdl",
            "fn broken() -> Int32 { return true; }",
            FailureBoundary::Semantic,
            "frontend.check.type-mismatch",
        ),
        (
            "uninitialized.mdl",
            "fn broken() -> Int32 { var value: Int32; return value; }",
            FailureBoundary::Semantic,
            "frontend.check.uninitialized-read",
        ),
        (
            "invalid_return.mdl",
            "fn broken() -> Int32 { return; }",
            FailureBoundary::Semantic,
            "frontend.check.return-value-required",
        ),
    ];

    for (name, source, boundary, expected_code) in cases {
        let first = compile_invalid(name, source);
        let second = compile_invalid(name, source);
        assert_eq!(failure_boundary(&first), boundary, "{name}");
        assert_eq!(failure_boundary(&second), boundary, "{name}");
        let first_rendered = rendered_failure(&first, expected_code);
        let second_rendered = rendered_failure(&second, expected_code);
        assert_eq!(first_rendered, second_rendered, "{name}");
        assert!(first_rendered.contains(name), "{first_rendered}");
    }
}

#[test]
fn recursion_reaches_the_typed_minecraft_lowering_failure() {
    let source = "fn again(flag: Bool) -> Bool { if (flag) { return again(false); } return flag; }";
    let first = compile_invalid("recursive.mdl", source);
    let second = compile_invalid("recursive.mdl", source);

    for failure in [&first, &second] {
        assert!(failure.checked_frontend().is_some());
        assert!(failure.source_to_core().is_some());
        assert!(failure.core_optimization().is_some());
        assert!(failure.lowering().is_none());
        let lowering = failure
            .minecraft_lowering_failure()
            .expect("recursion must fail at Minecraft lowering");
        assert_eq!(lowering.phase(), LoweringPhase::Legality);
        assert!(
            lowering
                .diagnostics()
                .contains_code("lower.recursive-call-abi")
        );
        let sources = failure.sources().expect("frontend completed");
        assert!(
            lowering
                .diagnostics()
                .findings()
                .iter()
                .all(|finding| sources.resolve_origin_span(finding.origin()).is_some())
        );
    }
    assert_eq!(
        rendered_failure(&first, "lower.recursive-call-abi"),
        rendered_failure(&second, "lower.recursive-call-abi")
    );
}

#[test]
fn invalid_options_are_rejected_before_the_compilation_facade() {
    assert_eq!(
        FrontendLimits::new(0, 1, 1),
        Err(FrontendLimitsError::TokenLimitExcludesEndOfFile)
    );
    let reserved = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("minecraft").unwrap(),
        ObjectiveName::new("mdl6.invalid").unwrap(),
    );
    assert_eq!(reserved, Err(LoweringOptionsError::ReservedPackNamespace));
    assert!(PackNamespace::new("Invalid Namespace").is_err());
    assert!(ObjectiveName::new("invalid objective").is_err());
}

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn source_compilation_runs_all_four_policies_on_vanilla_26_2() {
    if let Err(error) = run_official_server_conformance() {
        panic!("{error}");
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureBoundary {
    Syntax,
    Semantic,
}

fn compile_fixture(
    configuration: SourceConfiguration,
) -> Result<CompilationOutput, CompilationFailure> {
    compile_source(
        SourceInput::new(SCALAR_SOURCE_NAME, SCALAR_SOURCE),
        &compilation_options(configuration),
    )
}

fn compilation_options(configuration: SourceConfiguration) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(configuration.namespace).unwrap(),
        ObjectiveName::new(configuration.objective).unwrap(),
    )
    .unwrap()
    .with_optimization_level(configuration.minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(configuration.core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new(format!(
            "MDL Stage 6 source conformance: {}",
            configuration.label
        )),
    )
}

fn snapshot(output: &CompilationOutput) -> CompilationSnapshot {
    let sources = output.sources();
    let core = output.core_optimization();
    CompilationSnapshot {
        hir: output.checked_frontend().dump(sources),
        source_to_core: output
            .source_to_core()
            .functions()
            .map(|(source, function)| format!("{}->{function:?}", source.index()))
            .collect::<Vec<_>>()
            .join("\n"),
        core: CanonicalPrinter::new(core.program(), sources)
            .unwrap()
            .render(),
        core_report: core.report().dump(),
        lowering_report: output.lowering().dump_lowering(),
        target: MinecraftDebugDumper::program(output.lowering().program()),
        target_analysis: output.target_analysis().as_ref().map_or_else(
            |failure| format!("analysis-error: {failure}"),
            TargetExecutionCostReport::dump,
        ),
        pack: pack_dump(output),
        trace: trace_dump(output.emission().trace()),
        abi: abi_dump(output),
    }
}

fn pack_dump(output: &CompilationOutput) -> String {
    let mut dump = String::new();
    for file in output.emission().pack().files() {
        writeln!(
            dump,
            "{} bytes={}",
            file.path().as_str(),
            file.bytes().len()
        )
        .unwrap();
        for byte in file.bytes() {
            write!(dump, "{byte:02x}").unwrap();
        }
        dump.push('\n');
    }
    dump
}

fn trace_dump(trace: &TraceMap) -> String {
    let mut dump = String::new();
    for record in trace.records() {
        writeln!(
            dump,
            "{:?}:{}:{}:{:?}",
            record.function_id(),
            record.function(),
            record.line(),
            record.origin()
        )
        .unwrap();
    }
    dump
}

fn abi_dump(output: &CompilationOutput) -> String {
    let mut dump = String::new();
    for source in output.checked_frontend().function_ids() {
        let core = output.source_to_core().function(source).unwrap();
        let function = output.source_function_abi(source).unwrap();
        writeln!(
            dump,
            "source={} core={core:?} entry={} params={:?} results={:?}",
            source.index(),
            function.entry_resource(),
            function.parameter_homes(),
            function.result_homes()
        )
        .unwrap();
    }
    dump
}

fn assert_complete_maps(output: &CompilationOutput) {
    assert_eq!(output.checked_frontend().function_count(), FUNCTION_COUNT);
    assert_eq!(output.source_to_core().len(), FUNCTION_COUNT);
    assert_eq!(output.lowering().map().len(), FUNCTION_COUNT);
    assert!(output.target_analysis().is_ok());
    for source in output.checked_frontend().function_ids() {
        let core = output
            .source_to_core()
            .function(source)
            .expect("every source function must map to Core");
        assert_eq!(
            output.source_function_abi(source),
            output.lowering().map().function(core)
        );
    }
}

fn assert_source_origin_continuity(output: &CompilationOutput) {
    let sources = output.sources();
    let records = output
        .emission()
        .trace()
        .records()
        .filter(|record| {
            let resource = record.function().to_string();
            !resource.ends_with(":__mdl/load") && !resource.contains(":__mdl/init/")
        })
        .collect::<Vec<_>>();
    assert!(!records.is_empty());
    for record in records {
        let span = sources
            .resolve_origin_span(record.origin())
            .unwrap_or_else(|| {
                panic!(
                    "{}:{} has no source-resolvable origin {:?}",
                    record.function(),
                    record.line(),
                    record.origin()
                )
            });
        assert!(!sources.files().slice(span).unwrap().is_empty());
    }
}

fn compile_invalid(name: &str, source: &str) -> CompilationFailure {
    compile_source(
        SourceInput::new(name, source),
        &compilation_options(CONFIGURATIONS[0]),
    )
    .expect_err("negative fixture unexpectedly compiled")
}

fn failure_boundary(failure: &CompilationFailure) -> FailureBoundary {
    match failure {
        CompilationFailure::Syntax { .. } => FailureBoundary::Syntax,
        CompilationFailure::Semantic { .. } => FailureBoundary::Semantic,
        other => panic!("unexpected failure boundary: {other}"),
    }
}

fn rendered_failure(failure: &CompilationFailure, expected_code: &str) -> String {
    let diagnostics = failure
        .diagnostics()
        .or_else(|| {
            failure
                .minecraft_lowering_failure()
                .map(mdl_compiler::lower::minecraft::LoweringFailure::diagnostics)
        })
        .expect("failure must retain diagnostics");
    assert!(diagnostics.contains_code(expected_code));
    let sources = failure.sources().expect("source ingestion completed");
    render_diagnostics(diagnostics, sources)
}

#[allow(
    clippy::too_many_lines,
    reason = "one server lifecycle owns the complete four-policy Stage 6 vertical proof"
)]
fn run_official_server_conformance() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let server_config = ServerConfig::new(java, server_jar);

    let compiled = CONFIGURATIONS
        .into_iter()
        .map(|configuration| {
            compile_fixture(configuration)
                .map(|output| (configuration, output))
                .map_err(|error| format!("{} compilation failed: {error}", configuration.label))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let preserve_success = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve_success).map_err(|error| error.to_string())?;
    for (configuration, output) in &compiled {
        sandbox
            .install_datapack(
                configuration.pack_name,
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
        .start(&server_config)
        .map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    let result = exercise_all_policies(&mut server, &compiled).and_then(|()| {
        let preserved = compiled
            .iter()
            .map(|(_, output)| {
                source_function(output, CHOOSE)
                    .parameter_homes()
                    .get(1)
                    .map(|(_, slot)| slot.clone())
                    .ok_or_else(|| "choose ABI omitted its left parameter".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (index, slot) in preserved.iter().enumerate() {
            let value = 700_i32 + i32::try_from(index).map_err(|error| error.to_string())?;
            set_score(&mut server, slot, value)?;
        }

        let checkpoint = server.log_checkpoint();
        server
            .command("reload")
            .map_err(|error| error.to_string())?;
        server
            .command("say MDL_STAGE6_SOURCE_RELOAD_COMPLETE")
            .map_err(|error| error.to_string())?;
        server
            .wait_for_command_log("MDL_STAGE6_SOURCE_RELOAD_COMPLETE")
            .map_err(|error| error.to_string())?;
        server
            .check_datapack_logs_since(checkpoint)
            .map_err(|error| error.to_string())?;
        for (index, slot) in preserved.iter().enumerate() {
            let expected = 700_i32 + i32::try_from(index).map_err(|error| error.to_string())?;
            expect_score(&mut server, slot, expected)?;
        }
        exercise_all_policies(&mut server, &compiled)
    });

    if let Err(error) = result {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 6 source conformance sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn exercise_all_policies(
    server: &mut TestServer,
    compiled: &[(SourceConfiguration, CompilationOutput)],
) -> Result<(), String> {
    for (configuration, output) in compiled {
        let prefix = configuration
            .label
            .replace("core-", "")
            .replace("-minecraft-", "_")
            .replace('-', "_");
        invoke(
            server,
            source_function(output, CHOOSE),
            &[1, i32::MIN, i32::MAX],
            &[i32::MIN],
            &format!("{prefix}_choose_true"),
        )?;
        invoke(
            server,
            source_function(output, CHOOSE),
            &[0, i32::MIN, i32::MAX],
            &[i32::MAX],
            &format!("{prefix}_choose_false"),
        )?;
        invoke(
            server,
            source_function(output, CHOOSE_FOR_SIGN),
            &[-1, 41, 73],
            &[41],
            &format!("{prefix}_call_negative"),
        )?;
        invoke(
            server,
            source_function(output, CHOOSE_FOR_SIGN),
            &[0, 41, 73],
            &[73],
            &format!("{prefix}_call_nonnegative"),
        )?;
        invoke(
            server,
            source_function(output, INVERT),
            &[1],
            &[0],
            &format!("{prefix}_invert_true"),
        )?;
        invoke(
            server,
            source_function(output, INVERT),
            &[0],
            &[1],
            &format!("{prefix}_invert_false"),
        )?;
        invoke(
            server,
            source_function(output, SAME),
            &[0, 0],
            &[1],
            &format!("{prefix}_bool_equal"),
        )?;
        invoke(
            server,
            source_function(output, SAME),
            &[0, 1],
            &[0],
            &format!("{prefix}_bool_unequal"),
        )?;
        invoke(
            server,
            source_function(output, EXERCISE_VOID),
            &[0, 12, 34],
            &[],
            &format!("{prefix}_void"),
        )?;
        let (_, choose_result) = source_function(output, CHOOSE)
            .result_homes()
            .first()
            .ok_or_else(|| "choose ABI omitted its result".to_owned())?;
        expect_score(server, choose_result, 34)?;
    }
    Ok(())
}

fn source_function(output: &CompilationOutput, index: usize) -> &LoweredFunction {
    let source = source_function_id(output, index);
    output
        .source_function_abi(source)
        .unwrap_or_else(|| panic!("source function {index} has no generated ABI"))
}

fn source_function_id(output: &CompilationOutput, index: usize) -> SourceFunctionId {
    output
        .checked_frontend()
        .function_ids()
        .nth(index)
        .unwrap_or_else(|| panic!("source function {index} is absent"))
}

fn invoke(
    server: &mut TestServer,
    function: &LoweredFunction,
    arguments: &[i32],
    expected_results: &[i32],
    observation: &str,
) -> Result<(), String> {
    if function.parameter_homes().len() != arguments.len()
        || function.result_homes().len() != expected_results.len()
    {
        return Err(format!(
            "{observation} disagrees with generated ABI: {} parameters, {} results",
            function.parameter_homes().len(),
            function.result_homes().len()
        ));
    }
    for ((ty, slot), value) in function.parameter_homes().iter().zip(arguments) {
        if *ty == CoreType::Bool && !matches!(value, 0 | 1) {
            return Err(format!("non-normalized Boolean argument {value}"));
        }
        set_score(server, slot, *value)?;
    }
    server
        .command(&format!(
            "execute store success storage mdl_test:stage6_source observations.{observation} byte 1 run function {}",
            function.entry_resource()
        ))
        .map_err(|error| error.to_string())?;
    for ((_, slot), expected) in function.result_homes().iter().zip(expected_results) {
        expect_score(server, slot, *expected)?;
    }
    let marker = format!("MDL_STAGE6_SOURCE_{}", observation.to_ascii_uppercase());
    server
        .command(&format!(
            "execute if data storage mdl_test:stage6_source observations{{{observation}:1b}} run say {marker}"
        ))
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(&marker)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn set_score(server: &mut TestServer, slot: &RegisterSlot, value: i32) -> Result<(), String> {
    server
        .command(&format!(
            "scoreboard players set {} {} {value}",
            slot.holder(),
            slot.objective()
        ))
        .map_err(|error| error.to_string())
}

fn expect_score(server: &mut TestServer, slot: &RegisterSlot, expected: i32) -> Result<(), String> {
    server
        .command(&format!(
            "scoreboard players get {} {}",
            slot.holder(),
            slot.objective()
        ))
        .map_err(|error| error.to_string())?;
    let line = server
        .wait_for_command_log(&format!("{} has", slot.holder()))
        .map_err(|error| error.to_string())?;
    let expected_text = format!("{} has {expected} [{}]", slot.holder(), slot.objective());
    if line.contains(&expected_text) {
        Ok(())
    } else {
        Err(format!("expected {expected_text:?}, got {line:?}"))
    }
}
