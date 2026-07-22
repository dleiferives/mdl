use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

fn pack_text(source: &str) -> String {
    let output = compile_source(SourceInput::new("ps14.mdl", source), &options()).unwrap();
    output
        .emission()
        .pack()
        .files()
        .iter()
        .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// PS-14: `mc.entities(Player)` must lower to the structured `@e[type=
/// minecraft:player,...]` selector -- there is no `@a`-shorthand base selector
/// anywhere in `EntitySelector` (confirmed reading `ir/minecraft/selector.rs`
/// directly), so this is the correct, minimal rendering, mirroring
/// `EntityKind::ArmorStand`'s existing `@e[type=minecraft:armor_stand,...]`.
#[test]
fn entities_player_query_renders_the_structured_player_selector() {
    let source = r#"
export fn greet_player() {
    run.as(mc.entities(Player).with_tag("x").limit(1)) |reader| {
        reader.say("MDL_PS14_HELLO");
    }
}
"#;
    let pack = pack_text(source);
    assert!(
        pack.contains("as @e[type=minecraft:player,tag=x,limit=1]"),
        "{pack}"
    );
}

/// PS-14: a player's armor-slot equipment read (`.chest.count`) must lower
/// inline exactly like an `ArmorStand`'s equivalent chain -- same schema-
/// walk, same `NbtPath` shape, only the selector's entity kind differs.
/// Mirrors `ps12c_entity_nbt_path_lowering.rs`'s inline-route assertions.
#[test]
fn player_equipment_chest_count_lowers_inline_through_the_scalar_scratch_conversion() {
    let source = r#"
export fn read_chest_count() {
    run.as(mc.entities(Player).limit(1)) |reader| {
        const count: Int32 = reader.equipment.chest.count;
        if (count > 0) {
            reader.say("MDL_PS14_PLAYER_CHEST_OK");
        }
    }
}
"#;
    let pack = pack_text(source);
    assert!(pack.contains("entity_nbt_scalar_scratch"), "{pack}");
    assert!(pack.contains("\"equipment\".\"chest\".\"count\""), "{pack}");
    assert!(pack.contains("execute store result score"), "{pack}");
    assert!(pack.contains("run data get storage"), "{pack}");
    assert!(
        pack.contains("as @e[type=minecraft:player,limit=1]"),
        "{pack}"
    );
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps14_player").unwrap(),
        ObjectiveName::new("ps14.player").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::None);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-14 Player entity kind"),
    )
}
