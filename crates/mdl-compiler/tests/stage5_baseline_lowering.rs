use mdl_compiler::analysis::minecraft::{
    AnalysisArithmeticCaps, CountBound, CountUpperKind, ResolvedTargetExecutionRoot,
    RootExecutionSummary, TargetExecutionAnalysisCompletion, TargetExecutionAnalysisLimits,
    TargetExecutionCostReport,
};
use mdl_compiler::datapack::{DatapackArtifact, EmissionOptions, TraceMap, emit_datapack};
use mdl_compiler::ir::core::{
    BlockId, BlockTarget, CoreFunctionLinkage, CoreProgram, CoreType, FunctionBuilder, FunctionId,
    Terminator, TerminatorKind, ValueId,
};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    LoweringOptions, LoweringOutput, MinecraftOptimizationLevel, lower_to_minecraft,
};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions, optimize_core};
use mdl_compiler::source::{Origin, OriginId, SourceContext};
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
    assert_eq!(first.report(), second.report());
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
        first_emission.footprint().dump(),
        second_emission.footprint().dump()
    );
    assert_eq!(
        first_emission.trace().records().collect::<Vec<_>>(),
        second_emission.trace().records().collect::<Vec<_>>()
    );
    let limits = target_analysis_limits(&first);
    let first_cost = first.analyze_target_execution(limits).unwrap();
    let second_cost = second.analyze_target_execution(limits).unwrap();
    assert_eq!(first_cost, second_cost);
    for deterministic_dump in [
        first.report().dump(),
        first_cost.dump(),
        first_emission.footprint().dump(),
    ] {
        assert!(!deterministic_dump.contains("elapsed"));
        assert!(!deterministic_dump.contains("wall-time"));
        assert!(!deterministic_dump.contains("duration"));
    }
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
    let statistics = output.report().statistics();
    assert_eq!(statistics.core_functions(), EMPTY_FUNCTIONS + 1);
    assert_eq!(statistics.target_functions(), EMPTY_FUNCTIONS + 3);
    assert_eq!(statistics.branch_arms(), 0);
    assert_eq!(statistics.selected_recipes(), 0);
    assert_eq!(statistics.consumed_blocks(), 0);
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

#[test]
fn baseline_verifies_twenty_thousand_sparse_realizations() {
    const FUNCTIONS: usize = 1_000;
    const ADDITIONS: usize = 10;
    const VALUES: usize = FUNCTIONS * (ADDITIONS * 2 + 1);

    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let functions = (0..FUNCTIONS)
        .map(|index| {
            declare(
                &mut core,
                &format!("sparse_scale_{index}"),
                vec![],
                vec![CoreType::I32],
            )
        })
        .collect::<Vec<_>>();
    for function in functions {
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let mut sum = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        for value in 0..ADDITIONS {
            let value = builder
                .i32_constant(i32::try_from(value).unwrap(), OriginId::UNKNOWN)
                .unwrap();
            sum = builder
                .i32_add_wrapping(sum, value, OriginId::UNKNOWN)
                .unwrap();
        }
        return_values(&mut builder, vec![sum]);
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
    }

    let output = lower_to_minecraft(&core, &sources, &baseline_options()).unwrap();
    let statistics = output.report().statistics();
    assert_eq!(statistics.realizations(), VALUES);
    assert_eq!(statistics.materializations(), FUNCTIONS * (ADDITIONS + 1));
    assert_eq!(statistics.homes(), VALUES + FUNCTIONS);
}

#[test]
fn baseline_contracts_a_unique_then_arm_terminal_call_through_the_real_target_pipeline() {
    assert_terminal_call_contraction(TerminalCallArms::Then);
}

#[test]
fn baseline_contracts_a_unique_else_arm_terminal_call_through_the_real_target_pipeline() {
    assert_terminal_call_contraction(TerminalCallArms::Else);
}

#[test]
fn baseline_contracts_both_independent_terminal_call_arms() {
    assert_terminal_call_contraction(TerminalCallArms::Both);
}

