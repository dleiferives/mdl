use std::env;
use std::path::PathBuf;

use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox};

const SOURCE: &str = include_str!(
    "../../mdl-compiler/tests/source-fixtures/pre-scheduler/be2_block_entity_nbt_write.mdl"
);

/// PS-16 Stage 1 (BE-2): a whole-slot container write against the real
/// pinned Java 26.2 server. This is the milestone's own required proof, not
/// a nice-to-have: the original plan (`data modify ...
/// Items[{Slot:N}].field set value ...`) was measured directly against this
/// same server to report success while silently synthesizing a malformed
/// NBT entry missing the mandatory `id` field, which then trips a real
/// "Serialization errors" warning on the very next deserialization pass —
/// for what the MDL program considers ordinary, successful behavior (see
/// `notes/compiler/pre-scheduler/ps-16-block-entity-nbt-writes.md`). The
/// whole-slot `item replace` shape this milestone actually ships was
/// confirmed clean for both an occupied and an unoccupied slot; the two
/// `assert!(...matching_log_lines_since...)` checks below are the automated
/// regression proving that stays true, not just that the write "ran".
#[test]
#[ignore = "requires the pinned official Minecraft 26.2 server bundle and Java 25"]
fn whole_slot_write_matches_java_26_2_with_no_serialization_errors() {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .expect("set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR");
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let output = compile_source(
        SourceInput::new("be2_block_entity_nbt_write.mdl", SOURCE),
        &options(),
    )
    .unwrap();
    // Function resource names are compiler-generated identities
    // (`__mdl/f0/b0`, ...), not derived from the source name, so the two
    // exported functions are told apart by declaration order instead —
    // `write_unoccupied_slot` is declared first, `write_occupied_slot`
    // second, in `be2_block_entity_nbt_write.mdl`. `function_ids()` iterates
    // in dense declaration order.
    let functions = output.checked_frontend().function_ids().collect::<Vec<_>>();
    let entry = |index: usize| {
        output
            .source_function_abi(functions[index])
            .unwrap()
            .entry_resource()
            .to_string()
    };
    let write_unoccupied = entry(0);
    let write_occupied = entry(1);

    let sandbox = ServerSandbox::create(env::var_os("MDL_KEEP_TEST_DIR").is_some()).unwrap();
    sandbox
        .install_datapack(
            "be2_chest",
            output
                .emission()
                .pack()
                .files()
                .iter()
                .map(|file| (file.path().as_str(), file.bytes())),
        )
        .unwrap();
    let mut server = sandbox.start(&ServerConfig::new(java, server_jar)).unwrap();

    server.command("forceload add 0 0").unwrap();
    server.wait_for_command_log("Marked chunk [0, 0]").unwrap();
    server.command("setblock 0 4 0 minecraft:chest").unwrap();

    // Slot 7 has never been populated — the exact case this milestone's own
    // research measured as dangerous under the rejected `data modify`
    // partial-write shape.
    let checkpoint = server.log_checkpoint();
    server
        .command(&format!("function {write_unoccupied}"))
        .unwrap();
    server.wait_for_command_log("BE2_UNOCCUPIED_OK").unwrap();
    server.command("data get block 0 4 0 Items").unwrap();
    server.command("say BE2_BARRIER_1").unwrap();
    server.wait_for_command_log("BE2_BARRIER_1").unwrap();
    assert!(
        server
            .matching_log_lines_since(checkpoint, "Serialization errors")
            .unwrap()
            .is_empty(),
        "an unoccupied-slot whole-slot write must not trip a Serialization errors warning \
         (this is the exact danger the doc's own measurement found and this milestone exists \
         to avoid)"
    );

    // Slot 12 is pre-populated by a direct console write before the
    // compiled write overwrites it — the occupied-slot case.
    server
        .command("item replace block 0 4 0 container.12 with minecraft:stone 1")
        .unwrap();
    server.wait_for_command_log("Replaced").unwrap();
    let checkpoint = server.log_checkpoint();
    server
        .command(&format!("function {write_occupied}"))
        .unwrap();
    server.wait_for_command_log("BE2_OCCUPIED_OK").unwrap();
    server.command("data get block 0 4 0 Items").unwrap();
    server.command("say BE2_BARRIER_2").unwrap();
    server.wait_for_command_log("BE2_BARRIER_2").unwrap();
    assert!(
        server
            .matching_log_lines_since(checkpoint, "Serialization errors")
            .unwrap()
            .is_empty(),
        "an occupied-slot whole-slot write (overwrite) must not trip a Serialization errors \
         warning either"
    );

    server.shutdown().unwrap();
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("be2_chest").unwrap(),
        ObjectiveName::new("be2.chest").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-16 Stage 1 block-entity NBT write semantics"),
    )
}
