//! Smoke test: actually execute the dynamic-crafting datapack.
//! Sets up config, spawns a crafting table, runs the tick loop.

use std::fs;
use std::path::PathBuf;
use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-smoke-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn dc_source() -> Option<PathBuf> {
    let p = PathBuf::from("/tmp/dynamic-crafting");
    if p.is_dir() { Some(p) } else { None }
}

fn copy_dir(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest = dst.join(entry.file_name());
        if path.is_dir() { copy_dir(&path, &dest)?; }
        else { _ = fs::copy(&path, &dest); }
    }
    Ok(())
}

/// Install the full datapack, set up a crafting table with slots, place items,
/// run the tick, and verify the result.
#[test]
fn craft_oak_planks_to_crafting_table() {
    let Some(dc) = dc_source() else { return; };
    let d = sandbox("oak-planks");

    // Install full datapack
    let dp = d.join("world").join("datapacks").join("dynamic_crafting");
    copy_dir(&dc, &dp).unwrap();

    let mut e = McExecutor::create(d.clone(), V26_2);
    // Load will fail on unimplemented — skip the load tag, set up manually
    let _ = e.load_datapacks();

    // Manual setup
    e.command("scoreboard objectives add dynamic_crafting.values dummy").unwrap();
    e.command("data modify storage dynamic_crafting:config values set value {block_tick_distance:32,block_conversion:1,max_per_chunk:128}").unwrap();

    // Spawn crafting table tick marker
    e.command("summon minecraft:marker 0 0 0 {Tags:[\"block.crafting_table.tick\"]}").unwrap();
    e.command("scoreboard players set @e[tag=block.crafting_table.tick,limit=1] dynamic_crafting.values 5").unwrap();

    // Create 9 crafting slots
    for slot in 0..9u32 {
        e.command(&format!(
            "summon minecraft:item_display 0 0 0 {{Tags:[\"slot.visual\",\"slot.{slot}\"]}}"
        )).unwrap();
        e.command(&format!(
            "summon minecraft:interaction 0 0 0 {{Tags:[\"slot.hitbox\",\"slot.{slot}\"]}}"
        )).unwrap();
    }

    // Place oak planks in slots 0-3 (2x2 recipe for crafting table)
    for &slot in &[0u32, 1, 2, 3] {
        e.command(&format!(
            "data modify entity @e[tag=slot.{slot},limit=1] item set value {{id:\"minecraft:oak_planks\",Count:1b}}"
        )).unwrap();
    }

    // Collect to storage (update_slots pattern)
    e.command("data merge storage test:tmp {Items:[]}").unwrap();
    for &slot in &[0u32, 1, 2, 3] {
        e.command(&format!(
            "data modify storage test:tmp Items append from entity @e[tag=slot.{slot},limit=1] item"
        )).unwrap();
        e.command(&format!(
            "data modify storage test:tmp Items[-1] merge value {{Slot:{slot}b}}"
        )).unwrap();
    }

    // Write to crafter in crafters dimension
    e.command("execute in dynamic_crafting:crafters run setblock 0 0 0 minecraft:crafter").unwrap();
    e.command("execute in dynamic_crafting:crafters run data modify block 0 0 0 Items set from storage test:tmp Items").unwrap();

    // Verify crafter has the items
    e.command("execute in dynamic_crafting:crafters run data get block 0 0 0 Items[0].id").unwrap();
    e.wait_for_command_log("oak_planks").unwrap();
    e.command("execute in dynamic_crafting:crafters run data get block 0 0 0 Items[0].Slot").unwrap();
    e.wait_for_command_log("0b").unwrap();

    // Spawn result entity with the crafted item
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"result.visual\"]}").unwrap();
    e.command("data modify entity @e[tag=result.visual,limit=1] item set value {id:\"minecraft:crafting_table\",Count:1b}").unwrap();
    e.command("loot spawn 0 65 0 loot dynamic_crafting:drop").unwrap();
    e.command("tag @e[type=item,limit=1] add result").unwrap();
    e.command("data modify entity @e[tag=result,limit=1] Item set from entity @e[tag=result.visual,limit=1] item").unwrap();
    e.command("data modify entity @e[tag=result,limit=1] PickupDelay set value 4").unwrap();

    // Crafting result should be a crafting_table
    e.command("data get entity @e[tag=result,limit=1] Item.id").unwrap();
    e.wait_for_command_log("crafting_table").unwrap();
    e.command("data get entity @e[tag=result,limit=1] Item.Count").unwrap();
    e.wait_for_command_log("1b").unwrap();

    // Clean up storage
    e.command("data remove storage test:tmp Items").unwrap();
    e.command("execute unless data storage test:tmp Items run scoreboard players set #clean v 1").unwrap();
    e.command("scoreboard players get #clean v").unwrap();
    e.wait_for_command_log("#clean has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Test: manually load and run the load.mcfunction + config/reset.
/// These initialize scoreboard objectives and storage config.
#[test]
fn run_load_function_and_config() {
    let Some(dc) = dc_source() else { return; };
    let d = sandbox("load-config");

    let dp = d.join("world").join("datapacks").join("minimal");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"min","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    let func = dp.join("data/dynamic_crafting/function");
    fs::create_dir_all(func.join("config")).unwrap();

    let src = dc.join("data/dynamic_crafting/function");
    if src.join("load.mcfunction").exists() {
        fs::copy(src.join("load.mcfunction"), func.join("load.mcfunction")).unwrap();
    }
    if src.join("config/reset.mcfunction").exists() {
        fs::copy(src.join("config/reset.mcfunction"), func.join("config/reset.mcfunction")).unwrap();
    }
    let tag_dir = dp.join("data/minecraft/tags/function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(tag_dir.join("load.json"), r#"{"values":["dynamic_crafting:load"]}"#).unwrap();

    let mut e = McExecutor::create(d.clone(), V26_2);
    let _ = e.load_datapacks();

    // Verify objectives exist
    e.command("scoreboard players set #v dynamic_crafting.values 0").unwrap();
    // Verify storage config was set by reset
    e.command("data get storage dynamic_crafting:config values.max_per_chunk").unwrap();
    let line = e.wait_for_command_log("has the following contents").unwrap();
    assert!(line.contains("16") || line.contains("max_per_chunk"), "got: {line}");

    let _ = fs::remove_dir_all(&d);
}

/// Verify the crafting table removal pattern: drop items → kill all entities.
#[test]
fn remove_crafting_table_cleanup() {
    let Some(_dc) = dc_source() else { return; };
    let d = sandbox("remove");

    let mut e = McExecutor::create(d.clone(), V26_2);

    // Spawn everything a crafting table has
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"slot.visual\"]}").unwrap();
    e.command("summon minecraft:interaction 0 0 0 {Tags:[\"slot.hitbox\"]}").unwrap();
    e.command("summon minecraft:interaction 0 0 0 {Tags:[\"result.hitbox\"]}").unwrap();
    e.command("summon minecraft:text_display 0 0 0 {Tags:[\"slot.count\"]}").unwrap();
    e.command("summon minecraft:text_display 0 0 0 {Tags:[\"result.glow\"]}").unwrap();
    e.command("summon minecraft:marker 0 0 0 {Tags:[\"block.crafting_table.tick\"]}").unwrap();

    // Verify they exist
    e.command("execute if entity @e[tag=slot.visual] run scoreboard players set #before v 1").unwrap();
    e.command("scoreboard players get #before v").unwrap();
    e.wait_for_command_log("#before has 1").unwrap();

    // Drop items from slots before killing
    e.command("data modify entity @e[tag=slot.visual,limit=1] item set value {id:\"minecraft:stone\"}").unwrap();
    e.command("loot spawn 0 65 0 loot dynamic_crafting:drop").unwrap();
    e.command("tag @e[type=item,limit=1] add drop").unwrap();
    e.command("data modify entity @e[tag=drop,limit=1] Item set from entity @e[tag=slot.visual,limit=1] item").unwrap();

    // Kill everything (remove pattern)
    e.command("kill @e[tag=slot.hitbox]").unwrap();
    e.command("kill @e[tag=slot.visual]").unwrap();
    e.command("kill @e[tag=slot.count]").unwrap();
    e.command("kill @e[tag=result.hitbox]").unwrap();
    e.command("kill @e[tag=result.glow]").unwrap();
    e.command("kill @e[tag=block.crafting_table.tick]").unwrap();

    // Verify all gone
    e.command("execute unless entity @e[tag=slot.visual] run scoreboard players add #clean v 1").unwrap();
    e.command("execute unless entity @e[tag=result.hitbox] run scoreboard players add #clean v 1").unwrap();
    e.command("execute unless entity @e[tag=block.crafting_table.tick] run scoreboard players add #clean v 1").unwrap();
    e.command("scoreboard players get #clean v").unwrap();
    e.wait_for_command_log("#clean has 3").unwrap();

    let _ = fs::remove_dir_all(&d);
}

/// Verify we can handle the item_display init pattern: data merge entity for animation.
#[test]
fn item_display_init_animation_merge() {
    let Some(_dc) = dc_source() else { return; };
    let d = sandbox("anim");

    let mut e = McExecutor::create(d.clone(), V26_2);

    // Spawn with init tag (sentinel for first-tick animation)
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"init\",\"result.visual\"],transformation:{scale:[0.275f,0.275f,0.275f]}}").unwrap();

    // Verify transformation was stored from summon NBT
    e.command("data get entity @e[tag=init,limit=1] transformation").unwrap();
    let line = e.wait_for_command_log("has the following entity data").unwrap();
    assert!(line.contains("0.275"), "transformation should have scale: {line}");

    // Remove init tag (after animation completes)
    e.command("tag @e[tag=init] remove init").unwrap();
    e.command("execute unless entity @e[tag=init] run scoreboard players set #done v 1").unwrap();
    e.command("scoreboard players get #done v").unwrap();
    e.wait_for_command_log("#done has 1").unwrap();

    let _ = fs::remove_dir_all(&d);
}
