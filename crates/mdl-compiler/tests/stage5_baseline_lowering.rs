use mdl_compiler::datapack::{DatapackArtifact, EmissionOptions, emit_datapack};
use mdl_compiler::ir::core::{
    BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
    TerminatorKind, ValueId,
};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    LoweringOptions, LoweringOutput, MinecraftOptimizationLevel, lower_to_minecraft,
};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions, optimize_core};
use mdl_compiler::source::{OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;

#[test]
fn baseline_omits_unused_pure_work_after_explicit_core_none() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let function = declare(&mut core, "unused_pure", vec![], vec![]);
    let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
    let left = builder.i32_constant(20, OriginId::UNKNOWN).unwrap();
    let right = builder.i32_constant(22, OriginId::UNKNOWN).unwrap();
    builder
        .i32_add_wrapping(left, right, OriginId::UNKNOWN)
        .unwrap();
    let boolean = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
    builder.bool_not(boolean, OriginId::UNKNOWN).unwrap();
    return_values(&mut builder, vec![]);
    core.define_function(function, builder.finish().unwrap())
        .unwrap();

    let unoptimized = optimize_core(
        core,
        &sources,
        &CoreOptimizationOptions::new(CoreOptimizationLevel::None),
    )
    .unwrap();
    assert_eq!(unoptimized.report().level(), CoreOptimizationLevel::None);
    assert!(unoptimized.report().steps().is_empty());
    let body = unoptimized
        .program()
        .function(function)
        .unwrap()
        .body()
        .unwrap();
    assert_eq!(body.instruction_counts().attached, 5);

    let output = lower_to_minecraft(unoptimized.program(), &sources, &baseline_options()).unwrap();
    let report = output.dump_lowering();
    assert!(report.contains("optimization-level Baseline"));
    let section = report_function_section(&report, "unused_pure");
    assert_eq!(section.matches("omitted-pure").count(), 5);
    assert!(!section.contains(" instruction 0 scalar"));

    let emission = emit_datapack(
        output.program(),
        &sources,
        &EmissionOptions::new("stage 5 baseline omission"),
    )
    .unwrap();
    let rendered = emitted_entry(emission.pack(), &output, function);
    assert_eq!(rendered, "return 1\n");
    let resource = output.map().function(function).unwrap().entry_resource();
    let path = resource.pack_path(JavaEditionTarget::V26_2);
    assert_eq!(
        emission.pack().file(path.as_str()).unwrap().bytes(),
        b"return 1\n"
    );
}

#[test]
fn overflowing_add_result_masks_use_only_the_required_semantic_and_recipe_homes() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let neither = declare(
        &mut core,
        "overflow_neither",
        vec![CoreType::I32, CoreType::I32],
        vec![],
    );
    let sum = declare(
        &mut core,
        "overflow_sum",
        vec![CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
    );
    let flag = declare(
        &mut core,
        "overflow_flag",
        vec![CoreType::I32, CoreType::I32],
        vec![CoreType::Bool],
    );
    let both = declare(
        &mut core,
        "overflow_both",
        vec![CoreType::I32, CoreType::I32],
        vec![CoreType::I32, CoreType::Bool],
    );
    define_overflow_mask(&mut core, &sources, neither, OverflowMask::Neither);
    define_overflow_mask(&mut core, &sources, sum, OverflowMask::Sum);
    define_overflow_mask(&mut core, &sources, flag, OverflowMask::Flag);
    define_overflow_mask(&mut core, &sources, both, OverflowMask::Both);

    let output = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    let report = output.dump_lowering();
    let emission = emit_datapack(
        output.program(),
        &sources,
        &EmissionOptions::new("stage 5 overflowing masks"),
    )
    .unwrap();

    let neither_section = report_function_section(&report, "overflow_neither");
    assert!(neither_section.contains("instruction 0 omitted-pure"));
    assert_eq!(
        emitted_entry(emission.pack(), &output, neither),
        "return 1\n"
    );
    assert!(!report.contains("#f0qb0"));
    assert!(!report.contains("#f0qi0"));

    let sum_section = report_function_section(&report, "overflow_sum");
    assert!(sum_section.contains("result 0 semantic"));
    assert!(sum_section.contains("result 1 recipe-temporary"));
    assert!(report.contains("#f1qb0"));
    assert!(!report.contains("#f1qi0"));
    let sum_rendered = emitted_entry(emission.pack(), &output, sum);
    assert!(sum_rendered.contains("#f1qb0"));
    assert!(!sum_rendered.contains("#f1qi0"));
    assert!(sum_rendered.contains("#f1r0"));

    let flag_section = report_function_section(&report, "overflow_flag");
    assert!(flag_section.contains("result 0 recipe-temporary"));
    assert!(flag_section.contains("result 1 semantic"));
    assert!(report.contains("#f2qi0"));
    assert!(!report.contains("#f2qb0"));
    let flag_rendered = emitted_entry(emission.pack(), &output, flag);
    assert!(flag_rendered.contains("#f2qi0"));
    assert!(!flag_rendered.contains("#f2qb0"));
    assert!(flag_rendered.contains("#f2r0"));

    let both_section = report_function_section(&report, "overflow_both");
    assert!(both_section.contains("result 0 semantic"));
    assert!(both_section.contains("result 1 semantic"));
    assert!(!both_section.contains("recipe-temporary"));
    assert!(!report.contains("#f3qb0"));
    assert!(!report.contains("#f3qi0"));
    let both_rendered = emitted_entry(emission.pack(), &output, both);
    assert!(!both_rendered.contains("#f3q"));
    assert!(both_rendered.contains("#f3r0"));
    assert!(both_rendered.contains("#f3r1"));

    for function in [sum, flag, both] {
        let rendered = emitted_entry(emission.pack(), &output, function);
        assert!(rendered.contains("scoreboard players operation"));
        assert!(rendered.lines().count() > 1);
    }
}

