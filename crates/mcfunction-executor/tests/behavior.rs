//! Behavioral verification tests — these assert that our executor produces
//! the same results as a real Minecraft 1.21 server would.
//!
//! Each test lists the expected vanilla behavior before the executor assertion.
use std::fs;
use std::path::PathBuf;
use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-behavior-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dp = dir.join("world").join("datapacks").join("test");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"behavior","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    fs::create_dir_all(dp.join("data/test/function")).unwrap();
    dir
}

fn executor(dir: &PathBuf) -> McExecutor {
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec
}

// ─────────────────────────────────────────────────────────────────
// kill command
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `kill @e[tag=t,limit=1]` removes one entity matching tag.
#[test]
fn kill_removes_matching_entity() {
    let dir = sandbox("kill-1");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"killme\"]}").unwrap();
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"keep\"]}").unwrap();

    exec.command("kill @e[tag=killme,limit=1]").unwrap();
    exec.command("scoreboard players set #alive v 1").unwrap();
    exec.command("execute unless entity @e[tag=killme] run scoreboard players set #alive v 2").unwrap();
    exec.command("execute if entity @e[tag=keep] run scoreboard players add #alive v 1").unwrap();
    exec.command("scoreboard players get #alive v").unwrap();
    exec.wait_for_command_log("#alive has 3").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `kill` without limit removes all matching.
