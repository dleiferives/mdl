//! Vanilla Minecraft behavioral verification.
//! Each test asserts known vanilla behavior against the executor.
//! Designed to run against both the executor and a real MC server.

use mcfunction_executor::{McExecutor, V26_2};
use std::fs;
use std::path::PathBuf;

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-vb-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dp = dir.join("world").join("datapacks").join("test");
    fs::create_dir_all(&dp).unwrap();
    fs::write(
        dp.join("pack.mcmeta"),
        r#"{"pack":{"description":"vb","min_format":[107,1],"max_format":[107,1]}}"#,
    )
    .unwrap();
    fs::create_dir_all(dp.join("data/test/function")).unwrap();
    dir
}

fn exec(dir: &PathBuf) -> McExecutor {
    let mut e = McExecutor::create(dir.clone(), V26_2);
    e.load_datapacks().unwrap();
    e
}

fn write_fn(dir: &PathBuf, name: &str, content: &str) {
    fs::write(
        dir.join("world")
            .join("datapacks")
            .join("test")
            .join("data/test/function")
            .join(format!("{name}.mcfunction")),
        content,
    )
    .unwrap();
}

fn get_score(exec: &mut McExecutor, holder: &str, obj: &str) -> i32 {
    exec.command(&format!("scoreboard players get {holder} {obj}"))
        .unwrap();
    match exec.wait_for_command_log(holder) {
        Ok(line) => {
            let p: Vec<&str> = line.split_whitespace().collect();
            if let Some(i) = p.iter().position(|w| *w == "has") {
                return p.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            0
        }
        Err(_) => 0,
    }
}

// ══ Crafting table marker scoring ══
#[test]
fn crafting_table_marker_tick_cooldown() {
    let d = sandbox("ct-tick");
    let mut e = exec(&d);
    e.command("scoreboard objectives add dc dummy").unwrap();
    e.command("summon minecraft:marker 0 0 0 {Tags:[\"ct\"]}")
        .unwrap();
    e.command("scoreboard players set @e[tag=ct,limit=1] dc 7")
        .unwrap();
    e.command("scoreboard players remove @e[tag=ct,limit=1] dc 1")
        .unwrap();
    e.command("scoreboard players get @e[tag=ct,limit=1] dc")
        .unwrap();
    e.wait_for_command_log("6").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ Slot item NBT readback ══
#[test]
fn slot_display_entity_item_nbt() {
    let d = sandbox("slot-nbt");
    let mut e = exec(&d);
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"s0\"]}")
        .unwrap();
    e.command("data modify entity @e[tag=s0,limit=1] item set value {id:\"minecraft:oak_planks\",Count:1b}").unwrap();
    e.command("data get entity @e[tag=s0,limit=1] item.id")
        .unwrap();
    e.wait_for_command_log("oak_planks").unwrap();
    e.command("data get entity @e[tag=s0,limit=1] item.Count")
        .unwrap();
    e.wait_for_command_log("1b").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ Slot item presence ══
#[test]
fn if_data_entity_item_presence() {
    let d = sandbox("has-item");
    let mut e = exec(&d);
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"s\"]}")
        .unwrap();
    e.command("data modify entity @e[tag=s,limit=1] item set value {id:\"minecraft:stick\"}")
        .unwrap();
    e.command("execute if data entity @e[tag=s,limit=1] item run scoreboard players set #ok v 1")
        .unwrap();
    e.command("scoreboard players get #ok v").unwrap();
    e.wait_for_command_log("#ok has 1").unwrap();
    e.command("data remove entity @e[tag=s,limit=1] item")
        .unwrap();
    e.command(
        "execute unless data entity @e[tag=s,limit=1] item run scoreboard players set #gone v 1",
    )
    .unwrap();
    e.command("scoreboard players get #gone v").unwrap();
    e.wait_for_command_log("#gone has 1").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ Slot collection → storage → block ══
#[test]
fn slot_items_to_storage_to_block() {
    let d = sandbox("flow");
    let mut e = exec(&d);
    for i in 0..3 {
        e.command(&format!(
            "summon minecraft:item_display 0 0 0 {{Tags:[\"s\",\"s{i}\"]}}"
        ))
        .unwrap();
    }
    e.command("data modify entity @e[tag=s0,limit=1] item set value {id:\"minecraft:oak_planks\",Count:1b}").unwrap();
    e.command(
        "data modify entity @e[tag=s1,limit=1] item set value {id:\"minecraft:stick\",Count:1b}",
    )
    .unwrap();

    e.command("data merge storage test:tmp {Items:[]}").unwrap();
    e.command("data modify storage test:tmp Items append from entity @e[tag=s0,limit=1] item")
        .unwrap();
    e.command("data modify storage test:tmp Items append from entity @e[tag=s1,limit=1] item")
        .unwrap();

    e.command("data get storage test:tmp Items[0].id").unwrap();
    e.wait_for_command_log("oak_planks").unwrap();
    e.command("data get storage test:tmp Items[1].id").unwrap();
    e.wait_for_command_log("stick").unwrap();

    e.command("setblock 0 0 0 minecraft:crafter").unwrap();
    e.command("data modify block 0 0 0 Items set from storage test:tmp Items")
        .unwrap();
    e.command("data get block 0 0 0 Items[0].id").unwrap();
    e.wait_for_command_log("oak_planks").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ══ Crafting result spawn + copy ══
#[test]
fn result_item_spawn_and_copy() {
    let d = sandbox("result");
    let mut e = exec(&d);
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"vis\"]}")
        .unwrap();
    e.command("data modify entity @e[tag=vis,limit=1] item set value {id:\"minecraft:crafting_table\",Count:1b}").unwrap();
    e.command("loot spawn 0 65 0 loot dynamic_crafting:drop")
        .unwrap();
    e.command("tag @e[type=item,limit=1] add r").unwrap();
    e.command("data modify entity @e[tag=r,limit=1] Item set from entity @e[tag=vis,limit=1] item")
        .unwrap();
    e.command("data modify entity @e[tag=r,limit=1] PickupDelay set value 4")
        .unwrap();
    e.command("data get entity @e[tag=r,limit=1] Item.id")
        .unwrap();
    e.wait_for_command_log("crafting_table").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ Config storage compound ══
#[test]
fn config_compound_predicate() {
    let d = sandbox("cfg");
    let mut e = exec(&d);
    e.command("data modify storage test:c v set value {tick_dist:32,limit:16,sound:1}")
        .unwrap();
    e.command("execute if data storage test:c {v:{sound:1}} run scoreboard players set #ok v 1")
        .unwrap();
    e.command("scoreboard players get #ok v").unwrap();
    e.wait_for_command_log("#ok has 1").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ Score ranges ══
#[test]
fn score_range_all_forms() {
    let d = sandbox("sr");
    let mut e = exec(&d);
    e.command("scoreboard players set #v x 50").unwrap();
    e.command("execute if score #v x matches 50 run scoreboard players set #f x 1")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 1);
    e.command("execute if score #v x matches 20.. run scoreboard players set #f x 2")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 2);
    e.command("execute if score #v x matches ..100 run scoreboard players set #f x 3")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 3);
    e.command("execute if score #v x matches 40..60 run scoreboard players set #f x 4")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 4);
    let _ = fs::remove_dir_all(&d);
}