#[test]
fn baseline_keeps_effectful_calls_but_copies_only_demanded_result_indices() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let callee = declare(
        &mut core,
        "two_results",
        vec![],
        vec![CoreType::Bool, CoreType::I32],
    );
    let unused = declare(&mut core, "unused_call_results", vec![], vec![]);
    let later = declare(&mut core, "later_call_result", vec![], vec![CoreType::I32]);

    let mut callee_builder = FunctionBuilder::new(&core, &sources, callee).unwrap();
    let boolean = callee_builder
        .bool_constant(true, OriginId::UNKNOWN)
        .unwrap();
    let integer = callee_builder.i32_constant(73, OriginId::UNKNOWN).unwrap();
    return_values(&mut callee_builder, vec![boolean, integer]);
    core.define_function(callee, callee_builder.finish().unwrap())
        .unwrap();

    let mut unused_builder = FunctionBuilder::new(&core, &sources, unused).unwrap();
    unused_builder
        .call(callee, vec![], OriginId::UNKNOWN)
        .unwrap();
    return_values(&mut unused_builder, vec![]);
    core.define_function(unused, unused_builder.finish().unwrap())
        .unwrap();

    let mut later_builder = FunctionBuilder::new(&core, &sources, later).unwrap();
    let results = later_builder
        .call(callee, vec![], OriginId::UNKNOWN)
        .unwrap();
    return_values(&mut later_builder, vec![results[1]]);
    core.define_function(later, later_builder.finish().unwrap())
        .unwrap();

    let output = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    let report = output.dump_lowering();
    let emission = emit_datapack(
        output.program(),
        &sources,
        &EmissionOptions::new("stage 5 indexed calls"),
    )
    .unwrap();
    let unused_section = report_function_section(&report, "unused_call_results");
    assert!(unused_section.contains("instruction 0 call"));
    assert!(unused_section.contains("result-slot 0 omitted"));
    assert!(unused_section.contains("result-slot 1 omitted"));
    let unused_rendered = emitted_entry(emission.pack(), &output, unused);
    assert_eq!(
        unused_rendered.matches("function mdl:__mdl/f0/b0").count(),
        1
    );
    assert!(!unused_rendered.contains("#f0r0"));
    assert!(!unused_rendered.contains("#f0r1"));
    assert_eq!(unused_rendered.lines().count(), 2);

    let later_section = report_function_section(&report, "later_call_result");
    assert!(later_section.contains("result-slot 0 omitted"));
    assert!(later_section.contains("result-slot 1 semantic result=1"));
    let later_rendered = emitted_entry(emission.pack(), &output, later);
    assert_eq!(
        later_rendered.matches("function mdl:__mdl/f0/b0").count(),
        1
    );
    assert!(!later_rendered.contains("#f0r0"));
    assert!(later_rendered.contains("= #f0r1 mdl.reg"));
    assert!(later_rendered.contains("#f2r0"));
}

