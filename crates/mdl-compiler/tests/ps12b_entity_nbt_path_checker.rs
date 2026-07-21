use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationFailure, CompilationOptions, CoreGenerationFailure, FrontendLimits, SourceInput,
    compile_source,
};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const SOURCE: &str = r#"
export fn read_book() {
    run.as(mc.entities(ArmorStand).with_tag("mdl_ps12b_book").limit(1)) |reader| {
        const page: String = reader.equipment.mainhand.components."minecraft:written_book_content".pages[0].raw;
        if (page.ends_with_ascii("+")) {
            reader.say("MDL_PS12B_BOOK_OK");
        }
    }
}
"#;

/// PS-12B ships checking (grammar, schema table, unified chained-postfix
/// checker) but deliberately not Core lowering yet (PS-12C). This proves the
/// boundary is honest: a program using the new entity-NBT syntax checks
/// cleanly and then fails Core generation with a clear, structured,
/// non-panicking error — not silently, not with a crash.
#[test]
fn entity_nbt_path_checks_cleanly_and_fails_core_generation_with_a_clear_error() {
    let failure = compile_source(SourceInput::new("ps12b.mdl", SOURCE), &options())
        .expect_err("Core lowering for entity-NBT paths is not implemented until PS-12C");
    let CompilationFailure::CoreGeneration {
        checked_frontend,
        failure,
        ..
    } = failure
    else {
        panic!("expected a CoreGeneration failure, entity-NBT path checking should have succeeded");
    };
    // One entity-NBT read, one `say` — both checked successfully.
    assert_eq!(checked_frontend.external_operation_count(), 2);
    assert!(matches!(
        *failure,
        CoreGenerationFailure::Invariant(
            mdl_compiler::frontend::CoreGenerationInvariant::EntityNbtReadLoweringNotImplemented { .. }
        )
    ));
}

fn options() -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps12b_book").unwrap(),
        ObjectiveName::new("ps12b.book").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::None);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-12B checker boundary regression"),
    )
}