#[test]
fn baseline_reports_and_preserves_a_terminal_call_block_with_two_incoming_occurrences() {
    let mut sources = SourceContext::new();
    let fixture = terminal_call_fixture(&mut sources, TerminalCallArms::Shared);
    let none = lower_to_minecraft(
        &fixture.core,
        &sources,
        &baseline_options().with_optimization_level(MinecraftOptimizationLevel::None),
    )
    .unwrap();
    let baseline = lower_to_minecraft(&fixture.core, &sources, &baseline_options()).unwrap();
    let repeated = lower_to_minecraft(&fixture.core, &sources, &baseline_options()).unwrap();
    let report = baseline.dump_lowering();

    assert_eq!(report, repeated.dump_lowering());
    assert!(report.contains("control branch-arms=2 selected=0 consumed=0"));
    assert_eq!(
        report
            .matches("reason=incoming-edge-occurrence-count(actual=2)")
            .count(),
        2
    );
    assert!(!report.contains("recipe=InlineZeroAbiTerminalCall consumed="));

    let options = EmissionOptions::new("stage 5 shared terminal-call rejection");
    let none_emission = emit_datapack(none.program(), &sources, &options).unwrap();
    let baseline_emission = emit_datapack(baseline.program(), &sources, &options).unwrap();
    let shared_resource = block_resource(&baseline, fixture.caller, 1);
    assert_eq!(
        emitted_resource(baseline_emission.pack(), &shared_resource),
        format!(
            "function {}\nreturn 1\n",
            fixture.callee_resource(&baseline)
        )
    );
    assert_eq!(none_emission.pack(), baseline_emission.pack());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalCallArms {
    Then,
    Else,
    Both,
    Shared,
}

struct TerminalCallFixture {
    core: CoreProgram,
    callee: FunctionId,
    caller: FunctionId,
    consumed_blocks: Vec<usize>,
    then_block: usize,
    else_block: usize,
    origins: TerminalCallOrigins,
}

#[derive(Clone, Copy)]
struct TerminalCallOrigins {
    scalar: OriginId,
    branch: OriginId,
    then_call: OriginId,
    then_return: OriginId,
    else_call: OriginId,
    else_return: OriginId,
    callee_return: OriginId,
}

impl TerminalCallFixture {
    fn callee_resource(&self, output: &LoweringOutput) -> String {
        output
            .map()
            .function(self.callee)
            .unwrap()
            .entry_resource()
            .to_string()
    }

    fn arm_origins(&self, block: usize) -> (OriginId, OriginId) {
        if block == self.then_block {
            (self.origins.then_call, self.origins.then_return)
        } else if block == self.else_block {
            (self.origins.else_call, self.origins.else_return)
        } else {
            panic!("block {block} is not a terminal-call fixture arm")
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one end-to-end assertion keeps report, artifact, trace, and whole-target cost evidence for the same closed recipe fixture together"
)]
fn assert_terminal_call_contraction(arms: TerminalCallArms) {
    let mut sources = SourceContext::new();
    let fixture = terminal_call_fixture(&mut sources, arms);
    let none = lower_to_minecraft(
        &fixture.core,
        &sources,
        &baseline_options().with_optimization_level(MinecraftOptimizationLevel::None),
    )
    .unwrap();
    let baseline = lower_to_minecraft(&fixture.core, &sources, &baseline_options()).unwrap();
    let repeated = lower_to_minecraft(&fixture.core, &sources, &baseline_options()).unwrap();

    let report = baseline.dump_lowering();
    assert_eq!(report, repeated.dump_lowering());
    assert_eq!(baseline.report(), repeated.report());
    let statistics = baseline.report().statistics();
    let selected = u64::try_from(fixture.consumed_blocks.len()).unwrap();
    assert_eq!(statistics.branch_arms(), 2);
    assert_eq!(statistics.selected_recipes(), selected);
    assert_eq!(statistics.consumed_blocks(), selected);
    assert!(report.contains(&format!(
        "control branch-arms=2 selected={} consumed={}",
        fixture.consumed_blocks.len(),
        fixture.consumed_blocks.len()
    )));
    assert_eq!(
        report
            .matches("recipe=InlineZeroAbiTerminalCall consumed=")
            .count(),
        fixture.consumed_blocks.len()
    );
    assert_eq!(
        report.matches("advantage=runtime-dominance").count(),
        fixture.consumed_blocks.len()
    );
    assert_eq!(
        report
            .matches("impact=NonIncreasing(UniqueTerminalArmContraction)")
            .count(),
        fixture.consumed_blocks.len()
    );
    assert_eq!(
        report
            .matches("completion=ExactlyOneOnNormalCompletion")
            .count(),
        fixture.consumed_blocks.len()
    );
    assert_eq!(
        report
            .matches("cost baseline kind=ReturnDispatcher")
            .count(),
        fixture.consumed_blocks.len()
    );
    assert_eq!(
        report
            .matches("cost selected kind=InlineZeroAbiTerminalCall")
            .count(),
        fixture.consumed_blocks.len()
    );
    for block in &fixture.consumed_blocks {
        let (call, terminal_return) = fixture.arm_origins(*block);
        assert!(report.contains(&format!(
            "origins=[{:?},{call:?},{terminal_return:?}]",
            fixture.origins.branch
        )));
    }

    let emission_options = EmissionOptions::new("stage 5 terminal-call contraction");
    let none_emission = emit_datapack(none.program(), &sources, &emission_options).unwrap();
    let baseline_emission = emit_datapack(baseline.program(), &sources, &emission_options).unwrap();
    let repeated_emission = emit_datapack(repeated.program(), &sources, &emission_options).unwrap();
    assert_eq!(baseline_emission.pack(), repeated_emission.pack());

    let callee_resource = fixture.callee_resource(&baseline);
    for block in &fixture.consumed_blocks {
        let resource = block_resource(&none, fixture.caller, *block);
        assert_eq!(
            emitted_resource(none_emission.pack(), &resource),
            format!("function {callee_resource}\nreturn 1\n")
        );
        let path = function_pack_path(&resource);
        assert!(
            baseline_emission.pack().file(&path).is_none(),
            "consumed block resource {resource} remained in the Baseline artifact"
        );
    }

    let none_entry = emitted_entry(none_emission.pack(), &none, fixture.caller);
    let baseline_entry = emitted_entry(baseline_emission.pack(), &baseline, fixture.caller);
    let then_resource =
        selected_arm_resource(&baseline, &fixture, fixture.then_block, &callee_resource);
    let else_resource =
        selected_arm_resource(&baseline, &fixture, fixture.else_block, &callee_resource);
    let scalar_lines = usize::from(arms == TerminalCallArms::Then) * 2;
    assert_final_dispatcher(
        &none_entry,
        &block_resource(&none, fixture.caller, fixture.then_block),
        &block_resource(&none, fixture.caller, fixture.else_block),
        scalar_lines,
    );
    assert_final_dispatcher(
        &baseline_entry,
        &then_resource,
        &else_resource,
        scalar_lines,
    );
    if arms == TerminalCallArms::Then {
        let scalar_prefix = baseline_entry.lines().take(2).collect::<Vec<_>>();
        assert_eq!(scalar_prefix.len(), 2);
        assert!(
            scalar_prefix
                .iter()
                .all(|line| line.starts_with("scoreboard players "))
        );
        assert!(scalar_prefix[0].contains(" set "));
        assert!(scalar_prefix[1].contains(" operation "));
    }
    assert_terminal_call_trace(
        none_emission.trace(),
        baseline_emission.trace(),
        &none,
        &baseline,
        &fixture,
        arms,
    );

    let none_paths = artifact_paths(none_emission.pack());
    let baseline_paths = artifact_paths(baseline_emission.pack());
    let removed_paths = none_paths
        .iter()
        .filter(|path| !baseline_paths.contains(path))
        .cloned()
        .collect::<Vec<_>>();
    let expected_removed_paths = fixture
        .consumed_blocks
        .iter()
        .map(|block| function_pack_path(&block_resource(&none, fixture.caller, *block)))
        .collect::<Vec<_>>();
    assert_eq!(removed_paths, expected_removed_paths);
    assert!(baseline_paths.iter().all(|path| none_paths.contains(path)));

    assert_target_cost_contraction(
        &none,
        &baseline,
        fixture.callee,
        fixture.caller,
        fixture.consumed_blocks.len(),
    );
}

fn terminal_call_fixture(
    sources: &mut SourceContext,
    arms: TerminalCallArms,
) -> TerminalCallFixture {
    let origins = terminal_call_origins(sources);
    let mut core = CoreProgram::new();
    let terminal = declare_export(&mut core, "terminal_callee", vec![], vec![]);
    let dispatcher = declare_export(
        &mut core,
        "terminal_dispatcher",
        vec![CoreType::Bool],
        vec![],
    );

    let mut callee_builder = FunctionBuilder::new(&core, sources, terminal).unwrap();
    callee_builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            origins.callee_return,
        ))
        .unwrap();
    core.define_function(terminal, callee_builder.finish().unwrap())
        .unwrap();

    let mut builder = FunctionBuilder::new(&core, sources, dispatcher).unwrap();
    let input_condition = entry_parameters(&builder)[0];
    let condition = if arms == TerminalCallArms::Then {
        builder.bool_not(input_condition, origins.scalar).unwrap()
    } else {
        input_condition
    };
    let then_block = builder.create_block(origins.then_call).unwrap();
    let else_block = if arms == TerminalCallArms::Shared {
        then_block
    } else {
        builder.create_block(origins.else_call).unwrap()
    };
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            origins.branch,
        ))
        .unwrap();

    builder.switch_to_block(then_block).unwrap();
    if matches!(
        arms,
        TerminalCallArms::Then | TerminalCallArms::Both | TerminalCallArms::Shared
    ) {
        builder.call(terminal, vec![], origins.then_call).unwrap();
    }
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            origins.then_return,
        ))
        .unwrap();

    if else_block != then_block {
        builder.switch_to_block(else_block).unwrap();
        if matches!(arms, TerminalCallArms::Else | TerminalCallArms::Both) {
            builder.call(terminal, vec![], origins.else_call).unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                origins.else_return,
            ))
            .unwrap();
    }
    core.define_function(dispatcher, builder.finish().unwrap())
        .unwrap();

    let consumed_blocks = match arms {
        TerminalCallArms::Then => vec![1],
        TerminalCallArms::Else => vec![2],
        TerminalCallArms::Both => vec![1, 2],
        TerminalCallArms::Shared => vec![],
    };
    TerminalCallFixture {
        core,
        callee: terminal,
        caller: dispatcher,
        consumed_blocks,
        then_block: 1,
        else_block: usize::from(arms != TerminalCallArms::Shared) + 1,
        origins,
    }
}

