use std::fs;
use std::path::PathBuf;

use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcfunction-executor-func-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dp = dir.join("world").join("datapacks").join("test");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"func","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    fs::create_dir_all(dp.join("data").join("test").join("function")).unwrap();
    dir
}

#[test]
fn function_return_value() {
    let dir = sandbox("return-value");
    let dp = dir.join("world").join("datapacks").join("test");

    fs::write(
        dp.join("data/test/function/ret.mcfunction"),
        "scoreboard objectives add test dummy\nscoreboard players set #val test 0\nreturn 7\nscoreboard players set #val test 99\n",
    ).unwrap();

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("function test:ret").unwrap();
    exec.command("scoreboard players get #val test").unwrap();
    let line = exec.wait_for_command_log("#val has").unwrap();
    assert!(line.contains("#val has 0"), "should not have executed after return: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn function_return_fail() {
    let dir = sandbox("return-fail");
    let dp = dir.join("world").join("datapacks").join("test");

    fs::write(
        dp.join("data/test/function/failfn.mcfunction"),
        "scoreboard objectives add test dummy\nscoreboard players set #flag test 1\nreturn fail\nscoreboard players set #flag test 2\n",
    ).unwrap();

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("function test:failfn").unwrap();
    exec.command("scoreboard players get #flag test").unwrap();
    let line = exec.wait_for_command_log("#flag has").unwrap();
    assert!(line.contains("#flag has 1"), "should not have set to 2: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn function_tag_execution() {
    let dir = sandbox("tag-exec");
    let dp = dir.join("world").join("datapacks").join("test");

    fs::write(
        dp.join("data/test/function/first.mcfunction"),
        "scoreboard objectives add tagtest dummy\nscoreboard players set #a tagtest 10\n",
    ).unwrap();
    fs::write(
        dp.join("data/test/function/second.mcfunction"),
        "scoreboard players set #b tagtest 20\n",
    ).unwrap();

    // Create tag directory
    let tag_dir = dp.join("data").join("test").join("tags").join("function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(
        tag_dir.join("mytag.json"),
        r#"{"values":["test:first","test:second"]}"#,
    ).unwrap();

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("function #test:mytag").unwrap();
    exec.command("scoreboard players get #a tagtest").unwrap();
    let line = exec.wait_for_command_log("#a has").unwrap();
    assert!(line.contains("#a has 10"), "got: {line}");

    exec.command("scoreboard players get #b tagtest").unwrap();
    let line = exec.wait_for_command_log("#b has").unwrap();
    assert!(line.contains("#b has 20"), "got: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_if_function_condition() {
    let dir = sandbox("if-func");
    let dp = dir.join("world").join("datapacks").join("test");

    fs::write(
        dp.join("data/test/function/condfn.mcfunction"),
        "scoreboard objectives add cond dummy\nscoreboard players set #marker cond 1\n",
    ).unwrap();

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    // test:condfn exists, so condition is true
    exec.command("scoreboard players set #trigger cond 0").unwrap();
    exec.command("execute if function test:condfn run scoreboard players set #trigger cond 999").unwrap();
    exec.command("scoreboard players get #trigger cond").unwrap();
    let line = exec.wait_for_command_log("#trigger has").unwrap();
    assert!(line.contains("#trigger has 999"), "got: {line}");

    // test:missing doesn't exist, condition is false
    exec.command("scoreboard players set #trigger cond 0").unwrap();
    exec.command("execute if function test:missing run scoreboard players set #trigger cond 777").unwrap();
    exec.command("scoreboard players get #trigger cond").unwrap();
    let line = exec.wait_for_command_log("#trigger has").unwrap();
    assert!(line.contains("#trigger has 0"), "got: {line}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn function_sequence_limit() {
    let dir = sandbox("seq-limit");
    let dp = dir.join("world").join("datapacks").join("test");

    // A function with many commands
    let mut commands = String::new();
    commands.push_str("scoreboard objectives add seq dummy\n");
    for i in 1..=10 {
        commands.push_str(&format!("scoreboard players set #step seq {i}\n"));
    }

    fs::write(dp.join("data/test/function/longfn.mcfunction"), commands).unwrap();

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    // Set sequence limit to 3
    exec.command("gamerule max_command_sequence_length 3").unwrap();
    exec.command("function test:longfn").unwrap();
    exec.command("scoreboard players get #step seq").unwrap();
    let line = exec.wait_for_command_log("#step has").unwrap();
    // Should stop after ~3 commands
    assert!(!line.contains("has 10"), "should have stopped early: {line}");

    // Restore and verify it completes
    exec.command("gamerule max_command_sequence_length 65536").unwrap();
    exec.command("function test:longfn").unwrap();
    exec.command("scoreboard players get #step seq").unwrap();
    let line = exec.wait_for_command_log("#step has").unwrap();
    assert!(line.contains("has 10"), "should complete: {line}");

    let _ = fs::remove_dir_all(&dir);
}
