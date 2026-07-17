use std::env;
use std::path::PathBuf;

use mdl_test::{ServerConfig, ServerSandbox, TestServer};

const PACK_META: &str = concat!(
    "{\n",
    "  \"pack\": {\n",
    "    \"description\": \"MDL Stage 7.5 spatial semantics probes\",\n",
    "    \"min_format\": [107, 1],\n",
    "    \"max_format\": [107, 1]\n",
    "  }\n",
    "}\n",
);

const PROBE_FUNCTION: &str = concat!(
    "summon minecraft:armor_stand 1 5 2 {Tags:[\"mdl75_as_source\"],Marker:1b,NoGravity:1b}\n",
    "summon minecraft:armor_stand 1 5 2 {Tags:[\"mdl75_at_source\"],Marker:1b,NoGravity:1b}\n",
    "setblock 103 24 105 minecraft:diamond_block\n",
    "setblock 4 9 7 minecraft:gold_block\n",
    "execute positioned 100 20 100 as @e[tag=mdl75_as_source,limit=1] positioned ~3 ~4 ~5 if block ~ ~ ~ minecraft:diamond_block run say MDL75_AS_RETAINS_POSITION\n",
    "execute positioned 100 20 100 as @e[tag=mdl75_at_source,limit=1] at @s positioned ~3 ~4 ~5 if block ~ ~ ~ minecraft:gold_block run say MDL75_AT_COPIES_POSITION\n",
    "summon minecraft:armor_stand 0 5 0 {Tags:[\"mdl75_local\"],Marker:1b,NoGravity:1b}\n",
    "execute positioned 10 10 10 rotated 0 0 positioned ^ ^ ^2 run teleport @e[tag=mdl75_local,limit=1] ~ ~ ~\n",
    "execute positioned 10 10 12 if entity @e[tag=mdl75_local,distance=..0.01] run say MDL75_LOCAL_USES_ROTATION\n",
    "setblock -2 5 2 minecraft:emerald_block\n",
    "execute positioned -1.2 5.8 2.9 align xyz if block ~ ~ ~ minecraft:emerald_block run say MDL75_ALIGN_FLOORS\n",
    "summon minecraft:armor_stand 0 5 0 {Tags:[\"mdl75_rotation\"],Marker:1b,NoGravity:1b}\n",
    "execute rotated 30 10 rotated ~20 ~-5 as @e[tag=mdl75_rotation,limit=1] run teleport @s ~ ~ ~ ~ ~\n",
    "execute if entity @e[tag=mdl75_rotation,y_rotation=49.9..50.1,x_rotation=4.9..5.1] run say MDL75_RELATIVE_ROTATION\n",
    "summon minecraft:armor_stand 0 5 0 {Tags:[\"mdl75_anchor\"],NoGravity:1b}\n",
    "setblock 0 5 0 minecraft:gold_block\n",
    "setblock 0 6 0 minecraft:diamond_block\n",
    "execute as @e[tag=mdl75_anchor,limit=1] at @s anchored feet positioned ^ ^ ^ if block ~ ~ ~ minecraft:gold_block run say MDL75_ANCHOR_FEET\n",
    "execute as @e[tag=mdl75_anchor,limit=1] at @s anchored eyes positioned ^ ^ ^ if block ~ ~ ~ minecraft:diamond_block run say MDL75_ANCHOR_EYES\n",
    "execute in minecraft:the_nether run setblock 10 10 10 minecraft:diamond_block\n",
    "execute in minecraft:the_nether run setblock 80 10 80 minecraft:gold_block\n",
    "execute positioned 80 10 80 in minecraft:the_nether if block ~ ~ ~ minecraft:diamond_block run say MDL75_IN_SCALES_CURRENT_POSITION\n",
    "execute positioned 80 10 80 in minecraft:the_nether if block ~ ~ ~ minecraft:gold_block run say MDL75_IN_PRESERVES_CURRENT_POSITION\n",
    "execute in minecraft:the_nether positioned 80 10 80 if block ~ ~ ~ minecraft:gold_block run say MDL75_MODIFIER_ORDER\n",
    "summon minecraft:armor_stand 0 5 0 {Tags:[\"mdl75_tp_result\"],Marker:1b,NoGravity:1b}\n",
    "execute store success score #tp_success mdl75.probe store result score #tp_result mdl75.probe run teleport @e[tag=mdl75_tp_result,limit=1] 2 5 2\n",
    "execute if score #tp_success mdl75.probe matches 1 if score #tp_result mdl75.probe matches 1 run say MDL75_TELEPORT_NATIVE_SUCCESS_RESULT\n",
    "execute store success score #empty_success mdl75.probe store result score #empty_result mdl75.probe run teleport @e[tag=mdl75_absent] 2 5 2\n",
    "execute if score #empty_success mdl75.probe matches 0 if score #empty_result mdl75.probe matches 0 run say MDL75_TELEPORT_EMPTY_RESULT\n",
    "summon minecraft:armor_stand 0 5 0 {Tags:[\"mdl75_cross\"],Marker:1b,NoGravity:1b}\n",
    "execute as @e[tag=mdl75_cross,limit=1] in minecraft:the_nether positioned 3 5 3 run teleport @s ~ ~ ~\n",
    "execute in minecraft:the_nether positioned 3 5 3 if entity @e[tag=mdl75_cross,distance=..0.01] run say MDL75_TELEPORT_CROSS_DIMENSION\n",
    "summon minecraft:armor_stand 0 5 0 {Tags:[\"mdl75_frame_immutable\"],Marker:1b,NoGravity:1b}\n",
    "execute positioned 10 5 10 run function mdl75:teleport_frame\n",
    "say MDL75_PROBE_COMPLETE\n",
);

