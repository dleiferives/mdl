use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const SOURCE: &str = r"
const Inner = struct {
    count: Int32,
    ready: Bool,
};

const State = struct {
    pc: Int32,
    inner: Inner,
};

fn identity(value: State) -> State {
    return value;
}

export fn aggregate_flow(value: Int32) -> Int32 {
    var state: State = State{
        .pc = value,
        .inner = Inner{ .count = 3, .ready = true },
    };
    if (state.inner.ready) {
        state = identity(State{
            .pc = state.pc +% state.inner.count,
            .inner = state.inner,
        });
    }
    return state.pc;
}
";

#[test]
fn nested_aggregate_calls_and_branch_updates_lower_to_scalar_core() {
    for optimization in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        let output = compile_source(
            SourceInput::new("aggregate.mdl", SOURCE),
            &options(optimization),
        )
        .unwrap();
        let function = output
            .checked_frontend()
            .function_ids()
            .nth(1)
            .and_then(|function| output.source_to_core().function(function))
            .unwrap();
        let evaluator = CoreEvaluator::for_verified_program(
            output.core_optimization().program(),
            CoreEvaluationLimits::new(10_000, 1_024),
        );
        for (input, expected) in [(0, 3), (7, 10), (i32::MAX, i32::MIN + 2)] {
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
        PackNamespace::new("ps2_aggregate").unwrap(),
        ObjectiveName::new("ps2.aggregate").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(optimization),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 100_000, 100_000),
        EmissionOptions::new("PS-2 fixed aggregate test"),
    )
}