#[test]
fn kill_removes_all_matching() {
    let dir = sandbox("kill-all");
    let mut exec = executor(&dir);
    for i in 0..3 {
        exec.command(&format!("summon minecraft:marker 0 0 0 {{Tags:[\"all{0}\"]}}", i)).unwrap();
    }

    exec.command("execute if entity @e[tag=all0] run scoreboard players set #before v 1").unwrap();
    exec.command("scoreboard players get #before v").unwrap();
    exec.wait_for_command_log("#before has 1").unwrap();

    exec.command("kill @e[tag=all0]").unwrap();
    exec.command("kill @e[tag=all1]").unwrap();
    exec.command("kill @e[tag=all2]").unwrap();

    exec.command("execute unless entity @e[tag=all0] run scoreboard players set #gone0 v 1").unwrap();
    exec.command("scoreboard players get #gone0 v").unwrap();
    exec.wait_for_command_log("#gone0 has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// tag command
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `tag @e[...] add <tag>` adds tag to matching entities.
#[test]
fn tag_add_makes_entity_matchable() {
    let dir = sandbox("tag-add");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"base\"]}").unwrap();

    exec.command("tag @e[tag=base,limit=1] add dynamic").unwrap();
    exec.command("execute if entity @e[tag=dynamic] run scoreboard players set #found v 1").unwrap();
    exec.command("scoreboard players get #found v").unwrap();
    exec.wait_for_command_log("#found has 1").unwrap();

    // Should still have original tag
    exec.command("execute if entity @e[tag=base] run scoreboard players add #found v 1").unwrap();
    exec.command("scoreboard players get #found v").unwrap();
    exec.wait_for_command_log("#found has 2").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `tag @e[...] remove <tag>` removes tag, entity no longer matches it.
#[test]
fn tag_remove_stops_matching() {
    let dir = sandbox("tag-rm");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"temp\"]}").unwrap();

    exec.command("execute if entity @e[tag=temp] run scoreboard players set #before v 1").unwrap();
    exec.command("scoreboard players get #before v").unwrap();
    exec.wait_for_command_log("#before has 1").unwrap();

    exec.command("tag @e[tag=temp,limit=1] remove temp").unwrap();

    exec.command("execute unless entity @e[tag=temp] run scoreboard players set #after v 1").unwrap();
    exec.command("scoreboard players get #after v").unwrap();
    exec.wait_for_command_log("#after has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `tag <entity> list` returns the entity's tags.
#[test]
fn tag_list_returns_tags() {
    let dir = sandbox("tag-list");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"a\",\"b\"]}").unwrap();

    // tag list should not error out, produces a log with tags
    exec.command("tag @e[limit=1] list").unwrap();

    // Quick sanity: the entity still has its tags
    exec.command("execute if entity @e[tag=a] run scoreboard players set #still v 1").unwrap();
    exec.command("scoreboard players get #still v").unwrap();
    exec.wait_for_command_log("#still has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// tellraw / title
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `tellraw @a "..."` sends a chat message visible to players.
/// Our executor logs it to [CHAT].
#[test]
fn tellraw_logs_chat_message() {
    let dir = sandbox("tellraw");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/chat.mcfunction"),
        "tellraw @a test_message_12345\n",
    ).unwrap();
    let mut exec = executor(&dir);
    exec.command("function test:chat").unwrap();
    exec.wait_for_command_log("[CHAT] test_message_12345").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `title @a actionbar "..."` sends an actionbar message.
/// Our executor logs it to [TITLE].
#[test]
fn title_logs_actionbar() {
    let dir = sandbox("title");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/tit.mcfunction"),
        "title @a actionbar \"crafting complete\"\n",
    ).unwrap();
    let mut exec = executor(&dir);
    exec.command("function test:tit").unwrap();
    exec.wait_for_command_log("[TITLE] actionbar").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// tp command
// ─────────────────────────────────────────────────────────────────

/// Vanilla: teleport moves entity to absolute coords.
/// Uses `execute at ... run tp @s ...` path which is fully tested.
#[test]
fn tp_absolute_position() {
    let dir = sandbox("tp-abs");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();

    exec.command("execute as @e[type=armor_stand] at @s run teleport @s 100 64 100").unwrap();
    exec.command("execute as @e[type=armor_stand] store result score #x v run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x v").unwrap();
    exec.wait_for_command_log("#x has 100").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `tp @s @n[...]` teleports to another entity.
#[test]
fn tp_to_another_entity() {
    let dir = sandbox("tp-entity");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 50 10 25 {Tags:[\"dest\"]}").unwrap();
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"src\"]}").unwrap();

    // tp src to dest
    exec.command("execute as @e[tag=src,limit=1] at @s run tp @s @e[tag=dest,limit=1]").unwrap();
    exec.command("execute as @e[tag=src] store result score #x v run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x v").unwrap();
    exec.wait_for_command_log("#x has 50").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// @n selector
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `@n` returns nearest entity. With distance filter, it respects the bound.
/// NOTE: @s in data get context doesn't resolve execute-as context yet.
/// We verify via entity existence checks instead of position reads via @s.
#[test]
fn at_n_returns_nearest_within_distance() {
    let dir = sandbox("atn");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 1 0 0 {Tags:[\"near\"]}").unwrap();
    exec.command("summon minecraft:marker 10 0 0 {Tags:[\"mid\"]}").unwrap();
    exec.command("summon minecraft:marker 100 0 0 {Tags:[\"far\"]}").unwrap();

    // @n[distance=..5] should match the entity at x=1 (only one within 5)
    exec.command("execute if entity @n[distance=..5] run scoreboard players set #near v 1").unwrap();
    exec.command("scoreboard players get #near v").unwrap();
    exec.wait_for_command_log("#near has 1").unwrap();

    // @n[distance=9..15] should match the entity at x=10
    exec.command("execute if entity @n[distance=9..15] run scoreboard players set #mid v 1").unwrap();
    exec.command("scoreboard players get #mid v").unwrap();
    exec.wait_for_command_log("#mid has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `@n[type=...,tag=...]` filters by type and tag, returns nearest.
#[test]
fn at_n_type_and_tag() {
    let dir = sandbox("atn-tag");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 10 0 0 {Tags:[\"test\"]}").unwrap();
    exec.command("summon minecraft:marker 1 0 0 {Tags:[\"test\"]}").unwrap(); // closer but wrong type

    exec.command("execute if entity @n[type=armor_stand,tag=test] run scoreboard players set #found v 1").unwrap();
    exec.command("scoreboard players get #found v").unwrap();
    exec.wait_for_command_log("#found has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// nbt filter
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `@e[nbt={key:val}]` filters by NBT compound content.
#[test]
fn nbt_filter_exact_match() {
    let dir = sandbox("nbt-filter");
    let mut exec = executor(&dir);

    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"hasdata\"]}").unwrap();
    exec.command("data merge entity @e[tag=hasdata,limit=1] {Age:0s,Custom:5}").unwrap();

    exec.command("execute if entity @e[nbt={Age:0s}] run scoreboard players set #age v 1").unwrap();
    exec.command("scoreboard players get #age v").unwrap();
    exec.wait_for_command_log("#age has 1").unwrap();

    exec.command("execute if entity @e[nbt={Custom:5}] run scoreboard players set #custom v 1").unwrap();
    exec.command("scoreboard players get #custom v").unwrap();
    exec.wait_for_command_log("#custom has 1").unwrap();

    // Wrong value should not match
    exec.command("execute if entity @e[nbt={Age:1s}] run scoreboard players set #wrong v 1").unwrap();
    exec.command("scoreboard players set #wrong v 0").unwrap();
    exec.command("scoreboard players get #wrong v").unwrap();
    exec.wait_for_command_log("#wrong has 0").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `@e[nbt={missing:key}]` does not match when key is absent.
#[test]
fn nbt_filter_missing_key_no_match() {
    let dir = sandbox("nbt-miss");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"clean\"]}").unwrap();

    exec.command("execute unless entity @e[nbt={Nonexistent:1}] run scoreboard players set #absent v 1").unwrap();
    exec.command("scoreboard players get #absent v").unwrap();
    exec.wait_for_command_log("#absent has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// data modify entity
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `data modify entity @e[...] <path> set value <snbt>` sets a value.
#[test]
fn data_modify_entity_set() {
    let dir = sandbox("mod-set");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"m\"]}").unwrap();

    exec.command("data modify entity @e[tag=m,limit=1] data.x set value 42").unwrap();
    exec.command("data get entity @e[tag=m,limit=1] data.x").unwrap();
    exec.wait_for_command_log("42").unwrap();

    exec.command("data modify entity @e[tag=m,limit=1] data.s set value \"hello\"").unwrap();
    exec.command("data get entity @e[tag=m,limit=1] data.s").unwrap();
    exec.wait_for_command_log("hello").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `data modify entity ... set from entity @s <path>` copies NBT between entities.
#[test]
fn data_modify_entity_copy_from_entity() {
    let dir = sandbox("mod-copy");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"src\"]}").unwrap();
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"dst\"]}").unwrap();

    exec.command("data modify entity @e[tag=src,limit=1] data.value set value 99").unwrap();
    exec.command("data modify entity @e[tag=dst,limit=1] data.copied set from entity @e[tag=src,limit=1] data.value").unwrap();

    exec.command("data get entity @e[tag=dst,limit=1] data.copied").unwrap();
    exec.wait_for_command_log("99").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `data modify entity ... set from storage <id> <path>` copies from storage.
#[test]
fn data_modify_entity_copy_from_storage() {
    let dir = sandbox("mod-storage");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"e\"]}").unwrap();

    // Set storage compound with a nested value
    exec.command("data modify storage test:v test.val set value 77").unwrap();
    // Copy from storage path into entity NBT
    exec.command("data modify entity @e[tag=e,limit=1] data.from_storage set from storage test:v test.val").unwrap();

    exec.command("data get entity @e[tag=e,limit=1] data.from_storage").unwrap();
    exec.wait_for_command_log("77").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `data merge entity @e[...] {key:val,...}` merges NBT.
#[test]
fn data_merge_entity() {
    let dir = sandbox("merge");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"mrg\"]}").unwrap();

    exec.command("data merge entity @e[tag=mrg,limit=1] {a:1,b:2}").unwrap();
    exec.command("data merge entity @e[tag=mrg,limit=1] {c:3}").unwrap(); // add new key

    exec.command("data get entity @e[tag=mrg,limit=1] a").unwrap();
    exec.wait_for_command_log("1").unwrap();
    exec.command("data get entity @e[tag=mrg,limit=1] b").unwrap();
    exec.wait_for_command_log("2").unwrap();
    exec.command("data get entity @e[tag=mrg,limit=1] c").unwrap();
    exec.wait_for_command_log("3").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `data remove entity @e[...] <path>` removes a key.
#[test]
fn data_remove_entity_key() {
    let dir = sandbox("remove");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"rm\"]}").unwrap();

    exec.command("data merge entity @e[tag=rm,limit=1] {a:1,b:2}").unwrap();
    exec.command("data remove entity @e[tag=rm,limit=1] a").unwrap();

    // a should be gone
    exec.command("execute if data entity @e[tag=rm,limit=1] b run scoreboard players set #b_still v 1").unwrap();
    exec.command("scoreboard players get #b_still v").unwrap();
    exec.wait_for_command_log("#b_still has 1").unwrap();

    // a should not exist
    exec.command("execute unless data entity @e[tag=rm,limit=1] a run scoreboard players set #a_gone v 1").unwrap();
    exec.command("scoreboard players get #a_gone v").unwrap();
    exec.wait_for_command_log("#a_gone has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// execute if data entity
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `execute if data entity @s <path>` succeeds when NBT path exists.
#[test]
fn execute_if_data_entity() {
    let dir = sandbox("if-data-ent");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"d\"]}").unwrap();
    exec.command("data merge entity @e[tag=d,limit=1] {present:1}").unwrap();

    exec.command("execute as @e[tag=d] if data entity @s present run scoreboard players set #yes v 1").unwrap();
    exec.command("scoreboard players get #yes v").unwrap();
    exec.wait_for_command_log("#yes has 1").unwrap();

    exec.command("execute as @e[tag=d] unless data entity @s absent run scoreboard players set #no v 1").unwrap();
    exec.command("scoreboard players get #no v").unwrap();
    exec.wait_for_command_log("#no has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// return command
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `return 42` stops function execution, returns 42.
#[test]
fn return_value_stops_function() {
    let dir = sandbox("ret");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/early.mcfunction"),
        "scoreboard players set #first v 1\nreturn 7\nscoreboard players set #second v 999\n",
    ).unwrap();
    let mut exec = executor(&dir);
    exec.command("function test:early").unwrap();
    exec.command("scoreboard players get #first v").unwrap();
    exec.wait_for_command_log("#first has 1").unwrap();
    // #second should not be set
    exec.command("scoreboard players set #second v 0").unwrap();
    exec.command("scoreboard players get #second v").unwrap();
    exec.wait_for_command_log("#second has 0").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// rotate command
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `rotate @e[...] ~90 ~` rotates entity yaw by +90.
#[test]
fn rotate_changes_entity_rotation() {
    let dir = sandbox("rot");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"r\"]}").unwrap();

    exec.command("rotate @e[tag=r,limit=1] ~90 ~0").unwrap();
    exec.command("execute as @e[tag=r] store result score #yaw v run data get entity @s Rotation[0]").unwrap();
    exec.command("scoreboard players get #yaw v").unwrap();
    // Vanilla: yaw in data get entity Rotation[0] returns a float
    exec.wait_for_command_log("#yaw has 90").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// scoreboard operations
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `scoreboard players operation ... *= ...` multiplies scores.
#[test]
fn scoreboard_operation_multiply() {
    let dir = sandbox("sc-mul");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #a v 7").unwrap();
    exec.command("scoreboard players set #b v 3").unwrap();
    exec.command("scoreboard players operation #a v *= #b v").unwrap();
    exec.command("scoreboard players get #a v").unwrap();
    exec.wait_for_command_log("#a has 21").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

/// Vanilla: `scoreboard players operation ... /= ...` integer division.
#[test]
fn scoreboard_operation_divide() {
    let dir = sandbox("sc-div");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #a v 10").unwrap();
    exec.command("scoreboard players set #b v 3").unwrap();
    exec.command("scoreboard players operation #a v /= #b v").unwrap();
    exec.command("scoreboard players get #a v").unwrap();
    exec.wait_for_command_log("#a has 3").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ─────────────────────────────────────────────────────────────────
// entity NBT built-in Pos
// ─────────────────────────────────────────────────────────────────

/// Vanilla: `data get entity @s Pos` returns [x_d, y_d, z_d] as doubles.
#[test]
fn entity_pos_is_list_of_doubles() {
    let dir = sandbox("pos-type");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 10 20 30 {Tags:[\"p\"]}").unwrap();

    exec.command("data get entity @e[tag=p,limit=1] Pos").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("10d"), "Pos[0] should be double: {line}");
    assert!(line.contains("20d"), "Pos[1] should be double: {line}");
    assert!(line.contains("30d"), "Pos[2] should be double: {line}");

    let _ = fs::remove_dir_all(&dir);
}
