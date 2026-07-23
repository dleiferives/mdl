use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

fn pack_text(source: &str) -> String {
    let output = compile_source(SourceInput::new("be2.mdl", source), &options()).unwrap();
    output
        .emission()
        .pack()
        .files()
        .iter()
        .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// PS-16 Stage 1: a whole-slot entity-NBT write with a literal slot lowers
/// unconditionally to `item replace block <pos> container.<slot> with <item>
/// <count>` — no `data modify` at all, no occupancy check, no macro helper
/// (mirrors BE-1's own literal-only-first staging). This is the doc's own
/// scope decision, grounded in a real measurement against the pinned server:
/// `item replace` is confirmed clean for both an occupied and an unoccupied
/// slot, unlike a `data modify ... Items[{Slot:N}].field set value ...`
/// partial write.
#[test]
fn literal_slot_write_lowers_inline_to_item_replace_with_no_macro_helper() {
    let source = r#"
export fn write_chest_slot() {
    mc.block(Chest, 0, 4, 0).Items[3] = .{.id = "minecraft:diamond", .count = 5};
}
"#;
    let pack = pack_text(source);
    assert!(!pack.contains("mdl:__mdl/macro"), "{pack}");
    assert!(!pack.contains("CommandKind::Macro"), "{pack}");
    // No `data modify ... Items` at all — the dangerous partial-write shape
    // this milestone's own research measured and rejected in favor of
    // `item replace`. The compiler's own init/load boilerplate legitimately
    // uses `data modify` elsewhere, so this checks specifically for an
    // `Items`-targeting one rather than the substring everywhere in the pack.
    assert!(
        !pack.contains("data modify") || !pack.contains("Items"),
        "{pack}"
    );

    let write_line = pack
        .lines()
        .find(|line| line.contains("item replace block"))
        .expect("inline item-replace write line must be present");
    assert_eq!(
        write_line, "item replace block 0 4 0 container.3 with minecraft:diamond 5",
        "{pack}"
    );
}

/// The same write shape at a different literal slot and item/count, proving
/// the lowering is parametric on the whole-slot literal, not hardcoded.
#[test]
fn a_different_literal_slot_item_and_count_lowers_to_the_matching_command() {
    let source = r#"
export fn write_chest_slot() {
    mc.block(Chest, 10, -5, 20).Items[26] = .{.id = "minecraft:redstone_block", .count = 1};
}
"#;
    let pack = pack_text(source);
    let write_line = pack
        .lines()
        .find(|line| line.contains("item replace block"))
        .expect("inline item-replace write line must be present");
    assert_eq!(
        write_line, "item replace block 10 -5 20 container.26 with minecraft:redstone_block 1",
        "{pack}"
    );
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("be2_chest").unwrap(),
        ObjectiveName::new("be2.chest").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::None);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-16 Stage 1 block-entity NBT write lowering regression"),
    )
}