fn terminal_call_origins(sources: &mut SourceContext) -> TerminalCallOrigins {
    let file = sources.add_file("terminal-call.mdl", "sbtTrRe").unwrap();
    let mut source_origin = |offset| {
        let span = sources.span(file, offset, offset + 1).unwrap();
        sources.add_origin(Origin::Source(span)).unwrap()
    };
    let origins = TerminalCallOrigins {
        scalar: source_origin(0),
        branch: source_origin(1),
        then_call: source_origin(2),
        then_return: source_origin(3),
        else_call: source_origin(4),
        else_return: source_origin(5),
        callee_return: source_origin(6),
    };
    let ids = [
        origins.scalar,
        origins.branch,
        origins.then_call,
        origins.then_return,
        origins.else_call,
        origins.else_return,
        origins.callee_return,
    ];
    assert!(ids.iter().all(|origin| *origin != OriginId::UNKNOWN));
    for (index, origin) in ids.iter().enumerate() {
        assert!(!ids[..index].contains(origin));
    }
    origins
}

fn selected_arm_resource(
    output: &LoweringOutput,
    fixture: &TerminalCallFixture,
    block: usize,
    callee_resource: &str,
) -> String {
    if fixture.consumed_blocks.contains(&block) {
        callee_resource.to_owned()
    } else {
        block_resource(output, fixture.caller, block)
    }
}