const TELEPORT_FRAME_FUNCTION: &str = concat!(
    "teleport @e[tag=mdl75_frame_immutable,limit=1] ~ ~1 ~\n",
    "execute positioned ~1 ~ ~ if entity @e[tag=mdl75_frame_immutable,x=9.99,y=5.99,z=9.99,dx=0.02,dy=0.02,dz=0.02] run say MDL75_TELEPORT_FRAME_IMMUTABLE\n",
);

const EXPECTED_MARKERS: [&str; 13] = [
    "MDL75_AS_RETAINS_POSITION",
    "MDL75_AT_COPIES_POSITION",
    "MDL75_LOCAL_USES_ROTATION",
    "MDL75_ALIGN_FLOORS",
    "MDL75_RELATIVE_ROTATION",
    "MDL75_ANCHOR_FEET",
    "MDL75_ANCHOR_EYES",
    "MDL75_IN_SCALES_CURRENT_POSITION",
    "MDL75_MODIFIER_ORDER",
    "MDL75_TELEPORT_NATIVE_SUCCESS_RESULT",
    "MDL75_TELEPORT_EMPTY_RESULT",
    "MDL75_TELEPORT_CROSS_DIMENSION",
    "MDL75_TELEPORT_FRAME_IMMUTABLE",
];

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn java_26_2_spatial_commands_match_stage7_5_contracts() {
    if let Err(error) = run_probes() {
        panic!("{error}");
    }
}

fn run_probes() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let config = ServerConfig::new(java, server_jar);
    let preserve_success = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve_success).map_err(|error| error.to_string())?;
    sandbox
        .install_datapack(
            "mdl_stage75_spatial",
            [
                ("pack.mcmeta", PACK_META),
                ("data/mdl75/function/probe.mcfunction", PROBE_FUNCTION),
                (
                    "data/mdl75/function/teleport_frame.mcfunction",
                    TELEPORT_FRAME_FUNCTION,
                ),
            ],
        )
        .map_err(|error| error.to_string())?;

    let mut server = sandbox.start(&config).map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    if let Err(error) = exercise(&mut server) {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 7.5 spatial probe sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn exercise(server: &mut TestServer) -> Result<(), String> {
    server
        .command("scoreboard objectives add mdl75.probe dummy")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Created new objective")
        .map_err(|error| error.to_string())?;
    server
        .command("execute in minecraft:overworld run forceload add -16 -16 127 127")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("force loaded")
        .map_err(|error| error.to_string())?;
    server
        .command("execute in minecraft:the_nether run forceload add -16 -16 127 127")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("force loaded")
        .map_err(|error| error.to_string())?;

    let checkpoint = server.log_checkpoint();
    server
        .command("execute in minecraft:overworld positioned 0 5 0 run function mdl75:probe")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("MDL75_PROBE_COMPLETE")
        .map_err(|error| error.to_string())?;
    for marker in EXPECTED_MARKERS {
        let lines = server
            .matching_log_lines_since(checkpoint, marker)
            .map_err(|error| error.to_string())?;
        if lines.len() != 1 {
            return Err(format!(
                "Java 26.2 spatial probe {marker} appeared {} times instead of once: {lines:?}",
                lines.len()
            ));
        }
    }
    let contradictory = server
        .matching_log_lines_since(checkpoint, "MDL75_IN_PRESERVES_CURRENT_POSITION")
        .map_err(|error| error.to_string())?;
    if !contradictory.is_empty() {
        return Err(format!(
            "Java 26.2 execute-in probe both scaled and preserved the position: {contradictory:?}"
        ));
    }
    Ok(())
}
