use std::fs;
use std::path::PathBuf;

use mcfunction_executor::{McExecutor, V26_2};

fn build_pack(root: &PathBuf) {
    let dp = root.join("world").join("datapacks").join("mdl_community");
    fs::create_dir_all(&dp).unwrap();

    fs::write(
        dp.join("pack.mcmeta"),
        r#"{"pack":{"description":"community","min_format":[107,1],"max_format":[107,1]}}"#,
    ).unwrap();

    let func_dir = dp.join("data").join("mdl").join("function");
    fs::create_dir_all(&func_dir).unwrap();

    fs::write(func_dir.join("init.mcfunction"), concat!(
        "scoreboard objectives add var dummy\n",
        "scoreboard objectives add const dummy\n",
        "data modify storage mdl:state Tape set value [10,20,30]\n",
        "scoreboard players set #max_val const 255\n",
        "scoreboard players set #two const 2\n",
    )).unwrap();

    let tag_dir = dp.join("data").join("minecraft").join("tags").join("function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(tag_dir.join("load.json"), r#"{"values":["mdl:init"]}"#).unwrap();
}

fn setup(name: &str) -> (PathBuf, McExecutor) {
    let dir = std::env::temp_dir().join(format!("mdl-community-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    build_pack(&dir);
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    (dir, exec)
}

#[test]
fn nbt_list_shift() {
    let (dir, mut exec) = setup("list-shift");
    exec.command("data modify storage mdl:test xs set value [1,2,3]").unwrap();
    exec.command("data remove storage mdl:test xs[0]").unwrap();
    exec.command("data get storage mdl:test xs").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(!line.contains("\"1\""), "1 should be removed: {line}");
    assert!(line.contains("2"), "2 should remain: {line}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scoreboard_arithmetic() {
    let (dir, mut exec) = setup("arithmetic");
    exec.command("scoreboard objectives add raw dummy").unwrap();
    exec.command("scoreboard players set #a raw 10").unwrap();
    exec.command("scoreboard players add #a raw 5").unwrap();
    exec.command("scoreboard players remove #a raw 3").unwrap();
    exec.command("scoreboard players get #a raw").unwrap();
    exec.wait_for_command_log("#a has 12").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_score_comparison() {
    let (dir, mut exec) = setup("compare");
    exec.command("scoreboard objectives add cmp dummy").unwrap();
    exec.command("scoreboard players set #a cmp 5").unwrap();
    exec.command("scoreboard players set #b cmp 5").unwrap();
    exec.command("scoreboard players set #match cmp 0").unwrap();
    exec.command("execute if score #a cmp = #b cmp run scoreboard players set #match cmp 1").unwrap();
    exec.command("scoreboard players get #match cmp").unwrap();
    exec.wait_for_command_log("#match has 1").unwrap();

    exec.command("scoreboard players set #a cmp 3").unwrap();
    exec.command("scoreboard players set #b cmp 7").unwrap();
    exec.command("scoreboard players set #match cmp 0").unwrap();
    exec.command("execute if score #a cmp < #b cmp run scoreboard players set #match cmp 1").unwrap();
    exec.command("scoreboard players get #match cmp").unwrap();
    exec.wait_for_command_log("#match has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_score_match_range() {
    let (dir, mut exec) = setup("ranges");
    exec.command("scoreboard objectives add rt dummy").unwrap();
    exec.command("scoreboard players set #val rt 1").unwrap();
    exec.command("scoreboard players set #matched rt 0").unwrap();
    exec.command("execute if score #val rt matches 0..1 run scoreboard players set #matched rt 1").unwrap();
    exec.command("scoreboard players get #matched rt").unwrap();
    exec.wait_for_command_log("#matched has 1").unwrap();

    exec.command("scoreboard players set #val rt 2").unwrap();
    exec.command("scoreboard players set #matched rt 0").unwrap();
    exec.command("execute if score #val rt matches 0..1 run scoreboard players set #matched rt 1").unwrap();
    exec.command("scoreboard players get #matched rt").unwrap();
    exec.wait_for_command_log("#matched has 0").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nbt_score_roundtrip() {
    let (dir, mut exec) = setup("roundtrip");
    exec.command("scoreboard objectives add rtt dummy").unwrap();
    exec.command("scoreboard players set #src rtt 99").unwrap();
    exec.command("execute store result storage mdl:test result int 1 run scoreboard players get #src rtt").unwrap();
    exec.command("execute store result score #dst rtt run data get storage mdl:test result").unwrap();
    exec.command("scoreboard players get #dst rtt").unwrap();
    exec.wait_for_command_log("#dst has 99").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn store_storage_types() {
    let (dir, mut exec) = setup("typed");
    exec.command("scoreboard objectives add t td").unwrap();
    exec.command("scoreboard players set #src t 42").unwrap();

    for ty in &["byte", "short", "int", "long", "float", "double"] {
        exec.command(&format!("execute store result storage mdl:test stored_{ty} {ty} 1 run scoreboard players get #src t")).unwrap();
    }

    exec.command("data get storage mdl:test stored_byte").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("42b"), "byte: {line}");

    exec.command("data get storage mdl:test stored_long").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("42L"), "long: {line}");

    exec.command("data get storage mdl:test stored_double").unwrap();
    let line = exec.wait_for_command_log("42").unwrap();
    assert!(line.contains("42"), "double: {line}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tape_operations() {
    let (dir, mut exec) = setup("tape");
    // Tape already initialized by load tag: [10,20,30]

    // Read Tape[0] → score #val var
    exec.command("scoreboard players set #val var 0").unwrap();
    exec.command("execute store result score #val var run data get storage mdl:state Tape[0]").unwrap();
    exec.command("scoreboard players get #val var").unwrap();
    exec.wait_for_command_log("#val has 10").unwrap();

    // Write back: Tape[0] = 99
    exec.command("scoreboard players set #val var 99").unwrap();
    exec.command("execute store result storage mdl:state Tape[0] int 1 run scoreboard players get #val var").unwrap();
    exec.command("data get storage mdl:state Tape").unwrap();
    let line = exec.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("99"), "Tape should have 99: {line}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scheduling() {
    let (dir, mut exec) = setup("sched-install");
    // Write schedule functions AFTER load
    let dp = dir.join("world").join("datapacks").join("mdl_community");
    let func_dir = dp.join("data").join("mdl").join("function");
    fs::write(func_dir.join("sched_init.mcfunction"), "scoreboard players set #count var 0\nschedule function mdl:sched_tick 1t replace\n").unwrap();
    fs::write(func_dir.join("sched_tick.mcfunction"), concat!(
        "scoreboard players add #count var 1\n",
        "execute if score #count var matches 1..2 run schedule function mdl:sched_tick 1t replace\n",
        "execute if score #count var matches 3 run scoreboard players set #done var 1\n",
    )).unwrap();

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec.command("function mdl:sched_init").unwrap();

    // Tick 1: sched_tick runs, #count=1, reschedules
    // Tick 2: sched_tick runs, #count=2, reschedules
    // Tick 3: sched_tick runs, #count=3, sets #done=1
    exec.command("scoreboard players get #done var").unwrap();
    exec.wait_for_command_log("#done has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}