fn assert_final_dispatcher(
    rendered: &str,
    then_resource: &str,
    else_resource: &str,
    scalar_lines: usize,
) {
    let lines = rendered.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), scalar_lines + 2);
    let [conditional, fallback] = &lines[scalar_lines..] else {
        panic!("branch source did not end in exactly two dispatcher commands")
    };
    assert!(conditional.starts_with("execute if score "));
    assert!(conditional.contains(" mdl.reg matches 1 run "));
    assert!(conditional.ends_with(&format!("return run function {then_resource}")));
    assert_eq!(fallback, &format!("return run function {else_resource}"));
}

fn assert_terminal_call_trace(
    none_trace: &TraceMap,
    baseline_trace: &TraceMap,
    none: &LoweringOutput,
    baseline: &LoweringOutput,
    fixture: &TerminalCallFixture,
    arms: TerminalCallArms,
) {
    let none_entry = none
        .map()
        .function(fixture.caller)
        .unwrap()
        .entry_resource()
        .to_string();
    let baseline_entry = baseline
        .map()
        .function(fixture.caller)
        .unwrap()
        .entry_resource()
        .to_string();
    let mut none_entry_origins = Vec::new();
    let mut baseline_entry_origins = Vec::new();
    if arms == TerminalCallArms::Then {
        none_entry_origins.extend([fixture.origins.scalar; 2]);
        baseline_entry_origins.extend([fixture.origins.scalar; 2]);
    }
    none_entry_origins.extend([fixture.origins.branch; 2]);
    baseline_entry_origins.push(fixture.origins.branch);
    baseline_entry_origins.push(if fixture.consumed_blocks.contains(&fixture.else_block) {
        fixture.origins.else_call
    } else {
        fixture.origins.branch
    });
    assert_eq!(trace_origins(none_trace, &none_entry), none_entry_origins);
    assert_eq!(
        trace_origins(baseline_trace, &baseline_entry),
        baseline_entry_origins
    );

    for block in &fixture.consumed_blocks {
        let resource = block_resource(none, fixture.caller, *block);
        let (call, terminal_return) = fixture.arm_origins(*block);
        assert_eq!(
            trace_origins(none_trace, &resource),
            vec![call, terminal_return]
        );
        assert!(trace_origins(baseline_trace, &resource).is_empty());
    }
}

