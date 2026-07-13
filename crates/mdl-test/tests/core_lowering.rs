use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use mdl_compiler::datapack::{EmissionOptions, emit_datapack};
use mdl_compiler::ir::core::{
    BlockId, BlockTarget, CanonicalPrinter, CoreProgram, CoreType, FunctionBuilder, FunctionId,
    I32Predicate, Terminator, TerminatorKind, ValueId,
};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    LoweredFunction, LoweringOptions, MinecraftOptimizationLevel, RegisterSlot, lower_to_minecraft,
};
use mdl_compiler::source::{Origin, OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer};

const SENTINEL: &str = "storage mdl:__mdl/init/v0/6d646c2e726567 \"initialized\"";

#[derive(Clone, Copy)]
struct FixtureIds {
    choose: FunctionId,
    call_choose: FunctionId,
    sum_down: FunctionId,
    countdown_swap: FunctionId,
}

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn core_lowering_runs_on_vanilla_26_2() {
    if let Err(error) = run_conformance(MinecraftOptimizationLevel::None, "stage4") {
        panic!("{error}");
    }
}

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn baseline_core_lowering_runs_on_vanilla_26_2() {
    if let Err(error) = run_conformance(MinecraftOptimizationLevel::Baseline, "stage5-baseline") {
        panic!("{error}");
    }
}

