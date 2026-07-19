use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, CompilationOutput, FrontendLimits, FunctionVisibility, SourceFunctionId,
    SourceInput, compile_source,
};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreType, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::scenario::{
    CompilerPolicy, CorePolicy, DeploymentFile, DriverCommand, GenerationNonce, MinecraftPolicy,
    ObservationPath, ObservationSet, ObservationValue, OwnedResource, PolicyDeployment,
    ResourceLocation, ScenarioEntry, ScenarioId, ScenarioSpec, TestName,
    run_semantic_policy_scenario,
};
use mdl_test::{ServerConfig, sha256_file};

const SOURCE: &str = include_str!(
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/ps2_composition_rehearsal.mdl"
);
const SERVER_SHA256: &str = "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";
const RESULT_OBJECTIVE: &str = "mdl.ps2.comp";

#[derive(Clone, Copy)]
struct Configuration {
    suffix: &'static str,
    policy: CompilerPolicy,
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
}

const CONFIGURATIONS: [Configuration; 4] = [
    Configuration {
        suffix: "nn",
        policy: CompilerPolicy::new(CorePolicy::None, MinecraftPolicy::None),
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::None,
    },
    Configuration {
        suffix: "nb",
        policy: CompilerPolicy::new(CorePolicy::None, MinecraftPolicy::Baseline),
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
    Configuration {
        suffix: "bn",
        policy: CompilerPolicy::new(CorePolicy::Baseline, MinecraftPolicy::None),
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::None,
    },
    Configuration {
        suffix: "bb",
        policy: CompilerPolicy::new(CorePolicy::Baseline, MinecraftPolicy::Baseline),
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
];

#[test]
fn composition_policy_deployments_share_the_core_oracle() {
    let (deployments, expected) = compile_deployments().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(expected, 12);
    assert_eq!(deployments.len(), CompilerPolicy::ALL.len());
    build_scenario(expected).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn composition_rehearsal_matches_all_four_policies_on_vanilla() {
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
        return Err(format!(
            "expected a pinned Java 26.2 server, got SHA-256 {hash}"
        ));
    }
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let (deployments, expected) = compile_deployments()?;
    let scenario = build_scenario(expected)?;
    run_semantic_policy_scenario(
        &ServerConfig::new(java, server_jar),
        env::var_os("MDL_KEEP_TEST_DIR").is_some(),
        &scenario,
        &deployments,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn compile_deployments() -> Result<(Vec<PolicyDeployment>, i32), String> {
    let mut expected = None;
    let mut deployments = Vec::with_capacity(CONFIGURATIONS.len());
    for configuration in CONFIGURATIONS {
        let (deployment, result) = compile_deployment(configuration)?;
        if expected
            .replace(result)
            .is_some_and(|previous| previous != result)
        {
            return Err("Core policies disagree on the PS-2 rehearsal".to_owned());
        }
        deployments.push(deployment);
    }
    Ok((
        deployments,
        expected.ok_or_else(|| "empty policy matrix".to_owned())?,
    ))
}

fn compile_deployment(configuration: Configuration) -> Result<(PolicyDeployment, i32), String> {
    let namespace = format!("mdl_ps2_comp_{}", configuration.suffix);
    let objective = format!("mps2.{}", configuration.suffix);
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(&namespace).map_err(|error| error.to_string())?,
        ObjectiveName::new(&objective).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?
    .with_optimization_level(configuration.minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    let output = compile_source(
        SourceInput::new("ps2_composition_rehearsal.mdl", SOURCE),
        &CompilationOptions::new(
            FrontendLimits::DEFAULT,
            CoreOptimizationOptions::new(configuration.core),
            lowering,
            TargetExecutionAnalysisLimits::new(arithmetic, 1_000_000, 1_000_000),
            EmissionOptions::new("MDL PS-2 composition differential"),
        ),
    )
    .map_err(|error| error.to_string())?;
    let (source_function, expected) = evaluate_checksum(&output)?;
    let abi = output
        .source_function_abi(source_function)
        .ok_or_else(|| "checksum export has no ABI".to_owned())?;
    let [(parameter_type, parameter)] = abi.parameter_homes() else {
        return Err("checksum export has the wrong parameter ABI".to_owned());
    };
    let [(result_type, result)] = abi.result_homes() else {
        return Err("checksum export has the wrong result ABI".to_owned());
    };
    if *parameter_type != CoreType::I32 || *result_type != CoreType::I32 {
        return Err("checksum ABI is not scalar Int32".to_owned());
    }
    let prelude = vec![
        DriverCommand::new(format!(
            "scoreboard players set {} {} 10",
            parameter.holder(),
            parameter.objective(),
        ))
        .map_err(|error| error.to_string())?,
    ];
    let publish = vec![
        DriverCommand::new(format!(
            "scoreboard players operation #result {RESULT_OBJECTIVE} = {} {}",
            result.holder(),
            result.objective(),
        ))
        .map_err(|error| error.to_string())?,
    ];
    let files = output
        .emission()
        .pack()
        .files()
        .iter()
        .map(|file| DeploymentFile::new(file.path().as_str(), file.bytes().to_vec()))
        .collect::<Vec<_>>();
    Ok((
        PolicyDeployment::new(
            configuration.policy,
            namespace,
            files,
            ScenarioEntry::Export(TestName::new("rehearse_checksum").unwrap()),
            prelude,
            ResourceLocation::parse(&abi.entry_resource().to_string())
                .map_err(|error| error.to_string())?,
            publish,
        ),
        expected,
    ))
}

fn evaluate_checksum(output: &CompilationOutput) -> Result<(SourceFunctionId, i32), String> {
    let source_function = output
        .checked_frontend()
        .function_ids()
        .filter(|function| {
            output.checked_frontend().function_visibility(*function)
                == Some(FunctionVisibility::DatapackExport)
        })
        .last()
        .ok_or_else(|| "compiled rehearsal has no checksum export".to_owned())?;
    let core_function = output
        .source_to_core()
        .function(source_function)
        .ok_or_else(|| "checksum export has no Core function".to_owned())?;
    let evaluation = CoreEvaluator::for_verified_program(
        output.core_optimization().program(),
        CoreEvaluationLimits::new(1_000_000, 1_024),
    )
    .evaluate(core_function, &[CoreValue::I32(10)])
    .map_err(|error| error.to_string())?;
    let [CoreValue::I32(result)] = evaluation.results() else {
        return Err("checksum evaluator result is not one Int32".to_owned());
    };
    Ok((source_function, *result))
}

fn build_scenario(expected: i32) -> Result<mdl_test::scenario::MinecraftScenario, String> {
    let objective = TestName::new(RESULT_OBJECTIVE).map_err(|error| error.to_string())?;
    let owned = OwnedResource::Objective(objective.clone());
    let expected = ObservationSet::try_new([(
        ObservationPath::Score {
            holder: "#result".into(),
            objective: objective.clone(),
        },
        ObservationValue::Score(Some(expected)),
    )])
    .map_err(|error| error.to_string())?;
    ScenarioSpec::new(
        ScenarioId::new("composition/parser-tape-fuel").map_err(|error| error.to_string())?,
        GenerationNonce::new(0x002c_0a11).map_err(|error| error.to_string())?,
        ScenarioEntry::Export(
            TestName::new("rehearse_checksum").map_err(|error| error.to_string())?,
        ),
    )
    .with_owned_resources(vec![owned.clone()])
    .with_cleanup(vec![owned])
    .with_expected(expected)
    .build()
    .map_err(|error| error.to_string())
}
