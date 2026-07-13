use std::env;
use std::fs::OpenOptions;
use std::num::NonZeroU32;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::{EmissionOptions, emit_datapack};
use mdl_compiler::ir::core::{CoreProgram, CoreType, FunctionBuilder, Terminator, TerminatorKind};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    LoweringOptions, MinecraftOptimizationLevel, lower_to_minecraft,
};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions, optimize_core};
use mdl_compiler::source::{OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{MeasurementMetadata, MeasurementProtocol, MeasurementRecord, MeasurementSample};

const WARMUP_ITERATIONS: u32 = 1;
const DEFAULT_SAMPLE_ITERATIONS: u32 = 8;
const FIXTURES: [(&str, usize); 3] = [("tiny", 8), ("normal", 256), ("scale", 4_096)];

const CONFIGURATIONS: [Configuration; 4] = [
    Configuration::new(
        "core-none+mc-none",
        "m00",
        "m00.reg",
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::None,
    ),
    Configuration::new(
        "core-none+mc-baseline",
        "m01",
        "m01.reg",
        CoreOptimizationLevel::None,
        MinecraftOptimizationLevel::Baseline,
    ),
    Configuration::new(
        "core-baseline+mc-none",
        "m10",
        "m10.reg",
        CoreOptimizationLevel::Baseline,
        MinecraftOptimizationLevel::None,
    ),
    Configuration::new(
        "core-baseline+mc-baseline",
        "m11",
        "m11.reg",
        CoreOptimizationLevel::Baseline,
        MinecraftOptimizationLevel::Baseline,
    ),
];

const SUBJECTS: [MeasurementSubject; 5] = [
    MeasurementSubject::CoreOptimization,
    MeasurementSubject::MinecraftLowering,
    MeasurementSubject::TargetCostAnalysis,
    MeasurementSubject::DatapackEmission,
    MeasurementSubject::CompleteCompilation,
];

#[test]
#[ignore = "environment-specific Stage 5 measurements; inspect a --release run"]
fn records_raw_interleaved_stage5_measurements_without_timing_thresholds() {
    let metadata = capture_metadata();
    assert!(
        metadata.jvm().is_none(),
        "compiler-only samples must not claim a Java/server environment"
    );
    assert_eq!(
        metadata.cargo_profile(),
        "release",
        "Stage 5 timings must be built with cargo test --release"
    );
    let sample_iterations = sample_iterations();
    for (fixture, rounds) in FIXTURES {
        let sources = SourceContext::new();
        let program = measurement_program(&sources, rounds);
        let protocol = MeasurementProtocol::new(
            fixture,
            WARMUP_ITERATIONS,
            sample_iterations,
            SUBJECTS.map(MeasurementSubject::label),
            CONFIGURATIONS.map(|configuration| configuration.label),
        );
        run_warmups(&program, &sources, &protocol);
        let samples = collect_samples(&program, &sources, &protocol);
        let record = MeasurementRecord::new(metadata.clone(), protocol, samples)
            .expect("the harness must follow its declared measurement schedule exactly");
        write_record(&record);
        write_generated_code_evidence(fixture, &program, &sources);
    }
}

#[derive(Clone, Copy)]
struct Configuration {
    label: &'static str,
    namespace: &'static str,
    objective: &'static str,
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
}

impl Configuration {
    const fn new(
        label: &'static str,
        namespace: &'static str,
        objective: &'static str,
        core: CoreOptimizationLevel,
        minecraft: MinecraftOptimizationLevel,
    ) -> Self {
        Self {
            label,
            namespace,
            objective,
            core,
            minecraft,
        }
    }

    fn lowering_options(self) -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new(self.namespace).unwrap(),
            ObjectiveName::new(self.objective).unwrap(),
        )
        .unwrap()
        .with_optimization_level(self.minecraft)
    }
}

#[derive(Clone, Copy)]
enum MeasurementSubject {
    CoreOptimization,
    MinecraftLowering,
    TargetCostAnalysis,
    DatapackEmission,
    CompleteCompilation,
}

impl MeasurementSubject {
    const fn label(self) -> &'static str {
        match self {
            Self::CoreOptimization => "core-optimization",
            Self::MinecraftLowering => "minecraft-lowering",
            Self::TargetCostAnalysis => "target-cost-analysis",
            Self::DatapackEmission => "datapack-emission",
            Self::CompleteCompilation => "complete-compilation",
        }
    }

    fn named(label: &str) -> Self {
        SUBJECTS
            .into_iter()
            .find(|subject| subject.label() == label)
            .unwrap_or_else(|| panic!("measurement protocol named unknown subject {label:?}"))
    }
}

fn run_warmups(program: &CoreProgram, sources: &SourceContext, protocol: &MeasurementProtocol) {
    for iteration in 0..protocol.warmup_iterations() {
        for subject in protocol.subject_order().map(MeasurementSubject::named) {
            for configuration in protocol
                .configurations_for_iteration(iteration)
                .map(configuration_named)
            {
                let _ = measure(subject, configuration, program, sources);
            }
        }
    }
}

