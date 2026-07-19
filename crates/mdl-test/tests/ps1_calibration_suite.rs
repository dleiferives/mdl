use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use mdl_test::scenario::{
    CompilerPolicy, DeploymentFile, DriverCommand, EntityCollection, EntityComparison,
    EntityRecord, EntitySelector, Executor, GenerationNonce, InvocationFrame, NbtValue,
    ObservationPath, ObservationSet, ObservationValue, OwnedResource, PolicyDeployment,
    ResourceLocation, ScenarioEntry, ScenarioId, ScenarioSpec, SemanticScenarioCase, TestName,
    run_semantic_policy_suite,
};
use mdl_test::{ServerConfig, sha256_file};

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const JAVA_26_2_EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";
const PACK_META: &str = concat!(
    "{\n",
    "  \"pack\": {\n",
    "    \"description\": \"MDL PS-1 batched calibration\",\n",
    "    \"min_format\": [107, 1],\n",
    "    \"max_format\": [107, 1]\n",
    "  }\n",
    "}\n",
);

#[test]
fn batched_calibration_constructs_bounded_nbt_and_nested_entity_cases() {
    let suite = calibration().unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(suite.len(), 2);
    assert_eq!(suite[0].1.len(), CompilerPolicy::ALL.len());
    assert_eq!(suite[1].1.len(), CompilerPolicy::ALL.len());
}

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn batched_nbt_and_nested_entity_calibration_runs_on_vanilla_26_2() {
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
    let suite = calibration()?;
    let cases = suite
        .iter()
        .map(|(scenario, deployments)| SemanticScenarioCase::new(scenario, deployments))
        .collect::<Vec<_>>();
    run_semantic_policy_suite(
        &ServerConfig::new(java, server_jar),
        env::var_os("MDL_KEEP_TEST_DIR").is_some(),
        &cases,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

fn calibration()
-> Result<Vec<(mdl_test::scenario::MinecraftScenario, Vec<PolicyDeployment>)>, String> {
    Ok(vec![storage_case()?, entity_case()?])
}

fn storage_case() -> Result<(mdl_test::scenario::MinecraftScenario, Vec<PolicyDeployment>), String>
{
    let storage = ResourceLocation::parse("mdl_ps1_suite:state").map_err(|e| e.to_string())?;
    let expected_nbt = NbtValue::parse_snbt(r#"{count:3,items:[1,2,3],text:"hello ☃"}"#)
        .map_err(|error| error.to_string())?;
    let expected = ObservationSet::try_new([(
        ObservationPath::Storage {
            storage: storage.clone(),
            path: "value".into(),
        },
        ObservationValue::Nbt(Some(expected_nbt)),
    )])
    .map_err(|error| error.to_string())?;
    let owned = OwnedResource::Storage(storage);
    let scenario = ScenarioSpec::new(
        ScenarioId::new("minecraft/batched-storage").map_err(|e| e.to_string())?,
        GenerationNonce::new(0x0000_1001).map_err(|e| e.to_string())?,
        ScenarioEntry::Export(TestName::new("write_storage").map_err(|e| e.to_string())?),
    )
    .with_owned_resources(vec![owned.clone()])
    .with_cleanup(vec![owned])
    .with_expected(expected)
    .build()
    .map_err(|error| error.to_string())?;
    let deployments = deployments("storage", "write_storage", |namespace| {
        format!(
            "data modify storage mdl_ps1_suite:state value set value {{count:3,items:[1,2,3],text:\"hello ☃\"}}\n# {namespace}\n"
        )
    })?;
    Ok((scenario, deployments))
}

fn entity_case() -> Result<(mdl_test::scenario::MinecraftScenario, Vec<PolicyDeployment>), String> {
    let objective = TestName::new("mdl.ps1.ent").map_err(|e| e.to_string())?;
    let tag = TestName::new("mdl_ps1_entity").map_err(|e| e.to_string())?;
    let dimension = ResourceLocation::parse("minecraft:overworld").map_err(|e| e.to_string())?;
    let empty_record = EntityRecord::new(BTreeMap::new());
    let expected = ObservationSet::try_new([
        (
            ObservationPath::Score {
                holder: "#one".into(),
                objective: objective.clone(),
            },
            ObservationValue::Score(Some(3)),
        ),
        (
            ObservationPath::Score {
                holder: "#zero".into(),
                objective: objective.clone(),
            },
            ObservationValue::Score(None),
        ),
        (
            ObservationPath::Score {
                holder: "#outer".into(),
                objective: objective.clone(),
            },
            ObservationValue::Score(Some(3)),
        ),
        (
            ObservationPath::Score {
                holder: "#nested".into(),
                objective: objective.clone(),
            },
            ObservationValue::Score(Some(9)),
        ),
        (
            ObservationPath::Entities {
                tag: tag.clone(),
                comparison: EntityComparison::Multiset,
            },
            ObservationValue::Entities(EntityCollection::Multiset(BTreeMap::from([(
                empty_record,
                3,
            )]))),
        ),
    ])
    .map_err(|error| error.to_string())?;
    let resources = vec![
        OwnedResource::Objective(objective.clone()),
        OwnedResource::EntityTag(tag.clone()),
        OwnedResource::ForcedChunk {
            dimension,
            x: 0,
            z: 0,
        },
    ];
    let scenario = ScenarioSpec::new(
        ScenarioId::new("minecraft/batched-entities").map_err(|e| e.to_string())?,
        GenerationNonce::new(0x0000_1002).map_err(|e| e.to_string())?,
        ScenarioEntry::Export(TestName::new("nested_entities").map_err(|e| e.to_string())?),
    )
    .with_invocation(InvocationFrame::server().with_executor(Executor::Entities(
        EntitySelector::new("@e[tag=mdl_ps1_entity]").map_err(|e| e.to_string())?,
    )))
    .with_owned_resources(resources.clone())
    .with_cleanup(resources)
    .with_expected(expected)
    .build()
    .map_err(|error| error.to_string())?;
    let deployments = CompilerPolicy::ALL
        .into_iter()
        .enumerate()
        .map(|(index, policy)| {
            let namespace = format!("mdl_ps1_entity_{index}");
            let prelude = (0..3)
                .map(|x| {
                    DriverCommand::new(format!(
                        "summon minecraft:armor_stand {x} 80 0 {{Tags:[\"mdl_ps1_entity\"],Marker:1b,Invisible:1b,NoGravity:1b}}"
                    ))
                    .map_err(|error| error.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let root = concat!(
                "execute as @e[tag=mdl_ps1_absent] run scoreboard players add #zero mdl.ps1.ent 1\n",
                "execute as @e[tag=mdl_ps1_entity,limit=1] run scoreboard players add #one mdl.ps1.ent 1\n",
                "scoreboard players add #outer mdl.ps1.ent 1\n",
                "execute as @e[tag=mdl_ps1_entity] run scoreboard players add #nested mdl.ps1.ent 1\n",
            );
            deployment(policy, namespace, "nested_entities", root.to_owned(), prelude)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((scenario, deployments))
}

fn deployments(
    stem: &str,
    entry: &str,
    root: impl Fn(&str) -> String,
) -> Result<Vec<PolicyDeployment>, String> {
    CompilerPolicy::ALL
        .into_iter()
        .enumerate()
        .map(|(index, policy)| {
            let namespace = format!("mdl_ps1_{stem}_{index}");
            deployment(
                policy,
                namespace.clone(),
                entry,
                root(&namespace),
                Vec::new(),
            )
        })
        .collect()
}

fn deployment(
    policy: CompilerPolicy,
    namespace: String,
    logical_entry: &str,
    root: String,
    prelude: Vec<DriverCommand>,
) -> Result<PolicyDeployment, String> {
    Ok(PolicyDeployment::new(
        policy,
        namespace.clone(),
        vec![
            DeploymentFile::new("pack.mcmeta", PACK_META.as_bytes().to_vec()),
            DeploymentFile::new(
                format!("data/{namespace}/function/root.mcfunction"),
                root.into_bytes(),
            ),
        ],
        ScenarioEntry::Export(TestName::new(logical_entry).map_err(|e| e.to_string())?),
        prelude,
        ResourceLocation::new(namespace, "root").map_err(|e| e.to_string())?,
        Vec::new(),
    ))
}
