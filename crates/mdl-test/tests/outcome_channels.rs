use std::env;
use std::path::PathBuf;

use mdl_test::scenario::{
    CommandOutcomeExpectation, CompilerPolicy, CompletionExpectation, DeploymentFile,
    GenerationNonce, ObservationPath, ObservationSet, ObservationValue, OutcomeChannelExpectation,
    OwnedResource, PolicyDeployment, ResourceLocation, ScenarioEntry, ScenarioId, ScenarioMode,
    ScenarioSpec, TestName, run_semantic_policy_scenario,
};
use mdl_test::{ServerConfig, sha256_file};

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const JAVA_26_2_EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";
const OBJECTIVE: &str = "mdl.ps1.out";
const PACK_META: &str = concat!(
    "{\n",
    "  \"pack\": {\n",
    "    \"description\": \"MDL PS-1 command outcome calibration\",\n",
    "    \"min_format\": [107, 1],\n",
    "    \"max_format\": [107, 1]\n",
    "  }\n",
    "}\n",
);

#[test]
fn synchronous_outcome_calibration_has_a_complete_typed_matrix() {
    let (scenario, deployments) = calibration().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(scenario.policies(), CompilerPolicy::ALL);
    assert_eq!(scenario.expected().iter().len(), 18);
    assert_eq!(deployments.len(), CompilerPolicy::ALL.len());
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn synchronous_command_outcome_classes_are_distinct_on_vanilla_26_2() {
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
    let (scenario, deployments) = calibration()?;
    run_semantic_policy_scenario(
        &ServerConfig::new(java, server_jar),
        env::var_os("MDL_KEEP_TEST_DIR").is_some(),
        &scenario,
        &deployments,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn calibration() -> Result<(mdl_test::scenario::MinecraftScenario, Vec<PolicyDeployment>), String> {
    let objective = TestName::new(OBJECTIVE).map_err(|error| error.to_string())?;
    let expected = ObservationSet::try_new(
        [
            ("no_result", 99, 99, 1),
            ("failed", 0, 0, 1),
            ("successful_zero", 1, 0, 1),
            ("returned_zero", 1, 0, 1),
            ("returned_nonzero", 1, 7, 1),
            ("no_context", 99, 99, 1),
        ]
        .into_iter()
        .flat_map(|(case, success, result, continued)| {
            [
                score_observation(&objective, format!("#{case}_s"), success),
                score_observation(&objective, format!("#{case}_r"), result),
                score_observation(&objective, format!("#{case}_c"), continued),
            ]
        }),
    )
    .map_err(|error| error.to_string())?;
    let owned = OwnedResource::Objective(objective);
    let scenario = ScenarioSpec::new(
        ScenarioId::new("minecraft/outcome-matrix").map_err(|error| error.to_string())?,
        GenerationNonce::new(0x0000_0c01).map_err(|error| error.to_string())?,
        ScenarioEntry::Export(TestName::new("outcome_matrix").map_err(|error| error.to_string())?),
    )
    .with_mode(ScenarioMode::Semantic {
        outcome: Some(CommandOutcomeExpectation::new(
            OutcomeChannelExpectation::Unavailable,
            OutcomeChannelExpectation::Unavailable,
            CompletionExpectation::Continued,
        )),
    })
    .with_owned_resources(vec![owned.clone()])
    .with_cleanup(vec![owned])
    .with_expected(expected)
    .build()
    .map_err(|error| error.to_string())?;
    let deployments = CompilerPolicy::ALL
        .into_iter()
        .enumerate()
        .map(|(index, policy)| deployment(index, policy))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((scenario, deployments))
}

fn score_observation(
    objective: &TestName,
    holder: String,
    expected: i32,
) -> (ObservationPath, ObservationValue) {
    (
        ObservationPath::Score {
            holder: holder.into_boxed_str(),
            objective: objective.clone(),
        },
        ObservationValue::Score(Some(expected)),
    )
}

fn deployment(index: usize, policy: CompilerPolicy) -> Result<PolicyDeployment, String> {
    let namespace = format!("mdl_ps1_outcome_{index}");
    let matrix = matrix_function(&namespace);
    let files = vec![
        DeploymentFile::new("pack.mcmeta", PACK_META.as_bytes().to_vec()),
        DeploymentFile::new(
            format!("data/{namespace}/function/matrix.mcfunction"),
            matrix.into_bytes(),
        ),
        DeploymentFile::new(
            format!("data/{namespace}/function/no_result.mcfunction"),
            b"scoreboard players add #unused mdl.ps1.out 0\n".to_vec(),
        ),
        DeploymentFile::new(
            format!("data/{namespace}/function/return_zero.mcfunction"),
            b"return 0\n".to_vec(),
        ),
        DeploymentFile::new(
            format!("data/{namespace}/function/return_seven.mcfunction"),
            b"return 7\n".to_vec(),
        ),
    ];
    Ok(PolicyDeployment::new(
        policy,
        namespace.clone(),
        files,
        ScenarioEntry::Export(TestName::new("outcome_matrix").map_err(|error| error.to_string())?),
        Vec::new(),
        ResourceLocation::new(namespace, "matrix").map_err(|error| error.to_string())?,
        Vec::new(),
    ))
}

fn matrix_function(namespace: &str) -> String {
    let mut commands = vec![format!(
        "data modify storage {namespace}:fixture zero set value 0"
    )];
    add_case(
        &mut commands,
        "no_result",
        &format!("function {namespace}:no_result"),
        Some(99),
    );
    add_case(
        &mut commands,
        "failed",
        &format!("data get storage {namespace}:fixture missing 1"),
        None,
    );
    add_case(
        &mut commands,
        "successful_zero",
        &format!("data get storage {namespace}:fixture zero 1"),
        None,
    );
    add_case(
        &mut commands,
        "returned_zero",
        &format!("function {namespace}:return_zero"),
        None,
    );
    add_case(
        &mut commands,
        "returned_nonzero",
        &format!("function {namespace}:return_seven"),
        None,
    );
    add_case(
        &mut commands,
        "no_context",
        &format!("execute as @e[tag=mdl_ps1_outcome_absent] run function {namespace}:return_seven"),
        Some(99),
    );
    format!("{}\n", commands.join("\n"))
}

fn add_case(commands: &mut Vec<String>, case: &str, command: &str, sentinel: Option<i32>) {
    if let Some(sentinel) = sentinel {
        commands.push(format!(
            "scoreboard players set #{case}_s {OBJECTIVE} {sentinel}"
        ));
        commands.push(format!(
            "scoreboard players set #{case}_r {OBJECTIVE} {sentinel}"
        ));
    }
    commands.push(format!("scoreboard players set #{case}_c {OBJECTIVE} 0"));
    commands.push(format!(
        "execute store success score #{case}_s {OBJECTIVE} store result score #{case}_r {OBJECTIVE} run {command}"
    ));
    commands.push(format!("scoreboard players set #{case}_c {OBJECTIVE} 1"));
}