fn collect_samples(
    program: &CoreProgram,
    sources: &SourceContext,
    protocol: &MeasurementProtocol,
) -> Vec<MeasurementSample> {
    let mut samples = Vec::new();
    let mut ordinal = 0_u64;
    for iteration in 0..protocol.sample_iterations().get() {
        for subject in protocol.subject_order().map(MeasurementSubject::named) {
            for configuration in protocol
                .configurations_for_iteration(iteration)
                .map(configuration_named)
            {
                let elapsed = measure(subject, configuration, program, sources);
                samples.push(MeasurementSample::completed(
                    ordinal,
                    subject.label(),
                    configuration.label,
                    u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
                ));
                ordinal += 1;
            }
        }
    }
    samples
}

fn configuration_named(label: &str) -> Configuration {
    CONFIGURATIONS
        .into_iter()
        .find(|configuration| configuration.label == label)
        .unwrap_or_else(|| panic!("measurement protocol named unknown configuration {label:?}"))
}

fn measure(
    subject: MeasurementSubject,
    configuration: Configuration,
    program: &CoreProgram,
    sources: &SourceContext,
) -> Duration {
    match subject {
        MeasurementSubject::CoreOptimization => {
            let candidate = program.clone();
            let started = Instant::now();
            let optimized = optimize_owned(candidate, sources, configuration);
            let elapsed = started.elapsed();
            drop(optimized);
            elapsed
        }
        MeasurementSubject::MinecraftLowering => {
            let optimized = optimize(program, sources, configuration);
            let started = Instant::now();
            let lowered = lower_to_minecraft(
                optimized.program(),
                sources,
                &configuration.lowering_options(),
            )
            .unwrap();
            let elapsed = started.elapsed();
            drop(lowered);
            elapsed
        }
        MeasurementSubject::TargetCostAnalysis => {
            let lowered = lower(program, sources, configuration);
            let started = Instant::now();
            let cost = lowered
                .analyze_target_execution(analysis_limits(&lowered))
                .unwrap();
            let elapsed = started.elapsed();
            drop(cost);
            elapsed
        }
        MeasurementSubject::DatapackEmission => {
            let lowered = lower(program, sources, configuration);
            let started = Instant::now();
            let emitted = emit_datapack(
                lowered.program(),
                sources,
                &EmissionOptions::new("Stage 5 measurement"),
            )
            .unwrap();
            let elapsed = started.elapsed();
            drop(emitted);
            elapsed
        }
        MeasurementSubject::CompleteCompilation => {
            let candidate = program.clone();
            let started = Instant::now();
            let optimized = optimize_owned(candidate, sources, configuration);
            let lowered = lower_to_minecraft(
                optimized.program(),
                sources,
                &configuration.lowering_options(),
            )
            .unwrap();
            let emitted = emit_datapack(
                lowered.program(),
                sources,
                &EmissionOptions::new("Stage 5 measurement"),
            )
            .unwrap();
            let elapsed = started.elapsed();
            drop(emitted);
            drop(lowered);
            drop(optimized);
            elapsed
        }
    }
}

fn optimize(
    program: &CoreProgram,
    sources: &SourceContext,
    configuration: Configuration,
) -> mdl_compiler::opt::core::CoreOptimizationOutput {
    optimize_owned(program.clone(), sources, configuration)
}

fn optimize_owned(
    program: CoreProgram,
    sources: &SourceContext,
    configuration: Configuration,
) -> mdl_compiler::opt::core::CoreOptimizationOutput {
    optimize_core(
        program,
        sources,
        &CoreOptimizationOptions::new(configuration.core),
    )
    .unwrap()
}

fn lower(
    program: &CoreProgram,
    sources: &SourceContext,
    configuration: Configuration,
) -> mdl_compiler::lower::minecraft::LoweringOutput {
    let optimized = optimize(program, sources, configuration);
    lower_to_minecraft(
        optimized.program(),
        sources,
        &configuration.lowering_options(),
    )
    .unwrap()
}

fn analysis_limits(
    lowered: &mdl_compiler::lower::minecraft::LoweringOutput,
) -> TargetExecutionAnalysisLimits {
    let assumptions = lowered
        .map()
        .execution_contract()
        .command_limits()
        .assumptions();
    TargetExecutionAnalysisLimits::new(
        AnalysisArithmeticCaps::minimum_for(assumptions),
        1_000_000,
        10_000_000,
    )
}