#[test]
fn baseline_materializes_demanded_typed_cycles_and_erases_unused_cycles() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let demanded = declare(
        &mut core,
        "demanded_cycles",
        vec![CoreType::Bool, CoreType::Bool, CoreType::I32, CoreType::I32],
        vec![CoreType::Bool, CoreType::Bool, CoreType::I32, CoreType::I32],
    );
    let unused = declare(
        &mut core,
        "unused_cycle",
        vec![CoreType::Bool, CoreType::I32, CoreType::I32],
        vec![],
    );
    define_demanded_cycles(&mut core, &sources, demanded);
    define_unused_cycle(&mut core, &sources, unused);

    let output = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    let report = output.dump_lowering();
    let emission = emit_datapack(
        output.program(),
        &sources,
        &EmissionOptions::new("stage 5 typed edge cycles"),
    )
    .unwrap();
    let demanded_section = report_function_section(&report, "demanded_cycles");
    assert!(demanded_section.contains("edge-temporary"));
    assert!(demanded_section.contains("holder=#f0tb0 type=Bool"));
    assert!(demanded_section.contains("holder=#f0ti0 type=I32"));
    assert!(!demanded_section.contains("type=untyped"));
    let demanded_backedge_resource = block_resource(&output, demanded, 2);
    let demanded_rendered = emitted_resource(emission.pack(), &demanded_backedge_resource);
    assert_eq!(demanded_rendered.matches("#f0tb0").count(), 2);
    assert_eq!(demanded_rendered.matches("#f0ti0").count(), 2);
    assert!(!demanded_rendered.contains("#f0t0"));
    assert_eq!(
        demanded_rendered
            .matches("scoreboard players operation")
            .count(),
        6
    );

    let unused_section = report_function_section(&report, "unused_cycle");
    assert!(!unused_section.contains("edge-temporary"));
    assert!(!report.contains("#f1tb0"));
    assert!(!report.contains("#f1ti0"));
    let unused_backedge_resource = block_resource(&output, unused, 2);
    let unused_rendered = emitted_resource(emission.pack(), &unused_backedge_resource);
    assert!(!unused_rendered.contains("scoreboard players operation"));
    assert_eq!(unused_rendered, "return run function mdl:__mdl/f1/b1\n");
}

#[test]
fn baseline_lowering_and_datapack_emission_are_repeatedly_deterministic() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let function = declare(
        &mut core,
        "deterministic",
        vec![CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
    );
    let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
    let parameters = entry_parameters(&builder);
    let (sum, _) = builder
        .i32_add_overflowing(parameters[0], parameters[1], OriginId::UNKNOWN)
        .unwrap();
    return_values(&mut builder, vec![sum]);
    core.define_function(function, builder.finish().unwrap())
        .unwrap();

    let first = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    let second = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    assert_eq!(first.dump_lowering(), second.dump_lowering());
    assert_eq!(first.map(), second.map());
    assert_eq!(
        MinecraftDebugDumper::program(first.program()),
        MinecraftDebugDumper::program(second.program())
    );

    let emission_options = EmissionOptions::new("stage 5 deterministic baseline");
    let first_emission = emit_datapack(first.program(), &sources, &emission_options).unwrap();
    let second_emission = emit_datapack(second.program(), &sources, &emission_options).unwrap();
    assert_eq!(first_emission.pack(), second_emission.pack());
    assert_eq!(first_emission.footprint(), second_emission.footprint());
    assert_eq!(
        first_emission.trace().records().collect::<Vec<_>>(),
        second_emission.trace().records().collect::<Vec<_>>()
    );
}

#[test]
fn baseline_coalescing_removes_one_real_home_and_edge_copy() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let function = declare(&mut core, "coalesced_copy", vec![], vec![CoreType::I32]);
    let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
    let source = builder.i32_constant(37, OriginId::UNKNOWN).unwrap();
    let join = builder.create_block(OriginId::UNKNOWN).unwrap();
    let parameter = append_parameter(&mut builder, join, CoreType::I32);
    jump(&mut builder, join, vec![source]);
    builder.switch_to_block(join).unwrap();
    return_values(&mut builder, vec![parameter]);
    core.define_function(function, builder.finish().unwrap())
        .unwrap();

    let none = lower_to_minecraft(
        &core,
        &sources,
        &baseline_options().with_optimization_level(MinecraftOptimizationLevel::None),
    )
    .unwrap();
    let baseline = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    let none_report = none.dump_lowering();
    let baseline_report = baseline.dump_lowering();
    assert_eq!(
        none_report
            .lines()
            .filter(|line| line.starts_with("home "))
            .count(),
        3
    );
    assert_eq!(
        baseline_report
            .lines()
            .filter(|line| line.starts_with("home "))
            .count(),
        2
    );
    assert!(baseline_report.contains("coalescing tracked-values=2"));
    assert!(baseline_report.contains("candidates=1 merges=1"));

    let emission_options = EmissionOptions::new("stage 5 real coalescing");
    let none_pack = emit_datapack(none.program(), &sources, &emission_options).unwrap();
    let baseline_pack = emit_datapack(baseline.program(), &sources, &emission_options).unwrap();
    let none_entry = emitted_entry(none_pack.pack(), &none, function);
    let baseline_entry = emitted_entry(baseline_pack.pack(), &baseline, function);
    assert_eq!(
        none_entry.matches("scoreboard players operation").count(),
        baseline_entry
            .matches("scoreboard players operation")
            .count()
            + 1
    );
    assert!(baseline_entry.lines().count() < none_entry.lines().count());
}

