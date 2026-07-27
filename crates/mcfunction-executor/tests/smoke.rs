use std::fs;
use std::path::PathBuf;

use mcfunction_executor::{McExecutor, V26_2};

fn create_sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcfunction-executor-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let dp = dir.join("world").join("datapacks").join("smoke_test");
    fs::create_dir_all(&dp).unwrap();

    fs::write(
        dp.join("pack.mcmeta"),
        r#"{"pack":{"description":"smoke","min_format":[107,1],"max_format":[107,1]}}"#,
    )
    .unwrap();

    // Create just an empty data namespace so load_datapacks doesn't fail
    fs::create_dir_all(dp.join("data").join("smoke")).unwrap();

    dir
}

fn create_sandbox_with_function(name: &str) -> PathBuf {
    let dir = create_sandbox(name);
    let dp = dir.join("world").join("datapacks").join("smoke_test");
    let func_dir = dp.join("data").join("smoke").join("function");
    fs::create_dir_all(&func_dir).unwrap();
    fs::write(
        func_dir.join("test.mcfunction"),
        concat!(
            "scoreboard objectives add smoke dummy\n",
            "scoreboard players set #result smoke 42\n",
        ),
    )
    .unwrap();
    dir
}

#[test]
fn smoke_scoreboard_set_and_get() {
    let sandbox = create_sandbox_with_function("sb-set-get");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("function smoke:test").unwrap();
    exec.command("scoreboard players get #result smoke")
        .unwrap();
    let line = exec.wait_for_command_log("has 42").unwrap();
    assert!(line.contains("#result has 42"), "got: {line}");

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn smoke_scoreboard_arithmetic() {
    let sandbox = create_sandbox("sb-arithmetic");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard objectives add calc dummy")
        .unwrap();
    exec.command("scoreboard players set #a calc 10").unwrap();
    exec.command("scoreboard players add #a calc 5").unwrap();
    exec.command("scoreboard players get #a calc").unwrap();
    let line = exec.wait_for_command_log("has 15").unwrap();
    assert!(line.contains("#a has 15"), "got: {line}");

    exec.command("scoreboard players remove #a calc 3").unwrap();
    exec.command("scoreboard players get #a calc").unwrap();
    let line = exec.wait_for_command_log("has 12").unwrap();
    assert!(line.contains("#a has 12"), "got: {line}");

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn smoke_scoreboard_operation() {
    let sandbox = create_sandbox("sb-operation");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard objectives add calc dummy")
        .unwrap();
    exec.command("scoreboard players set #a calc 10").unwrap();
    exec.command("scoreboard players set #b calc 3").unwrap();

    exec.command("scoreboard players operation #a calc *= #b calc")
        .unwrap();
    exec.command("scoreboard players get #a calc").unwrap();
    let line = exec.wait_for_command_log("has 30").unwrap();
    assert!(line.contains("#a has 30"), "got: {line}");

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn smoke_execute_if_score() {
    let sandbox = create_sandbox("exec-if");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard objectives add calc dummy")
        .unwrap();
    exec.command("scoreboard players set #flag calc 1").unwrap();
    exec.command("scoreboard players set #target calc 0")
        .unwrap();

    exec.command(
        "execute if score #flag calc matches 1 run scoreboard players set #target calc 99",
    )
    .unwrap();
    exec.command("scoreboard players get #target calc").unwrap();
    let line = exec.wait_for_command_log("has 99").unwrap();
    assert!(line.contains("#target has 99"), "got: {line}");

    exec.command(
        "execute unless score #flag calc matches 1 run scoreboard players set #target calc 77",
    )
    .unwrap();
    exec.command("scoreboard players get #target calc").unwrap();
    let line = exec.wait_for_command_log("has 99").unwrap();
    assert!(line.contains("#target has 99"), "still: {line}");

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn smoke_gamerule_and_limits() {
    let sandbox = create_sandbox("gamerule");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("gamerule max_command_sequence_length 5")
        .unwrap();
    let line = exec.wait_for_command_log("is now set to: 5").unwrap();
    assert!(line.contains("5"));

    exec.command("gamerule max_command_sequence_length 65536")
        .unwrap();
    let _ = exec.wait_for_command_log("is now set to: 65536").unwrap();

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn smoke_say_and_log() {
    let sandbox = create_sandbox("say");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("say Hello World").unwrap();
    let line = exec.wait_for_command_log("Hello World").unwrap();
    assert!(line.contains("[Server] Hello World"), "got: {line}");

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn missing_scores_do_not_behave_like_zero() {
    let sandbox = create_sandbox("missing-scores");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard players set #target calc 0")
        .unwrap();
    exec.command(
        "execute if score #missing calc matches 0 run scoreboard players set #target calc 1",
    )
    .unwrap();
    exec.command("scoreboard players set #left calc 0").unwrap();
    exec.command(
        "execute if score #left calc = #missing calc run scoreboard players set #target calc 2",
    )
    .unwrap();

    exec.command("scoreboard players get #target calc").unwrap();
    exec.wait_for_command_log("#target has 0").unwrap();

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn unsupported_commands_are_failures_and_auditable() {
    let sandbox = create_sandbox("unsupported");
    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    exec.load_datapacks().unwrap();
    let checkpoint = exec.log_checkpoint();

    exec.command("not_a_minecraft_command").unwrap();
    exec.wait_for_command_log("unsupported command").unwrap();
    let error = exec
        .check_datapack_logs_since(checkpoint)
        .expect_err("unsupported command must fail the audit");
    assert!(error.contains("not_a_minecraft_command"), "{error}");

    let _ = fs::remove_dir_all(&sandbox);
}

#[test]
fn malformed_function_tags_are_load_errors() {
    let sandbox = create_sandbox("malformed-tag");
    let tag_dir = sandbox.join("world/datapacks/smoke_test/data/smoke/tags/function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(tag_dir.join("broken.json"), r#"{"values":"not-an-array"}"#).unwrap();

    let mut exec = McExecutor::create(sandbox.clone(), V26_2);
    let error = exec
        .load_datapacks()
        .expect_err("malformed function tag must fail loading");
    assert!(error.contains("values"), "{error}");

    let _ = fs::remove_dir_all(&sandbox);
}
