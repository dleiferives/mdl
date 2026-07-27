use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use mcfunction_executor::{McExecutor, V26_2};

/// Build a representative datapack exercising: scoreboard ops, NBT storage,
/// execute if/unless conditions, store capture, function return, tags.
fn build_fixture_datapack(sandbox_root: &PathBuf) {
    let dp = sandbox_root.join("world").join("datapacks").join("mdl_xv");
    fs::create_dir_all(&dp).unwrap();

    // pack.mcmeta
    fs::write(
        dp.join("pack.mcmeta"),
        r#"{"pack":{"description":"cross-validation","min_format":[107,1],"max_format":[107,1]}}"#,
    )
    .unwrap();

    let func_dir = dp.join("data").join("mdl").join("function");
    fs::create_dir_all(&func_dir).unwrap();

    // init: sets up objectives
    fs::write(
        func_dir.join("init.mcfunction"),
        concat!(
            "scoreboard objectives add xv dummy\n",
            "scoreboard objectives add xv2 dummy\n",
        ),
    )
    .unwrap();

    // arithmetic: set + add + remove + multiply
    fs::write(
        func_dir.join("arithmetic.mcfunction"),
        concat!(
            "scoreboard players set #a xv 10\n",
            "scoreboard players add #a xv 5\n",
            "scoreboard players remove #a xv 3\n",
            "scoreboard players set #b xv 2\n",
            "scoreboard players operation #a xv *= #b xv\n",
        ),
    )
    .unwrap();

    // storage: basic set/get/copy/merge/remove
    fs::write(
        func_dir.join("storage_ops.mcfunction"),
        concat!(
            "data modify storage mdl:test source set value {b:2,a:1}\n",
            "data modify storage mdl:test ordered set value [1,2]\n",
            "data modify storage mdl:test ordered append value 3\n",
            "data modify storage mdl:test ordered prepend value 0\n",
            "data modify storage mdl:test copied set from storage mdl:test source\n",
            "data modify storage mdl:test copied merge value {c:3}\n",
        ),
    )
    .unwrap();

    // conditions: if/unless score, if data
    fs::write(
        func_dir.join("conditions.mcfunction"),
        concat!(
            "scoreboard players set #flag xv2 1\n",
            "scoreboard players set #target xv2 0\n",
            "execute if score #flag xv2 matches 1 run scoreboard players set #target xv2 99\n",
            "execute unless score #flag xv2 matches 0 run scoreboard players set #target xv2 50\n",
            "execute if data storage mdl:test copied run scoreboard players set #has_data xv2 1\n",
        ),
    )
    .unwrap();

    // store: execute store result/success
    fs::write(
        func_dir.join("store_test.mcfunction"),
        concat!(
            "scoreboard players set #src xv 7\n",
            "execute store result score #dest xv run scoreboard players get #src xv\n",
            "execute store success score #succ xv run scoreboard players get #src xv\n",
        ),
    )
    .unwrap();

    // return value
    fs::write(
        func_dir.join("return_val.mcfunction"),
        "return 42\nscoreboard players set #never xv 999\n",
    )
    .unwrap();

    // Load tag
    let tag_dir = dp.join("data").join("minecraft").join("tags").join("function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(
        tag_dir.join("load.json"),
        r#"{"values":["mdl:init"]}"#,
    )
    .unwrap();
}

struct ExecutorAssertions {
    scores: Vec<(String, String, i32)>,  // (holder, objective, expected_value)
    storage_checks: Vec<(String, String, String)>,  // (storage_id, path, expected_snbt_substring)
    log_contains: Vec<String>,
}

fn run_on_real_server(sandbox: &PathBuf) -> Result<ExecutorAssertions, String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "MDL_SERVER_JAR not set".to_owned())?;
    let java = env::var_os("MDL_JAVA").unwrap_or_else(|| "java".into());

    // We'd need to spawn a real Java server. This is a placeholder for now.
    // The real implementation uses ServerSandbox from mdl-test.
    let _ = (server_jar, java, sandbox);
    Err("real server execution requires mdl-test integration".to_owned())
}