#[test]
fn baseline_handles_twenty_thousand_values_across_many_functions_without_dense_output() {
    const EMPTY_FUNCTIONS: usize = 32;
    const VALUES: usize = 20_000;

    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let mut empties = Vec::with_capacity(EMPTY_FUNCTIONS);
    for index in 0..EMPTY_FUNCTIONS {
        empties.push(declare(
            &mut core,
            &format!("empty_{index}"),
            vec![],
            vec![],
        ));
    }
    let scale = declare(&mut core, "scale", vec![], vec![CoreType::I32]);
    for function in empties {
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        return_values(&mut builder, vec![]);
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
    }
    let mut builder = FunctionBuilder::new(&core, &sources, scale).unwrap();
    let mut last = None;
    for value in 0..VALUES {
        last = Some(
            builder
                .i32_constant(i32::try_from(value).unwrap(), OriginId::UNKNOWN)
                .unwrap(),
        );
    }
    return_values(&mut builder, vec![last.unwrap()]);
    core.define_function(scale, builder.finish().unwrap())
        .unwrap();

    let output = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    assert_eq!(output.map().len(), EMPTY_FUNCTIONS + 1);
    assert_eq!(output.program().functions().len(), EMPTY_FUNCTIONS + 3);
    let emission = emit_datapack(
        output.program(),
        &sources,
        &EmissionOptions::new("stage 5 scale"),
    )
    .unwrap();
    let scale_rendered = emitted_entry(emission.pack(), &output, scale);
    assert_eq!(scale_rendered.lines().count(), 3);
    assert!(scale_rendered.contains(" 19999"));
    assert!(!scale_rendered.contains(" 19998"));
    assert_eq!(scale_rendered.matches("scoreboard players set").count(), 1);
    assert_eq!(
        scale_rendered
            .matches("scoreboard players operation")
            .count(),
        1
    );
}

#[derive(Clone, Copy)]
enum OverflowMask {
    Neither,
    Sum,
    Flag,
    Both,
}

