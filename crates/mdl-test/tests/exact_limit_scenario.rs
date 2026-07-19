use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use mdl_test::scenario::{
    CompilerPolicy, DeploymentFile, DriverCommand, EntityCollection, EntityComparison,
    EntityRecord, ExactLimitContract, ExactLimitKind, GenerationNonce, ObservationPath,
    ObservationSet, ObservationValue, OwnedResource, PolicyDeployment, ResourceLocation,
    ScenarioEntry, ScenarioId, ScenarioMode, ScenarioSpec, TestName,
    run_exact_limit_policy_scenario,
};
use mdl_test::{ServerConfig, sha256_file};

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const JAVA_26_2_EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";
const OBJECTIVE: &str = "mdl.ps1.limit";
const ROOT_FUNCTION: &str = concat!(
    "scoreboard players add #body mdl.ps1.limit 1\n",
    "scoreboard players set #completed mdl.ps1.limit 1\n",
);
const FORK_ROOT_FUNCTION: &str = concat!(
    "execute as @e[tag=mdl_ps1_fork] run scoreboard players add #body mdl.ps1.limit 1\n",
    "scoreboard players set #completed mdl.ps1.limit 1\n",
);
const PACK_META: &str = concat!(
    "{\n",
    "  \"pack\": {\n",
    "    \"description\": \"MDL PS-1 bare exact-limit calibration\",\n",
    "    \"min_format\": [107, 1],\n",
    "    \"max_format\": [107, 1]\n",
    "  }\n",
    "}\n",
);