fn trace_origins(trace: &TraceMap, resource: &str) -> Vec<OriginId> {
    let resource = mdl_compiler::ir::minecraft::FunctionResourceId::parse(resource).unwrap();
    trace
        .records()
        .filter(|record| record.function() == &resource)
        .map(mdl_compiler::datapack::TraceRecord::origin)
        .collect()
}

fn artifact_paths(pack: &DatapackArtifact) -> Vec<String> {
    pack.files()
        .iter()
        .map(|file| file.path().as_str().to_owned())
        .collect()
}

fn function_pack_path(resource: &str) -> String {
    let (namespace, path) = resource
        .split_once(':')
        .unwrap_or_else(|| panic!("invalid generated function resource {resource}"));
    format!("data/{namespace}/function/{path}.mcfunction")
}

fn assert_target_cost_contraction(
    none: &LoweringOutput,
    baseline: &LoweringOutput,
    terminal: FunctionId,
    dispatcher: FunctionId,
    consumed_blocks: usize,
) {
    let limits = target_analysis_limits(none);
    let none_cost = none.analyze_target_execution(limits).unwrap();
    let baseline_cost = baseline.analyze_target_execution(limits).unwrap();
    assert_eq!(
        none_cost.completion(),
        TargetExecutionAnalysisCompletion::Complete
    );
    assert_eq!(
        baseline_cost.completion(),
        TargetExecutionAnalysisCompletion::Complete
    );
    assert_census_contraction(&none_cost, &baseline_cost, consumed_blocks);
    assert_eq!(none_cost.roots().len(), baseline_cost.roots().len());
    for (label, function) in [("callee", terminal), ("caller", dispatcher)] {
        assert_root_cost_nonincreasing(
            label,
            public_root(&none_cost, none, function),
            public_root(&baseline_cost, baseline, function),
        );
    }
    assert_root_cost_nonincreasing(
        "load-tag",
        non_function_root(&none_cost),
        non_function_root(&baseline_cost),
    );

    let caller_before = public_root(&none_cost, none, dispatcher);
    let caller_after = public_root(&baseline_cost, baseline, dispatcher);
    assert!(
        finite_upper(caller_after.sequence_operations())
            < finite_upper(caller_before.sequence_operations())
    );
    assert!(
        finite_upper(caller_after.internal_function_invocations())
            < finite_upper(caller_before.internal_function_invocations())
    );
}

