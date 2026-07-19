use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, CompilationOutput, FrontendLimits, SourceFunctionId, SourceInput,
    compile_source,
};
use mdl_compiler::ir::core::{
    CanonicalPrinter, CoreEvaluationLimits, CoreEvaluator, CoreType, CoreValue,
};
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

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const JAVA_26_2_EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";
const SOURCE: &str = r"fn choose(flag: Bool, left: Int32, right: Int32) -> Int32 {
    if (flag) { return left; }
    return right;
}

export fn choose_through_call(flag: Bool, left: Int32, right: Int32) -> Int32 {
    return choose(flag, left, right);
}
";
const RESULT_OBJECTIVE: &str = "mdl.ps1.case";

#[derive(Clone, Copy)]
struct PolicyConfiguration {
    suffix: &'static str,
    policy: CompilerPolicy,
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
}

const CONFIGURATIONS: [PolicyConfiguration; 4] = [
    PolicyConfiguration {
        suffix: "nn",
        policy: CompilerPolicy::new(CorePolicy::None, MinecraftPolicy::None),
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::None,
    },
    PolicyConfiguration {
        suffix: "nb",
        policy: CompilerPolicy::new(CorePolicy::None, MinecraftPolicy::Baseline),
        core: CoreOptimizationLevel::None,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
    PolicyConfiguration {
        suffix: "bn",
        policy: CompilerPolicy::new(CorePolicy::Baseline, MinecraftPolicy::None),
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::None,
    },
    PolicyConfiguration {
        suffix: "bb",
        policy: CompilerPolicy::new(CorePolicy::Baseline, MinecraftPolicy::Baseline),
        core: CoreOptimizationLevel::Baseline,
        minecraft: MinecraftOptimizationLevel::Baseline,
    },
];

#[test]
fn scalar_policy_deployments_are_derived_from_core_semantics_and_public_abi() {
    let (deployments, expected) = compile_deployments().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(expected, 41);
    assert_eq!(deployments.len(), CompilerPolicy::ALL.len());
    for (deployment, policy) in deployments.iter().zip(CompilerPolicy::ALL) {
        assert_eq!(deployment.policy(), policy);
    }
    build_scenario(expected).unwrap_or_else(|error| panic!("{error}"));
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn scalar_core_result_matches_all_four_policies_in_one_vanilla_server() {
    if let Err(error) = run_server_test() {
        panic!("{error}");
    }
}

fn run_server_test() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let hash = sha256_file(&server_jar).map_err(|error| error.to_string())?;
    if ![JAVA_26_2_SERVER_SHA256, JAVA_26_2_EXTRACTED_SERVER_SHA256].contains(&hash.as_str()) {
        return Err(format!(
            "expected a pinned Java 26.2 server, got SHA-256 {hash}"
        ));
    }
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let (deployments, expected) = compile_deployments()?;
    let scenario = build_scenario(expected)?;
    let run = run_semantic_policy_scenario(
        &ServerConfig::new(java, server_jar),
        env::var_os("MDL_KEEP_TEST_DIR").is_some(),
        &scenario,
        &deployments,
    )
    .map_err(|error| error.to_string())?;
    if run.observations().len() != CompilerPolicy::ALL.len() {
        return Err("runner omitted a compiler policy observation".to_owned());
    }
    Ok(())
}

fn compile_deployments() -> Result<(Vec<PolicyDeployment>, i32), String> {
    let mut deployments = Vec::with_capacity(CONFIGURATIONS.len());
    let mut expected = None;
    for configuration in CONFIGURATIONS {
        let (deployment, result) = compile_deployment(configuration)?;
        if expected
            .replace(result)
            .is_some_and(|previous| previous != result)
        {
            return Err("optimized Core policies disagree in the reference evaluator".to_owned());
        }
        deployments.push(deployment);
    }
    Ok((
        deployments,
        expected.ok_or_else(|| "policy matrix is empty".to_owned())?,
    ))
}

fn compile_deployment(
    configuration: PolicyConfiguration,
) -> Result<(PolicyDeployment, i32), String> {
    let namespace = format!("mdl_ps1_{}", configuration.suffix);
    let objective = format!("mps1.{}", configuration.suffix);
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(&namespace).map_err(|error| error.to_string())?,
        ObjectiveName::new(&objective).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?
    .with_optimization_level(configuration.minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    let options = CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(configuration.core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("MDL PS-1 scalar policy differential"),
    );
    let output = compile_source(SourceInput::new("ps1_scalar.mdl", SOURCE), &options)
        .map_err(|error| error.to_string())?;
    let (source_function, expected) = evaluate_export(&output)?;
    let abi = output
        .source_function_abi(source_function)
        .ok_or_else(|| "source export has no lowered ABI".to_owned())?;
    let arguments = [1, 41, 73];
    if abi.parameter_homes().len() != arguments.len() || abi.result_homes().len() != 1 {
        return Err(
            "generated ABI does not match choose_through_call(Bool, Int32, Int32)".to_owned(),
        );
    }
    let prelude = abi
        .parameter_homes()
        .iter()
        .zip(arguments)
        .map(|((ty, slot), value)| {
            if *ty == CoreType::Bool && !matches!(value, 0 | 1) {
                return Err("invalid Boolean ABI fixture".to_owned());
            }
            DriverCommand::new(format!(
                "scoreboard players set {} {} {value}",
                slot.holder(),
                slot.objective()
            ))
            .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (result_type, result_slot) = &abi.result_homes()[0];
    if *result_type != CoreType::I32 {
        return Err("generated choose result is not Int32".to_owned());
    }
    let publish = vec![
        DriverCommand::new(format!(
            "scoreboard players operation #result {RESULT_OBJECTIVE} = {} {}",
            result_slot.holder(),
            result_slot.objective()
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
    let entry = ResourceLocation::parse(&abi.entry_resource().to_string())
        .map_err(|error| error.to_string())?;
    let artifacts = vec![
        DeploymentFile::new(
            "compiler/core.ir",
            CanonicalPrinter::new(output.core_optimization().program(), output.sources())
                .map_err(|error| error.to_string())?
                .render()
                .into_bytes(),
        ),
        DeploymentFile::new(
            "compiler/core-optimization.txt",
            format!("{:#?}\n", output.core_optimization().report()).into_bytes(),
        ),
    ];
    Ok((
        PolicyDeployment::new(
            configuration.policy,
            namespace,
            files,
            ScenarioEntry::Export(
                TestName::new("choose_through_call").map_err(|error| error.to_string())?,
            ),
            prelude,
            entry,
            publish,
        )
        .with_artifacts(artifacts),
        expected,
    ))
}

fn evaluate_export(output: &CompilationOutput) -> Result<(SourceFunctionId, i32), String> {
    let source_function = output
        .checked_frontend()
        .function_ids()
        .find(|function| output.source_function_abi(*function).is_some())
        .ok_or_else(|| "compiled source has no exported function ABI".to_owned())?;
    let core_function = output
        .source_to_core()
        .function(source_function)
        .ok_or_else(|| "source function has no Core identity".to_owned())?;
    let evaluation = CoreEvaluator::for_verified_program(
        output.core_optimization().program(),
        CoreEvaluationLimits::new(1_000, 32),
    )
    .evaluate(
        core_function,
        &[
            CoreValue::Bool(true),
            CoreValue::I32(41),
            CoreValue::I32(73),
        ],
    )
    .map_err(|error| error.to_string())?;
    let [CoreValue::I32(expected)] = evaluation.results() else {
        return Err("Core evaluator returned the wrong scalar result shape".to_owned());
    };
    Ok((source_function, *expected))
}

fn build_scenario(expected: i32) -> Result<mdl_test::scenario::MinecraftScenario, String> {
    let objective = TestName::new(RESULT_OBJECTIVE).map_err(|error| error.to_string())?;
    let result_path = ObservationPath::Score {
        holder: "#result".into(),
        objective: objective.clone(),
    };
    let expected =
        ObservationSet::try_new([(result_path, ObservationValue::Score(Some(expected)))])
            .map_err(|error| error.to_string())?;
    let owned = OwnedResource::Objective(objective);
    ScenarioSpec::new(
        ScenarioId::new("scalar/choose").map_err(|error| error.to_string())?,
        GenerationNonce::new(0x005c_41a2).map_err(|error| error.to_string())?,
        ScenarioEntry::Export(
            TestName::new("choose_through_call").map_err(|error| error.to_string())?,
        ),
    )
    .with_owned_resources(vec![owned.clone()])
    .with_cleanup(vec![owned])
    .with_expected(expected)
    .build()
    .map_err(|error| error.to_string())
}