fn write_generated_code_evidence(fixture: &str, program: &CoreProgram, sources: &SourceContext) {
    for configuration in CONFIGURATIONS {
        let lowered = lower(program, sources, configuration);
        let cost = lowered
            .analyze_target_execution(analysis_limits(&lowered))
            .unwrap();
        let emitted = emit_datapack(
            lowered.program(),
            sources,
            &EmissionOptions::new("Stage 5 measurement"),
        )
        .unwrap();
        let assumptions = cost.assumptions();
        let census = cost.census();
        let stats = cost.stats();
        let footprint = emitted.footprint();
        eprintln!(
            "stage5-generated-code fixture={fixture} configuration={} target={:?} completion={:?} assumptions=sequence:{}/forks:{} cost=functions:{}/tags:{}/tag-entries:{}/top-level-commands:{}/command-nodes:{}/execute-stages:{}/calls:{}/condition-calls:{}/score:{}/data:{}/returns:{}/raw:{}/regions:{}/roots:{} work=graph-entities:{}/graph-edges:{}/tag-entries:{}/solver-updates:{}/command-transfer-visits:{}/retained-functions:{}/retained-regions:{}/retained-roots:{} artifact=files:{}/metadata:{}/functions:{}/function-tags:{}/function-lines:{}/utf8-bytes:{}/max-function-line-utf16:{}/trace-records:{}",
            configuration.label,
            cost.target(),
            cost.completion(),
            assumptions.max_command_sequence_length(),
            assumptions.max_command_forks(),
            census.functions(),
            census.function_tags(),
            census.function_tag_entries(),
            census.top_level_commands(),
            census.command_nodes(),
            census.execute_stages(),
            census.function_calls(),
            census.function_condition_calls(),
            census.score_commands(),
            census.data_commands(),
            census.return_commands(),
            census.raw_commands(),
            cost.regions().len(),
            cost.roots().len(),
            stats.graph_entities(),
            stats.graph_edges(),
            stats.tag_expansion_entries(),
            stats.solver_updates(),
            stats.command_transfer_visits(),
            stats.retained_functions(),
            stats.retained_regions(),
            stats.retained_roots(),
            footprint.files().len(),
            footprint.metadata_files(),
            footprint.function_files(),
            footprint.function_tag_files(),
            footprint.physical_function_lines(),
            footprint.total_utf8_bytes(),
            footprint.maximum_function_line_utf16_units(),
            footprint.trace_records(),
        );
        for (index, root) in cost.roots().iter().enumerate() {
            eprintln!(
                "stage5-generated-code-root fixture={fixture} configuration={} index={index} root={:?} entry={:?} sequence={:?} execute={:?} calls={:?} score-nbt={:?} max-chain={:?} sequence-limit={:?} fork-limit={:?}",
                configuration.label,
                root.root(),
                root.entry_region(),
                root.sequence_operations(),
                root.execute_stages(),
                root.internal_function_invocations(),
                root.score_nbt_command_executions(),
                root.maximum_chain_expansion(),
                root.sequence_limit_status(),
                root.fork_limit_status(),
            );
        }
    }
}

fn measurement_program(sources: &SourceContext, rounds: usize) -> CoreProgram {
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some("measurement"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&program, sources, function).unwrap();
    let entry = builder.entry_block();
    let mut current = builder.body().block(entry).unwrap().parameters()[0].value();
    let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
    for _ in 0..rounds {
        let identity = builder
            .i32_add_wrapping(current, zero, OriginId::UNKNOWN)
            .unwrap();
        let first = builder
            .i32_add_wrapping(identity, identity, OriginId::UNKNOWN)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(identity, identity, OriginId::UNKNOWN)
            .unwrap();
        current = builder
            .i32_add_wrapping(first, duplicate, OriginId::UNKNOWN)
            .unwrap();
    }
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![current]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    program
}

fn capture_metadata() -> MeasurementMetadata {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let revision = checked_git_stdout(&workspace, &["rev-parse", "HEAD"]);
    let status = checked_git_output(&workspace, &["status", "--porcelain=v1"]);
    MeasurementMetadata::new(revision, !status.stdout.is_empty(), "minecraft-java-26.2")
}

fn sample_iterations() -> NonZeroU32 {
    match env::var("MDL_MEASUREMENT_SAMPLES") {
        Ok(value) => value
            .parse::<u32>()
            .ok()
            .and_then(NonZeroU32::new)
            .unwrap_or_else(|| {
                panic!("MDL_MEASUREMENT_SAMPLES must be a nonzero u32, received {value:?}")
            }),
        Err(env::VarError::NotPresent) => NonZeroU32::new(DEFAULT_SAMPLE_ITERATIONS).unwrap(),
        Err(env::VarError::NotUnicode(_)) => {
            panic!("MDL_MEASUREMENT_SAMPLES must be valid Unicode")
        }
    }
}

fn checked_git_output(workspace: &Path, arguments: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(workspace)
        .output()
        .unwrap_or_else(|error| panic!("run git {arguments:?}: {error}"));
    assert!(
        output.status.success(),
        "git {arguments:?} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    output
}

fn checked_git_stdout(workspace: &Path, arguments: &[&str]) -> String {
    let output = checked_git_output(workspace, arguments);
    let stdout = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("git {arguments:?} emitted non-UTF-8 output: {error}"));
    let stdout = stdout.trim();
    assert!(!stdout.is_empty(), "git {arguments:?} emitted empty output");
    stdout.to_owned()
}

fn write_record(record: &MeasurementRecord) {
    if let Some(path) = env::var_os("MDL_MEASUREMENT_JSONL") {
        let output = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("measurement output must be writable");
        record.write_json_line(output).unwrap();
    } else {
        eprint!("{}", record.to_json_line().unwrap());
    }
}