#[test]
fn exact_limit_calibration_keeps_the_completion_write_inside_the_measured_root() {
    let (scenario, deployments) = calibration().unwrap_or_else(|error| panic!("{error}"));
    let ScenarioMode::ExactLimit(contract) = scenario.mode() else {
        panic!("expected exact-limit scenario");
    };
    assert_eq!(contract.kind(), ExactLimitKind::Sequence);
    assert_eq!(contract.configured_limit(), 2);
    assert!(!contract.root_completes());
    assert_eq!(deployments.len(), CompilerPolicy::ALL.len());
    assert_eq!(ROOT_FUNCTION.lines().count(), 2);
    assert!(ROOT_FUNCTION.ends_with("#completed mdl.ps1.limit 1\n"));

    let (fork, fork_deployments) = fork_calibration().unwrap_or_else(|error| panic!("{error}"));
    let ScenarioMode::ExactLimit(contract) = fork.mode() else {
        panic!("expected exact-limit scenario");
    };
    assert_eq!(contract.kind(), ExactLimitKind::Fork);
    assert_eq!(contract.configured_limit(), 3);
    assert!(contract.root_completes());
    assert_eq!(fork_deployments.len(), CompilerPolicy::ALL.len());
    assert_eq!(FORK_ROOT_FUNCTION.lines().count(), 2);
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn exact_sequence_and_fork_boundaries_run_without_semantic_wrappers_on_vanilla_26_2() {
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
    run_exact_limit_policy_scenario(
        &ServerConfig::new(java.clone(), server_jar.clone()),
        env::var_os("MDL_KEEP_TEST_DIR").is_some(),
        &scenario,
        &deployments,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())?;
    let (scenario, deployments) = fork_calibration()?;
    run_exact_limit_policy_scenario(
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
    let completion = ObservationPath::Score {
        holder: "#completed".into(),
        objective: objective.clone(),
    };
    let expected = ObservationSet::try_new([(
        ObservationPath::Score {
            holder: "#body".into(),
            objective: objective.clone(),
        },
        ObservationValue::Score(Some(1)),
    )])
    .map_err(|error| error.to_string())?;
    let owned = OwnedResource::Objective(objective);
    let scenario = ScenarioSpec::new(
        ScenarioId::new("minecraft/sequence-interruption").map_err(|error| error.to_string())?,
        GenerationNonce::new(0x0000_0e01).map_err(|error| error.to_string())?,
        ScenarioEntry::Export(TestName::new("sequence_probe").map_err(|error| error.to_string())?),
    )
    .with_mode(ScenarioMode::ExactLimit(ExactLimitContract::new(
        ExactLimitKind::Sequence,
        2,
        completion,
        false,
    )))
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

fn deployment(index: usize, policy: CompilerPolicy) -> Result<PolicyDeployment, String> {
    let namespace = format!("mdl_ps1_limit_{index}");
    Ok(PolicyDeployment::new(
        policy,
        namespace.clone(),
        vec![
            DeploymentFile::new("pack.mcmeta", PACK_META.as_bytes().to_vec()),
            DeploymentFile::new(
                format!("data/{namespace}/function/probe.mcfunction"),
                ROOT_FUNCTION.as_bytes().to_vec(),
            ),
        ],
        ScenarioEntry::Export(TestName::new("sequence_probe").map_err(|error| error.to_string())?),
        Vec::new(),
        ResourceLocation::new(namespace, "probe").map_err(|error| error.to_string())?,
        Vec::new(),
    ))
}

fn fork_calibration()
-> Result<(mdl_test::scenario::MinecraftScenario, Vec<PolicyDeployment>), String> {
    let objective = TestName::new(OBJECTIVE).map_err(|error| error.to_string())?;
    let tag = TestName::new("mdl_ps1_fork").map_err(|error| error.to_string())?;
    let completion = ObservationPath::Score {
        holder: "#completed".into(),
        objective: objective.clone(),
    };
    let expected = ObservationSet::try_new([
        (
            ObservationPath::Score {
                holder: "#body".into(),
                objective: objective.clone(),
            },
            ObservationValue::Score(None),
        ),
        (
            ObservationPath::Entities {
                tag: tag.clone(),
                comparison: EntityComparison::Multiset,
            },
            ObservationValue::Entities(EntityCollection::Multiset(BTreeMap::from([(
                EntityRecord::new(BTreeMap::new()),
                3,
            )]))),
        ),
    ])
    .map_err(|error| error.to_string())?;
    let dimension = ResourceLocation::parse("minecraft:overworld").map_err(|e| e.to_string())?;
    let resources = vec![
        OwnedResource::Objective(objective),
        OwnedResource::EntityTag(tag),
        OwnedResource::ForcedChunk {
            dimension,
            x: 0,
            z: 0,
        },
    ];
    let scenario = ScenarioSpec::new(
        ScenarioId::new("minecraft/fork-interruption").map_err(|error| error.to_string())?,
        GenerationNonce::new(0x0000_0e02).map_err(|error| error.to_string())?,
        ScenarioEntry::Export(TestName::new("fork_probe").map_err(|error| error.to_string())?),
    )
    .with_mode(ScenarioMode::ExactLimit(ExactLimitContract::new(
        ExactLimitKind::Fork,
        3,
        completion,
        true,
    )))
    .with_owned_resources(resources.clone())
    .with_cleanup(resources)
    .with_expected(expected)
    .build()
    .map_err(|error| error.to_string())?;
    let deployments = CompilerPolicy::ALL
        .into_iter()
        .enumerate()
        .map(|(index, policy)| fork_deployment(index, policy))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((scenario, deployments))
}

fn fork_deployment(index: usize, policy: CompilerPolicy) -> Result<PolicyDeployment, String> {
    let namespace = format!("mdl_ps1_fork_{index}");
    let prelude = (0..3)
        .map(|x| {
            DriverCommand::new(format!(
                "summon minecraft:armor_stand {x} 80 0 {{Tags:[\"mdl_ps1_fork\"],Marker:1b,Invisible:1b,NoGravity:1b}}"
            ))
            .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PolicyDeployment::new(
        policy,
        namespace.clone(),
        vec![
            DeploymentFile::new("pack.mcmeta", PACK_META.as_bytes().to_vec()),
            DeploymentFile::new(
                format!("data/{namespace}/function/probe.mcfunction"),
                FORK_ROOT_FUNCTION.as_bytes().to_vec(),
            ),
        ],
        ScenarioEntry::Export(TestName::new("fork_probe").map_err(|error| error.to_string())?),
        prelude,
        ResourceLocation::new(namespace, "probe").map_err(|error| error.to_string())?,
        Vec::new(),
    ))
}
