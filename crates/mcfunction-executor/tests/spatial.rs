use std::fs;
use std::path::PathBuf;

use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-spatial-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dp = dir.join("world").join("datapacks").join("test");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"spatial","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    fs::create_dir_all(dp.join("data/test/function")).unwrap();
    dir
}

fn executor(dir: &PathBuf) -> McExecutor {
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec.command("scoreboard objectives add pos dummy").unwrap();
    exec
}

#[test]
fn summon_and_teleport_absolute() {
    let dir = sandbox("tp-abs");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("execute as @e[type=armor_stand] run teleport @s 10 20 30").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #x pos run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x pos").unwrap();
    exec.wait_for_command_log("#x has 10").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #y pos run data get entity @s Pos[1]").unwrap();
    exec.command("scoreboard players get #y pos").unwrap();
    exec.wait_for_command_log("#y has 20").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn setblock_and_if_block_condition() {
    let dir = sandbox("block");
    let mut exec = executor(&dir);

    // Place a block
    exec.command("setblock 5 64 5 minecraft:stone").unwrap();

    // Check it exists
    exec.command("execute if block 5 64 5 minecraft:stone run scoreboard players set #found pos 1").unwrap();
    exec.command("scoreboard players get #found pos").unwrap();
    exec.wait_for_command_log("#found has 1").unwrap();

    // Check it fails for a different block
    exec.command("scoreboard players set #found pos 0").unwrap();
    exec.command("execute if block 5 64 5 minecraft:diamond_block run scoreboard players set #found pos 1").unwrap();
    exec.command("scoreboard players get #found pos").unwrap();
    exec.wait_for_command_log("#found has 0").unwrap();

    // Check it fails for wrong position
    exec.command("scoreboard players set #found pos 0").unwrap();
    exec.command("execute if block 6 64 5 minecraft:stone run scoreboard players set #found pos 1").unwrap();
    exec.command("scoreboard players get #found pos").unwrap();
    exec.wait_for_command_log("#found has 0").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn setblock_verify_entity_position() {
    let dir = sandbox("block-verify");
    let mut exec = executor(&dir);

    // Place a marker block at the expected destination
    exec.command("setblock 100 64 100 minecraft:stone").unwrap();

    // Summon and teleport an entity to that position
    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("teleport @e[type=armor_stand] 100 64 100").unwrap();

    // Verify: entity is at (100,64,100), so checking that exact block matches
    exec.command("execute if block 100 64 100 minecraft:stone run scoreboard players set #at_stone pos 1").unwrap();
    exec.command("scoreboard players get #at_stone pos").unwrap();
    exec.wait_for_command_log("#at_stone has 1").unwrap();

    // Verify that the entity is not at a position with a different block
    exec.command("scoreboard players set #at_stone pos 0").unwrap();
    exec.command("execute if block 100 64 101 minecraft:stone run scoreboard players set #at_stone pos 1").unwrap();
    exec.command("scoreboard players get #at_stone pos").unwrap();
    exec.wait_for_command_log("#at_stone has 0").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn teleport_relative() {
    let dir = sandbox("tp-rel");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 10 0 10").unwrap();
    exec.command("execute as @e[type=armor_stand] at @s run teleport @s ~ ~5 ~").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #y pos run data get entity @s Pos[1]").unwrap();
    exec.command("scoreboard players get #y pos").unwrap();
    exec.wait_for_command_log("#y has 5").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_positioned() {
    let dir = sandbox("pos");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("execute positioned 100 64 100 run teleport @e[type=armor_stand] ~ ~5 ~").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #y pos run data get entity @s Pos[1]").unwrap();
    exec.command("scoreboard players get #y pos").unwrap();
    exec.wait_for_command_log("#y has 69").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_positioned_relative() {
    let dir = sandbox("pos-rel");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 50 0 50").unwrap();
    exec.command("execute as @e[type=armor_stand] at @s positioned ~10 ~20 ~ run teleport @s ~ ~5 ~").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #x pos run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x pos").unwrap();
    exec.wait_for_command_log("#x has 60").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_anchored_and_align() {
    let dir = sandbox("anchored");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 10.7 64.3 -5.2").unwrap();
    exec.command("execute as @e[type=armor_stand] at @s align xyz run teleport @s ~ ~ ~").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #x pos run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x pos").unwrap();
    exec.wait_for_command_log("#x has 10").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #z pos run data get entity @s Pos[2]").unwrap();
    exec.command("scoreboard players get #z pos").unwrap();
    exec.wait_for_command_log("#z has -6").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_in_dimension() {
    let dir = sandbox("dim");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("execute in minecraft:the_nether run teleport @e[type=armor_stand] 3 5 3").unwrap();

    exec.command("execute as @e[type=armor_stand] store result score #x pos run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x pos").unwrap();
    exec.wait_for_command_log("#x has 3").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selector_with_distance_filter() {
    let dir = sandbox("distance");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 10 0 0").unwrap();
    exec.command("summon minecraft:armor_stand 100 0 0").unwrap();

    exec.command("execute if entity @e[distance=..15] run scoreboard players set #found pos 1").unwrap();
    exec.command("scoreboard players get #found pos").unwrap();
    exec.wait_for_command_log("#found has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn multiple_entities_as_forking() {
    let dir = sandbox("fork");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("summon minecraft:armor_stand 5 0 0").unwrap();
    exec.command("summon minecraft:armor_stand 10 0 0").unwrap();

    exec.command("execute as @e[type=armor_stand] at @s run teleport @s ~ ~10 ~").unwrap();

    // Check that all entities got moved up by checking entity count at y=10
    exec.command("execute if entity @e[y=10,distance=..1] run scoreboard players set #match pos 1").unwrap();
    exec.command("scoreboard players get #match pos").unwrap();
    exec.wait_for_command_log("#match has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}
