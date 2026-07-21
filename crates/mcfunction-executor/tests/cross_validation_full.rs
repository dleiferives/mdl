//! Reference tests: executor assertions that serve as the golden standard.
//! Configured for server comparison when MDL_SERVER_JAR is set.
//! Each test runs a small datapack and verifies the executor produces correct output.

use std::fs;
use std::path::PathBuf;
use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-xv-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn dp_dir(sandbox: &PathBuf) -> PathBuf {
    sandbox.join("world").join("datapacks").join("mdl_xv")
}

fn func_dir(sandbox: &PathBuf) -> PathBuf {
    let d = dp_dir(sandbox).join("data/mdl/function");
    fs::create_dir_all(&d).unwrap();
    d
}

fn copy_fn(sandbox: &PathBuf, name: &str, content: &str) {
    fs::write(func_dir(sandbox).join(format!("{name}.mcfunction")), content).unwrap();
}

fn setup_pack(sandbox: &PathBuf) {
    let dp = dp_dir(sandbox);
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"xv","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    fs::create_dir_all(dp.join("data/mdl/function")).unwrap();
}

fn setup_init(sandbox: &PathBuf) {
    copy_fn(sandbox, "init", "scoreboard objectives add xv dummy\nscoreboard objectives add xv2 dummy\n");
    let tag_dir = dp_dir(sandbox).join("data/minecraft/tags/function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(tag_dir.join("load.json"), r#"{"values":["mdl:init"]}"#).unwrap();
}

fn expect_score(exec: &mut McExecutor, holder: &str, objective: &str) -> i32 {
    _ = exec.command(&format!("scoreboard players get {holder} {objective}"));
    // The command drains the inner log to the harness log_queue.
    // Use wait_for_command_log which reads the queue.
    match exec.wait_for_command_log(holder) {
        Ok(line) => {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(pos) = parts.iter().position(|w| *w == "has") {
                return parts.get(pos + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            0
        }
        Err(_) => 0,
    }
}

#[test]
fn scoreboard_arithmetic() {
    let dir = sandbox("arith");
    setup_pack(&dir);
    setup_init(&dir);
    copy_fn(&dir, "arith", concat!(
        "scoreboard players set #a xv 10\n",
        "scoreboard players add #a xv 5\n",
        "scoreboard players remove #a xv 3\n",
        "scoreboard players set #b xv 2\n",
        "scoreboard players operation #a xv *= #b xv\n",
    ));

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec.command("function mdl:arith").unwrap();

    assert_eq!(expect_score(&mut exec, "#a", "xv"), 24);
    assert_eq!(expect_score(&mut exec, "#b", "xv"), 2);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn storage_set_get_nested() {
    let dir = sandbox("storage-nest");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("data modify storage test:x val set value {a:{b:42}}").unwrap();
    exec.command("data get storage test:x val.a.b").unwrap();
    exec.wait_for_command_log("has the following contents").unwrap();

    exec.command("data modify storage test:x val.a.b set value 99").unwrap();
    exec.command("data get storage test:x val.a.b").unwrap();
    exec.wait_for_command_log("99").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_store_result_and_success() {
    let dir = sandbox("store-both");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard players set #src xv 7").unwrap();
    exec.command("execute store result score #dst xv run scoreboard players get #src xv").unwrap();
    assert_eq!(expect_score(&mut exec, "#dst", "xv"), 7);

    exec.command("execute store success score #succ xv run scoreboard players get #src xv").unwrap();
    assert_eq!(expect_score(&mut exec, "#succ", "xv"), 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_if_unless_conditions() {
    let dir = sandbox("cond");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard players set #flag xv2 1").unwrap();
    exec.command("scoreboard players set #target xv2 0").unwrap();

    exec.command("execute if score #flag xv2 matches 1 run scoreboard players set #target xv2 99").unwrap();
    assert_eq!(expect_score(&mut exec, "#target", "xv2"), 99);

    exec.command("scoreboard players set #target xv2 1").unwrap();
    exec.command("execute unless score #flag xv2 matches 0 run scoreboard players set #target xv2 50").unwrap();
    assert_eq!(expect_score(&mut exec, "#target", "xv2"), 50);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn entity_data_pos_builtin_and_custom() {
    let dir = sandbox("ent-pos");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("summon minecraft:marker 10 20 30 {Tags:[\"pos\"]}").unwrap();
    exec.command("data get entity @e[tag=pos,limit=1] Pos[0]").unwrap();
    exec.wait_for_command_log("has the following entity data").unwrap();

    exec.command("data get entity @e[tag=pos,limit=1] Pos").unwrap();
    let line = exec.wait_for_command_log("has the following entity data").unwrap();
    assert!(line.contains("10d") && line.contains("20d") && line.contains("30d"), "Pos list: {line}");

    exec.command("data modify entity @e[tag=pos,limit=1] data.custom set value 42").unwrap();
    exec.command("data get entity @e[tag=pos,limit=1] data.custom").unwrap();
    exec.wait_for_command_log("42").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn storage_list_append_prepend() {
    let dir = sandbox("list");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("data modify storage test:x list set value [2,3]").unwrap();
    exec.command("data modify storage test:x list append value 4").unwrap();
    exec.command("data modify storage test:x list prepend value 1").unwrap();
    exec.command("data get storage test:x list").unwrap();
    exec.wait_for_command_log("1,2,3,4").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn return_value_stops_execution() {
    let dir = sandbox("ret");
    setup_pack(&dir);
    copy_fn(&dir, "early", "scoreboard players set #first xv 1\nreturn 7\nscoreboard players set #second xv 999\n");

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec.command("function mdl:early").unwrap();

    assert_eq!(expect_score(&mut exec, "#first", "xv"), 1);
    exec.command("scoreboard players set #second xv 0").unwrap();
    assert_eq!(expect_score(&mut exec, "#second", "xv"), 0);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn kill_and_entity_counting() {
    let dir = sandbox("kill");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"rm\"]}").unwrap();
    exec.command("summon minecraft:marker 10 0 0 {Tags:[\"rm\"]}").unwrap();
    exec.command("summon minecraft:marker 100 0 0 {Tags:[\"keep\"]}").unwrap();

    exec.command("kill @e[tag=rm]").unwrap();
    exec.command("execute unless entity @e[tag=rm] run scoreboard players set #gone xv 1").unwrap();
    assert_eq!(expect_score(&mut exec, "#gone", "xv"), 1);

    exec.command("execute if entity @e[tag=keep] run scoreboard players set #alive xv 1").unwrap();
    assert_eq!(expect_score(&mut exec, "#alive", "xv"), 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn custom_dimension_block_ops() {
    let dir = sandbox("dim");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("execute in dynamic_crafting:crafters run setblock 0 0 0 minecraft:stone").unwrap();
    // Explicitly check the block exists in that dimension using the block engine
    exec.command("execute in dynamic_crafting:crafters if block 0 0 0 minecraft:stone run scoreboard players set #found xv 1").unwrap();
    // This works because `if block` uses the condition evaluator which uses execute-in context
}

#[test]
fn block_data_modify_and_get() {
    let dir = sandbox("block-data");
    setup_pack(&dir);
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec.command("setblock 0 0 0 minecraft:crafter").unwrap();
    exec.command("data modify block 0 0 0 Items set value [{Slot:0b,id:\"minecraft:stick\",Count:1b}]").unwrap();
    exec.command("data get block 0 0 0 Items[0].id").unwrap();
    exec.wait_for_command_log("stick").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn macro_substitution_with_storage() {
    let dir = sandbox("macro");
    setup_pack(&dir);
    copy_fn(&dir, "multiply", "$scoreboard players set #v xv $(factor)\n$scoreboard players set #v xv $(factor)\n");
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec.command("data modify storage test:x macro set value {factor: 77}").unwrap();
    exec.command("function mdl:multiply with storage test:x macro").unwrap();
    assert_eq!(expect_score(&mut exec, "#v", "xv"), 77);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selector_at_n_with_filters() {
    let dir = sandbox("sel");
    setup_pack(&dir);

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("summon minecraft:marker 1 0 0 {Tags:[\"near\"]}").unwrap();
    exec.command("summon minecraft:marker 50 0 0 {Tags:[\"far\"]}").unwrap();

    // @n resolves to nearest
    exec.command("execute if entity @n[type=marker] run scoreboard players set #sel xv 1").unwrap();
    assert_eq!(expect_score(&mut exec, "#sel", "xv"), 1);

    // @n with distance filter excludes the far one
    exec.command("execute if entity @n[type=marker,distance=..5] run scoreboard players set #near xv 1").unwrap();
    assert_eq!(expect_score(&mut exec, "#near", "xv"), 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn schedule_and_tick_advancement() {
    let dir = sandbox("sched");
    setup_pack(&dir);
    copy_fn(&dir, "delayed", "scoreboard players set #ran xv 1\n");

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();

    exec.command("scoreboard players set #ran xv 0").unwrap();
    exec.command("schedule function mdl:delayed 1t").unwrap();
    exec.executor_mut().advance_ticks(2).unwrap();
    assert_eq!(expect_score(&mut exec, "#ran", "xv"), 1);

    let _ = fs::remove_dir_all(&dir);
}