fn define_overflow_mask(
    core: &mut CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    mask: OverflowMask,
) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let parameters = entry_parameters(&builder);
    let (sum, flag) = builder
        .i32_add_overflowing(parameters[0], parameters[1], OriginId::UNKNOWN)
        .unwrap();
    let results = match mask {
        OverflowMask::Neither => vec![],
        OverflowMask::Sum => vec![sum],
        OverflowMask::Flag => vec![flag],
        OverflowMask::Both => vec![sum, flag],
    };
    return_values(&mut builder, results);
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn define_demanded_cycles(core: &mut CoreProgram, sources: &SourceContext, function: FunctionId) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let initial = entry_parameters(&builder);
    let header = builder.create_block(OriginId::UNKNOWN).unwrap();
    let boolean_left = append_parameter(&mut builder, header, CoreType::Bool);
    let boolean_right = append_parameter(&mut builder, header, CoreType::Bool);
    let integer_left = append_parameter(&mut builder, header, CoreType::I32);
    let integer_right = append_parameter(&mut builder, header, CoreType::I32);
    let backedge = builder.create_block(OriginId::UNKNOWN).unwrap();
    let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
    let exit_boolean_left = append_parameter(&mut builder, exit, CoreType::Bool);
    let exit_boolean_right = append_parameter(&mut builder, exit, CoreType::Bool);
    let exit_integer_left = append_parameter(&mut builder, exit, CoreType::I32);
    let exit_integer_right = append_parameter(&mut builder, exit, CoreType::I32);

    jump(&mut builder, header, initial);
    builder.switch_to_block(header).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: boolean_left,
                then_target: BlockTarget::new(
                    exit,
                    vec![boolean_left, boolean_right, integer_left, integer_right],
                ),
                else_target: BlockTarget::new(backedge, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(backedge).unwrap();
    jump(
        &mut builder,
        header,
        vec![boolean_right, boolean_left, integer_right, integer_left],
    );
    builder.switch_to_block(exit).unwrap();
    return_values(
        &mut builder,
        vec![
            exit_boolean_left,
            exit_boolean_right,
            exit_integer_left,
            exit_integer_right,
        ],
    );
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn define_unused_cycle(core: &mut CoreProgram, sources: &SourceContext, function: FunctionId) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let initial = entry_parameters(&builder);
    let header = builder.create_block(OriginId::UNKNOWN).unwrap();
    let condition = append_parameter(&mut builder, header, CoreType::Bool);
    let left = append_parameter(&mut builder, header, CoreType::I32);
    let right = append_parameter(&mut builder, header, CoreType::I32);
    let backedge = builder.create_block(OriginId::UNKNOWN).unwrap();
    let exit = builder.create_block(OriginId::UNKNOWN).unwrap();

    jump(&mut builder, header, initial);
    builder.switch_to_block(header).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(exit, vec![]),
                else_target: BlockTarget::new(backedge, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(backedge).unwrap();
    jump(&mut builder, header, vec![condition, right, left]);
    builder.switch_to_block(exit).unwrap();
    return_values(&mut builder, vec![]);
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn declare(
    core: &mut CoreProgram,
    name: &str,
    parameters: Vec<CoreType>,
    results: Vec<CoreType>,
) -> FunctionId {
    core.declare_function(Some(name), parameters, results, OriginId::UNKNOWN)
        .unwrap()
}

fn entry_parameters(builder: &FunctionBuilder<'_>) -> Vec<ValueId> {
    builder
        .body()
        .block(builder.entry_block())
        .unwrap()
        .parameters()
        .iter()
        .map(mdl_compiler::ir::core::BlockParam::value)
        .collect()
}

fn append_parameter(builder: &mut FunctionBuilder<'_>, block: BlockId, ty: CoreType) -> ValueId {
    builder
        .append_block_parameter(block, ty, OriginId::UNKNOWN)
        .unwrap()
}

fn jump(builder: &mut FunctionBuilder<'_>, block: BlockId, arguments: Vec<ValueId>) {
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(block, arguments)),
            OriginId::UNKNOWN,
        ))
        .unwrap();
}

fn return_values(builder: &mut FunctionBuilder<'_>, values: Vec<ValueId>) {
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(values),
            OriginId::UNKNOWN,
        ))
        .unwrap();
}

fn baseline_options() -> LoweringOptions {
    LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl").unwrap(),
        ObjectiveName::new("mdl.reg").unwrap(),
    )
    .unwrap()
    .with_optimization_level(MinecraftOptimizationLevel::Baseline)
}

fn emitted_entry(pack: &DatapackArtifact, output: &LoweringOutput, function: FunctionId) -> String {
    let resource = output
        .map()
        .function(function)
        .unwrap()
        .entry_resource()
        .to_string();
    emitted_resource(pack, &resource)
}

fn block_resource(output: &LoweringOutput, function: FunctionId, block_index: usize) -> String {
    let entry = output
        .map()
        .function(function)
        .unwrap()
        .entry_resource()
        .to_string();
    entry.strip_suffix("/b0").map_or_else(
        || panic!("unexpected generated entry resource {entry}"),
        |prefix| format!("{prefix}/b{block_index}"),
    )
}

fn emitted_resource(pack: &DatapackArtifact, resource: &str) -> String {
    let (namespace, path) = resource
        .split_once(':')
        .unwrap_or_else(|| panic!("invalid generated function resource {resource}"));
    let artifact_path = format!("data/{namespace}/function/{path}.mcfunction");
    std::str::from_utf8(
        pack.file(&artifact_path)
            .unwrap_or_else(|| panic!("missing generated function {resource}"))
            .bytes(),
    )
    .unwrap()
    .to_owned()
}

fn report_function_section<'a>(report: &'a str, name: &str) -> &'a str {
    let marker = format!("name=Some(\"{name}\")");
    let marker_start = report
        .find(&marker)
        .unwrap_or_else(|| panic!("missing report function {name}"));
    let start = report[..marker_start]
        .rfind("\nfunction ")
        .map_or(0, |index| index + 1);
    let rest = &report[start..];
    let end = rest[1..]
        .find("\nfunction ")
        .map_or(rest.len(), |index| index + 1);
    &rest[..end]
}
