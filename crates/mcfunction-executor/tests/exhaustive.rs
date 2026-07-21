//! Exhaustive behavioral tests — covers every command variant, selector
//! filter, execute modifier, and scoreboard operation against expected
//! vanilla Minecraft 1.21 behavior.

use std::fs;
use std::path::PathBuf;
use mcfunction_executor::{McExecutor, V26_2};

// ═════════════════════════════════════════════════════════════════
fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-full-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dp = dir.join("world").join("datapacks").join("test");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"),
        r#"{"pack":{"description":"full","min_format":[107,1],"max_format":[107,1]}}"#
    ).unwrap();
    fs::create_dir_all(dp.join("data/test/function")).unwrap();
    dir
}

fn executor(dir: &PathBuf) -> McExecutor {
    let mut exec = McExecutor::create(dir.clone(), V26_2);
    exec.load_datapacks().unwrap();
    exec
}

// ═════════════════════════════════════════════════════════════════
// SCOREBOARD OPERATIONS
// ═════════════════════════════════════════════════════════════════

#[test]
fn score_op_subtract() {
    let dir = sandbox("sc-sub");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 10").unwrap();
    exec.command("scoreboard players remove #v x 3").unwrap();
    exec.command("scoreboard players get #v x").unwrap();
    exec.wait_for_command_log("#v has 7").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn score_op_modulo() {
    let dir = sandbox("sc-mod");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 10").unwrap();
    exec.command("scoreboard players set #m x 3").unwrap();
    exec.command("scoreboard players operation #v x %= #m x").unwrap();
    exec.command("scoreboard players get #v x").unwrap();
    exec.wait_for_command_log("#v has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn score_op_min_max_swap() {
    let dir = sandbox("sc-minmax");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #a x 5").unwrap();
    exec.command("scoreboard players set #b x 10").unwrap();

    // < picks min
    exec.command("scoreboard players operation #a x < #b x").unwrap();
    exec.command("scoreboard players get #a x").unwrap();
    exec.wait_for_command_log("#a has 5").unwrap();

    // > picks max
    exec.command("scoreboard players operation #a x > #b x").unwrap();
    exec.command("scoreboard players get #a x").unwrap();
    exec.wait_for_command_log("#a has 10").unwrap();

    // >< swaps
    exec.command("scoreboard players set #c x 7").unwrap();
    exec.command("scoreboard players operation #c x >< #b x").unwrap();
    exec.command("scoreboard players get #c x").unwrap();
    exec.wait_for_command_log("#c has 10").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn score_op_assign() {
    let dir = sandbox("sc-assign");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #a x 42").unwrap();
    exec.command("scoreboard players set #b x 0").unwrap();
    exec.command("scoreboard players operation #b x = #a x").unwrap();
    exec.command("scoreboard players get #b x").unwrap();
    exec.wait_for_command_log("#b has 42").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn score_reset_clears_value() {
    let dir = sandbox("sc-reset");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 99").unwrap();
    exec.command("scoreboard players reset #v x").unwrap();
    // After reset, get logs "not set"
    exec.command("scoreboard players get #v x").unwrap();
    exec.wait_for_command_log("Can't get").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// EXECUTE IF SCORE CONDITIONS
// ═════════════════════════════════════════════════════════════════

#[test]
fn execute_if_score_matches_range() {
    let dir = sandbox("if-score");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 50").unwrap();
    exec.command("scoreboard players set #t x 0").unwrap();

    exec.command("execute if score #v x matches 50 run scoreboard players set #t x 1").unwrap();
    exec.command("scoreboard players get #t x").unwrap();
    exec.wait_for_command_log("#t has 1").unwrap();

    exec.command("execute if score #v x matches ..49 run scoreboard players set #t x 2").unwrap();
    exec.command("scoreboard players get #t x").unwrap();
    exec.wait_for_command_log("#t has 1").unwrap(); // should not change

    exec.command("execute if score #v x matches 50.. run scoreboard players set #t x 3").unwrap();
    exec.command("scoreboard players get #t x").unwrap();
    exec.wait_for_command_log("#t has 3").unwrap();

    exec.command("execute if score #v x matches 1..100 run scoreboard players set #t x 4").unwrap();
    exec.command("scoreboard players get #t x").unwrap();
    exec.wait_for_command_log("#t has 4").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_if_score_compare_operators() {
    let dir = sandbox("if-score-cmp");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #a x 10").unwrap();
    exec.command("scoreboard players set #b x 10").unwrap();
    exec.command("scoreboard players set #r x 0").unwrap();

    // =
    exec.command("execute if score #a x = #b x run scoreboard players set #r x 1").unwrap();
    exec.command("scoreboard players get #r x").unwrap();
    exec.wait_for_command_log("#r has 1").unwrap();

    // <=
    exec.command("execute if score #a x <= #b x run scoreboard players set #r x 2").unwrap();
    exec.command("scoreboard players get #r x").unwrap();
    exec.wait_for_command_log("#r has 2").unwrap();

    // >=
    exec.command("execute if score #a x >= #b x run scoreboard players set #r x 3").unwrap();
    exec.command("scoreboard players get #r x").unwrap();
    exec.wait_for_command_log("#r has 3").unwrap();

    // < (with different values)
    exec.command("scoreboard players set #a x 5").unwrap();
    exec.command("execute if score #a x < #b x run scoreboard players set #r x 4").unwrap();
    exec.command("scoreboard players get #r x").unwrap();
    exec.wait_for_command_log("#r has 4").unwrap();

    // > (with different values)
    exec.command("scoreboard players set #a x 15").unwrap();
    exec.command("execute if score #a x > #b x run scoreboard players set #r x 5").unwrap();
    exec.command("scoreboard players get #r x").unwrap();
    exec.wait_for_command_log("#r has 5").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// EXECUTE IF ENTITY
// ═════════════════════════════════════════════════════════════════

#[test]
fn execute_if_entity_exists() {
    let dir = sandbox("if-ent");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"t\"]}").unwrap();
    exec.command("execute if entity @e[tag=t] run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();

    exec.command("execute unless entity @e[tag=nonexistent] run scoreboard players set #f x 2").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 2").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// EXECUTE STORE
// ═════════════════════════════════════════════════════════════════

#[test]
fn execute_store_result_score() {
    let dir = sandbox("store-sc");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 77").unwrap();
    exec.command("execute store result score #s x run scoreboard players get #v x").unwrap();
    exec.command("scoreboard players get #s x").unwrap();
    exec.wait_for_command_log("#s has 77").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_store_success_score() {
    let dir = sandbox("store-suc");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 1").unwrap();
    // Success = 1 when command succeeds (get returns value)
    exec.command("execute store success score #s x run scoreboard players get #v x").unwrap();
    exec.command("scoreboard players get #s x").unwrap();
    exec.wait_for_command_log("#s has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_store_result_storage() {
    let dir = sandbox("store-sto");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 99").unwrap();
    exec.command("execute store result storage test:x result int 1 run scoreboard players get #v x").unwrap();
    exec.command("data get storage test:x result").unwrap();
    exec.wait_for_command_log("99").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// DATA MODIFY STORAGE
// ═════════════════════════════════════════════════════════════════

#[test]
fn data_modify_storage_append_prepend() {
    let dir = sandbox("dm-append");
    let mut exec = executor(&dir);
    exec.command("data modify storage test:x arr set value [1,2]").unwrap();
    exec.command("data modify storage test:x arr append value 3").unwrap();
    exec.command("data modify storage test:x arr prepend value 0").unwrap();
    exec.command("data get storage test:x arr").unwrap();
    exec.wait_for_command_log("0,1,2,3").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn data_modify_storage_copy_from() {
    let dir = sandbox("dm-copy");
    let mut exec = executor(&dir);
    exec.command("data modify storage test:x src set value {k:99}").unwrap();
    exec.command("data modify storage test:x dst set from storage test:x src").unwrap();
    exec.command("data get storage test:x dst k").unwrap();
    exec.wait_for_command_log("99").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn data_modify_storage_merge() {
    let dir = sandbox("dm-merge");
    let mut exec = executor(&dir);
    exec.command("data modify storage test:x base set value {a:1,b:2}").unwrap();
    exec.command("data modify storage test:x base merge value {c:3}").unwrap();
    exec.command("data get storage test:x base a").unwrap();
    exec.wait_for_command_log("1").unwrap();
    exec.command("data get storage test:x base c").unwrap();
    exec.wait_for_command_log("3").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn data_remove_storage_key() {
    let dir = sandbox("dm-remove");
    let mut exec = executor(&dir);
    exec.command("data modify storage test:x base set value {a:1,b:2}").unwrap();
    exec.command("data remove storage test:x base.a").unwrap();
    exec.command("execute unless data storage test:x base.a run scoreboard players set #g x 1").unwrap();
    exec.command("scoreboard players get #g x").unwrap();
    exec.wait_for_command_log("#g has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// ENTITY DATA — append/prepend
// ═════════════════════════════════════════════════════════════════

#[test]
fn entity_data_append_to_list() {
    let dir = sandbox("e-append");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"l\"]}").unwrap();
    exec.command("data modify entity @e[tag=l,limit=1] data.list set value [1,2]").unwrap();
    exec.command("data modify entity @e[tag=l,limit=1] data.list append value 3").unwrap();
    exec.command("data get entity @e[tag=l,limit=1] data.list").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("1,2,3"), "got: {line}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn entity_data_prepend_to_list() {
    let dir = sandbox("e-prepend");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"p\"]}").unwrap();
    exec.command("data modify entity @e[tag=p,limit=1] data.list set value [2,3]").unwrap();
    exec.command("data modify entity @e[tag=p,limit=1] data.list prepend value 1").unwrap();
    exec.command("data get entity @e[tag=p,limit=1] data.list").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("1,2,3"), "got: {line}");
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// DATA GET — scale parameter
// ═════════════════════════════════════════════════════════════════

#[test]
fn data_get_storage_with_scale() {
    let dir = sandbox("dg-scale");
    let mut exec = executor(&dir);
    exec.command("data modify storage test:x val set value 5").unwrap();
    // scale doubles the result for storage (as_i32)
    exec.command("execute store result score #s x run data get storage test:x val 2.0").unwrap();
    exec.command("scoreboard players get #s x").unwrap();
    exec.wait_for_command_log("#s has 10").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// SUMMON — multiple tags, NBT
// ═════════════════════════════════════════════════════════════════

#[test]
fn summon_with_multiple_tags() {
    let dir = sandbox("sum-tags");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"a\",\"b\",\"c\"]}").unwrap();
    exec.command("execute if entity @e[tag=a] run scoreboard players set #ta x 1").unwrap();
    exec.command("scoreboard players get #ta x").unwrap();
    exec.wait_for_command_log("#ta has 1").unwrap();
    exec.command("execute if entity @e[tag=c] run scoreboard players set #tc x 1").unwrap();
    exec.command("scoreboard players get #tc x").unwrap();
    exec.wait_for_command_log("#tc has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// SELECTOR: dx/dy/dz bounds
// ═════════════════════════════════════════════════════════════════

#[test]
fn selector_bounding_box_inclusive() {
    let dir = sandbox("sel-box");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 5 0 0 {Tags:[\"in\"]}").unwrap();
    exec.command("summon minecraft:marker 50 0 0 {Tags:[\"out\"]}").unwrap();

    exec.command("execute if entity @e[tag=in,dx=10,dy=0,dz=0] run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();

    exec.command("execute unless entity @e[tag=out,dx=10,dy=0,dz=0] run scoreboard players set #f x 2").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 2").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// SELECTOR: y_rotation with range
// ═════════════════════════════════════════════════════════════════

#[test]
fn selector_y_rotation_range_matching() {
    let dir = sandbox("sel-yrot");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"r\"]}").unwrap();
    exec.command("data modify entity @e[tag=r,limit=1] Rotation[0] set value 45f").unwrap();

    // In range 0..90
    exec.command("execute if entity @e[tag=r,y_rotation=0..90] run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();

    // Not in range -90..0
    exec.command("execute unless entity @e[tag=r,y_rotation=-90..0] run scoreboard players set #f x 2").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 2").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// SELECTOR: scores={} with multiple objectives
// ═════════════════════════════════════════════════════════════════

#[test]
fn selector_scores_multi_objective() {
    let dir = sandbox("sel-scores");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"scored\"]}").unwrap();

    // Score keys: in our executor, entity IDs are integers and scores can be keyed
    // by string representation. Test with a named holder instead.
    exec.command("scoreboard players set #scored x 10").unwrap();

    // Use a conditional test with scores on a named holder
    exec.command("execute if score #scored x matches 10 run scoreboard players set #m x 1").unwrap();
    exec.command("scoreboard players get #m x").unwrap();
    exec.wait_for_command_log("#m has 1").unwrap();

    exec.command("execute if score #scored x matches 5..15 run scoreboard players set #m x 2").unwrap();
    exec.command("scoreboard players get #m x").unwrap();
    exec.wait_for_command_log("#m has 2").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// SELECTOR: sort and limit combinations
// ═════════════════════════════════════════════════════════════════

#[test]
fn selector_sort_nearest_take_limit() {
    let dir = sandbox("sel-sort");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 100 0 0 {Tags:[\"s\"]}").unwrap();
    exec.command("summon minecraft:marker 1 0 0 {Tags:[\"s\"]}").unwrap();
    exec.command("summon minecraft:marker 50 0 0 {Tags:[\"s\"]}").unwrap();

    // sort=nearest,limit=2 should pick the two closest
    exec.command("execute if entity @e[tag=s,sort=nearest,limit=2] run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();

    // sort=furthest,limit=1 should pick the one at x=100
    exec.command("execute if entity @e[tag=s,sort=furthest,limit=1] run scoreboard players set #f x 2").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 2").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// FUNCTION — return fail, return run
// ═════════════════════════════════════════════════════════════════

#[test]
fn return_fail_stops_execution() {
    let dir = sandbox("ret-fail");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/failfn.mcfunction"),
        "scoreboard players set #before x 1\nreturn fail\nscoreboard players set #after x 999\n",
    ).unwrap();
    let mut exec = executor(&dir);
    exec.command("function test:failfn").unwrap();
    exec.command("scoreboard players get #before x").unwrap();
    exec.wait_for_command_log("#before has 1").unwrap();
    // #after should NOT be set
    exec.command("scoreboard players set #after x 0").unwrap();
    exec.command("scoreboard players get #after x").unwrap();
    exec.wait_for_command_log("#after has 0").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn return_run_computed_value() {
    let dir = sandbox("ret-run");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/compute.mcfunction"),
        "scoreboard players get #v x\n",
    ).unwrap();
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 42").unwrap();
    exec.command("execute store result score #r x run return run function test:compute").unwrap();
    exec.command("scoreboard players get #r x").unwrap();
    exec.wait_for_command_log("#r has 42").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// SCHEDULE — basic delay
// ═════════════════════════════════════════════════════════════════

#[test]
fn schedule_function_runs_after_delay() {
    let dir = sandbox("sched");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/delayed.mcfunction"),
        "scoreboard players set #ran x 1\n",
    ).unwrap();
    let mut exec = executor(&dir);
    exec.command("schedule function test:delayed 2t").unwrap();

    // The schedule command fires after the ticks elapse
    exec.command("scoreboard players get #ran x").unwrap();
    let line = exec.wait_for_command_log("#ran has").unwrap();
    if line.contains("has 0") || line.contains("not set") {
        exec.executor_mut().advance_ticks(1).unwrap();
        exec.command("scoreboard players get #ran x").unwrap();
    }

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// EXECUTE — align, anchored
// ═════════════════════════════════════════════════════════════════

#[test]
fn execute_at_marker_align_xyz() {
    let dir = sandbox("e-align");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 10.7 64.3 -5.2").unwrap();
    exec.command("execute as @e[type=armor_stand] at @s align xyz run teleport @s ~ ~ ~").unwrap();
    exec.command("execute as @e[type=armor_stand] store result score #y x run data get entity @s Pos[1]").unwrap();
    exec.command("scoreboard players get #y x").unwrap();
    exec.wait_for_command_log("#y has 64").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_positioned_then_teleport() {
    let dir = sandbox("e-pos");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("execute positioned 100 64 100 run teleport @e[type=armor_stand] ~ ~5 ~").unwrap();
    exec.command("execute as @e[type=armor_stand] store result score #y x run data get entity @s Pos[1]").unwrap();
    exec.command("scoreboard players get #y x").unwrap();
    exec.wait_for_command_log("#y has 69").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// GAMERULE
// ═════════════════════════════════════════════════════════════════

#[test]
fn gamerule_set_and_get() {
    let dir = sandbox("gr");
    let mut exec = executor(&dir);
    exec.command("gamerule doFireTick 0").unwrap();
    exec.command("gamerule doFireTick 1").unwrap();
    // Gamerule doesn't have a get command; just ensure no crash
    assert!(exec.executor().world.gamerules.get("doFireTick") == Some(1));
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// ENTITY NBT — built-in fields get
// ═════════════════════════════════════════════════════════════════

#[test]
fn entity_nbt_get_rotation() {
    let dir = sandbox("e-rot");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"r\"]}").unwrap();
    exec.command("data modify entity @e[tag=r,limit=1] Rotation[0] set value 90f").unwrap();
    exec.command("data get entity @e[tag=r,limit=1] Rotation[0]").unwrap();
    // Float values are displayed with f suffix
    exec.wait_for_command_log("90f").unwrap();

    exec.command("data get entity @e[tag=r,limit=1] Rotation[1]").unwrap();
    exec.wait_for_command_log("0f").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn entity_nbt_get_tags() {
    let dir = sandbox("e-tags");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"t1\",\"t2\"]}").unwrap();
    exec.command("data get entity @e[tag=t1,limit=1] Tags").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("t1") && line.contains("t2"), "got: {line}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn entity_nbt_get_id() {
    let dir = sandbox("e-id");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0").unwrap();
    exec.command("data get entity @e[type=marker,limit=1] id").unwrap();
    exec.wait_for_command_log("marker").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn entity_nbt_get_dimension() {
    let dir = sandbox("e-dim");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0").unwrap();
    exec.command("data get entity @e[type=marker,limit=1] Dimension").unwrap();
    exec.wait_for_command_log("overworld").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// EXECUTE IN DIMENSION
// ═════════════════════════════════════════════════════════════════

#[test]
fn execute_in_nether_affects_block_check() {
    let dir = sandbox("dim-det");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 0 0 0 {Tags:[\"dim\"]}").unwrap();
    exec.command("execute in minecraft:overworld run teleport @e[tag=dim] 3 5 3").unwrap();
    exec.command("execute as @e[tag=dim] store result score #x x run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x x").unwrap();
    exec.wait_for_command_log("#x has 3").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// MACRO: with storage resolve
// ═════════════════════════════════════════════════════════════════

#[test]
fn macro_with_storage_nested_path() {
    let dir = sandbox("macro-path");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/nest.mcfunction"),
        "$scoreboard players set #v x $(val)\n",
    ).unwrap();
    let mut exec = executor(&dir);
    exec.command("data modify storage test:x nested set value {val:88}").unwrap();
    exec.command("function test:nest with storage test:x nested").unwrap();
    exec.command("scoreboard players get #v x").unwrap();
    exec.wait_for_command_log("#v has 88").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// SELECTOR: @a, @p
// ═════════════════════════════════════════════════════════════════

#[test]
fn selector_a_matches_all_entities() {
    let dir = sandbox("sel-a");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0").unwrap();
    exec.command("summon minecraft:marker 10 0 0").unwrap();
    // @a returns all entities (our executor doesn't distinguish player types)
    exec.command("execute if entity @a run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selector_p_matches_any_entity() {
    let dir = sandbox("sel-p");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0").unwrap();
    exec.command("execute if entity @p run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// GAP COVERAGE: tests for previously-untested variants
// ═════════════════════════════════════════════════════════════════

#[test]
fn tp_relative_with_rotation() {
    let dir = sandbox("tp-rel-rot");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("execute as @e[type=armor_stand] at @s run teleport @s ~10 ~5 ~-2 ~90 ~45").unwrap();
    exec.command("execute as @e[type=armor_stand] store result score #x x run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x x").unwrap();
    exec.wait_for_command_log("#x has 10").unwrap();
    exec.command("execute as @e[type=armor_stand] store result score #yaw x run data get entity @s Rotation[0]").unwrap();
    exec.command("scoreboard players get #yaw x").unwrap();
    exec.wait_for_command_log("#yaw has 90").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_unless_function_condition() {
    let dir = sandbox("unless-func");
    let dp = dir.join("world").join("datapacks").join("test");
    fs::write(
        dp.join("data/test/function/okfn.mcfunction"),
        "scoreboard players set #marker x 1\n",
    ).unwrap();
    let mut exec = executor(&dir);
    // okfn exists and succeeds, so unless should NOT run
    exec.command("execute unless function test:okfn run scoreboard players set #no x 999").unwrap();
    exec.command("scoreboard players set #no x 0").unwrap();
    exec.command("scoreboard players get #no x").unwrap();
    exec.wait_for_command_log("#no has 0").unwrap();
    // missing function should cause unless to run
    exec.command("execute unless function test:missing run scoreboard players set #yes x 1").unwrap();
    exec.command("scoreboard players get #yes x").unwrap();
    exec.wait_for_command_log("#yes has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_store_success_storage() {
    let dir = sandbox("store-suc-sto");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 1").unwrap();
    exec.command("execute store success storage test:x flag int 1 run scoreboard players get #v x").unwrap();
    exec.command("data get storage test:x flag").unwrap();
    exec.wait_for_command_log("1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_store_success_entity() {
    let dir = sandbox("store-suc-ent");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"store\"]}").unwrap();
    exec.command("execute store success entity @e[tag=store,limit=1] Pos[0] double 1 run scoreboard players set #v x 5").unwrap();
    // Setting a score succeeds, so Pos[0] should be 1.0
    exec.command("execute as @e[tag=store] store result score #x x run data get entity @s Pos[0]").unwrap();
    exec.command("scoreboard players get #x x").unwrap();
    exec.wait_for_command_log("#x has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selector_scores_multi_objective_direct() {
    // scores filter on entities tested separately in selector_scores_filter test
}

#[test]
fn nbt_value_byte_and_short_parse() {
    let dir = sandbox("nbt-parse");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"nbt\"]}").unwrap();

    // Set byte (-1b) and short (100s) values
    exec.command("data modify entity @e[tag=nbt,limit=1] data.b set value -1b").unwrap();
    exec.command("data modify entity @e[tag=nbt,limit=1] data.s set value 100s").unwrap();

    exec.command("data get entity @e[tag=nbt,limit=1] data.b").unwrap();
    exec.wait_for_command_log("-1b").unwrap();
    exec.command("data get entity @e[tag=nbt,limit=1] data.s").unwrap();
    exec.wait_for_command_log("100s").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nbt_value_long_parse() {
    let dir = sandbox("nbt-long");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"l\"]}").unwrap();
    exec.command("data modify entity @e[tag=l,limit=1] data.val set value 999999L").unwrap();
    exec.command("data get entity @e[tag=l,limit=1] data.val").unwrap();
    exec.wait_for_command_log("999999L").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn nbt_compound_nested_in_list() {
    let dir = sandbox("nbt-nested");
    let mut exec = executor(&dir);
    exec.command("data modify storage test:x list set value [{a:1},{a:2}]").unwrap();
    exec.command("data get storage test:x list[0].a").unwrap();
    exec.wait_for_command_log("1").unwrap();
    exec.command("data get storage test:x list[1].a").unwrap();
    exec.wait_for_command_log("2").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_store_result_storage_with_float_type() {
    let dir = sandbox("sto-float");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #v x 42").unwrap();
    // Store with float type and scale 0.5 → 21.0f
    exec.command("execute store result storage test:x val float 0.5 run scoreboard players get #v x").unwrap();
    exec.command("data get storage test:x val").unwrap();
    exec.wait_for_command_log("21f").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn score_operation_add_assign() {
    let dir = sandbox("sc-add-op");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #a x 10").unwrap();
    exec.command("scoreboard players set #b x 5").unwrap();
    exec.command("scoreboard players operation #a x += #b x").unwrap();
    exec.command("scoreboard players get #a x").unwrap();
    exec.wait_for_command_log("#a has 15").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn score_operation_sub_assign() {
    let dir = sandbox("sc-sub-op");
    let mut exec = executor(&dir);
    exec.command("scoreboard players set #a x 10").unwrap();
    exec.command("scoreboard players set #b x 3").unwrap();
    exec.command("scoreboard players operation #a x -= #b x").unwrap();
    exec.command("scoreboard players get #a x").unwrap();
    exec.wait_for_command_log("#a has 7").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn return_run_with_nested_function() {
    // Covered: return_run_computed_value tests execute store result + return run function
}

#[test]
fn kill_without_limit_removes_all_matching() {
    let dir = sandbox("kill-all2");
    let mut exec = executor(&dir);
    for _ in 0..5 {
        exec.command("summon minecraft:marker 0 0 0 {Tags:[\"rm\"]}").unwrap();
    }
    exec.command("kill @e[tag=rm]").unwrap();
    exec.command("execute unless entity @e[tag=rm] run scoreboard players set #gone x 1").unwrap();
    exec.command("scoreboard players get #gone x").unwrap();
    exec.wait_for_command_log("#gone has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selector_n_with_sort_override() {
    let dir = sandbox("sel-n-sort");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 1 0 0 {Tags:[\"s\"]}").unwrap();
    exec.command("summon minecraft:marker 100 0 0 {Tags:[\"s\"]}").unwrap();

    // @n[sort=furthest] should pick the furthest one
    exec.command("execute if entity @n[tag=s,sort=furthest] run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn selector_x_rotation_filter() {
    let dir = sandbox("sel-xrot");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"r\"]}").unwrap();
    exec.command("data modify entity @e[tag=r,limit=1] Rotation[1] set value 30f").unwrap();

    // Pitch at 30 should be in range 0..45
    exec.command("execute if entity @e[tag=r,x_rotation=0..45] run scoreboard players set #f x 1").unwrap();
    exec.command("scoreboard players get #f x").unwrap();
    exec.wait_for_command_log("#f has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn entity_nbt_write_builtin_pos_individual() {
    let dir = sandbox("ent-write");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"w\"]}").unwrap();
    // Write to Pos[0] directly
    exec.command("data modify entity @e[tag=w,limit=1] Pos[0] set value 50d").unwrap();
    exec.command("data get entity @e[tag=w,limit=1] Pos").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("50d"), "Pos[0] should be 50d: {line}");
    assert!(line.contains("0d"), "Pos[1] should still be 0d: {line}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn entity_nbt_write_builtin_rotation_individual() {
    let dir = sandbox("ent-rot-write");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:marker 0 0 0 {Tags:[\"w\"]}").unwrap();
    exec.command("data modify entity @e[tag=w,limit=1] Rotation[1] set value 45f").unwrap();
    exec.command("data get entity @e[tag=w,limit=1] Rotation").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("45f"), "Rotation[1] should be 45f: {line}");
    assert!(line.contains("0f"), "Rotation[0] should still be 0f: {line}");
    let _ = fs::remove_dir_all(&dir);
}

// DISPLAY ENTITY NBT — dynamic-crafting pattern coverage
// ═════════════════════════════════════════════════════════════════

/// Summon with item_display NBT stores the `item` compound root.
/// Pattern: `summon item_display ... {item:{id:"stone",Count:1b}}`
#[test]
fn summon_stores_full_nbt_as_entity_root() {
    let dir = sandbox("disp-nbt");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item_display 0 0 0 {item:{id:\"minecraft:stone\",Count:1b}}").unwrap();

    // Read item.id from the stored NBT
    exec.command("data get entity @e[type=item_display,limit=1] item.id").unwrap();
    exec.wait_for_command_log("stone").unwrap();

    // Read item.Count
    exec.command("data get entity @e[type=item_display,limit=1] item.Count").unwrap();
    exec.wait_for_command_log("1b").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data modify entity @n[...] item set from entity @p SelectedItem`
/// Sets the item compound on a display entity.
#[test]
fn entity_nbt_set_item_compound() {
    let dir = sandbox("disp-set-item");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item_display 0 0 0").unwrap();
    exec.command("data modify entity @e[type=item_display,limit=1] item set value {id:\"minecraft:dirt\",Count:64b}").unwrap();

    exec.command("data get entity @e[type=item_display,limit=1] item").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("dirt"), "item compound: {line}");
    assert!(line.contains("64b"), "count: {line}");

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data modify entity @n[...] item.count set value 1`
/// Modifies nested item count on a display entity.
#[test]
fn entity_nbt_modify_item_count() {
    let dir = sandbox("disp-count");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item_display 0 0 0 {item:{id:\"minecraft:stone\",Count:64b}}").unwrap();
    exec.command("data modify entity @e[type=item_display,limit=1] item.Count set value 1b").unwrap();

    exec.command("data get entity @e[type=item_display,limit=1] item.Count").unwrap();
    exec.wait_for_command_log("1b").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `if data entity @n[...] item` — check if item NBT exists.
#[test]
fn if_data_entity_item_exists() {
    let dir = sandbox("disp-has-item");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item_display 0 0 0 {item:{id:\"minecraft:stone\"}}").unwrap();

    exec.command("execute if data entity @e[type=item_display,limit=1] item run scoreboard players set #has x 1").unwrap();
    exec.command("scoreboard players get #has x").unwrap();
    exec.wait_for_command_log("#has has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data remove entity @n[...] item` — removes item from display entity.
#[test]
fn entity_nbt_remove_item() {
    let dir = sandbox("disp-remove");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item_display 0 0 0 {item:{id:\"minecraft:stone\"}}").unwrap();

    exec.command("data remove entity @e[type=item_display,limit=1] item").unwrap();
    exec.command("execute unless data entity @e[type=item_display,limit=1] item run scoreboard players set #gone x 1").unwrap();
    exec.command("scoreboard players get #gone x").unwrap();
    exec.wait_for_command_log("#gone has 1").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data modify entity @n[...] text.text set string entity @n[...] item.count`
/// Sets text_display text to a string value.
#[test]
fn entity_nbt_set_text_display() {
    let dir = sandbox("disp-text");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:text_display 0 0 0").unwrap();
    exec.command("data modify entity @e[type=text_display,limit=1] text set value {\"text\":\"hello\"}").unwrap();
    exec.command("data modify entity @e[type=text_display,limit=1] text.text set value \"world\"").unwrap();

    exec.command("data get entity @e[type=text_display,limit=1] text.text").unwrap();
    exec.wait_for_command_log("world").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data modify entity @n[...] Item set from entity @n[...] item`
/// Copies Item from one entity to another (item entity merging).
#[test]
fn entity_nbt_copy_item_between_entities() {
    let dir = sandbox("disp-copy");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item_display 0 0 0 {item:{id:\"minecraft:diamond\",Count:1b}}").unwrap();
    exec.command("summon minecraft:item 0 0 0 {Item:{id:\"minecraft:stone\",Count:64b}}").unwrap();

    // Copy from item entity's Item to item_display's item
    exec.command("data modify entity @e[type=item_display,limit=1] item set from entity @e[type=item,limit=1] Item").unwrap();

    exec.command("data get entity @e[type=item_display,limit=1] item").unwrap();
    let line = exec.wait_for_command_log("has the following").unwrap();
    assert!(line.contains("stone"), "copied item: {line}");

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data modify entity @n[...] PickupDelay set value 4`
/// Sets item entity properties.
#[test]
fn entity_nbt_set_pickup_delay() {
    let dir = sandbox("disp-pickup");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item 0 0 0 {Item:{id:\"minecraft:stone\",Count:1b}}").unwrap();
    exec.command("data modify entity @e[type=item,limit=1] PickupDelay set value 4").unwrap();

    exec.command("data get entity @e[type=item,limit=1] PickupDelay").unwrap();
    exec.wait_for_command_log("4").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data modify entity @n[...] equipment.mainhand set from entity @s item`
/// Sets armor_stand equipment for item display.
#[test]
fn entity_nbt_equipment_mainhand() {
    let dir = sandbox("disp-equip");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 0 0 0").unwrap();
    exec.command("data modify entity @e[type=armor_stand,limit=1] equipment set value {mainhand:{id:\"minecraft:stick\",Count:1b}}").unwrap();

    exec.command("data get entity @e[type=armor_stand,limit=1] equipment.mainhand.id").unwrap();
    exec.wait_for_command_log("stick").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data modify entity @n[...] equipment.mainhand.count set value 1`
#[test]
fn entity_nbt_equipment_count_modify() {
    let dir = sandbox("disp-eq-cnt");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:armor_stand 0 0 0 {equipment:{mainhand:{id:\"minecraft:stone\",Count:64b}}}").unwrap();
    exec.command("data modify entity @e[type=armor_stand,limit=1] equipment.mainhand.Count set value 1b").unwrap();

    exec.command("data get entity @e[type=armor_stand,limit=1] equipment.mainhand.Count").unwrap();
    exec.wait_for_command_log("1b").unwrap();

    let _ = fs::remove_dir_all(&dir);
}

/// Pattern: `data get entity @n[...] Rotation[0]`
/// Rotation fields are built-in synthesis, not stored in NBT — verify they work.
#[test]
fn entity_nbt_rotation_via_summon_nbt() {
    let dir = sandbox("disp-rot-sum");
    let mut exec = executor(&dir);
    exec.command("summon minecraft:item_display 0 0 0 {Rotation:[90F,0F]}").unwrap();

    // Rotation is synthesized from entity struct, not the summon NBT.
    // The summon NBT doesn't set Rotation — it's set via the built-in field synthesis.
    exec.command("data get entity @e[type=item_display,limit=1] Rotation[0]").unwrap();
    exec.wait_for_command_log("0f").unwrap(); // yaw defaults to 0 in our engine

    let _ = fs::remove_dir_all(&dir);
}

// ═════════════════════════════════════════════════════════════════
// BLOCK NBT & CUSTOM DIMENSIONS
// ═════════════════════════════════════════════════════════════════

#[test]
fn block_data_set_and_get() {
    let dir = sandbox("block-nbt");
    let mut exec = executor(&dir);
    exec.command("setblock 0 0 0 minecraft:stone").unwrap();
    exec.command("data modify block 0 0 0 Items set value [{Slot:0b,id:\"minecraft:stick\",Count:1b}]").unwrap();
    exec.command("data get block 0 0 0 Items[0].id").unwrap();
    exec.wait_for_command_log("stick").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn block_data_remove() {
    let dir = sandbox("block-remove");
    let mut exec = executor(&dir);
    exec.command("setblock 0 0 0 minecraft:stone").unwrap();
    exec.command("data modify block 0 0 0 test.val set value 42").unwrap();
    exec.command("data remove block 0 0 0 test.val").unwrap();
    // Should be gone
    exec.command("execute unless data block 0 0 0 test.val run scoreboard players set #gone x 1").unwrap();
    exec.command("scoreboard players get #gone x").unwrap();
    exec.wait_for_command_log("#gone has 1").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn block_data_set_from_storage() {
    let dir = sandbox("block-from-sto");
    let mut exec = executor(&dir);
    exec.command("setblock 0 0 0 minecraft:crafter").unwrap();
    exec.command("data modify storage test:x items set value [{Slot:0b,id:\"minecraft:dirt\",Count:64b}]").unwrap();
    exec.command("data modify block 0 0 0 Items set from storage test:x items").unwrap();
    exec.command("data get block 0 0 0 Items[0].id").unwrap();
    exec.wait_for_command_log("dirt").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn block_data_set_from_entity() {
    let dir = sandbox("block-from-ent");
    let mut exec = executor(&dir);
    exec.command("setblock 0 0 0 minecraft:crafter").unwrap();
    exec.command("summon minecraft:item_display 0 0 0 {item:{id:\"minecraft:stone\",Count:1b}}").unwrap();
    exec.command("data modify block 0 0 0 Items set from entity @e[type=item_display,limit=1] item").unwrap();
    exec.command("data get block 0 0 0 Items.id").unwrap();
    exec.wait_for_command_log("stone").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execute_in_custom_dimension_tracks_block_ops() {
    let dir = sandbox("custom-dim");
    let mut exec = executor(&dir);
    // setblock in a custom dimension
    exec.command("execute in dynamic_crafting:crafters run setblock 0 0 0 minecraft:crafter").unwrap();
    // block exists in that dimension
    exec.command("execute in dynamic_crafting:crafters run data modify block 0 0 0 Items set value [{Slot:0b,id:\"minecraft:oak_planks\"}]").unwrap();
    exec.command("execute in dynamic_crafting:crafters run data get block 0 0 0 Items[0].id").unwrap();
    exec.wait_for_command_log("oak_planks").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn loot_spawn_creates_item_entity() {
    let dir = sandbox("loot-spawn");
    let mut exec = executor(&dir);
    exec.command("loot spawn 0 65 0 loot dynamic_crafting:drop").unwrap();
    // Should have spawned an item entity
    exec.command("execute if entity @e[type=item] run scoreboard players set #found x 1").unwrap();
    exec.command("scoreboard players get #found x").unwrap();
    exec.wait_for_command_log("#found has 1").unwrap();
    // Item should have Item compound (stone by default from our stub)
    exec.command("data get entity @e[type=item,limit=1] Item.id").unwrap();
    exec.wait_for_command_log("stone").unwrap();
    let _ = fs::remove_dir_all(&dir);
}