fn target_analysis_limits(output: &LoweringOutput) -> TargetExecutionAnalysisLimits {
    let assumptions = output
        .map()
        .execution_contract()
        .command_limits()
        .configured_assumptions();
    TargetExecutionAnalysisLimits::new(
        AnalysisArithmeticCaps::minimum_for(assumptions),
        100_000,
        100_000,
    )
}

fn assert_census_contraction(
    none: &TargetExecutionCostReport,
    baseline: &TargetExecutionCostReport,
    consumed_blocks: usize,
) {
    let before = none.census();
    let after = baseline.census();
    assert_eq!(before.functions() - after.functions(), consumed_blocks);
    assert_eq!(
        before.top_level_commands() - after.top_level_commands(),
        consumed_blocks * 2
    );
    assert_eq!(
        before.command_nodes() - after.command_nodes(),
        consumed_blocks * 2
    );
    assert_eq!(
        before.function_calls() - after.function_calls(),
        consumed_blocks
    );
    assert_eq!(
        before.return_commands() - after.return_commands(),
        consumed_blocks
    );
    assert_eq!(before.execute_stages(), after.execute_stages());
    assert_eq!(before.score_commands(), after.score_commands());
    assert_eq!(before.data_commands(), after.data_commands());
    assert_eq!(before.raw_commands(), after.raw_commands());
}

fn public_root<'a>(
    report: &'a TargetExecutionCostReport,
    output: &LoweringOutput,
    function: FunctionId,
) -> &'a RootExecutionSummary {
    let resource = output.map().function(function).unwrap().entry_resource();
    let target = output
        .program()
        .functions()
        .find_map(|(function, data)| (data.resource() == resource).then_some(function))
        .unwrap_or_else(|| panic!("missing target entry resource {resource}"));
    report
        .roots()
        .iter()
        .find(|root| {
            matches!(
                root.root(),
                ResolvedTargetExecutionRoot::Function(function) if *function == target
            )
        })
        .unwrap_or_else(|| panic!("missing target-cost root for {resource}"))
}

fn non_function_root(report: &TargetExecutionCostReport) -> &RootExecutionSummary {
    let mut roots = report
        .roots()
        .iter()
        .filter(|root| !matches!(root.root(), ResolvedTargetExecutionRoot::Function(_)));
    let root = roots.next().expect("missing generated load-tag cost root");
    assert!(roots.next().is_none(), "multiple non-function cost roots");
    root
}

fn assert_root_cost_nonincreasing(
    root: &str,
    before: &RootExecutionSummary,
    after: &RootExecutionSummary,
) {
    for (metric, before, after) in [
        (
            "sequence",
            before.sequence_operations(),
            after.sequence_operations(),
        ),
        ("execute", before.execute_stages(), after.execute_stages()),
        (
            "calls",
            before.internal_function_invocations(),
            after.internal_function_invocations(),
        ),
        (
            "score-nbt",
            before.score_nbt_command_executions(),
            after.score_nbt_command_executions(),
        ),
        (
            "chain",
            before.maximum_chain_expansion(),
            after.maximum_chain_expansion(),
        ),
    ] {
        assert!(
            after.lower() <= before.lower(),
            "{root} root {metric} lower bound regressed: {before:?} -> {after:?}"
        );
        assert!(
            finite_upper(after) <= finite_upper(before),
            "{root} root {metric} upper bound regressed: {before:?} -> {after:?}"
        );
    }
}

fn finite_upper(bound: CountBound) -> u64 {
    match bound.upper().kind() {
        CountUpperKind::Finite(upper) => upper,
        other => panic!("expected a finite target-cost bound, found {other:?}"),
    }
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

fn declare_export(
    core: &mut CoreProgram,
    name: &str,
    parameters: Vec<CoreType>,
    results: Vec<CoreType>,
) -> FunctionId {
    core.declare_function_with_linkage(
        Some(name),
        CoreFunctionLinkage::DatapackExport,
        parameters,
        results,
        OriginId::UNKNOWN,
    )
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
