use std::fs;
use mcfunction_executor::{McExecutor, V26_2};

fn bf_executor(dir: &std::path::PathBuf) -> McExecutor {
    let dp = dir.join("world").join("datapacks").join("bfi");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"bf","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    fs::create_dir_all(dp.join("data/bfi/function")).unwrap();

    fs::write(dp.join("data/bfi/function/init.mcfunction"), concat!(
        "scoreboard objectives add var dummy\n",
        "scoreboard objectives add const dummy\n",
        "scoreboard players set #max_val const 255\n",
        "data modify storage bfi:internal root.Memory.Current set value 0\n",
        "data modify storage bfi:internal root.Memory.Left set value []\n",
        "data modify storage bfi:internal root.Memory.Right set value [0,0,0,0,0]\n",
    )).unwrap();

    fs::write(dp.join("data/bfi/function/op_add.mcfunction"), concat!(
        "execute store result score $val var run data get storage bfi:internal root.Memory.Current\n",
        "scoreboard players add $val var 1\n",
        "execute if score $val var <= #max_val const store result storage bfi:internal root.Memory.Current int 1 run scoreboard players get $val var\n",
        "execute if score $val var > #max_val const run data modify storage bfi:internal root.Memory.Current set value 0\n",
    )).unwrap();

    fs::write(dp.join("data/bfi/function/op_sub.mcfunction"), concat!(
        "execute store result score $val var run data get storage bfi:internal root.Memory.Current\n",
        "scoreboard players remove $val var 1\n",
        "execute if score $val var >= 0 const store result storage bfi:internal root.Memory.Current int 1 run scoreboard players get $val var\n",
        "execute if score $val var < 0 const run data modify storage bfi:internal root.Memory.Current set value 255\n",
    )).unwrap();

    fs::write(dp.join("data/bfi/function/op_right.mcfunction"), concat!(
        "data modify storage bfi:internal root.Memory.Left append from storage bfi:internal root.Memory.Current\n",
        "data modify storage bfi:internal root.Memory.Current set from storage bfi:internal root.Memory.Right[0]\n",
        "data remove storage bfi:internal root.Memory.Right[0]\n",
    )).unwrap();

    fs::write(dp.join("data/bfi/function/op_left.mcfunction"), concat!(
        "data modify storage bfi:internal root.Memory.Right prepend from storage bfi:internal root.Memory.Current\n",
        "data modify storage bfi:internal root.Memory.Current set from storage bfi:internal root.Memory.Left[-1]\n",
        "data remove storage bfi:internal root.Memory.Left[-1]\n",
    )).unwrap();

    fs::write(dp.join("data/bfi/function/run.mcfunction"), concat!(
        "function bfi:op_add\n",
        "function bfi:op_right\n",
        "function bfi:op_sub\n",
        "scoreboard players set #done var 1\n",
    )).unwrap();

    let tag_dir = dp.join("data/minecraft/tags/function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(tag_dir.join("load.json"), r#"{"values":["bfi:init"]}"#).unwrap();

    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec
}

fn get_cell(exec: &McExecutor) -> Option<i32> {
    exec.executor().world.storage.get("bfi:internal", "root.Memory.Current").map(|v| v.as_i32())
}

fn get_left_last(exec: &McExecutor) -> Option<i32> {
    exec.executor().world.storage.get("bfi:internal", "root.Memory.Left[-1]").map(|v| v.as_i32())
}

fn get_score(exec: &McExecutor, holder: &str) -> Option<i32> {
    exec.executor().world.scoreboard.get(holder, "var")
}

#[test]
fn add_wraps_overflow() {
    let dir = std::env::temp_dir().join("mdl-bf-add");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut exec = bf_executor(&dir);
    exec.command("data modify storage bfi:internal root.Memory.Current set value 255").unwrap();
    exec.command("function bfi:op_add").unwrap();
    assert_eq!(get_cell(&exec), Some(0), "255+1 wraps to 0");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sub_wraps_underflow() {
    let dir = std::env::temp_dir().join("mdl-bf-sub");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut exec = bf_executor(&dir);
    exec.command("function bfi:op_sub").unwrap();
    assert_eq!(get_cell(&exec), Some(255), "0-1 wraps to 255");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn tape_move_right_and_left() {
    let dir = std::env::temp_dir().join("mdl-bf-lr");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut exec = bf_executor(&dir);
    exec.command("data modify storage bfi:internal root.Memory.Current set value 7").unwrap();
    exec.command("function bfi:op_right").unwrap();
    assert_eq!(get_left_last(&exec), Some(7), "Left[-1]=7 after push");
    assert_eq!(get_cell(&exec), Some(0), "Current=0 popped from Right[0]");
    exec.command("data modify storage bfi:internal root.Memory.Current set value 99").unwrap();
    exec.command("function bfi:op_left").unwrap();
    assert_eq!(get_cell(&exec), Some(7), "back to 7 after move left");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dispatch_sequence() {
    let dir = std::env::temp_dir().join("mdl-bf-run");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let mut exec = bf_executor(&dir);
    exec.command("function bfi:run").unwrap();
    // Cell 0: add→1, right (pushed to Left)
    // Cell 1: sub→255 (underflow)
    assert_eq!(get_left_last(&exec), Some(1), "Cell 0");
    assert_eq!(get_cell(&exec), Some(255), "Cell 1 underflow");
    assert_eq!(get_score(&exec, "#done"), Some(1), "completed");
    let _ = fs::remove_dir_all(&dir);
}
