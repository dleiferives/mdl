use mdl_compiler::datapack::{EmissionOptions, emit_datapack};
use mdl_compiler::ir::core::{
    BlockTarget, CoreProgram, CoreType, FunctionBuilder, Terminator, TerminatorKind,
};
use mdl_compiler::ir::minecraft::{ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    ActivationDepthContract, LoweringOptions, MinecraftOptimizationLevel, lower_to_minecraft,
};
use mdl_compiler::source::{OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;

#[test]
fn direct_multi_result_recursion_has_exact_typed_frames_under_both_lowering_policies() {
    let sources = SourceContext::new();
    let core = recursive_pair(&sources);

    for (ordinal, level) in [
        MinecraftOptimizationLevel::None,
        MinecraftOptimizationLevel::Baseline,
    ]
    .into_iter()
    .enumerate()
    {
        let namespace = PackNamespace::new(&format!("mdl8_pair_{ordinal}")).unwrap();
        let options = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            namespace,
            ObjectiveName::new(&format!("mdl8.p{ordinal}")).unwrap(),
        )
        .unwrap()
        .with_optimization_level(level);
        let output = lower_to_minecraft(&core, &sources, &options).unwrap();
        assert_eq!(
            output.map().execution_contract().activation_depth(),
            ActivationDepthContract::CallerBounded
        );
        let statistics = output.report().statistics();
        assert_eq!(statistics.recursive_call_occurrences(), 1);
        assert_eq!(statistics.recursive_spill_bridges(), 1);
        assert_eq!(statistics.physical_recipe_sequence_operations(), 7);
        assert_eq!(statistics.physical_recipe_forks(), 0);
        assert_eq!(
            statistics.physical_storages(),
            statistics.homes() + statistics.recursive_spill_bridges()
        );
        let report = output.report().dump();
        assert!(report.contains("abi result 0 type=I32 mode=DirectScore"));
        assert!(report.contains("abi result 1 type=Bool mode=DirectScore"));
        assert!(report.contains("class=ActivationNbtBool binding=RecursiveSpill"));

        let emission = emit_datapack(
            output.program(),
            &sources,
            &EmissionOptions::new("MDL Stage 8 recursive pair"),
        )
        .unwrap();
        let functions = emission
            .pack()
            .files()
            .iter()
            .filter(|file| file.path().as_str().ends_with(".mcfunction"))
            .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(functions.matches("\"frames\" append value {}").count(), 1);
        assert_eq!(functions.matches("data remove storage").count(), 1);
        assert_eq!(functions.matches("execute store result storage").count(), 1);
        assert!(functions.contains(" byte 1 run scoreboard players get "));
        assert_eq!(functions.matches("execute store result score").count(), 1);
    }
}

#[test]
fn nonrecursive_program_emits_no_recursive_runtime_support() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let function = core
        .declare_function(
            Some("plain"),
            vec![],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
    let value = builder.i32_constant(42, OriginId::UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![value]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();

    for (ordinal, level) in [
        MinecraftOptimizationLevel::None,
        MinecraftOptimizationLevel::Baseline,
    ]
    .into_iter()
    .enumerate()
    {
        let options = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new(&format!("mdl8_plain_{ordinal}")).unwrap(),
            ObjectiveName::new(&format!("mdl8.n{ordinal}")).unwrap(),
        )
        .unwrap()
        .with_optimization_level(level);
        let output = lower_to_minecraft(&core, &sources, &options).unwrap();
        let statistics = output.report().statistics();
        assert_eq!(statistics.recursive_call_occurrences(), 0);
        assert_eq!(statistics.recursive_spill_bridges(), 0);
        assert_eq!(statistics.physical_storages(), statistics.homes());
        let report = output.report().dump();
        assert!(!report.contains("RecursiveStack"));
        assert!(!report.contains("RecursiveSpill"));
        let emission = emit_datapack(
            output.program(),
            &sources,
            &EmissionOptions::new("MDL Stage 8 no recursive runtime"),
        )
        .unwrap();
        let functions = emission
            .pack()
            .files()
            .iter()
            .filter(|file| file.path().as_str().ends_with(".mcfunction"))
            .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!functions.contains("\"frames\""));
        assert!(!functions.contains("data remove storage"));
    }
}

fn recursive_pair(sources: &SourceContext) -> CoreProgram {
    let mut core = CoreProgram::new();
    let function = core
        .declare_function(
            Some("recursive_pair"),
            vec![CoreType::Bool, CoreType::I32],
            vec![CoreType::I32, CoreType::Bool],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
    let entry = builder.entry_block();
    let flag = builder.body().block(entry).unwrap().parameters()[0].value();
    let preserved = builder.body().block(entry).unwrap().parameters()[1].value();
    let recursive = builder.create_block(OriginId::UNKNOWN).unwrap();
    let base = builder.create_block(OriginId::UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: flag,
                then_target: BlockTarget::new(recursive, vec![]),
                else_target: BlockTarget::new(base, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();

    builder.switch_to_block(recursive).unwrap();
    let stop = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
    let child_input = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
    let child = builder
        .call(function, vec![stop, child_input], OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![child[0], flag]),
            OriginId::UNKNOWN,
        ))
        .unwrap();

    builder.switch_to_block(base).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![preserved, flag]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
    core
}
