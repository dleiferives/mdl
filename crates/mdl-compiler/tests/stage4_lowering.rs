use std::fmt::Write as _;

use mdl_compiler::analysis::minecraft::{
    AnalysisArithmeticCaps, TargetExecutionAnalysisCompletion, TargetExecutionAnalysisLimits,
};
use mdl_compiler::datapack::{DatapackArtifact, EmissionOptions, TraceMap, emit_datapack};
use mdl_compiler::ir::core::{
    BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, I32Predicate,
    Terminator, TerminatorKind, ValueId,
};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    ActivationContract, CommandLimitAssumptions, LoweringOptions, LoweringPhase,
    MinecraftOptimizationLevel, lower_to_minecraft,
};
use mdl_compiler::source::{OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;

const STAGE4_LOWERING_GOLDEN: &str = include_str!("golden/stage4/lowering.txt");
const STAGE4_TARGET_GOLDEN: &str = include_str!("golden/stage4/target.txt");
const STAGE4_PACK_GOLDEN: &str = include_str!("golden/stage4/pack.txt");
const STAGE4_TRACE_GOLDEN: &str = include_str!("golden/stage4/trace.txt");

#[test]
fn public_lowering_boundary_returns_only_verified_output_and_read_only_abi() {
    let sources = SourceContext::new();
    let mut core = CoreProgram::new();
    let identity = core
        .declare_function(
            Some("identity"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let mut builder = FunctionBuilder::new(&core, &sources, identity).unwrap();
    let parameter = builder
        .body()
        .block(builder.entry_block())
        .unwrap()
        .parameters()[0]
        .value();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![parameter]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(identity, builder.finish().unwrap())
        .unwrap();

    let output = lower_to_minecraft(&core, &sources, &options()).unwrap();
    let mapped = output.map().function(identity).unwrap();
    assert_eq!(output.map().len(), 1);
    assert_eq!(mapped.entry_resource().to_string(), "mdl:__mdl/f0/b0");
    assert_eq!(mapped.parameter_homes().len(), 1);
    assert_eq!(mapped.parameter_homes()[0].0, CoreType::I32);
    assert_eq!(mapped.parameter_homes()[0].1.holder().to_string(), "#f0v0");
    assert_eq!(mapped.result_homes().len(), 1);
    assert_eq!(mapped.result_homes()[0].0, CoreType::I32);
    assert_eq!(mapped.result_homes()[0].1.holder().to_string(), "#f0r0");
    assert_eq!(
        output.map().execution_contract().activation(),
        ActivationContract::SingleContextNonReentrant
    );
    assert_eq!(
        output
            .map()
            .execution_contract()
            .command_limits()
            .assumptions(),
        mdl_compiler::lower::minecraft::CommandLimitAssumptions::for_target(
            JavaEditionTarget::V26_2
        )
    );
    assert!(
        output
            .dump_lowering()
            .contains("function 0 name=Some(\"identity\")")
    );
    assert_eq!(output.program().target(), JavaEditionTarget::V26_2);
    let report = output.report();
    assert_eq!(
        report.optimization_level(),
        MinecraftOptimizationLevel::None
    );
    assert_eq!(report.target(), JavaEditionTarget::V26_2);
    assert_eq!(report.namespace().as_str(), "mdl");
    assert_eq!(report.register_objective().as_str(), "mdl.reg");
    assert_eq!(
        report.execution_contract(),
        output.map().execution_contract()
    );
    let statistics = report.statistics();
    assert_eq!(statistics.homes(), 2);
    assert_eq!(statistics.target_functions(), 3);
    assert_eq!(statistics.core_functions(), 1);
    assert_eq!(statistics.branch_arms(), 0);
    assert_eq!(statistics.selected_recipes(), 0);
    assert_eq!(statistics.consumed_blocks(), 0);
    assert_eq!(report.dump(), output.dump_lowering());

    let (program, map, report) = output.into_parts();
    assert_eq!(program.target(), JavaEditionTarget::V26_2);
    assert_eq!(map.len(), 1);
    assert_eq!(report.statistics(), statistics);
}

#[test]
fn command_limit_assumptions_do_not_change_stage4_target_or_artifact() {
    let sources = SourceContext::new();
    let (core, _) = large_straight_line_program(&sources, 1);
    let default = lower_to_minecraft(&core, &sources, &options()).unwrap();
    let assumptions = CommandLimitAssumptions::new(10, 4).unwrap();
    let overridden_options = options().with_command_limit_assumptions(assumptions);
    let overridden = lower_to_minecraft(&core, &sources, &overridden_options).unwrap();

    assert_ne!(
        default
            .map()
            .execution_contract()
            .command_limits()
            .assumptions(),
        assumptions
    );
    assert_eq!(
        overridden
            .map()
            .execution_contract()
            .command_limits()
            .assumptions(),
        assumptions
    );
    assert_eq!(
        MinecraftDebugDumper::program(default.program()),
        MinecraftDebugDumper::program(overridden.program())
    );

    let emission_options = EmissionOptions::new("stage 5 command-limit differential");
    let default_emission = emit_datapack(default.program(), &sources, &emission_options).unwrap();
    let overridden_emission =
        emit_datapack(overridden.program(), &sources, &emission_options).unwrap();
    assert_eq!(default_emission.pack(), overridden_emission.pack());
    assert_eq!(
        trace_dump(default_emission.trace()),
        trace_dump(overridden_emission.trace())
    );
    assert_eq!(
        default_emission.footprint(),
        overridden_emission.footprint()
    );
    assert_no_gamerule_commands(default_emission.pack());
    assert_no_gamerule_commands(overridden_emission.pack());
}

#[test]
fn structured_census_reconciles_with_trace_and_emitted_artifact() {
    let sources = SourceContext::new();
    let (core, _) = public_fixture(&sources);
    let output = lower_to_minecraft(&core, &sources, &options()).unwrap();
    let program = output.program();
    let assumptions = output
        .map()
        .execution_contract()
        .command_limits()
        .assumptions();
    let report = output
        .analyze_target_execution(TargetExecutionAnalysisLimits::new(
            AnalysisArithmeticCaps::minimum_for(assumptions),
            100_000,
            100_000,
        ))
        .unwrap();
    assert_eq!(
        report.completion(),
        TargetExecutionAnalysisCompletion::Complete
    );
    let emission = emit_datapack(
        program,
        &sources,
        &EmissionOptions::new("stage 5 census reconciliation"),
    )
    .unwrap();
    let census = report.census();
    let footprint = emission.footprint();

    assert_eq!(report.target(), program.target());
    assert_eq!(emission.trace().target(), program.target());
    assert_eq!(
        emission.trace().pack_format(),
        program.target().spec().data_pack_format()
    );
    assert_eq!(census.functions(), footprint.function_files());
    assert_eq!(census.function_tags(), footprint.function_tag_files());
    assert_eq!(
        census.top_level_commands(),
        footprint.physical_function_lines()
    );
    assert_eq!(census.top_level_commands(), footprint.trace_records());
    assert_eq!(footprint.metadata_files(), 1);
    assert_eq!(
        footprint.files().len(),
        1 + census.functions() + census.function_tags()
    );

    // These domains are intentionally distinct: nested structured commands render
    // on one physical line, and private helpers are not public Core ABI entries.
    assert!(census.command_nodes() > census.top_level_commands());
    assert!(output.map().len() < census.functions());

    for (function_id, function) in program.functions() {
        let trace = emission.trace().function(function_id).unwrap();
        assert_eq!(trace.function(), function.resource());
        assert_eq!(trace.len(), function.body().len());
        let path = function.resource().pack_path(program.target());
        let file = emission.pack().file(path.as_str()).unwrap();
        assert_eq!(
            file.bytes().split_inclusive(|byte| *byte == b'\n').count(),
            function.body().len()
        );
        for (command_id, command) in function.body().commands() {
            assert_eq!(trace.origin(command_id), Some(command.origin()));
        }
    }
    for (_, tag) in program.function_tags() {
        let path = tag.resource().pack_path(program.target());
        assert!(emission.pack().file(path.as_str()).is_some());
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one public fixture proves calls, diamonds, ordinary loops, and cyclic loops together"
)]
fn explicit_none_is_the_complete_stage4_compatibility_oracle() {
    let sources = SourceContext::new();
    let (core, fixture) = public_fixture(&sources);
    let legacy_options = options();
    let explicit_none_options = options().with_optimization_level(MinecraftOptimizationLevel::None);
    assert_eq!(
        legacy_options.optimization_level(),
        MinecraftOptimizationLevel::None
    );
    assert_eq!(legacy_options, explicit_none_options);

    let legacy = lower_to_minecraft(&core, &sources, &legacy_options).unwrap();
    let explicit_none = lower_to_minecraft(&core, &sources, &explicit_none_options).unwrap();
    let legacy_lowering = legacy.dump_lowering();
    let explicit_none_lowering = explicit_none.dump_lowering();
    let legacy_target = MinecraftDebugDumper::program(legacy.program());
    let explicit_none_target = MinecraftDebugDumper::program(explicit_none.program());
    assert_eq!(legacy_lowering, explicit_none_lowering);
    assert_eq!(legacy_target, explicit_none_target);
    assert_eq!(legacy_lowering, STAGE4_LOWERING_GOLDEN);
    assert_eq!(legacy_target, STAGE4_TARGET_GOLDEN);
    assert_eq!(legacy.map(), explicit_none.map());
    assert_eq!(
        legacy.map().execution_contract(),
        explicit_none.map().execution_contract()
    );

    let analysis_limits = TargetExecutionAnalysisLimits::new(
        AnalysisArithmeticCaps::minimum_for(
            legacy
                .map()
                .execution_contract()
                .command_limits()
                .assumptions(),
        ),
        100_000,
        100_000,
    );
    let legacy_execution = legacy.analyze_target_execution(analysis_limits).unwrap();
    let explicit_none_execution = explicit_none
        .analyze_target_execution(analysis_limits)
        .unwrap();
    assert_eq!(legacy_execution, explicit_none_execution);
    assert_eq!(legacy_execution.dump(), explicit_none_execution.dump());

    for (function, expected_entry, parameters, results) in [
        (fixture.choose, "mdl:__mdl/f0/b0", 3, 1),
        (fixture.call_choose, "mdl:__mdl/f1/b0", 3, 1),
        (fixture.sum_down, "mdl:__mdl/f2/b0", 2, 1),
        (fixture.countdown_swap, "mdl:__mdl/f3/b0", 3, 2),
    ] {
        let lowered = legacy.map().function(function).unwrap();
        assert_eq!(lowered.entry_resource().to_string(), expected_entry);
        assert_eq!(lowered.parameter_homes().len(), parameters);
        assert_eq!(lowered.result_homes().len(), results);
    }

    let legacy_emission = emit_datapack(
        legacy.program(),
        &sources,
        &EmissionOptions::new("stage 4 public proof"),
    )
    .unwrap();
    let explicit_none_emission = emit_datapack(
        explicit_none.program(),
        &sources,
        &EmissionOptions::new("stage 4 public proof"),
    )
    .unwrap();
    assert_eq!(legacy_emission.pack(), explicit_none_emission.pack());
    assert_eq!(
        legacy_emission.footprint(),
        explicit_none_emission.footprint()
    );
    for (legacy_file, explicit_none_file) in legacy_emission
        .pack()
        .files()
        .iter()
        .zip(explicit_none_emission.pack().files())
    {
        assert_eq!(legacy_file.path(), explicit_none_file.path());
        assert_eq!(legacy_file.bytes(), explicit_none_file.bytes());
    }
    let legacy_pack = pack_dump(legacy_emission.pack());
    let explicit_none_pack = pack_dump(explicit_none_emission.pack());
    assert_eq!(legacy_pack, explicit_none_pack);
    assert_eq!(legacy_pack, STAGE4_PACK_GOLDEN);
    let paths = legacy_emission
        .pack()
        .files()
        .iter()
        .map(|file| file.path().as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths.len(), 19);
    assert_eq!(paths[0], "data/mdl/function/__mdl/f0/b0.mcfunction");
    assert_eq!(paths.last().copied(), Some("pack.mcmeta"));
    assert_eq!(
        legacy_emission
            .pack()
            .file("data/mdl/function/__mdl/f1/b0.mcfunction")
            .unwrap()
            .bytes(),
        concat!(
            "scoreboard players operation #f0v0 mdl.reg = #f1v0 mdl.reg\n",
            "scoreboard players operation #f0v1 mdl.reg = #f1v1 mdl.reg\n",
            "scoreboard players operation #f0v2 mdl.reg = #f1v2 mdl.reg\n",
            "function mdl:__mdl/f0/b0\n",
            "scoreboard players operation #f1v3 mdl.reg = #f0r0 mdl.reg\n",
            "scoreboard players operation #f1r0 mdl.reg = #f1v3 mdl.reg\n",
            "return 1\n"
        )
        .as_bytes()
    );
    assert_eq!(
        legacy_emission
            .pack()
            .file("data/minecraft/tags/function/load.json")
            .unwrap()
            .bytes(),
        b"{\"values\":[\"mdl:__mdl/load\"]}\n"
    );
    let legacy_trace = trace_dump(legacy_emission.trace());
    let explicit_none_trace = trace_dump(explicit_none_emission.trace());
    assert_eq!(legacy_trace, explicit_none_trace);
    assert_eq!(legacy_trace, STAGE4_TRACE_GOLDEN);
    assert!(
        legacy_emission
            .trace()
            .records()
            .all(|record| record.origin() == OriginId::UNKNOWN)
    );
}

#[test]
fn public_failures_stop_before_output_and_report_the_exact_phase() {
    let sources = SourceContext::new();
    let mut undefined = CoreProgram::new();
    undefined
        .declare_function(Some("undefined"), vec![], vec![], OriginId::UNKNOWN)
        .unwrap();
    let failure = lower_to_minecraft(&undefined, &sources, &options()).unwrap_err();
    assert_eq!(failure.phase(), LoweringPhase::CoreVerification);
    assert!(
        failure
            .diagnostics()
            .contains_code("core.undefined-function")
    );
    assert_eq!(failure.dump_lowering(), None);

    let mut recursive = CoreProgram::new();
    let function = recursive
        .declare_function(Some("recursive"), vec![], vec![], OriginId::UNKNOWN)
        .unwrap();
    let mut builder = FunctionBuilder::new(&recursive, &sources, function).unwrap();
    builder.call(function, vec![], OriginId::UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    recursive
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    let failure = lower_to_minecraft(&recursive, &sources, &options()).unwrap_err();
    assert_eq!(failure.phase(), LoweringPhase::Legality);
    assert!(
        failure
            .diagnostics()
            .contains_code("lower.recursive-call-abi")
    );
    assert_eq!(failure.dump_lowering(), None);

    let mut unsupported = CoreProgram::new();
    let function = unsupported
        .declare_function(Some("unsupported"), vec![], vec![], OriginId::UNKNOWN)
        .unwrap();
    let mut builder = FunctionBuilder::new(&unsupported, &sources, function).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Unreachable,
            OriginId::UNKNOWN,
        ))
        .unwrap();
    unsupported
        .define_function(function, builder.finish().unwrap())
        .unwrap();
    let failure = lower_to_minecraft(&unsupported, &sources, &options()).unwrap_err();
    assert_eq!(failure.phase(), LoweringPhase::Legality);
    assert!(
        failure
            .diagnostics()
            .contains_code("lower.reachable-unreachable")
    );
    assert_eq!(failure.dump_lowering(), None);
}

#[test]
fn large_server_free_lowering_has_exact_linear_counts() {
    const VALUES: usize = 20_000;
    let sources = SourceContext::new();
    let (core, function) = large_straight_line_program(&sources, VALUES);
    let output = lower_to_minecraft(&core, &sources, &options()).unwrap();
    let dump = output.dump_lowering();
    assert!(dump.starts_with(&format!(
        "lowering-report V26_2 mdl mdl.reg homes={} target-functions=3\n",
        VALUES + 1
    )));
    assert_eq!(output.program().functions().len(), 3);
    assert_eq!(
        output
            .program()
            .functions()
            .map(|(_, function)| function.body().len())
            .sum::<usize>(),
        VALUES + 6
    );
    let emission = emit_datapack(
        output.program(),
        &sources,
        &EmissionOptions::new("stage 4 scale"),
    )
    .unwrap();
    assert_eq!(emission.pack().files().len(), 5);
    assert_eq!(emission.trace().records().count(), VALUES + 6);
    assert_eq!(
        output
            .map()
            .function(function)
            .unwrap()
            .result_homes()
            .len(),
        1
    );
}

#[test]
#[ignore = "non-gating geometric Core-to-Minecraft lowering benchmark"]
fn reports_geometric_lowering_scaling_without_a_timing_threshold() {
    use std::time::Instant;

    for values in [1_000, 2_000, 4_000, 8_000, 16_000] {
        let sources = SourceContext::new();
        let (core, _) = large_straight_line_program(&sources, values);
        let started = Instant::now();
        let output = lower_to_minecraft(&core, &sources, &options()).unwrap();
        eprintln!(
            "stage4-lowering values={values} functions={} elapsed={:?}",
            output.program().functions().len(),
            started.elapsed()
        );
    }
}

#[derive(Clone, Copy)]
struct PublicFixture {
    choose: FunctionId,
    call_choose: FunctionId,
    sum_down: FunctionId,
    countdown_swap: FunctionId,
}

fn public_fixture(sources: &SourceContext) -> (CoreProgram, PublicFixture) {
    let mut core = CoreProgram::new();
    let choose = declare_function(
        &mut core,
        "choose",
        vec![CoreType::Bool, CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
    );
    let call_choose = declare_function(
        &mut core,
        "call_choose",
        vec![CoreType::Bool, CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
    );
    let sum_down = declare_function(
        &mut core,
        "sum_down",
        vec![CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
    );
    let countdown_swap = declare_function(
        &mut core,
        "countdown_swap",
        vec![CoreType::I32, CoreType::I32, CoreType::I32],
        vec![CoreType::I32, CoreType::I32],
    );
    define_choose(&mut core, sources, choose);
    define_call_choose(&mut core, sources, call_choose, choose);
    define_sum_down(&mut core, sources, sum_down);
    define_countdown_swap(&mut core, sources, countdown_swap);
    (
        core,
        PublicFixture {
            choose,
            call_choose,
            sum_down,
            countdown_swap,
        },
    )
}

fn declare_function(
    core: &mut CoreProgram,
    name: &str,
    parameters: Vec<CoreType>,
    results: Vec<CoreType>,
) -> FunctionId {
    core.declare_function(Some(name), parameters, results, OriginId::UNKNOWN)
        .unwrap()
}

fn large_straight_line_program(
    sources: &SourceContext,
    values: usize,
) -> (CoreProgram, FunctionId) {
    assert!(values > 0);
    let mut core = CoreProgram::new();
    let function = declare_function(&mut core, "large", vec![], vec![CoreType::I32]);
    let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
    let mut last = None;
    for value in 0..values {
        last = Some(
            builder
                .i32_constant(i32::try_from(value).unwrap(), OriginId::UNKNOWN)
                .unwrap(),
        );
    }
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![last.unwrap()]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
    (core, function)
}

fn define_choose(core: &mut CoreProgram, sources: &SourceContext, function: FunctionId) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let condition = parameter(&builder, entry, 0);
    let left = parameter(&builder, entry, 1);
    let right = parameter(&builder, entry, 2);
    let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
    let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
    let join = builder.create_block(OriginId::UNKNOWN).unwrap();
    let result = builder
        .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(then_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![left])),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(else_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![right])),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(join).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn define_call_choose(
    core: &mut CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    choose: FunctionId,
) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let arguments = (0..3)
        .map(|index| parameter(&builder, entry, index))
        .collect();
    let result = builder.call(choose, arguments, OriginId::UNKNOWN).unwrap()[0];
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn define_sum_down(core: &mut CoreProgram, sources: &SourceContext, function: FunctionId) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let initial_counter = parameter(&builder, entry, 0);
    let initial_accumulator = parameter(&builder, entry, 1);
    let header = builder.create_block(OriginId::UNKNOWN).unwrap();
    let counter = builder
        .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    let accumulator = builder
        .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    let body = builder.create_block(OriginId::UNKNOWN).unwrap();
    let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
    let result = builder
        .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![initial_counter, initial_accumulator],
            )),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(header).unwrap();
    let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
    let done = builder
        .i32_compare(I32Predicate::SignedLe, counter, zero, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: done,
                then_target: BlockTarget::new(exit, vec![accumulator]),
                else_target: BlockTarget::new(body, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(body).unwrap();
    let minus_one = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
    let next_counter = builder
        .i32_add_wrapping(counter, minus_one, OriginId::UNKNOWN)
        .unwrap();
    let next_accumulator = builder
        .i32_add_wrapping(accumulator, counter, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![next_counter, next_accumulator],
            )),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(exit).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn define_countdown_swap(core: &mut CoreProgram, sources: &SourceContext, function: FunctionId) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let initial_count = parameter(&builder, entry, 0);
    let initial_left = parameter(&builder, entry, 1);
    let initial_right = parameter(&builder, entry, 2);
    let header = builder.create_block(OriginId::UNKNOWN).unwrap();
    let count = builder
        .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    let left = builder
        .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    let right = builder
        .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    let backedge = builder.create_block(OriginId::UNKNOWN).unwrap();
    let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
    let result_left = builder
        .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    let result_right = builder
        .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![initial_count, initial_left, initial_right],
            )),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(header).unwrap();
    let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
    let done = builder
        .i32_compare(I32Predicate::SignedLe, count, zero, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: done,
                then_target: BlockTarget::new(exit, vec![left, right]),
                else_target: BlockTarget::new(backedge, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(backedge).unwrap();
    let minus_one = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
    let next_count = builder
        .i32_add_wrapping(count, minus_one, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(header, vec![next_count, right, left])),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(exit).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result_left, result_right]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
    builder.body().block(block).unwrap().parameters()[index].value()
}

