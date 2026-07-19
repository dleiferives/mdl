use mdl_compiler::analysis::minecraft::{AnalysisArithmeticCaps, TargetExecutionAnalysisLimits};
use mdl_compiler::datapack::EmissionOptions;
use mdl_compiler::frontend::{
    CompilationOptions, FrontendLimits, FunctionVisibility, SourceInput, compile_source,
};
use mdl_compiler::ir::core::{CoreEvaluationError, CoreEvaluationLimits, CoreEvaluator, CoreValue};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions};
use mdl_compiler::target::JavaEditionTarget;

const SOURCE: &str = include_str!("source-fixtures/pre-scheduler/ps2_composition_rehearsal.mdl");

#[test]
fn public_facilities_compose_into_parser_tape_and_fuel_state_machine() {
    for core in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
        for minecraft in [
            MinecraftOptimizationLevel::None,
            MinecraftOptimizationLevel::Baseline,
        ] {
            let output = compile_source(
                SourceInput::new("ps2_composition_rehearsal.mdl", SOURCE),
                &options(core, minecraft),
            )
            .unwrap();
            let exports = output
                .checked_frontend()
                .function_ids()
                .filter(|source| {
                    output.checked_frontend().function_visibility(*source)
                        == Some(FunctionVisibility::DatapackExport)
                })
                .filter_map(|source| {
                    Some((
                        output.source_function_name(source)?,
                        output.source_to_core().function(source)?,
                    ))
                })
                .collect::<Vec<_>>();
            let exported = |name| {
                exports
                    .iter()
                    .find_map(|(actual, core)| (*actual == name).then_some(*core))
                    .unwrap_or_else(|| panic!("missing exported rehearsal function `{name}`"))
            };
            let validate = exported("validate_brackets");
            let rehearse = exported("rehearse");
            let checksum = exported("rehearse_checksum");
            let evaluator = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(1_000_000, 1_024),
            );

            assert_eq!(
                evaluator
                    .evaluate(rehearse, &[CoreValue::I32(8)])
                    .unwrap()
                    .results(),
                &[
                    CoreValue::list_i32(Vec::<i32>::new()),
                    CoreValue::list_i32(vec![2, 0]),
                    CoreValue::list_i32(vec![1, 2]),
                    CoreValue::list_i32(Vec::<i32>::new()),
                    CoreValue::I32(0),
                    CoreValue::I32(8),
                    CoreValue::I32(0),
                ]
            );
            for (source, status) in [("[[]]", 0), ("[", 1), ("]", 2)] {
                assert_eq!(
                    evaluator
                        .evaluate(validate, &[CoreValue::string(source)])
                        .unwrap()
                        .results(),
                    &[CoreValue::I32(status)]
                );
            }
            let one_short = evaluator.evaluate(rehearse, &[CoreValue::I32(7)]).unwrap();
            assert_eq!(
                one_short.results()[4..],
                [CoreValue::I32(0), CoreValue::I32(7), CoreValue::I32(3)]
            );
            let zero = evaluator.evaluate(rehearse, &[CoreValue::I32(0)]).unwrap();
            assert_eq!(
                zero.results()[4..],
                [CoreValue::I32(0), CoreValue::I32(0), CoreValue::I32(3)]
            );
            assert_eq!(
                evaluator
                    .evaluate(checksum, &[CoreValue::I32(10)])
                    .unwrap()
                    .results(),
                &[CoreValue::I32(12)]
            );
            let bounded = CoreEvaluator::for_verified_program(
                output.core_optimization().program(),
                CoreEvaluationLimits::new(1_000_000, 1_024).with_max_value_nodes(5),
            );
            assert!(matches!(
                bounded.evaluate(rehearse, &[CoreValue::I32(8)]),
                Err(CoreEvaluationError::ValueNodeLimitExceeded { limit: 5, .. })
            ));
            let pack = output
                .emission()
                .pack()
                .files()
                .iter()
                .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
                .collect::<String>();
            assert!(pack.contains(" append from storage "));
            assert!(pack.contains(" set string storage "));
        }
    }
}

fn options(
    core: CoreOptimizationLevel,
    minecraft: MinecraftOptimizationLevel,
) -> CompilationOptions {
    let lowering = LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("ps2_rehearsal").unwrap(),
        ObjectiveName::new("ps2.rehearse").unwrap(),
    )
    .unwrap()
    .with_optimization_level(minecraft);
    let arithmetic = AnalysisArithmeticCaps::minimum_for(lowering.command_limit_assumptions());
    CompilationOptions::new(
        FrontendLimits::DEFAULT,
        CoreOptimizationOptions::new(core),
        lowering,
        TargetExecutionAnalysisLimits::new(arithmetic, 1_000_000, 1_000_000),
        EmissionOptions::new("PS-2 composition rehearsal"),
    )
}