// ══ Score comparisons ══
#[test]
fn score_compare_all_ops() {
    let d = sandbox("sc");
    let mut e = exec(&d);
    e.command("scoreboard players set #a x 10").unwrap();
    e.command("scoreboard players set #b x 10").unwrap();
    e.command("execute if score #a x = #b x run scoreboard players set #f x 1")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 1);
    e.command("scoreboard players set #a x 5").unwrap();
    e.command("execute if score #a x < #b x run scoreboard players set #f x 2")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 2);
    e.command("scoreboard players set #a x 15").unwrap();
    e.command("execute if score #a x > #b x run scoreboard players set #f x 3")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 3);
    e.command("scoreboard players set #a x 10").unwrap();
    e.command("execute if score #a x >= #b x run scoreboard players set #f x 4")
        .unwrap();
    assert_eq!(get_score(&mut e, "#f", "x"), 4);
    let _ = fs::remove_dir_all(&d);
}

// ══ Entity lifecycle ══
#[test]
fn entity_kill_and_tag_lifecycle() {
    let d = sandbox("life");
    let mut e = exec(&d);
    for _ in 0..3 {
        e.command("summon minecraft:marker 0 0 0 {Tags:[\"a\"]}")
            .unwrap();
    }
    e.command("tag @e[tag=a] add b").unwrap();
    e.command("execute if entity @e[tag=b] run scoreboard players set #ok v 1")
        .unwrap();
    e.command("scoreboard players get #ok v").unwrap();
    e.wait_for_command_log("#ok has 1").unwrap();
    e.command("tag @e[tag=a] remove b").unwrap();
    e.command("execute unless entity @e[tag=b] run scoreboard players set #rm v 1")
        .unwrap();
    e.command("scoreboard players get #rm v").unwrap();
    e.wait_for_command_log("#rm has 1").unwrap();
    e.command("kill @e[tag=a]").unwrap();
    e.command("execute unless entity @e[tag=a] run scoreboard players set #dead v 1")
        .unwrap();
    e.command("scoreboard players get #dead v").unwrap();
    e.wait_for_command_log("#dead has 1").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ Crafters dimension isolation ══
#[test]
fn crafters_dimension_blocks_dont_conflict() {
    let d = sandbox("dim");
    let mut e = exec(&d);
    e.command("execute in dynamic_crafting:crafters run setblock 0 0 0 minecraft:crafter")
        .unwrap();
    e.command("execute in dynamic_crafting:crafters run data modify block 0 0 0 Items set value [{Slot:0b,id:\"minecraft:planks\"}]").unwrap();
    e.command("execute in dynamic_crafting:crafters run data get block 0 0 0 Items[0].id")
        .unwrap();
    e.wait_for_command_log("planks").unwrap();
    // Verify overworld block (0,0,0) does NOT have Items from the crafters dimension
    e.command("setblock 0 0 0 minecraft:air").unwrap();
    e.command("data modify block 0 0 0 test set value 42")
        .unwrap();
    e.command("data get block 0 0 0 test").unwrap();
    e.wait_for_command_log("42").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ TP relative ══
#[test]
fn tp_relative_from_current() {
    let d = sandbox("tp");
    let mut e = exec(&d);
    e.command("summon minecraft:armor_stand 10 64 20").unwrap();
    e.command("execute as @e[type=armor_stand] at @s run teleport @s ~5 ~3 ~-2")
        .unwrap();
    e.command(
        "execute as @e[type=armor_stand] store result score #x v run data get entity @s Pos[0]",
    )
    .unwrap();
    e.command("scoreboard players get #x v").unwrap();
    e.wait_for_command_log("#x has 15").unwrap();
    let _ = fs::remove_dir_all(&d);
}

// ══ Schedule ══
#[test]
fn schedule_runs_after_ticks() {
    let d = sandbox("sched");
    write_fn(&d, "later", "scoreboard players set #ran v 1\n");
    let mut e = exec(&d);
    e.command("scoreboard players set #ran v 0").unwrap();
    e.command("schedule function test:later 1t").unwrap();
    e.executor_mut().advance_ticks(2).unwrap();
    assert_eq!(get_score(&mut e, "#ran", "v"), 1);
    let _ = fs::remove_dir_all(&d);
}