fn pack_dump(pack: &DatapackArtifact) -> String {
    let mut output = String::new();
    for file in pack.files() {
        writeln!(output, "file {} bytes={}", file.path(), file.bytes().len()).unwrap();
        let contents = std::str::from_utf8(file.bytes()).expect("emitted datapack files are UTF-8");
        output.push_str(contents);
        if !contents.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("end-file\n");
    }
    output
}

fn trace_dump(trace: &TraceMap) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "trace target={:?} pack-format={:?}",
        trace.target(),
        trace.pack_format()
    )
    .unwrap();
    for record in trace.records() {
        writeln!(
            output,
            "function-id={:?} function={} command-id={:?} line={} origin={:?}",
            record.function_id(),
            record.function(),
            record.command(),
            record.line(),
            record.origin()
        )
        .unwrap();
    }
    output
}

fn assert_no_gamerule_commands(pack: &DatapackArtifact) {
    for file in pack
        .files()
        .iter()
        .filter(|file| file.path().as_str().ends_with(".mcfunction"))
    {
        let contents = std::str::from_utf8(file.bytes()).expect("emitted function files are UTF-8");
        for line in contents.lines() {
            assert!(
                !line
                    .split_ascii_whitespace()
                    .any(|token| token == "gamerule"),
                "compiler emitted a gamerule command in {}: {line:?}",
                file.path()
            );
        }
    }
}

fn options() -> LoweringOptions {
    LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl").unwrap(),
        ObjectiveName::new("mdl.reg").unwrap(),
    )
    .unwrap()
}
