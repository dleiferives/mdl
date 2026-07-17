use std::env;
use std::path::PathBuf;

use mdl_test::{ServerConfig, ServerSandbox, TestServer, sha256_file};

const JAVA_26_2_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const JAVA_26_2_EXTRACTED_SERVER_SHA256: &str =
    "183c0499c5f855570ee487dd38e141a53f0121f83a0b07a3bac2d8b6698823e8";
const OBJECTIVE: &str = "mdl8act";
const ENTITIES: &str = "@e[type=minecraft:armor_stand,tag=mdl8_activation]";
const PACK_META: &str = concat!(
    "{\n",
    "  \"pack\": {\n",
    "    \"description\": \"MDL Stage 8 activation evidence\",\n",
    "    \"min_format\": [107, 1],\n",
    "    \"max_format\": [107, 1]\n",
    "  }\n",
    "}\n",
);

const SPAWN: &str = concat!(
    "summon minecraft:armor_stand 0 80 0 {Tags:[\"mdl8_activation\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "summon minecraft:armor_stand 1 80 0 {Tags:[\"mdl8_activation\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "summon minecraft:armor_stand 2 80 0 {Tags:[\"mdl8_activation\"],Invisible:1b,Marker:1b,NoGravity:1b}\n",
    "schedule function mdl8_activation:await_entities 1t replace\n",
);
const AWAIT_ENTITIES: &str = concat!(
    "execute store result score #population mdl8act run execute if entity @e[type=minecraft:armor_stand,tag=mdl8_activation]\n",
    "execute if score #population mdl8act matches 3 run say MDL8_ENTITIES_READY\n",
    "execute unless score #population mdl8act matches 3 run schedule function mdl8_activation:await_entities 1t replace\n",
);

// Every child must finish `nested` before the next child starts. The shared state
// therefore advances by eleven between successive child observations, while each
// entity retains the exact before/nested/after values from its own activation.
const SERIAL_ROOT: &str = concat!(
    "scoreboard players set #shared mdl8act 100\n",
    "scoreboard players set #entered mdl8act 0\n",
    "execute as @e[type=minecraft:armor_stand,tag=mdl8_activation] run function mdl8_activation:serial_worker\n",
    "execute if score #shared mdl8act matches 133 if score #entered mdl8act matches 3 run say MDL8_SERIAL_COMPLETE\n",
);
const SERIAL_WORKER: &str = concat!(
    "scoreboard players operation @s mdl8act = #shared mdl8act\n",
    "scoreboard players add #entered mdl8act 1\n",
    "scoreboard players add #shared mdl8act 1\n",
    "function mdl8_activation:nested\n",
    "scoreboard players operation @s mdl8after = #shared mdl8act\n",
);
const NESTED: &str = concat!(
    "scoreboard players operation @s mdl8nest = #shared mdl8act\n",
    "scoreboard players add #shared mdl8act 10\n",
);

const CONTEXT_MATRIX_ROOT: &str = concat!(
    "scoreboard players set #zero mdl8act 0\n",
    "scoreboard players set #one mdl8act 0\n",
    "scoreboard players set #nested_forks mdl8act 0\n",
    "execute as @e[type=minecraft:armor_stand,tag=mdl8_absent] run function mdl8_activation:zero_worker\n",
    "execute as @e[type=minecraft:armor_stand,tag=mdl8_activation,limit=1] run function mdl8_activation:one_worker\n",
    "execute as @e[type=minecraft:armor_stand,tag=mdl8_activation] run execute as @e[type=minecraft:armor_stand,tag=mdl8_activation] run function mdl8_activation:nested_fork_worker\n",
    "execute if score #zero mdl8act matches 0 if score #one mdl8act matches 1 if score #nested_forks mdl8act matches 9 run say MDL8_CONTEXT_MATRIX\n",
);
const ZERO_WORKER: &str = "scoreboard players add #zero mdl8act 1\n";
const ONE_WORKER: &str = "scoreboard players add #one mdl8act 1\n";
const NESTED_FORK_WORKER: &str = "scoreboard players add #nested_forks mdl8act 1\n";

const COMPLETION_ROOT: &str = concat!(
    "scoreboard players set #returned mdl8act 0\n",
    "scoreboard players set #continued mdl8act 0\n",
    "execute as @e[type=minecraft:armor_stand,tag=mdl8_activation] run function mdl8_activation:return_worker\n",
    "execute as @e[type=minecraft:armor_stand,tag=mdl8_activation] run function mdl8_activation:failure_worker\n",
    "execute if score #returned mdl8act matches 3 if score #continued mdl8act matches 3 run say MDL8_COMPLETION_MATRIX\n",
);
const RETURN_WORKER: &str = concat!(
    "scoreboard players add #returned mdl8act 1\n",
    "return 7\n",
    "scoreboard players add #returned mdl8act 100\n",
);
const FAILURE_WORKER: &str = concat!(
    "execute if entity @e[type=minecraft:armor_stand,tag=mdl8_absent] run scoreboard players add #continued mdl8act 100\n",
    "scoreboard players add #continued mdl8act 1\n",
);

// This prototype exercises the exact tail operations proposed for recursive SCC
// frames. The child reads its own `[-1]` argument and the caller's `[-2]` spill,
// writes its result, and is removed only after the caller has consumed it.
const FRAME_ROOT: &str = concat!(
    "data modify storage mdl8_activation:runtime frames set value []\n",
    "data modify storage mdl8_activation:runtime frames append value {argument:7,result:0,spill:41}\n",
    "function mdl8_activation:frame_parent\n",
    "execute if data storage mdl8_activation:runtime {frames:[{argument:7,result:50,spill:41}]} run say MDL8_FRAME_COMPLETE\n",
    "data remove storage mdl8_activation:runtime frames[-1]\n",
    "execute unless data storage mdl8_activation:runtime frames[0] run say MDL8_FRAME_EMPTY\n",
);
const FRAME_PARENT: &str = concat!(
    "data modify storage mdl8_activation:runtime frames append value {argument:9,result:0,spill:0}\n",
    "data modify storage mdl8_activation:runtime frames[-1].spill set from storage mdl8_activation:runtime frames[-2].spill\n",
    "function mdl8_activation:frame_child\n",
    "data modify storage mdl8_activation:runtime frames[-2].result set from storage mdl8_activation:runtime frames[-1].result\n",
    "data remove storage mdl8_activation:runtime frames[-1]\n",
);
const FRAME_CHILD: &str = concat!(
    "execute store result score #argument mdl8act run data get storage mdl8_activation:runtime frames[-1].argument 1\n",
    "execute store result score #spill mdl8act run data get storage mdl8_activation:runtime frames[-1].spill 1\n",
    "scoreboard players operation #argument mdl8act += #spill mdl8act\n",
    "execute store result storage mdl8_activation:runtime frames[-1].result int 1 run scoreboard players get #argument mdl8act\n",
);

// The low sequence limit aborts the entire root after the frame append and before
// the normal pop. This pins the non-transactional residue contract used by the
// compiler's explicit load/recovery entry.
const ABORT_ROOT: &str = concat!(
    "data modify storage mdl8_activation:runtime frames set value []\n",
    "data modify storage mdl8_activation:runtime frames append value {poison:1b}\n",
    "function mdl8_activation:abort_inner\n",
    "data remove storage mdl8_activation:runtime frames[-1]\n",
);
const ABORT_INNER: &str = concat!(
    "scoreboard players add #abort mdl8act 1\n",
    "scoreboard players add #abort mdl8act 1\n",
    "scoreboard players add #abort mdl8act 1\n",
    "scoreboard players add #abort mdl8act 1\n",
);
const FRAME_RESET: &str = "data modify storage mdl8_activation:runtime frames set value []\n";

#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn stage8_activation_and_tail_frame_contract_matches_vanilla_26_2() {
    if let Err(error) = run_test() {
        panic!("{error}");
    }
}

fn run_test() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let actual_hash = sha256_file(&server_jar).map_err(|error| error.to_string())?;
    if ![JAVA_26_2_SERVER_SHA256, JAVA_26_2_EXTRACTED_SERVER_SHA256].contains(&actual_hash.as_str())
    {
        return Err(format!(
            "expected the pinned Java 26.2 bundle or extracted-server SHA-256, got {actual_hash}"
        ));
    }
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let preserve = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve).map_err(|error| error.to_string())?;
    install_pack(&sandbox)?;
    let mut server = sandbox
        .start(&ServerConfig::new(java, server_jar))
        .map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    if let Err(error) = exercise(&mut server) {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 8 activation sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn install_pack(sandbox: &ServerSandbox) -> Result<(), String> {
    sandbox
        .install_datapack(
            "mdl_stage8_activation",
            [
                ("pack.mcmeta", PACK_META),
                ("data/mdl8_activation/function/spawn.mcfunction", SPAWN),
                (
                    "data/mdl8_activation/function/await_entities.mcfunction",
                    AWAIT_ENTITIES,
                ),
                (
                    "data/mdl8_activation/function/serial_root.mcfunction",
                    SERIAL_ROOT,
                ),
                (
                    "data/mdl8_activation/function/serial_worker.mcfunction",
                    SERIAL_WORKER,
                ),
                ("data/mdl8_activation/function/nested.mcfunction", NESTED),
                (
                    "data/mdl8_activation/function/context_matrix_root.mcfunction",
                    CONTEXT_MATRIX_ROOT,
                ),
                (
                    "data/mdl8_activation/function/zero_worker.mcfunction",
                    ZERO_WORKER,
                ),
                (
                    "data/mdl8_activation/function/one_worker.mcfunction",
                    ONE_WORKER,
                ),
                (
                    "data/mdl8_activation/function/nested_fork_worker.mcfunction",
                    NESTED_FORK_WORKER,
                ),
                (
                    "data/mdl8_activation/function/completion_root.mcfunction",
                    COMPLETION_ROOT,
                ),
                (
                    "data/mdl8_activation/function/return_worker.mcfunction",
                    RETURN_WORKER,
                ),
                (
                    "data/mdl8_activation/function/failure_worker.mcfunction",
                    FAILURE_WORKER,
                ),
                (
                    "data/mdl8_activation/function/frame_root.mcfunction",
                    FRAME_ROOT,
                ),
                (
                    "data/mdl8_activation/function/frame_parent.mcfunction",
                    FRAME_PARENT,
                ),
                (
                    "data/mdl8_activation/function/frame_child.mcfunction",
                    FRAME_CHILD,
                ),
                (
                    "data/mdl8_activation/function/abort_root.mcfunction",
                    ABORT_ROOT,
                ),
                (
                    "data/mdl8_activation/function/abort_inner.mcfunction",
                    ABORT_INNER,
                ),
                (
                    "data/mdl8_activation/function/frame_reset.mcfunction",
                    FRAME_RESET,
                ),
            ],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn exercise(server: &mut TestServer) -> Result<(), String> {
    command_wait(server, "forceload add 0 0", "Marked chunk [0, 0]")?;
    command_wait(
        server,
        &format!("scoreboard objectives add {OBJECTIVE} dummy"),
        "Created new objective",
    )?;
    command_wait(
        server,
        "scoreboard objectives add mdl8after dummy",
        "Created new objective",
    )?;
    command_wait(
        server,
        "scoreboard objectives add mdl8nest dummy",
        "Created new objective",
    )?;
    server
        .command("function mdl8_activation:spawn")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("MDL8_ENTITIES_READY")
        .map_err(|error| error.to_string())?;

    command_wait(
        server,
        "function mdl8_activation:serial_root",
        "MDL8_SERIAL_COMPLETE",
    )?;
    // Irrespective of selector iteration order, serial complete unwind produces the
    // three distinct activation snapshots below.
    for score in [100, 111, 122] {
        command_wait(
            server,
            &format!(
                "execute if entity @e[type=minecraft:armor_stand,tag=mdl8_activation,scores={{mdl8act={score}}}] run say MDL8_BEFORE_{score}"
            ),
            &format!("MDL8_BEFORE_{score}"),
        )?;
    }
    for score in [101, 112, 123] {
        command_wait(
            server,
            &format!(
                "execute if entity @e[type=minecraft:armor_stand,tag=mdl8_activation,scores={{mdl8nest={score}}}] run say MDL8_NEST_{score}"
            ),
            &format!("MDL8_NEST_{score}"),
        )?;
    }
    for score in [111, 122, 133] {
        command_wait(
            server,
            &format!(
                "execute if entity @e[type=minecraft:armor_stand,tag=mdl8_activation,scores={{mdl8after={score}}}] run say MDL8_AFTER_{score}"
            ),
            &format!("MDL8_AFTER_{score}"),
        )?;
    }

    command_wait(
        server,
        "function mdl8_activation:context_matrix_root",
        "MDL8_CONTEXT_MATRIX",
    )?;
    command_wait(
        server,
        "function mdl8_activation:completion_root",
        "MDL8_COMPLETION_MATRIX",
    )?;

    command_wait(
        server,
        "function mdl8_activation:frame_root",
        "MDL8_FRAME_COMPLETE",
    )?;
    server
        .wait_for_command_log("MDL8_FRAME_EMPTY")
        .map_err(|error| error.to_string())?;

    set_gamerule(server, "minecraft:max_command_sequence_length", 5)?;
    server
        .command("function mdl8_activation:abort_root")
        .map_err(|error| error.to_string())?;
    set_gamerule(server, "minecraft:max_command_sequence_length", 65_536)?;
    command_wait(
        server,
        "execute if data storage mdl8_activation:runtime frames[0] run say MDL8_ABORT_RESIDUE",
        "MDL8_ABORT_RESIDUE",
    )?;
    server
        .command("function mdl8_activation:frame_reset")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Running function")
        .map_err(|error| error.to_string())?;
    command_wait(
        server,
        "execute unless data storage mdl8_activation:runtime frames[0] run say MDL8_ABORT_RECOVERED",
        "MDL8_ABORT_RECOVERED",
    )?;

    command_wait(server, &format!("kill {ENTITIES}"), "Killed 3 entities")?;
    command_wait(server, "forceload remove 0 0", "Unmarked chunk [0, 0]")
}

fn set_gamerule(server: &mut TestServer, name: &str, value: u32) -> Result<(), String> {
    server
        .command(&format!("gamerule {name} {value}"))
        .map_err(|error| error.to_string())?;
    let canonical = name.rsplit_once(':').map_or(name, |(_, name)| name);
    server
        .wait_for_command_log(&format!("Gamerule {canonical} is now set to: {value}"))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn command_wait(server: &mut TestServer, command: &str, expected: &str) -> Result<(), String> {
    server.command(command).map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(expected)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
