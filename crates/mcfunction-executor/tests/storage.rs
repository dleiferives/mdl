use std::fs;
use std::path::PathBuf;

use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcfunction-executor-storage-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dp = dir.join("world").join("datapacks").join("test");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"storage","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    fs::create_dir_all(dp.join("data").join("test")).unwrap();
    dir
}

#[test]
fn storage_set_and_get_compound() {
    let dir = sandbox("set-get");
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("data modify storage mdl:test test_compound set value {a:1,b:2}").unwrap();
    exec.command("data get storage mdl:test test_compound").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("{a:1,b:2}") || line.contains("{b:2,a:1}"), "got: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn storage_list_operations() {
    let dir = sandbox("list-ops");
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("data modify storage mdl:test ordered set value [1,2]").unwrap();
    exec.command("data get storage mdl:test ordered").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("[1,2]"), "got: {line}");

    exec.command("data modify storage mdl:test ordered append value 3").unwrap();
    exec.command("data get storage mdl:test ordered").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("1,2,3"), "got: {line}");

    exec.command("data modify storage mdl:test ordered prepend value 0").unwrap();
    exec.command("data get storage mdl:test ordered").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("0,1,2,3"), "got: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn storage_copy_and_merge() {
    let dir = sandbox("copy-merge");
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("data modify storage mdl:test source set value {b:2,a:1}").unwrap();
    exec.command("data modify storage mdl:test copied set from storage mdl:test source").unwrap();
    exec.command("data get storage mdl:test copied").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("{a:1,b:2}") || line.contains("{b:2,a:1}"), "got: {line}");

    exec.command("data modify storage mdl:test copied merge value {c:3}").unwrap();
    exec.command("data get storage mdl:test copied").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("c:3"), "got: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn storage_remove_key() {
    let dir = sandbox("remove-key");
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("data modify storage mdl:test test set value {a:1,b:2,c:3}").unwrap();
    exec.command("data remove storage mdl:test test.c").unwrap();
    exec.command("data get storage mdl:test test").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(!line.contains("c:3"), "should not contain c: {line}");
    assert!(line.contains("a:1"), "should still contain a: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_if_data_condition() {
    let dir = sandbox("if-data");
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard objectives add test dummy").unwrap();
    exec.command("scoreboard players set #flag test 0").unwrap();

    // Condition false (doesn't exist yet)
    exec.command("execute if data storage mdl:test result run scoreboard players set #flag test 1").unwrap();
    exec.command("scoreboard players get #flag test").unwrap();
    let line = exec.wait_for_command_log("#flag has").unwrap();
    assert!(line.contains("#flag has 0"), "got: {line}");

    // Create it
    exec.command("data modify storage mdl:test result set value 1").unwrap();

    // Now condition true
    exec.command("execute if data storage mdl:test result run scoreboard players set #flag test 1").unwrap();
    exec.command("scoreboard players get #flag test").unwrap();
    let line = exec.wait_for_command_log("#flag has").unwrap();
    assert!(line.contains("#flag has 1"), "got: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_store_result_score() {
    let dir = sandbox("store-score");
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard objectives add calc dummy").unwrap();
    exec.command("scoreboard players set #src calc 42").unwrap();
    exec.command("execute store result score #dest calc run scoreboard players get #src calc").unwrap();
    exec.command("scoreboard players get #dest calc").unwrap();
    let line = exec.wait_for_command_log("#dest has").unwrap();
    assert!(line.contains("#dest has 42"), "got: {line}");

    let _ = fs::remove_dir_all(&dir);
}
