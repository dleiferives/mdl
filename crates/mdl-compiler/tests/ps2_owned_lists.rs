use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const SOURCE: &str = r"
fn append(value: List<Int32>, element: Int32) -> List<Int32> {
    return value.push(element);
}

export fn list_round_trip(value: Int32) -> Int32 {
    var original: List<Int32> = List.empty();
    original = original.push(7);
    const copy: List<Int32> = original;
    var changed: List<Int32> = append(original, value);
    changed = changed.without_last();
    return copy.last_or_zero() +% changed.length();
}
";

#[test]
fn owned_lists_preserve_copy_semantics_across_calls_and_tail_operations() {
    for core in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        for minecraft in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let output = compile_source(
                SourceInput::new("list.mdl", SOURCE),
                &options(core, minecraft),
            )
            .unwrap();
            let function = output
                .checked_frontend()
                .function_ids()
                .nth(1)
                .and_then(|function| output.source_to_core().function(function))
                .unwrap();
            let evaluation = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(10_000, 1_024),
            )
            .evaluate(function, &[CoreValue::I32(99)])
            .unwrap();
            assert_eq!(evaluation.results(), &[CoreValue::I32(8)]);
            assert!(output.emission().pack().files().iter().any(|file| {
                std::str::from_utf8(file.bytes())
                    .is_ok_and(|contents| contents.contains("data modify storage"))
            }));
        }
    }
}

fn options(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps2_list").unwrap(),
        ObjectiveName::new("ps2.list").unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-2 owned list test"),
    )
}
