use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

fn pack_text(source: &str) -> String {
    let output = compile_source(SourceInput::new("be1.mdl", source), &options()).unwrap();
    output
        .emission()
        .pack()
        .files()
        .iter()
        .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// BE-1: a block-entity NBT read needs no `run.as(...)` at all — a block
/// position is self-contained in the command it lowers to, unlike an entity
/// read which needs an already-proven executor capture. This also proves the
/// real chest NBT shape measured against the pinned server: `Items` is a
/// top-level field (no `.components` wrapper the way entity equipment has),
/// and `Slot` renders as a `Byte` (`0b`), not a plain `Int32`.
#[test]
fn literal_chest_read_lowers_inline_with_no_macro_helper_and_no_ambient_context() {
    let source = r#"
export fn read_chest_count() {
    const count: Int32 = mc.block(Chest, 0, 4, 0).Items[0].count;
    if (count > 1) {
        unsafe minecraft("say BE1_STACK");
    }
}
"#;
    let pack = pack_text(source);
    assert!(!pack.contains("mdl:__mdl/macro"), "{pack}");
    assert!(!pack.contains("CommandKind::Macro"), "{pack}");

    let read_line = pack
        .lines()
        .find(|line| {
            line.contains("data modify storage")
                && line.contains("set from block")
                && !line.starts_with('$')
        })
        .expect("inline block-NBT read line must be present");
    assert!(
        read_line.contains("set from block 0 4 0"),
        "expected an absolute block position, no selector: {read_line}"
    );
    assert!(
        read_line.contains("\"Items\"[{Slot:0b}].\"count\""),
        "expected the measured real chest shape (flat Items list, Byte-typed \
         Slot match, no nested \"item\" wrapper): {read_line}"
    );
}

/// BE-1: the fail-soft contract generalizes to block reads exactly as it
/// already does for entity reads — a type-appropriate default, then an
/// attempt that silently keeps the default on any broken link (wrong block,
/// empty slot).
#[test]
fn chest_read_is_preceded_by_a_type_appropriate_default_init() {
    let source = r#"
export fn read_chest_count() {
    const count: Int32 = mc.block(Chest, 0, 4, 0).Items[0].count;
    if (count > 1) {
        unsafe minecraft("say BE1_STACK");
    }
}
"#;
    let pack = pack_text(source);
    assert!(
        pack.contains("entity_nbt_scalar_scratch"),
        "Int32 results route through the shared scratch/score conversion, \
         exactly like Bool: {pack}"
    );
    assert!(pack.contains("execute store result score"), "{pack}");
    assert!(pack.contains("run data get storage"), "{pack}");
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("be1_chest").unwrap(),
        ObjectiveName::new("be1.chest").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::None);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("BE-1 block-entity NBT lowering regression"),
    )
}
