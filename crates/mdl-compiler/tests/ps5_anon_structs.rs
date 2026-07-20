use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, FrontendLimits, FunctionVisibility, SourceInput, compile_source,
};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use serde::Deserialize;

const SOURCE: &str = include_str!("../../../tests/programs/anon-structs/src/main.mdl");
const CASES: &str = include_str!("../../../tests/programs/anon-structs/cases.json");

#[derive(Debug, Deserialize)]
struct Case {
    input: i32,
    expected: i32,
}

#[test]
fn anonymous_structs_and_destructuring_are_equivalent_across_policies() {
    let cases: Vec<Case> = serde_json::from_str(CASES).unwrap();
    for core in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        for minecraft in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let output = compile_source(
                SourceInput::new("ps5.mdl", SOURCE),
                &options(core, minecraft),
            )
            .unwrap();
            let source = output
                .checked_frontend()
                .function_ids()
                .find(|id| {
                    matches!(
                        output.checked_frontend().function_visibility(*id),
                        Some(FunctionVisibility::DatapackExport)
                    )
                })
                .expect("checksum export not found");
            let function = output.source_to_core().function(source).unwrap();
            let evaluator = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(100_000, 2_048),
            );
            for case in &cases {
                assert_eq!(
                    evaluator
                        .evaluate(function, &[CoreValue::I32(case.input)])
                        .unwrap()
                        .results(),
                    &[CoreValue::I32(case.expected)]
                );
            }
            let footprint = output.emission().footprint();
            assert!(footprint.function_files() > 0);
            assert!(footprint.physical_function_lines() > 0);
            assert!(footprint.total_utf8_bytes() > 0);
        }
    }
}

fn options(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps5_test").unwrap(),
        ObjectiveName::new("ps5.test").unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 1_000_000, 1_000_000),
        EmissionOptions::new("PS-5 anonymous structs and destructuring test"),
    )
}
