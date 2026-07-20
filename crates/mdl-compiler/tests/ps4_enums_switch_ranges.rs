use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{CompilationOptions, FrontendLimits, SourceInput, compile_source};
use mdl_compiler::ir::core::{CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;
use serde::Deserialize;

const SOURCE: &str = include_str!("../../../tests/programs/enums-switch-ranges/src/main.mdl");
const CASES: &str = include_str!("../../../tests/programs/enums-switch-ranges/cases.json");

#[derive(Deserialize)]
struct Case {
    input: i32,
    expected: i32,
}

#[test]
fn enums_switches_and_ranges_are_equivalent_across_policies() {
    let cases: Vec<Case> = serde_json::from_str(CASES).unwrap();
    for core in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        for minecraft in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let output = compile_source(
                SourceInput::new("ps4.mdl", SOURCE),
                &options(core, minecraft),
            )
            .unwrap();
            assert_eq!(output.checked_frontend().enum_count(), 1);
            let source = output.checked_frontend().function_ids().nth(2).unwrap();
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
            assert!(output.emission().pack().files().iter().any(|file| {
                std::str::from_utf8(file.bytes()).is_ok_and(|text| text.contains("matches 0..20"))
            }));
            let footprint = output.emission().footprint();
            assert!(footprint.function_files() > 0);
            assert!(footprint.physical_function_lines() > 0);
            assert!(footprint.total_utf8_bytes() > 0);
            let analysis = output.target_analysis().as_ref().unwrap();
            assert!(analysis.census().command_nodes() > 0);
            assert!(analysis.census().score_commands() > 0);
        }
    }
}

fn options(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps4_test").unwrap(),
        ObjectiveName::new("ps4.test").unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 1_000_000, 1_000_000),
        EmissionOptions::new("PS-4 enum/switch/range test"),
    )
}
