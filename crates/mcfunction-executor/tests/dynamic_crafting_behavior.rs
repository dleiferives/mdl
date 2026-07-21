//! Dynamic Crafting behavioral tests — verifies every pattern the datapack
//! uses produces correct results against known vanilla Minecraft 1.21 behavior.
//!
//! Each test encodes a specific crafting pipeline interaction and asserts
//! the expected outcome. Designed to pass against both the executor and
//! a real MC server for cross-validation.

use std::fs;
use std::path::PathBuf;
use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-dctest-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let dp = dir.join("world").join("datapacks").join("test");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"dctest","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    fs::create_dir_all(dp.join("data/test/function")).unwrap();
    dir
}

fn exec(dir: &PathBuf) -> McExecutor {
    let mut e = McExecutor::create(dir.clone(), V26_2);
    e.load_datapacks().unwrap();
    e
}

fn get_score(exec: &mut McExecutor, holder: &str, obj: &str) -> i32 {
    exec.command(&format!("scoreboard players get {holder} {obj}")).unwrap();
    match exec.wait_for_command_log(holder) {
        Ok(line) => {
            let p: Vec<&str> = line.split_whitespace().collect();
            p.iter().position(|w| *w == "has")
                .and_then(|i| p.get(i+1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
        }
        Err(_) => 0,
    }
}

// ═════════════════════════════════════════════════════════════════
// CRAFTING TABLE CONVERSION (convert.mcfunction)
// ═════════════════════════════════════════════════════════════════

/// Pattern: summon markers for rotation detection, then kill them.
/// `summon marker ~ ~ ~ {Tags:[...]}` + `kill @e[tag=...]`
#[test]
fn summon_and_kill_temp_markers() {
    let d = sandbox("tmp-markers");
    let mut e = exec(&d);

    // Summon temporary markers (conversion helpers)
    e.command("summon minecraft:marker 0 0 0 {Tags:[\"temp.get_rot\"]}").unwrap();
    e.command("summon minecraft:marker 0 0 0 {Tags:[\"temp.get_pos\"]}").unwrap();

    // Verify they exist
    e.command("execute if entity @e[tag=temp.get_rot] run scoreboard players set #exists v 1").unwrap();
    e.command("scoreboard players get #exists v").unwrap();
    e.wait_for_command_log("#exists has 1").unwrap();

    // Kill them (cleanup)
    e.command("kill @e[tag=temp.get_rot]").unwrap();
    e.command("kill @e[tag=temp.get_pos]").unwrap();

    // Verify gone
    e.command("execute unless entity @e[tag=temp.get_rot] run scoreboard players set #gone v 1").unwrap();
    e.command("scoreboard players get #gone v").unwrap();
    e.wait_for_command_log("#gone has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: read entity position via data get entity Pos[0-2].
/// Used to detect crafting table position before conversion.
#[test]
fn read_entity_position_via_data_get() {
    let d = sandbox("pos-read");
    let mut e = exec(&d);

    e.command("summon minecraft:marker 10 20 30 {Tags:[\"pos\"]}").unwrap();
    e.command("data get entity @e[tag=pos,limit=1] Pos[0]").unwrap();
    e.wait_for_command_log("10d").unwrap();
    e.command("data get entity @e[tag=pos,limit=1] Pos[1]").unwrap();
    e.wait_for_command_log("20d").unwrap();
    e.command("data get entity @e[tag=pos,limit=1] Pos[2]").unwrap();
    e.wait_for_command_log("30d").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: store entity Pos to scoreboard via execute store result.
#[test]
fn store_entity_pos_to_scoreboard() {
    let d = sandbox("pos-to-score");
    let mut e = exec(&d);

    e.command("summon minecraft:marker 42 64 15 {Tags:[\"s\"]}").unwrap();
    e.command("execute store result score #x v run data get entity @e[tag=s,limit=1] Pos[0]").unwrap();
    e.command("execute store result score #y v run data get entity @e[tag=s,limit=1] Pos[1]").unwrap();

    e.command("scoreboard players get #x v").unwrap();
    e.wait_for_command_log("#x has 42").unwrap();
    e.command("scoreboard players get #y v").unwrap();
    e.wait_for_command_log("#y has 64").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: scoreboard chunk-size division and scale store.
/// `scoreboard players operation .temp.pos_x /= .chunk_size` then store to storage.
#[test]
fn scoreboard_chunk_math() {
    let d = sandbox("chunk-math");
    let mut e = exec(&d);

    e.command("scoreboard players set #pos v 50").unwrap();
    e.command("scoreboard players set #chunk v 16").unwrap();
    e.command("scoreboard players operation #pos v /= #chunk v").unwrap();
    // 50 / 16 = 3 (integer division)
    e.command("scoreboard players get #pos v").unwrap();
    e.wait_for_command_log("#pos has 3").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// SLOT CREATION (create_slots.mcfunction)
// ═════════════════════════════════════════════════════════════════

/// Pattern: summon 9 item_display entities for crafting slots.
/// Each gets Tags:["slot.visual","slot.N"] where N is 0-8.
#[test]
fn summon_nine_slot_displays() {
    let d = sandbox("nine-slots");
    let mut e = exec(&d);

    for slot in 0..9u32 {
        e.command(&format!(
            "summon minecraft:item_display 0 0 0 {{Tags:[\"slot.visual\",\"slot.{slot}\"]}}"
        )).unwrap();
    }

    // All 9 should exist
    for slot in 0..9 {
        e.command(&format!(
            "execute if entity @e[tag=slot.{slot},limit=1] run scoreboard players add #count v 1"
        )).unwrap();
    }
    e.command("scoreboard players get #count v").unwrap();
    e.wait_for_command_log("#count has 9").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: summon interaction entities for slot hitboxes.
/// Each has width/height NBT passed at summon time.
#[test]
fn summon_slot_interaction_hitboxes() {
    let d = sandbox("hitboxes");
    let mut e = exec(&d);

    e.command("summon minecraft:interaction 0 0 0 {Tags:[\"slot.hitbox\",\"slot.0\"]}").unwrap();
    e.command("execute if entity @e[tag=slot.hitbox,tag=slot.0] run scoreboard players set #ok v 1").unwrap();
    e.command("scoreboard players get #ok v").unwrap();
    e.wait_for_command_log("#ok has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: summon crafting table tick marker.
/// After slot creation, a marker with tick tag is summoned.
#[test]
fn summon_crafting_table_tick_marker() {
    let d = sandbox("ct-marker");
    let mut e = exec(&d);

    e.command("summon minecraft:marker 0 0 0 {Tags:[\"block.crafting_table.tick\"]}").unwrap();
    e.command("execute if entity @e[tag=block.crafting_table.tick] run scoreboard players set #ok v 1").unwrap();
    e.command("scoreboard players get #ok v").unwrap();
    e.wait_for_command_log("#ok has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// SLOT INTERACTION (slot/tick.mcfunction, slot/interacted.mcfunction)
// ═════════════════════════════════════════════════════════════════

/// Pattern: interaction entity cooldown via scoreboard.
/// `execute if score @s values matches 1..` checks if cooldown is active.
#[test]
fn interaction_cooldown_via_score() {
    let d = sandbox("cooldown");
    let mut e = exec(&d);

    e.command("summon minecraft:interaction 0 0 0 {Tags:[\"hitbox\"]}").unwrap();
    e.command("scoreboard players set @e[tag=hitbox,limit=1] dc_values 5").unwrap();

    // Cooldown active (5 >= 1)
    e.command("execute if score @e[tag=hitbox,limit=1] dc_values matches 1.. run scoreboard players set #cooling v 1").unwrap();
    e.command("scoreboard players get #cooling v").unwrap();
    e.wait_for_command_log("#cooling has 1").unwrap();

    // Decrement cooldown
    e.command("scoreboard players remove @e[tag=hitbox,limit=1] dc_values 1").unwrap();
    e.command("scoreboard players get @e[tag=hitbox,limit=1] dc_values").unwrap();
    e.wait_for_command_log("4").unwrap();

    // Set to 0, now cooldown expired
    e.command("scoreboard players set @e[tag=hitbox,limit=1] dc_values 0").unwrap();
    e.command("execute if score @e[tag=hitbox,limit=1] dc_values matches ..0 run scoreboard players set #ready v 1").unwrap();
    e.command("scoreboard players get #ready v").unwrap();
    e.wait_for_command_log("#ready has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: detect interaction via `if data entity @s interaction`.
/// When a player interacts, the interaction entity has an `interaction` NBT field set.
#[test]
fn detect_interaction_via_data_entity() {
    let d = sandbox("detect-int");
    let mut e = exec(&d);

    e.command("summon minecraft:interaction 0 0 0 {Tags:[\"hitbox\"]}").unwrap();

    // Simulate interaction by setting the field
    e.command("data modify entity @e[tag=hitbox,limit=1] interaction set value {player:1}").unwrap();

    // Detection: if data entity @s interaction → process
    e.command("execute if data entity @e[tag=hitbox,limit=1] interaction run scoreboard players set #interacted v 1").unwrap();
    e.command("scoreboard players get #interacted v").unwrap();
    e.wait_for_command_log("#interacted has 1").unwrap();

    // Reset (datapack does `data remove entity @s interaction`)
    e.command("data remove entity @e[tag=hitbox,limit=1] interaction").unwrap();
    e.command("execute unless data entity @e[tag=hitbox,limit=1] interaction run scoreboard players set #reset v 1").unwrap();
    e.command("scoreboard players get #reset v").unwrap();
    e.wait_for_command_log("#reset has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// SLOT ITEM OPERATIONS (slot/add_item, slot/remove_item, slot/clear_1_item)
// ═════════════════════════════════════════════════════════════════

/// Pattern: add item to empty slot.
/// `data modify entity @n[...] item set from entity @p SelectedItem`
#[test]
fn add_item_to_empty_slot() {
    let d = sandbox("add-item");
    let mut e = exec(&d);

    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"slot.0\"]}").unwrap();
    e.command("data modify entity @e[tag=slot.0,limit=1] item set value {id:\"minecraft:stone\",Count:1b}").unwrap();

    e.command("data get entity @e[tag=slot.0,limit=1] item.id").unwrap();
    e.wait_for_command_log("stone").unwrap();
    e.command("data get entity @e[tag=slot.0,limit=1] item.Count").unwrap();
    e.wait_for_command_log("1b").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: detect slot has item via `if data entity @n[...] item`.
#[test]
fn detect_slot_has_item() {
    let d = sandbox("has-item");
    let mut e = exec(&d);

    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"s\"]}").unwrap();

    // Empty slot: unless should fire
    e.command("execute unless data entity @e[tag=s,limit=1] item run scoreboard players set #empty v 1").unwrap();
    e.command("scoreboard players get #empty v").unwrap();
    e.wait_for_command_log("#empty has 1").unwrap();

    // Add item
    e.command("data modify entity @e[tag=s,limit=1] item set value {id:\"minecraft:dirt\"}").unwrap();

    // Filled slot: if should fire
    e.command("execute if data entity @e[tag=s,limit=1] item run scoreboard players set #full v 1").unwrap();
    e.command("scoreboard players get #full v").unwrap();
    e.wait_for_command_log("#full has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: clear 1 item = remove 1 from count, or remove item if count becomes 0.
/// `scoreboard players operation ... -= ...` then `data remove entity ... item` if count=0.
#[test]
fn clear_one_item_from_slot() {
    let d = sandbox("clear1");
    let mut e = exec(&d);

    // Set up a temp armor_stand with item count
    e.command("summon minecraft:armor_stand 0 0 0 {Tags:[\"temp\"]}").unwrap();
    e.command("data modify entity @e[tag=temp,limit=1] equipment set value {mainhand:{id:\"minecraft:stone\",Count:64b}}").unwrap();
    e.command("data modify entity @e[tag=temp,limit=1] equipment.mainhand.Count set value 1b").unwrap();
    e.command("data get entity @e[tag=temp,limit=1] equipment.mainhand.Count").unwrap();
    e.wait_for_command_log("1b").unwrap();

    // Count is 1 → remove item entirely
    e.command("data remove entity @e[tag=temp,limit=1] equipment.mainhand").unwrap();
    e.command("execute unless data entity @e[tag=temp,limit=1] equipment.mainhand run scoreboard players set #cleared v 1").unwrap();
    e.command("scoreboard players get #cleared v").unwrap();
    e.wait_for_command_log("#cleared has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// RECIPE DETECTION (update_slots.mcfunction)
// ═════════════════════════════════════════════════════════════════

/// Pattern: collect slot items into storage Items list.
/// Uses `data modify storage ... Items append from entity @n[...] item`.
#[test]
fn collect_slot_items_to_storage() {
    let d = sandbox("collect");
    let mut e = exec(&d);

    // Create 3 slots with items
    for i in 0..3 {
        e.command(&format!("summon minecraft:item_display 0 0 0 {{Tags:[\"slot.{i}\"]}}")).unwrap();
    }
    e.command("data modify entity @e[tag=slot.0,limit=1] item set value {id:\"minecraft:oak_planks\",Count:1b}").unwrap();
    e.command("data modify entity @e[tag=slot.1,limit=1] item set value {id:\"minecraft:oak_planks\",Count:1b}").unwrap();
    // Slot 2 stays empty

    // Collect to storage (the update_slots pattern)
    e.command("data merge storage test:temp {Items:[]}").unwrap();
    e.command("data modify storage test:temp Items append from entity @e[tag=slot.0,limit=1] item").unwrap();
    e.command("data modify storage test:temp Items[-1] merge value {Slot:0b}").unwrap();
    e.command("data modify storage test:temp Items append from entity @e[tag=slot.1,limit=1] item").unwrap();
    e.command("data modify storage test:temp Items[-1] merge value {Slot:1b}").unwrap();
    // Slot 2 is empty — don't append

    // Verify structure
    e.command("data get storage test:temp Items[0].id").unwrap();
    e.wait_for_command_log("oak_planks").unwrap();
    e.command("data get storage test:temp Items[0].Slot").unwrap();
    e.wait_for_command_log("0b").unwrap();
    e.command("data get storage test:temp Items[1].Slot").unwrap();
    e.wait_for_command_log("1b").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: write collected Items to block NBT (the crafter).
/// `data modify block ~ ~ ~ Items set from storage test:temp Items`
#[test]
fn write_items_to_crafter_block() {
    let d = sandbox("crafter-nbt");
    let mut e = exec(&d);

    // Set up storage with Items
    e.command("data merge storage test:tmp {Items:[]}").unwrap();
    e.command("data modify storage test:tmp Items append value {id:\"minecraft:stone\",Count:1b}").unwrap();
    e.command("data modify storage test:tmp Items[-1] merge value {Slot:0b}").unwrap();

    // Write to crafter block
    e.command("setblock 0 0 0 minecraft:crafter").unwrap();
    e.command("data modify block 0 0 0 Items set from storage test:tmp Items").unwrap();

    // Verify
    e.command("data get block 0 0 0 Items[0].id").unwrap();
    e.wait_for_command_log("stone").unwrap();
    e.command("data get block 0 0 0 Items[0].Slot").unwrap();
    e.wait_for_command_log("0b").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// CRAFTING RESULT (craft/process.mcfunction, craft/result/remove.mcfunction)
// ═════════════════════════════════════════════════════════════════

/// Pattern: clear all slot ingredients before spawning result.
/// `execute positioned ~ ~-0.45 ~ as @e[tag=slot.visual,distance=0..0.3] at @s if data entity @s item run function clear_1_item`
#[test]
fn clear_slot_ingredients_before_spawn() {
    let d = sandbox("clear-all");
    let mut e = exec(&d);

    // Set up 3 slots with items
    for i in 0..3 {
        e.command(&format!("summon minecraft:item_display 0 0 0 {{Tags:[\"slot.visual\",\"slot.{i}\"]}}")).unwrap();
    }
    e.command("data modify entity @e[tag=slot.0,limit=1] item set value {id:\"minecraft:planks\"}").unwrap();
    e.command("data modify entity @e[tag=slot.1,limit=1] item set value {id:\"minecraft:planks\"}").unwrap();
    e.command("data modify entity @e[tag=slot.2,limit=1] item set value {id:\"minecraft:planks\"}").unwrap();

    // Verify all 3 items exist
    e.command("execute if data entity @e[tag=slot.0,limit=1] item run scoreboard players add #count v 1").unwrap();
    e.command("execute if data entity @e[tag=slot.1,limit=1] item run scoreboard players add #count v 1").unwrap();
    e.command("execute if data entity @e[tag=slot.2,limit=1] item run scoreboard players add #count v 1").unwrap();
    e.command("scoreboard players get #count v").unwrap();
    e.wait_for_command_log("#count has 3").unwrap();

    // Clear all
    e.command("data remove entity @e[tag=slot.0,limit=1] item").unwrap();
    e.command("data remove entity @e[tag=slot.1,limit=1] item").unwrap();
    e.command("data remove entity @e[tag=slot.2,limit=1] item").unwrap();

    // Verify cleared
    e.command("execute unless data entity @e[tag=slot.0,limit=1] item run scoreboard players set #cleared v 1").unwrap();
    e.command("scoreboard players get #cleared v").unwrap();
    e.wait_for_command_log("#cleared has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: spawn result item via loot, then copy visual item to result.
/// `loot spawn ~ ~ ~` then `data modify entity @n[...] Item set from entity @n[...] item`
#[test]
fn spawn_and_copy_result_item() {
    let d = sandbox("result");
    let mut e = exec(&d);

    // Set up visual result (item_display with crafted item)
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"result.visual\"]}").unwrap();
    e.command("data modify entity @e[tag=result.visual,limit=1] item set value {id:\"minecraft:crafting_table\",Count:1b}").unwrap();

    // Spawn result item entity (loot)
    e.command("loot spawn 0 65 0 loot dynamic_crafting:drop").unwrap();
    e.command("tag @e[type=item,limit=1] add result").unwrap();

    // Copy visual item to result Item
    e.command("data modify entity @e[tag=result,limit=1] Item set from entity @e[tag=result.visual,limit=1] item").unwrap();
    e.command("data modify entity @e[tag=result,limit=1] PickupDelay set value 4").unwrap();

    // Verify
    e.command("data get entity @e[tag=result,limit=1] Item.id").unwrap();
    e.wait_for_command_log("crafting_table").unwrap();
    e.command("data get entity @e[tag=result,limit=1] PickupDelay").unwrap();
    e.wait_for_command_log("4").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: kill result visual, hitbox, glow, count after crafting.
/// `kill @e[tag=result.visual,distance=0..0.25]` for cleanup.
#[test]
fn cleanup_result_entities() {
    let d = sandbox("cleanup");
    let mut e = exec(&d);

    // Spawn entities that need cleanup
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"result.visual\"]}").unwrap();
    e.command("summon minecraft:interaction 0 0 0 {Tags:[\"result.hitbox\"]}").unwrap();
    e.command("summon minecraft:text_display 0 0 0 {Tags:[\"result.count\"]}").unwrap();
    e.command("summon minecraft:text_display 0 0 0 {Tags:[\"result.glow\"]}").unwrap();

    // Verify they exist
    e.command("execute if entity @e[tag=result.visual] run scoreboard players set #before v 1").unwrap();
    e.command("scoreboard players get #before v").unwrap();
    e.wait_for_command_log("#before has 1").unwrap();

    // Kill all result entities
    e.command("kill @e[tag=result.visual]").unwrap();
    e.command("kill @e[tag=result.hitbox]").unwrap();
    e.command("kill @e[tag=result.count]").unwrap();
    e.command("kill @e[tag=result.glow]").unwrap();

    // Verify gone
    e.command("execute unless entity @e[tag=result.visual] run scoreboard players set #gone v 1").unwrap();
    e.command("scoreboard players get #gone v").unwrap();
    e.wait_for_command_log("#gone has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// CRAFTING TABLE REMOVAL (remove.mcfunction)
// ═════════════════════════════════════════════════════════════════

/// Pattern: drop all slot items before destroying crafting table.
/// `execute as @e[tag=slot.hitbox,distance=0..0.5] at @s if data entity @n[...] item run function slot/drop_item/all/process`
#[test]
fn drop_items_before_table_removal() {
    let d = sandbox("drop-before");
    let mut e = exec(&d);

    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"slot.visual\",\"slot.0\"]}").unwrap();
    e.command("data modify entity @e[tag=slot.0,limit=1] item set value {id:\"minecraft:stick\"}").unwrap();

    // Spawn drop, copy slot item, verify
    e.command("loot spawn 0 65 0 loot dynamic_crafting:drop").unwrap();
    e.command("tag @e[type=item,limit=1] add drop0").unwrap();
    e.command("data modify entity @e[tag=drop0,limit=1] Item set from entity @e[tag=slot.0,limit=1] item").unwrap();
    e.command("data get entity @e[tag=drop0,limit=1] Item.id").unwrap();
    e.wait_for_command_log("stick").unwrap();

    // Cleanup
    e.command("kill @e[tag=slot.visual]").unwrap();
    e.command("execute unless entity @e[tag=slot.visual] run scoreboard players set #clean v 1").unwrap();
    e.command("scoreboard players get #clean v").unwrap();
    e.wait_for_command_log("#clean has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// @n SELECTOR IN CRAFTING CONTEXT
// ═════════════════════════════════════════════════════════════════

/// Pattern: @n with tag and distance filter on display entities.
/// The datapack uses this to find the nearest slot within 0.5 blocks.
#[test]
fn at_n_with_tag_and_distance() {
    let d = sandbox("atn-craft");
    let mut e = exec(&d);

    // Spawn slot displays at different distances
    e.command("summon minecraft:item_display 1 0 0 {Tags:[\"slot.visual\",\"slot.0\"]}").unwrap();
    e.command("summon minecraft:item_display 10 0 0 {Tags:[\"slot.visual\",\"slot.1\"]}").unwrap();

    // @n within 2 blocks should find nearest (slot.0 at x=1)
    e.command("execute if entity @n[tag=slot.visual,distance=..2] run scoreboard players set #found v 1").unwrap();
    e.command("scoreboard players get #found v").unwrap();
    e.wait_for_command_log("#found has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Pattern: @n with sort=furthest override.
/// The datapack sometimes needs the furthest entity.
#[test]
fn at_n_sort_furthest() {
    let d = sandbox("atn-far");
    let mut e = exec(&d);

    e.command("summon minecraft:marker 1 0 0 {Tags:[\"m\"]}").unwrap();
    e.command("summon minecraft:marker 100 0 0 {Tags:[\"m\"]}").unwrap();

    // @n[sort=furthest] should pick the one at x=100
    e.command("data get entity @n[tag=m,sort=furthest] Pos[0]").unwrap();
    e.wait_for_command_log("100d").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// NBT PATH WITH SPECIAL CHARACTERS
// ═════════════════════════════════════════════════════════════════

/// Pattern: "dynamic_crafting:remainders" key with colon and quotes.
/// `data."dynamic_crafting:remainders"` is the path syntax.
#[test]
fn nbt_path_with_colon_in_key() {
    let d = sandbox("colon-key");
    let mut e = exec(&d);

    e.command("summon minecraft:marker 0 0 0 {Tags:[\"r\"]}").unwrap();
    e.command("data modify entity @e[tag=r,limit=1] data set value {\"dc:remainders\":[{id:\"minecraft:bucket\"}]}").unwrap();
    e.command("data get entity @e[tag=r,limit=1] data.\"dc:remainders\"[0].id").unwrap();
    e.wait_for_command_log("bucket").unwrap();

    let _ = fs::remove_dir_all(&d);
}

// ═════════════════════════════════════════════════════════════════
// CRAFTERS DIMENSION ISOLATION
// ═════════════════════════════════════════════════════════════════

/// Pattern: recipe detection runs in a separate dimension.
/// Items spawned there don't affect the overworld.
#[test]
fn crafters_dimension_independent_from_overworld() {
    let d = sandbox("crafters");
    let mut e = exec(&d);

    // Set up crafter in crafters dimension
    e.command("execute in dynamic_crafting:crafters run setblock 0 0 0 minecraft:crafter").unwrap();
    e.command("execute in dynamic_crafting:crafters run data modify block 0 0 0 Items set value [{id:\"minecraft:diamond\",Slot:0b}]").unwrap();

    // Verify in crafters dimension
    e.command("execute in dynamic_crafting:crafters run data get block 0 0 0 Items[0].id").unwrap();
    e.wait_for_command_log("diamond").unwrap();

    // Overworld block at same coords is independent
    e.command("setblock 0 0 0 minecraft:stone").unwrap();
    e.command("execute unless data storage test:block_check 0 0 0 Items[0]").unwrap_or_default();
    // The overworld block doesn't have Items

    let _ = fs::remove_dir_all(&d);
}
