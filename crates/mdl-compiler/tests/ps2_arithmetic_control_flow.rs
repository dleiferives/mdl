use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const SOURCE: &str = r"
export fn loop_control(value: Int32) -> Int32 {
    var current: Int32 = value;
    var result: Int32 = 0;
    while (current > 0) {
        current = current -% 1;
        if (current == 2) {
            continue;
        }
        if (current == 1) {
            break;
        }
        result = result +% current;
    }
    return result;
}
";

#[test]
fn source_loops_match_reference_results_before_and_after_core_optimization() {
    for optimization in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        let output = compile_source(
            SourceInput::new("ps2-loop.mdl", SOURCE),
            &options(optimization),
        )
        .unwrap();
        let source_function = output.checked_frontend().function_ids().next().unwrap();
        let function = output.source_to_core().function(source_function).unwrap();
        let evaluator = CoreEvaluator::for_verified_program(
            output.core_optimization().program(),
            CoreEvaluationLimits::new(10_000, 1_024),
        );
        for (input, expected) in [(0, 0), (1, 0), (3, 0), (5, 7), (8, 25)] {
            let evaluation = evaluator
                .evaluate(function, &[CoreValue::I32(input)])
                .unwrap();
            assert_eq!(evaluation.results(), &[CoreValue::I32(expected)]);
        }
    }
}

fn options(optimization: CoreOptimizationLevel) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps2_test").unwrap(),
        ObjectiveName::new("ps2.test").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(optimization),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-2 arithmetic/control-flow test"),
    )
}
