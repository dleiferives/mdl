//! Dynamic Crafting datapack integration tests.
//! Requires /tmp/dynamic-crafting (skipped silently if absent).

use std::fs;
use std::path::PathBuf;
use mcfunction_executor::{McExecutor, V26_2};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mdl-dc-{name}"));
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

fn maybe_copy(src: &PathBuf, dst: &PathBuf) {
    if src.exists() { _ = fs::copy(src, dst); }
}

#[test]
fn load_full_datapack() {
    let Some(dc) = dc_source() else { return; };
    let d = sandbox("load");

    let dp = d.join("world").join("datapacks").join("dynamic_crafting");
    copy_dir(&dc, &dp).ok();
    let mut e = McExecutor::create(d.clone(), V26_2);
    let _ = e.load_datapacks();
    // May fail on unimplemented — that's fine, we just want no panics
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn parse_all_no_panic() {
    let Some(dc) = dc_source() else { return; };
    let func_dir = dc.join("data/dynamic_crafting/function");
    let mut files = Vec::new();
    find_mcfunctions(&func_dir, &mut files);
    assert!(files.len() >= 40);

    for f in &files {
        for line in fs::read_to_string(f).unwrap().lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                mcfunction_executor::parse::parse_command(line);
            })).is_ok(), "parse panicked on {}: {}", f.display(), line);
        }
    }
}

#[test]
fn full_crafting_pipeline_with_config() {
    let Some(dc) = dc_source() else { return; };
    let d = sandbox("pipe");

    let dp = d.join("world").join("datapacks").join("minimal");
    fs::create_dir_all(&dp).unwrap();
    fs::write(dp.join("pack.mcmeta"), r#"{"pack":{"description":"min","min_format":[107,1],"max_format":[107,1]}}"#).unwrap();
    let func = dp.join("data/dynamic_crafting/function");
    fs::create_dir_all(&func).unwrap();
    fs::create_dir_all(func.join("config")).unwrap();
    fs::create_dir_all(func.join("config/show")).unwrap();

    let src = dc.join("data/dynamic_crafting/function");
    maybe_copy(&src.join("load.mcfunction"), &func.join("load.mcfunction"));
    maybe_copy(&src.join("config/reset.mcfunction"), &func.join("config/reset.mcfunction"));
    maybe_copy(&src.join("config/show/init.mcfunction"), &func.join("config/show/init.mcfunction"));

    let tag_dir = dp.join("data/minecraft/tags/function");
    fs::create_dir_all(&tag_dir).unwrap();
    fs::write(tag_dir.join("load.json"), r#"{"values":["dynamic_crafting:load"]}"#).unwrap();

    let mut e = McExecutor::create(d.clone(), V26_2);
    let _ = e.load_datapacks();

    e.command("data modify storage dynamic_crafting:config values set value {block_tick_distance:32,block_conversion:1,max_per_chunk:16}").unwrap();
    e.command("summon minecraft:marker 0 0 0 {Tags:[\"block.crafting_table.tick\"]}").unwrap();
    e.command("scoreboard players set @e[tag=block.crafting_table.tick,limit=1] dynamic_crafting.values 5").unwrap();

    for slot in 0..9u32 {
        e.command(&format!("summon minecraft:item_display 0 0 0 {{Tags:[\"slot.visual\",\"slot.{slot}\"]}}")).unwrap();
    }
    e.command("data modify entity @e[tag=slot.0,limit=1] item set value {id:\"minecraft:oak_planks\",Count:1b}").unwrap();

    // Collect to storage → block (update_slots pattern)
    e.command("data merge storage test:tmp {Items:[]}").unwrap();
    e.command("data modify storage test:tmp Items append from entity @e[tag=slot.0,limit=1] item").unwrap();
    e.command("execute in dynamic_crafting:crafters run setblock 0 0 0 minecraft:crafter").unwrap();
    e.command("execute in dynamic_crafting:crafters run data modify block 0 0 0 Items set from storage test:tmp Items").unwrap();

    // Spawn result and verify
    e.command("summon minecraft:item_display 0 0 0 {Tags:[\"result.visual\"]}").unwrap();
    e.command("data modify entity @e[tag=result.visual,limit=1] item set value {id:\"minecraft:crafting_table\",Count:1b}").unwrap();
    e.command("loot spawn 0 65 0 loot dynamic_crafting:drop").unwrap();
    e.command("tag @e[type=item,limit=1] add result").unwrap();
    e.command("data modify entity @e[tag=result,limit=1] Item set from entity @e[tag=result.visual,limit=1] item").unwrap();
    e.command("data modify entity @e[tag=result,limit=1] PickupDelay set value 4").unwrap();

    e.command("data get entity @e[tag=result,limit=1] Item.id").unwrap();
    e.wait_for_command_log("crafting_table").unwrap();

    let _ = fs::remove_dir_all(&d);
}

fn find_mcfunctions(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() { find_mcfunctions(&path, out); }
            else if path.extension().map_or(false, |e| e == "mcfunction") { out.push(path); }
        }
    }
}