fn run_conformance(level: MinecraftOptimizationLevel, label: &str) -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let config = ServerConfig::new(java, server_jar);
    let mut sources = SourceContext::new();
    let origin = sources
        .add_origin(Origin::Unknown)
        .map_err(|error| error.to_string())?;
    let (core, fixture) = build_fixture(&sources, origin);
    let options = lowering_options()?.with_optimization_level(level);
    let first = lower_to_minecraft(&core, &sources, &options).map_err(|error| error.to_string())?;
    let second =
        lower_to_minecraft(&core, &sources, &options).map_err(|error| error.to_string())?;
    let first_emission = emit_datapack(
        first.program(),
        &sources,
        &EmissionOptions::new(format!("MDL {label} Core lowering conformance")),
    )
    .map_err(|error| error.to_string())?;
    let second_emission = emit_datapack(
        second.program(),
        &sources,
        &EmissionOptions::new(format!("MDL {label} Core lowering conformance")),
    )
    .map_err(|error| error.to_string())?;
    if first.dump_lowering() != second.dump_lowering()
        || MinecraftDebugDumper::program(first.program())
            != MinecraftDebugDumper::program(second.program())
        || first_emission.pack() != second_emission.pack()
        || trace_dump(first_emission.trace()) != trace_dump(second_emission.trace())
    {
        return Err("repeated Core lowering or emission was not byte-identical".to_owned());
    }

    let preserve_success = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve_success).map_err(|error| error.to_string())?;
    sandbox
        .install_datapack(
            &format!("mdl_{}", label.replace('-', "_")),
            first_emission
                .pack()
                .files()
                .iter()
                .map(|file| (file.path().as_str(), file.bytes())),
        )
        .map_err(|error| error.to_string())?;
    write_artifact_dumps(
        &sandbox,
        &core,
        &sources,
        &first.dump_lowering(),
        &MinecraftDebugDumper::program(first.program()),
        &trace_dump(first_emission.trace()),
        label,
    )?;

    let mut server = sandbox.start(&config).map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    let result = exercise_pack(&mut server, first.map(), fixture);
    if let Err(error) = result {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\n{label} conformance sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn exercise_pack(
    server: &mut TestServer,
    map: &mdl_compiler::lower::minecraft::LoweringMap,
    fixture: FixtureIds,
) -> Result<(), String> {
    expect_marker(
        server,
        &format!("execute if data {SENTINEL} run say MDL_STAGE4_FRESH_INIT"),
        "MDL_STAGE4_FRESH_INIT",
    )?;

    invoke(
        server,
        map.function(fixture.call_choose).unwrap(),
        &[1, 40, 2],
        &[40],
    )?;
    invoke(
        server,
        map.function(fixture.call_choose).unwrap(),
        &[0, 40, 2],
        &[2],
    )?;
    invoke(
        server,
        map.function(fixture.sum_down).unwrap(),
        &[4, 10],
        &[20],
    )?;
    invoke(
        server,
        map.function(fixture.countdown_swap).unwrap(),
        &[3, 10, 20],
        &[20, 10],
    )?;

    let preserved = &map.function(fixture.choose).unwrap().parameter_homes()[1].1;
    set_score(server, preserved, 123)?;
    let checkpoint = server.log_checkpoint();
    server
        .command("reload")
        .map_err(|error| error.to_string())?;
    server
        .command("say MDL_STAGE4_RELOAD_COMPLETE")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("MDL_STAGE4_RELOAD_COMPLETE")
        .map_err(|error| error.to_string())?;
    server
        .check_datapack_logs_since(checkpoint)
        .map_err(|error| error.to_string())?;
    expect_score(server, preserved, 123)?;

    server
        .command(&format!("data remove {SENTINEL}"))
        .map_err(|error| error.to_string())?;
    for key in ["first", "second"] {
        server
            .command(&format!(
                "execute store success storage mdl:stage4 {key} byte 1 run function mdl:__mdl/load"
            ))
            .map_err(|error| error.to_string())?;
    }
    expect_marker(
        server,
        "execute if data storage mdl:stage4 {first:0b,second:0b} unless data storage mdl:__mdl/init/v0/6d646c2e726567 \"initialized\" run say MDL_STAGE4_COLLISION_REJECTED",
        "MDL_STAGE4_COLLISION_REJECTED",
    )
}

fn invoke(
    server: &mut TestServer,
    function: &LoweredFunction,
    arguments: &[i32],
    expected_results: &[i32],
) -> Result<(), String> {
    if function.parameter_homes().len() != arguments.len()
        || function.result_homes().len() != expected_results.len()
    {
        return Err("integration invocation disagrees with the lowering ABI".to_owned());
    }
    for ((ty, slot), value) in function.parameter_homes().iter().zip(arguments) {
        if *ty == CoreType::Bool && !matches!(value, 0 | 1) {
            return Err(format!(
                "non-normalized Boolean integration argument {value}"
            ));
        }
        set_score(server, slot, *value)?;
    }
    server
        .command(&format!("function {}", function.entry_resource()))
        .map_err(|error| error.to_string())?;
    for ((_, slot), expected) in function.result_homes().iter().zip(expected_results) {
        expect_score(server, slot, *expected)?;
    }
    Ok(())
}

fn set_score(server: &mut TestServer, slot: &RegisterSlot, value: i32) -> Result<(), String> {
    server
        .command(&format!(
            "scoreboard players set {} {} {value}",
            slot.holder(),
            slot.objective()
        ))
        .map_err(|error| error.to_string())
}

fn expect_score(server: &mut TestServer, slot: &RegisterSlot, expected: i32) -> Result<(), String> {
    server
        .command(&format!(
            "scoreboard players get {} {}",
            slot.holder(),
            slot.objective()
        ))
        .map_err(|error| error.to_string())?;
    let line = server
        .wait_for_command_log(&format!("{} has", slot.holder()))
        .map_err(|error| error.to_string())?;
    let expected_text = format!("{} has {expected} [{}]", slot.holder(), slot.objective());
    if line.contains(&expected_text) {
        Ok(())
    } else {
        Err(format!("expected {expected_text:?}, got {line:?}"))
    }
}

fn expect_marker(server: &mut TestServer, command: &str, marker: &str) -> Result<(), String> {
    server.command(command).map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(marker)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn write_artifact_dumps(
    sandbox: &ServerSandbox,
    core: &CoreProgram,
    sources: &SourceContext,
    lowering: &str,
    target: &str,
    trace: &str,
    label: &str,
) -> Result<(), String> {
    let core = CanonicalPrinter::new(core, sources)
        .map_err(|error| error.to_string())?
        .render();
    for (name, contents) in [
        (format!("{label}-core.txt"), core.as_str()),
        (format!("{label}-lowering.txt"), lowering),
        (format!("{label}-target.txt"), target),
        (format!("{label}-trace.txt"), trace),
    ] {
        fs::write(sandbox.root().join(&name), contents)
            .map_err(|error| format!("write retained {name}: {error}"))?;
    }
    Ok(())
}

fn trace_dump(trace: &mdl_compiler::datapack::TraceMap) -> String {
    let mut output = String::new();
    for record in trace.records() {
        writeln!(
            output,
            "{}:{} {:?}",
            record.function(),
            record.line(),
            record.origin()
        )
        .unwrap();
    }
    output
}

fn lowering_options() -> Result<LoweringOptions, String> {
    LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new("mdl").map_err(|error| error.to_string())?,
        ObjectiveName::new("mdl.reg").map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn build_fixture(sources: &SourceContext, origin: OriginId) -> (CoreProgram, FixtureIds) {
    let mut core = CoreProgram::new();
    let choose = declare_function(
        &mut core,
        "choose",
        vec![CoreType::Bool, CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
        origin,
    );
    let call_choose = declare_function(
        &mut core,
        "call_choose",
        vec![CoreType::Bool, CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
        origin,
    );
    let sum_down = declare_function(
        &mut core,
        "sum_down",
        vec![CoreType::I32, CoreType::I32],
        vec![CoreType::I32],
        origin,
    );
    let countdown_swap = declare_function(
        &mut core,
        "countdown_swap",
        vec![CoreType::I32, CoreType::I32, CoreType::I32],
        vec![CoreType::I32, CoreType::I32],
        origin,
    );
    define_choose(&mut core, sources, choose, origin);
    define_call_choose(&mut core, sources, call_choose, choose, origin);
    define_sum_down(&mut core, sources, sum_down, origin);
    define_countdown_swap(&mut core, sources, countdown_swap, origin);
    (
        core,
        FixtureIds {
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
    origin: OriginId,
) -> FunctionId {
    core.declare_function(Some(name), parameters, results, origin)
        .unwrap()
}

fn define_choose(
    core: &mut CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    origin: OriginId,
) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let condition = parameter(&builder, entry, 0);
    let left = parameter(&builder, entry, 1);
    let right = parameter(&builder, entry, 2);
    let then_block = builder.create_block(origin).unwrap();
    let else_block = builder.create_block(origin).unwrap();
    let join = builder.create_block(origin).unwrap();
    let result = builder
        .append_block_parameter(join, CoreType::I32, origin)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            origin,
        ))
        .unwrap();
    builder.switch_to_block(then_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![left])),
            origin,
        ))
        .unwrap();
    builder.switch_to_block(else_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![right])),
            origin,
        ))
        .unwrap();
    builder.switch_to_block(join).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            origin,
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
    origin: OriginId,
) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let arguments = (0..3)
        .map(|index| parameter(&builder, entry, index))
        .collect();
    let result = builder.call(choose, arguments, origin).unwrap()[0];
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            origin,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn define_sum_down(
    core: &mut CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    origin: OriginId,
) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let initial_counter = parameter(&builder, entry, 0);
    let initial_accumulator = parameter(&builder, entry, 1);
    let header = builder.create_block(origin).unwrap();
    let counter = builder
        .append_block_parameter(header, CoreType::I32, origin)
        .unwrap();
    let accumulator = builder
        .append_block_parameter(header, CoreType::I32, origin)
        .unwrap();
    let body = builder.create_block(origin).unwrap();
    let exit = builder.create_block(origin).unwrap();
    let result = builder
        .append_block_parameter(exit, CoreType::I32, origin)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![initial_counter, initial_accumulator],
            )),
            origin,
        ))
        .unwrap();
    builder.switch_to_block(header).unwrap();
    let zero = builder.i32_constant(0, origin).unwrap();
    let done = builder
        .i32_compare(I32Predicate::SignedLe, counter, zero, origin)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: done,
                then_target: BlockTarget::new(exit, vec![accumulator]),
                else_target: BlockTarget::new(body, vec![]),
            },
            origin,
        ))
        .unwrap();
    builder.switch_to_block(body).unwrap();
    let minus_one = builder.i32_constant(-1, origin).unwrap();
    let next_counter = builder
        .i32_add_wrapping(counter, minus_one, origin)
        .unwrap();
    let next_accumulator = builder
        .i32_add_wrapping(accumulator, counter, origin)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![next_counter, next_accumulator],
            )),
            origin,
        ))
        .unwrap();
    builder.switch_to_block(exit).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            origin,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn define_countdown_swap(
    core: &mut CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    origin: OriginId,
) {
    let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
    let entry = builder.entry_block();
    let initial_count = parameter(&builder, entry, 0);
    let initial_left = parameter(&builder, entry, 1);
    let initial_right = parameter(&builder, entry, 2);
    let header = builder.create_block(origin).unwrap();
    let count = builder
        .append_block_parameter(header, CoreType::I32, origin)
        .unwrap();
    let left = builder
        .append_block_parameter(header, CoreType::I32, origin)
        .unwrap();
    let right = builder
        .append_block_parameter(header, CoreType::I32, origin)
        .unwrap();
    let backedge = builder.create_block(origin).unwrap();
    let exit = builder.create_block(origin).unwrap();
    let result_left = builder
        .append_block_parameter(exit, CoreType::I32, origin)
        .unwrap();
    let result_right = builder
        .append_block_parameter(exit, CoreType::I32, origin)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                header,
                vec![initial_count, initial_left, initial_right],
            )),
            origin,
        ))
        .unwrap();
    builder.switch_to_block(header).unwrap();
    let zero = builder.i32_constant(0, origin).unwrap();
    let done = builder
        .i32_compare(I32Predicate::SignedLe, count, zero, origin)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: done,
                then_target: BlockTarget::new(exit, vec![left, right]),
                else_target: BlockTarget::new(backedge, vec![]),
            },
            origin,
        ))
        .unwrap();
    builder.switch_to_block(backedge).unwrap();
    let minus_one = builder.i32_constant(-1, origin).unwrap();
    let next_count = builder.i32_add_wrapping(count, minus_one, origin).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(header, vec![next_count, right, left])),
            origin,
        ))
        .unwrap();
    builder.switch_to_block(exit).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result_left, result_right]),
            origin,
        ))
        .unwrap();
    core.define_function(function, builder.finish().unwrap())
        .unwrap();
}

fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
    builder.body().block(block).unwrap().parameters()[index].value()
}