fn run_on_executor(sandbox: &PathBuf) -> ExecutorAssertions {
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    // Run init + arithmetic + storage + conditions + store + return
    exec.command("function mdl:arithmetic").unwrap();
    exec.command("function mdl:storage_ops").unwrap();
    exec.command("function mdl:conditions").unwrap();
    exec.command("function mdl:store_test").unwrap();
    exec.command("function mdl:return_val").unwrap();

    // Collect all score observations
    let mut scores = Vec::new();
    for (holder, objective) in &[
        ("#a", "xv"), ("#b", "xv"), ("#dest", "xv"), ("#succ", "xv"),
        ("#target", "xv2"), ("#has_data", "xv2"), ("#flag", "xv2"),
    ] {
        exec.command(&format!("scoreboard players get {holder} {objective}")).unwrap();
        let marker = format!("{holder} has");
        if let Ok(line) = exec.wait_for_command_log(&marker) {
            let value = parse_score_value(&line);
            scores.push((holder.to_string(), objective.to_string(), value));
        }
    }

    // Collect storage observations
    let mut storage_checks = Vec::new();
    for (storage, path) in &[
        ("mdl:test", "copied"),
        ("mdl:test", "ordered"),
        ("mdl:test", "source"),
    ] {
        exec.command(&format!("data get storage {storage} {path}")).unwrap();
        if let Ok(line) = exec.wait_for_command_log("has the following contents") {
            storage_checks.push((storage.to_string(), path.to_string(), line));
        }
    }

    ExecutorAssertions {
        scores,
        storage_checks,
        log_contains: vec![],
    }
}

fn parse_score_value(line: &str) -> i32 {
    // Format: "#holder has 42 [objective]"
    line.split_whitespace()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[test]
fn executor_arithmetic_scores() {
    let sandbox = std::env::temp_dir().join("mdl-xv-arithmetic");
    let _ = fs::remove_dir_all(&sandbox);
    fs::create_dir_all(&sandbox).unwrap();
    build_fixture_datapack(&sandbox);

    let results = run_on_executor(&sandbox);

    // Verify expected score values
    let expected = vec![
        ("#a", "xv", 24),       // (10+5-3)*2 = 24
        ("#b", "xv", 2),
        ("#dest", "xv", 7),
        ("#succ", "xv", 1),
        ("#target", "xv2", 50),   // unless #flag matches 0 → #flag=1 doesn't match 0 → runs the set
        ("#has_data", "xv2", 1),
        ("#flag", "xv2", 1),
    ];

    for (holder, obj, exp_val) in expected {
        let actual = results.scores.iter()
            .find(|(h, o, _)| h == holder && o == obj)
            .map(|(_, _, v)| *v);
        assert_eq!(
            actual,
            Some(exp_val),
            "{holder} {obj}: expected {exp_val}, got {actual:?}"
        );
    }

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn executor_storage_shapes() {
    let sandbox = std::env::temp_dir().join("mdl-xv-storage");
    let _ = fs::remove_dir_all(&sandbox);
    fs::create_dir_all(&sandbox).unwrap();
    build_fixture_datapack(&sandbox);

    let results = run_on_executor(&sandbox);

    // Verify storage shapes
    let ordered = results.storage_checks.iter()
        .find(|(_, p, _)| p == "ordered")
        .map(|(_, _, l)| l.clone())
        .expect("ordered storage");
    assert!(ordered.contains("0,1,2,3") || ordered.contains("0, 1, 2, 3"),
        "ordered list: {ordered}");

    let copied = results.storage_checks.iter()
        .find(|(_, p, _)| p == "copied")
        .map(|(_, _, l)| l.clone())
        .expect("copied storage");
    assert!(copied.contains("a:1"), "copied has a:1");
    assert!(copied.contains("b:2"), "copied has b:2");
    assert!(copied.contains("c:3"), "copied merged c:3");

    let _ = fs::remove_dir_all(&sandbox);
}
