use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const SOURCE: &str = include_str!("source-fixtures/pre-scheduler/ps2_runtime_strings.mdl");

#[test]
fn runtime_strings_traverse_utf16_tail_without_interpolating_commands() {
    for core in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        for minecraft in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let output = compile_source(
                SourceInput::new("string.mdl", SOURCE),
                &options(core, minecraft),
            )
            .unwrap();
            let function = output
                .checked_frontend()
                .function_ids()
                .next()
                .and_then(|function| output.source_to_core().function(function))
                .unwrap();
            let evaluation = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(100_000, 1_024),
            )
            .evaluate(function, &[])
            .unwrap();
            assert_eq!(
                evaluation.results(),
                &[CoreValue::list_i32(vec![62, 45, 43])]
            );
            let pack = output
                .emission()
                .pack()
                .files()
                .iter()
                .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
                .collect::<String>();
            assert!(pack.contains(" string storage "));
            assert!(pack.contains("execute if data storage"));
            assert!(!pack.contains("ignored😀+-> run"));
        }
    }
}

fn options(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps2_string").unwrap(),
        ObjectiveName::new("ps2.string").unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-2 runtime string test"),
    )
}
