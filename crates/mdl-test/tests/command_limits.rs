use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;

use mdl_test::{ServerConfig, ServerSandbox, TestServer};

const OBJECTIVE: &str = "mdl_limit";
const ENTITY_SELECTOR: &str = "@e[type=minecraft:armor_stand,tag=mdl_limit_test]";
const PACK_META: &str = concat!(
    "{\n",
    "  \"pack\": {\n",
    "    \"description\": \"MDL command-limit boundary tests\",\n",
    "    \"min_format\": [107, 1],\n",
    "    \"max_format\": [107, 1]\n",
    "  }\n",
    "}\n",
);
const SPAWN_FUNCTION: &str = concat!(
    "summon minecraft:armor_stand 0 80 0 {Tags:[\"mdl_limit_test\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "summon minecraft:armor_stand 1 80 0 {Tags:[\"mdl_limit_test\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "summon minecraft:armor_stand 2 80 0 {Tags:[\"mdl_limit_test\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "summon minecraft:armor_stand 3 80 0 {Tags:[\"mdl_limit_test\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "summon minecraft:armor_stand 4 80 0 {Tags:[\"mdl_limit_test\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "schedule function mdl_limit:await_entities 1t replace\n",
);
const AWAIT_ENTITIES_FUNCTION: &str = concat!(
    "execute store result score #population mdl_limit run execute if entity @e[type=minecraft:armor_stand,tag=mdl_limit_test]\n",
    "execute if score #population mdl_limit matches 5 run say MDL_LIMIT_ENTITIES_READY\n",
    "execute unless score #population mdl_limit matches 5 run schedule function mdl_limit:await_entities 1t replace\n",
);
const LEAF_FUNCTION: &str = "scoreboard players add #seq_leaf mdl_limit 1\n";
const MULTIPLICITY_WORKER_FUNCTION: &str = "scoreboard players add #function_body mdl_limit 1\n";
const TRUE_CONDITION_FUNCTION: &str = "return 1\n";

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn command_limit_boundaries_match_vanilla_26_2() {
    if let Err(error) = run_boundaries() {
        panic!("{error}");
    }
}

fn run_boundaries() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let config = ServerConfig::new(java, server_jar);
    let preserve_success = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve_success).map_err(|error| error.to_string())?;
    install_pack(&sandbox)?;

    let mut server = sandbox.start(&config).map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    if let Err(error) = exercise_boundaries(&mut server) {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\ncommand-limit sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn install_pack(sandbox: &ServerSandbox) -> Result<(), String> {
    sandbox
        .install_datapack(
            "mdl_command_limits",
            [
                ("pack.mcmeta", PACK_META),
                ("data/mdl_limit/function/spawn.mcfunction", SPAWN_FUNCTION),
                (
                    "data/mdl_limit/function/await_entities.mcfunction",
                    AWAIT_ENTITIES_FUNCTION,
                ),
                ("data/mdl_limit/function/leaf.mcfunction", LEAF_FUNCTION),
                (
                    "data/mdl_limit/function/multiplicity_worker.mcfunction",
                    MULTIPLICITY_WORKER_FUNCTION,
                ),
                (
                    "data/mdl_limit/function/condition_true.mcfunction",
                    TRUE_CONDITION_FUNCTION,
                ),
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn exercise_boundaries(server: &mut TestServer) -> Result<(), String> {
    command(
        server,
        &format!("scoreboard objectives add {OBJECTIVE} dummy"),
    )?;
    server
        .wait_for_command_log("Created new objective")
        .map_err(|error| error.to_string())?;
    prepare_entities(server)?;

    check_function_call_multiplicity(server)?;
    check_fork_four_boundary(server)?;
    check_sequence_ten_boundary(server)?;
    check_sequence_zero_boundary(server)?;
    check_low_fork_boundaries(server)?;

    set_gamerule(server, "minecraft:max_command_sequence_length", 65_536)?;
    set_gamerule(server, "minecraft:max_command_forks", 65_536)?;
    command(server, &format!("kill {ENTITY_SELECTOR}"))?;
    server
        .wait_for_command_log("Killed 5 entities")
        .map_err(|error| error.to_string())?;
    command(server, "forceload remove 0 0")?;
    server
        .wait_for_command_log("Unmarked chunk [0, 0]")
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn check_function_call_multiplicity(server: &mut TestServer) -> Result<(), String> {
    set_score(server, "#function_body", 0)?;
    command(
        server,
        &format!("execute as {ENTITY_SELECTOR} run function mdl_limit:multiplicity_worker"),
    )?;
    expect_score(server, "#function_body", 5)
}

fn prepare_entities(server: &mut TestServer) -> Result<(), String> {
    command(server, "forceload add 0 0")?;
    server
        .wait_for_command_log("Marked chunk [0, 0]")
        .map_err(|error| error.to_string())?;
    command(server, "function mdl_limit:spawn")?;
    server
        .wait_for_command_log("MDL_LIMIT_ENTITIES_READY")
        .map_err(|error| error.to_string())?;
    expect_score(server, "#population", 5)
}

fn check_fork_four_boundary(server: &mut TestServer) -> Result<(), String> {
    set_gamerule(server, "minecraft:max_command_forks", 4)?;
    for holder in ["#fork3", "#fork4", "#fork5"] {
        set_score(server, holder, 0)?;
    }
    for (contexts, holder) in [(3, "#fork3"), (4, "#fork4"), (5, "#fork5")] {
        command(
            server,
            &format!(
                "execute as @e[type=minecraft:armor_stand,tag=mdl_limit_test,limit={contexts}] run scoreboard players add {holder} {OBJECTIVE} 1"
            ),
        )?;
    }
    expect_score(server, "#fork3", 3)?;
    expect_score(server, "#fork4", 0)?;
    expect_score(server, "#fork5", 0)
}

fn check_sequence_ten_boundary(server: &mut TestServer) -> Result<(), String> {
    set_gamerule(server, "minecraft:max_command_sequence_length", 10)?;
    set_score(server, "#seq10", 0)?;
    set_score(server, "#seq11", 0)?;

    // Each `in` modifier and the final body consume one sequence operation.
    command(server, &execute_chain(9, "#seq10"))?;
    command(server, &execute_chain(10, "#seq11"))?;
    expect_score(server, "#seq10", 1)?;
    expect_score(server, "#seq11", 0)
}

fn check_sequence_zero_boundary(server: &mut TestServer) -> Result<(), String> {
    set_gamerule(server, "minecraft:max_command_sequence_length", 65_536)?;
    set_score(server, "#seq0_direct", 0)?;
    set_score(server, "#seq0_two", 0)?;
    set_score(server, "#seq_leaf", 0)?;
    set_gamerule(server, "minecraft:max_command_sequence_length", 0)?;

    command(server, "scoreboard players add #seq0_direct mdl_limit 1")?;
    command(server, &execute_chain(1, "#seq0_two"))?;
    command(server, "function mdl_limit:leaf")?;
    expect_score(server, "#seq0_direct", 1)?;
    expect_score(server, "#seq0_two", 0)?;
    expect_score(server, "#seq_leaf", 0)
}

fn check_low_fork_boundaries(server: &mut TestServer) -> Result<(), String> {
    set_gamerule(server, "minecraft:max_command_sequence_length", 65_536)?;
    set_score(server, "#fork_condition", 1)?;
    check_low_fork_modifier_boundary(server, 0)?;
    check_low_fork_modifier_boundary(server, 1)
}

fn check_low_fork_modifier_boundary(server: &mut TestServer, limit: u32) -> Result<(), String> {
    let expectations = [
        ("direct", 1),
        ("zero", 0),
        ("one", 0),
        ("in", 0),
        ("store", 0),
        ("if", 0),
        // The custom function-condition modifier bypasses BuildContexts' fork check.
        ("if_function", 1),
    ];
    for (suffix, _) in expectations {
        set_score(server, &fork_holder(limit, suffix), 0)?;
    }
    set_gamerule(server, "minecraft:max_command_forks", limit)?;

    command(
        server,
        &format!(
            "scoreboard players add {} {OBJECTIVE} 1",
            fork_holder(limit, "direct")
        ),
    )?;
    command(
        server,
        &format!(
            "execute as @e[type=minecraft:armor_stand,tag=mdl_limit_absent] run scoreboard players add {} {OBJECTIVE} 1",
            fork_holder(limit, "zero")
        ),
    )?;
    command(
        server,
        &format!(
            "execute as @e[type=minecraft:armor_stand,tag=mdl_limit_test,limit=1] run scoreboard players add {} {OBJECTIVE} 1",
            fork_holder(limit, "one")
        ),
    )?;
    command(
        server,
        &format!(
            "execute in minecraft:overworld run scoreboard players add {} {OBJECTIVE} 1",
            fork_holder(limit, "in")
        ),
    )?;
    command(
        server,
        &format!(
            "execute store result score #fork_stored {OBJECTIVE} run scoreboard players add {} {OBJECTIVE} 1",
            fork_holder(limit, "store")
        ),
    )?;
    command(
        server,
        &format!(
            "execute if score #fork_condition {OBJECTIVE} matches 1 run scoreboard players add {} {OBJECTIVE} 1",
            fork_holder(limit, "if")
        ),
    )?;
    command(
        server,
        &format!(
            "execute if function mdl_limit:condition_true run scoreboard players add {} {OBJECTIVE} 1",
            fork_holder(limit, "if_function")
        ),
    )?;

    for (suffix, expected) in expectations {
        expect_score(server, &fork_holder(limit, suffix), expected)?;
    }
    Ok(())
}

fn fork_holder(limit: u32, suffix: &str) -> String {
    format!("#fork{limit}_{suffix}")
}

fn execute_chain(stages: usize, holder: &str) -> String {
    let mut command = String::from("execute");
    for _ in 0..stages {
        command.push_str(" in minecraft:overworld");
    }
    write!(
        command,
        " run scoreboard players add {holder} {OBJECTIVE} 1"
    )
    .expect("writing to a String cannot fail");
    command
}

fn set_gamerule(server: &mut TestServer, name: &str, value: u32) -> Result<(), String> {
    command(server, &format!("gamerule {name} {value}"))?;
    let canonical_name = name.rsplit_once(':').map_or(name, |(_, name)| name);
    server
        .wait_for_command_log(&format!("Gamerule {canonical_name} is now set to: {value}"))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn set_score(server: &mut TestServer, holder: &str, value: i32) -> Result<(), String> {
    command(
        server,
        &format!("scoreboard players set {holder} {OBJECTIVE} {value}"),
    )
}

fn expect_score(server: &mut TestServer, holder: &str, expected: i32) -> Result<(), String> {
    command(
        server,
        &format!("scoreboard players get {holder} {OBJECTIVE}"),
    )?;
    let line = server
        .wait_for_command_log(&format!("{holder} has"))
        .map_err(|error| error.to_string())?;
    let expected_text = format!("{holder} has {expected} [{OBJECTIVE}]");
    if line.contains(&expected_text) {
        Ok(())
    } else {
        Err(format!("expected {expected_text:?}, got {line:?}"))
    }
}

fn command(server: &mut TestServer, text: &str) -> Result<(), String> {
    server.command(text).map_err(|error| error.to_string())
}
