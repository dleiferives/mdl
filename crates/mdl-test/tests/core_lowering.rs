use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use mdl_compiler::datapack::{DatapackArtifact, EmissionOptions, EmissionOutput, emit_datapack};
use mdl_compiler::ir::core::{
    BlockId, BlockTarget, CanonicalPrinter, CoreProgram, CoreType, FunctionBuilder, FunctionId,
    I32Predicate, Terminator, TerminatorKind, ValueId,
};
use mdl_compiler::ir::minecraft::{MinecraftDebugDumper, ObjectiveName, PackNamespace};
use mdl_compiler::lower::minecraft::{
    LoweredFunction, LoweringOptions, LoweringOutput, MinecraftOptimizationLevel, RegisterSlot,
    lower_to_minecraft,
};
use mdl_compiler::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions, optimize_core};
use mdl_compiler::source::{Origin, OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer};

#[derive(Clone, Copy)]
struct FixtureIds {
    choose: FunctionId,
    call_choose: FunctionId,
    sum_down: FunctionId,
    countdown_swap: FunctionId,
}

#[derive(Clone, Copy)]
struct CoreConfiguration {
    label: &'static str,
    runtime_label: &'static str,
    pack_name: &'static str,
    namespace: &'static str,
    objective: &'static str,
    core_level: CoreOptimizationLevel,
    minecraft_level: MinecraftOptimizationLevel,
}

const REFERENCE_CONFIGURATION: CoreConfiguration = CoreConfiguration {
    label: "core-none-minecraft-none",
    runtime_label: "none",
    pack_name: "mdl_stage5h_core_none",
    namespace: "mdl_stage5h_core_none",
    objective: "mdl5h.core.none",
    core_level: CoreOptimizationLevel::None,
    minecraft_level: MinecraftOptimizationLevel::None,
};

const BASELINE_CONFIGURATION: CoreConfiguration = CoreConfiguration {
    label: "core-baseline-minecraft-baseline",
    runtime_label: "baseline",
    pack_name: "mdl_stage5h_core_baseline",
    namespace: "mdl_stage5h_core_baseline",
    objective: "mdl5h.core.base",
    core_level: CoreOptimizationLevel::Baseline,
    minecraft_level: MinecraftOptimizationLevel::Baseline,
};

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn optimized_core_lowering_conformance_runs_on_vanilla_26_2() {
    if let Err(error) = run_official_server_conformance() {
        panic!("{error}");
    }
}

struct CompiledCorePack {
    configuration: CoreConfiguration,
    optimized_core: CoreProgram,
    fixture: FixtureIds,
    optimization_report: String,
    lowering: LoweringOutput,
    emission: EmissionOutput,
}

struct CompiledTerminalRecipes {
    fixture: TerminalCallRecipeFixture,
    none: LoweringOutput,
    none_emission: EmissionOutput,
    baseline: LoweringOutput,
    baseline_emission: EmissionOutput,
}

#[allow(
    clippy::too_many_lines,
    reason = "one server lifecycle owns all Stage 5H Core and terminal-recipe conformance packs"
)]
fn run_official_server_conformance() -> Result<(), String> {
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
    let reference = compile_core_pack(&core, fixture, &sources, REFERENCE_CONFIGURATION)?;
    let baseline = compile_core_pack(&core, fixture, &sources, BASELINE_CONFIGURATION)?;
    let accepted_merges = accepted_coalescing_merges(&baseline.lowering.dump_lowering());
    if accepted_merges == 0 {
        return Err(
            "the official-server Baseline fixture did not exercise a real home-coalescing merge"
                .to_owned(),
        );
    }
    let recipes = compile_terminal_call_recipes(&sources, origin)?;

    let preserve_success = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve_success).map_err(|error| error.to_string())?;
    for pack in [&reference, &baseline] {
        install_emission(&sandbox, pack.configuration.pack_name, &pack.emission)?;
        write_artifact_dumps(
            &sandbox,
            &pack.optimized_core,
            &sources,
            &pack.lowering.dump_lowering(),
            &MinecraftDebugDumper::program(pack.lowering.program()),
            &trace_dump(pack.emission.trace()),
            pack.configuration.label,
        )?;
        write_retained_file(
            &sandbox,
            &format!("{}-optimization.txt", pack.configuration.label),
            &pack.optimization_report,
        )?;
    }
    install_emission(&sandbox, "mdl_stage5g_none", &recipes.none_emission)?;
    install_emission(&sandbox, "mdl_stage5g_baseline", &recipes.baseline_emission)?;
    write_artifact_dumps(
        &sandbox,
        &recipes.fixture.core,
        &sources,
        &recipes.none.dump_lowering(),
        &MinecraftDebugDumper::program(recipes.none.program()),
        &trace_dump(recipes.none_emission.trace()),
        "terminal-recipe-none",
    )?;
    write_artifact_dumps(
        &sandbox,
        &recipes.fixture.core,
        &sources,
        &recipes.baseline.dump_lowering(),
        &MinecraftDebugDumper::program(recipes.baseline.program()),
        &trace_dump(recipes.baseline_emission.trace()),
        "terminal-recipe-baseline",
    )?;

    let mut server = sandbox.start(&config).map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    let result = exercise_terminal_call_recipe_differential(
        &mut server,
        &recipes.none,
        &recipes.baseline,
        &recipes.fixture,
    )
    .and_then(|()| exercise_core_differential(&mut server, &reference, &baseline));
    if let Err(error) = result {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 5H official-server sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())
}

fn compile_terminal_call_recipes(
    sources: &SourceContext,
    origin: OriginId,
) -> Result<CompiledTerminalRecipes, String> {
    let fixture = build_terminal_call_recipe_fixture(sources, origin);
    let none_options = lowering_options_for("mdl_stage5g_none", "mdl5g.none")?
        .with_optimization_level(MinecraftOptimizationLevel::None);
    let baseline_options = lowering_options_for("mdl_stage5g_baseline", "mdl5g.base")?
        .with_optimization_level(MinecraftOptimizationLevel::Baseline);
    let none = lower_to_minecraft(&fixture.core, sources, &none_options)
        .map_err(|error| error.to_string())?;
    let baseline = lower_to_minecraft(&fixture.core, sources, &baseline_options)
        .map_err(|error| error.to_string())?;
    let none_emission = emit_datapack(
        none.program(),
        sources,
        &EmissionOptions::new("MDL Stage 5G reference terminal-call recipe"),
    )
    .map_err(|error| error.to_string())?;
    let baseline_emission = emit_datapack(
        baseline.program(),
        sources,
        &EmissionOptions::new("MDL Stage 5G selected terminal-call recipe"),
    )
    .map_err(|error| error.to_string())?;
    assert_terminal_call_recipe_shape(
        &none,
        none_emission.pack(),
        &baseline,
        baseline_emission.pack(),
        &fixture,
    )?;
    Ok(CompiledTerminalRecipes {
        fixture,
        none,
        none_emission,
        baseline,
        baseline_emission,
    })
}

fn compile_core_pack(
    core: &CoreProgram,
    fixture: FixtureIds,
    sources: &SourceContext,
    configuration: CoreConfiguration,
) -> Result<CompiledCorePack, String> {
    let optimization_options = CoreOptimizationOptions::new(configuration.core_level);
    let first_optimization = optimize_core(core.clone(), sources, &optimization_options)
        .map_err(|error| error.to_string())?;
    let second_optimization = optimize_core(core.clone(), sources, &optimization_options)
        .map_err(|error| error.to_string())?;
    let first_core = CanonicalPrinter::new(first_optimization.program(), sources)
        .map_err(|error| error.to_string())?
        .render();
    let second_core = CanonicalPrinter::new(second_optimization.program(), sources)
        .map_err(|error| error.to_string())?
        .render();
    if first_optimization.report() != second_optimization.report() || first_core != second_core {
        return Err(format!(
            "{} Core optimization was not byte-stable",
            configuration.label
        ));
    }

    let options = lowering_options_for(configuration.namespace, configuration.objective)?
        .with_optimization_level(configuration.minecraft_level);
    let first = lower_to_minecraft(first_optimization.program(), sources, &options)
        .map_err(|error| error.to_string())?;
    let second = lower_to_minecraft(second_optimization.program(), sources, &options)
        .map_err(|error| error.to_string())?;
    let first_emission = emit_datapack(
        first.program(),
        sources,
        &EmissionOptions::new(format!(
            "MDL {} Core lowering conformance",
            configuration.label
        )),
    )
    .map_err(|error| error.to_string())?;
    let second_emission = emit_datapack(
        second.program(),
        sources,
        &EmissionOptions::new(format!(
            "MDL {} Core lowering conformance",
            configuration.label
        )),
    )
    .map_err(|error| error.to_string())?;
    if first.dump_lowering() != second.dump_lowering()
        || MinecraftDebugDumper::program(first.program())
            != MinecraftDebugDumper::program(second.program())
        || first_emission.pack() != second_emission.pack()
        || trace_dump(first_emission.trace()) != trace_dump(second_emission.trace())
    {
        return Err(format!(
            "{} repeated lowering or emission was not byte-identical",
            configuration.label
        ));
    }
    let optimization_report = first_optimization.report().dump();
    let (optimized_core, _) = first_optimization.into_parts();
    Ok(CompiledCorePack {
        configuration,
        optimized_core,
        fixture,
        optimization_report,
        lowering: first,
        emission: first_emission,
    })
}

fn install_emission(
    sandbox: &ServerSandbox,
    pack_name: &str,
    emission: &EmissionOutput,
) -> Result<(), String> {
    sandbox
        .install_datapack(
            pack_name,
            emission
                .pack()
                .files()
                .iter()
                .map(|file| (file.path().as_str(), file.bytes())),
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

struct TerminalCallRecipeFixture {
    core: CoreProgram,
    producer: FunctionId,
    terminal: FunctionId,
    dispatcher: FunctionId,
}

fn build_terminal_call_recipe_fixture(
    sources: &SourceContext,
    origin: OriginId,
) -> TerminalCallRecipeFixture {
    let mut core = CoreProgram::new();
    let producer = declare_function(
        &mut core,
        "stage5g_effect_producer",
        vec![],
        vec![CoreType::I32],
        origin,
    );
    let terminal = declare_function(&mut core, "stage5g_terminal_callee", vec![], vec![], origin);
    let dispatcher = declare_function(
        &mut core,
        "stage5g_dispatcher",
        vec![CoreType::Bool],
        vec![],
        origin,
    );

    let mut producer_builder = FunctionBuilder::new(&core, sources, producer).unwrap();
    let effect = producer_builder.i32_constant(73, origin).unwrap();
    producer_builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![effect]),
            origin,
        ))
        .unwrap();
    core.define_function(producer, producer_builder.finish().unwrap())
        .unwrap();

    let mut terminal_builder = FunctionBuilder::new(&core, sources, terminal).unwrap();
    terminal_builder.call(producer, vec![], origin).unwrap();
    terminal_builder
        .terminate(Terminator::new(TerminatorKind::Return(vec![]), origin))
        .unwrap();
    core.define_function(terminal, terminal_builder.finish().unwrap())
        .unwrap();

    let mut dispatcher_builder = FunctionBuilder::new(&core, sources, dispatcher).unwrap();
    let entry = dispatcher_builder.entry_block();
    let condition = parameter(&dispatcher_builder, entry, 0);
    let then_block = dispatcher_builder.create_block(origin).unwrap();
    let else_block = dispatcher_builder.create_block(origin).unwrap();
    dispatcher_builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            origin,
        ))
        .unwrap();
    for block in [then_block, else_block] {
        dispatcher_builder.switch_to_block(block).unwrap();
        dispatcher_builder.call(terminal, vec![], origin).unwrap();
        dispatcher_builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), origin))
            .unwrap();
    }
    core.define_function(dispatcher, dispatcher_builder.finish().unwrap())
        .unwrap();

    TerminalCallRecipeFixture {
        core,
        producer,
        terminal,
        dispatcher,
    }
}

fn assert_terminal_call_recipe_shape(
    none: &LoweringOutput,
    none_pack: &DatapackArtifact,
    baseline: &LoweringOutput,
    baseline_pack: &DatapackArtifact,
    fixture: &TerminalCallRecipeFixture,
) -> Result<(), String> {
    let none_report = none.dump_lowering();
    let baseline_report = baseline.dump_lowering();
    if none_report.contains("recipe=InlineZeroAbiTerminalCall") {
        return Err("the None lowering unexpectedly selected a control recipe".to_owned());
    }
    if !baseline_report.contains("control branch-arms=2 selected=2 consumed=2")
        || baseline_report
            .matches("recipe=InlineZeroAbiTerminalCall consumed=")
            .count()
            != 2
        || baseline_report
            .matches("completion=ExactlyOneOnNormalCompletion")
            .count()
            != 2
    {
        return Err(format!(
            "the Baseline lowering did not select both terminal-call recipes:\n{baseline_report}"
        ));
    }

    let none_terminal = lowered_entry_resource(none, fixture.terminal)?;
    let baseline_terminal = lowered_entry_resource(baseline, fixture.terminal)?;
    let none_entry = emitted_function(
        none_pack,
        &lowered_entry_resource(none, fixture.dispatcher)?,
    )?;
    let baseline_entry = emitted_function(
        baseline_pack,
        &lowered_entry_resource(baseline, fixture.dispatcher)?,
    )?;
    for block in [1, 2] {
        let none_wrapper = lowered_block_resource(none, fixture.dispatcher, block)?;
        let baseline_wrapper = lowered_block_resource(baseline, fixture.dispatcher, block)?;
        let wrapper = emitted_function(none_pack, &none_wrapper)?;
        if wrapper != format!("function {none_terminal}\nreturn 1\n") {
            return Err(format!(
                "the None arm wrapper {none_wrapper} had unexpected commands:\n{wrapper}"
            ));
        }
        if baseline_pack
            .file(&function_pack_path(&baseline_wrapper)?)
            .is_some()
        {
            return Err(format!(
                "the Baseline artifact retained consumed wrapper {baseline_wrapper}"
            ));
        }
        if !none_entry.contains(&none_wrapper) {
            return Err(format!(
                "the None dispatcher did not target retained wrapper {none_wrapper}"
            ));
        }
        if baseline_entry.contains(&baseline_wrapper) {
            return Err(format!(
                "the Baseline dispatcher still targeted consumed wrapper {baseline_wrapper}"
            ));
        }
    }
    if none_entry.contains(&none_terminal)
        || baseline_entry.matches(&baseline_terminal).count() != 2
        || baseline_entry.lines().count() != 2
    {
        return Err(format!(
            "the selected dispatcher did not directly target its zero-ABI callee on both arms:\n{baseline_entry}"
        ));
    }
    Ok(())
}

fn exercise_terminal_call_recipe_differential(
    server: &mut TestServer,
    none: &LoweringOutput,
    baseline: &LoweringOutput,
    fixture: &TerminalCallRecipeFixture,
) -> Result<(), String> {
    server
        .command("data modify storage mdl_test:stage5g observations set value {}")
        .map_err(|error| error.to_string())?;
    for (label, output) in [("none", none), ("baseline", baseline)] {
        for (path, condition) in [("false", 0), ("true", 1)] {
            invoke_terminal_call_recipe(
                server,
                output,
                fixture,
                condition,
                &format!("{label}_{path}"),
            )?;
        }
    }
    Ok(())
}

fn invoke_terminal_call_recipe(
    server: &mut TestServer,
    output: &LoweringOutput,
    fixture: &TerminalCallRecipeFixture,
    condition: i32,
    observation: &str,
) -> Result<(), String> {
    let dispatcher = output
        .map()
        .function(fixture.dispatcher)
        .ok_or_else(|| "missing Stage 5G dispatcher lowering map entry".to_owned())?;
    let [(CoreType::Bool, condition_home)] = dispatcher.parameter_homes() else {
        return Err("Stage 5G dispatcher did not retain its Boolean ABI".to_owned());
    };
    let producer = output
        .map()
        .function(fixture.producer)
        .ok_or_else(|| "missing Stage 5G effect-producer lowering map entry".to_owned())?;
    let [(CoreType::I32, effect_home)] = producer.result_homes() else {
        return Err("Stage 5G effect producer did not retain its I32 result ABI".to_owned());
    };
    set_score(server, condition_home, condition)?;
    set_score(server, effect_home, 0)?;
    server
        .command(&format!(
            "execute store success storage mdl_test:stage5g observations.{observation}_success byte 1 store result storage mdl_test:stage5g observations.{observation}_result int 1 run function {}",
            dispatcher.entry_resource()
        ))
        .map_err(|error| error.to_string())?;
    expect_score(server, effect_home, 73)?;
    let marker = format!("MDL_STAGE5G_{}", observation.to_ascii_uppercase());
    expect_marker(
        server,
        &format!(
            "execute if data storage mdl_test:stage5g observations{{{observation}_success:1b,{observation}_result:1}} run say {marker}"
        ),
        &marker,
    )
}

fn lowered_entry_resource(output: &LoweringOutput, function: FunctionId) -> Result<String, String> {
    output
        .map()
        .function(function)
        .map(|function| function.entry_resource().to_string())
        .ok_or_else(|| format!("missing lowering map entry for {function:?}"))
}

fn lowered_block_resource(
    output: &LoweringOutput,
    function: FunctionId,
    block: usize,
) -> Result<String, String> {
    let entry = lowered_entry_resource(output, function)?;
    entry
        .strip_suffix("/b0")
        .map(|prefix| format!("{prefix}/b{block}"))
        .ok_or_else(|| format!("unexpected generated entry resource {entry}"))
}

fn emitted_function(pack: &DatapackArtifact, resource: &str) -> Result<String, String> {
    let path = function_pack_path(resource)?;
    let file = pack
        .file(&path)
        .ok_or_else(|| format!("missing emitted function {resource}"))?;
    std::str::from_utf8(file.bytes())
        .map(str::to_owned)
        .map_err(|error| format!("emitted function {resource} was not UTF-8: {error}"))
}

fn function_pack_path(resource: &str) -> Result<String, String> {
    let (namespace, path) = resource
        .split_once(':')
        .ok_or_else(|| format!("invalid function resource {resource}"))?;
    Ok(format!("data/{namespace}/function/{path}.mcfunction"))
}

fn exercise_core_differential(
    server: &mut TestServer,
    reference: &CompiledCorePack,
    baseline: &CompiledCorePack,
) -> Result<(), String> {
    server
        .command("data modify storage mdl_test:stage5h observations set value {}")
        .map_err(|error| error.to_string())?;
    server
        .command("data modify storage mdl_test:stage5h collisions set value {}")
        .map_err(|error| error.to_string())?;
    for pack in [reference, baseline] {
        let marker = format!(
            "MDL_STAGE5H_{}_FRESH_INIT",
            pack.configuration.runtime_label.to_ascii_uppercase()
        );
        expect_marker(
            server,
            &format!(
                "execute if data {} run say {marker}",
                initialization_sentinel(pack.configuration)
            ),
            &marker,
        )?;
        exercise_core_functions(server, pack)?;
    }

    let reference_preserved = reference
        .lowering
        .map()
        .function(reference.fixture.choose)
        .ok_or_else(|| "reference lowering omitted choose".to_owned())?
        .parameter_homes()[1]
        .1
        .clone();
    let baseline_preserved = baseline
        .lowering
        .map()
        .function(baseline.fixture.choose)
        .ok_or_else(|| "Baseline lowering omitted choose".to_owned())?
        .parameter_homes()[1]
        .1
        .clone();
    set_score(server, &reference_preserved, 123)?;
    set_score(server, &baseline_preserved, 456)?;
    let checkpoint = server.log_checkpoint();
    server
        .command("reload")
        .map_err(|error| error.to_string())?;
    server
        .command("say MDL_STAGE5H_RELOAD_COMPLETE")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("MDL_STAGE5H_RELOAD_COMPLETE")
        .map_err(|error| error.to_string())?;
    server
        .check_datapack_logs_since(checkpoint)
        .map_err(|error| error.to_string())?;
    expect_score(server, &reference_preserved, 123)?;
    expect_score(server, &baseline_preserved, 456)?;
    for pack in [reference, baseline] {
        let marker = format!(
            "MDL_STAGE5H_{}_RELOAD_INIT",
            pack.configuration.runtime_label.to_ascii_uppercase()
        );
        expect_marker(
            server,
            &format!(
                "execute if data {} run say {marker}",
                initialization_sentinel(pack.configuration)
            ),
            &marker,
        )?;
        exercise_core_functions(server, pack)?;
    }

    exercise_initialization_collision(server, reference.configuration)?;
    exercise_initialization_collision(server, baseline.configuration)
}

fn exercise_core_functions(server: &mut TestServer, pack: &CompiledCorePack) -> Result<(), String> {
    let map = pack.lowering.map();
    let fixture = pack.fixture;
    let label = pack.configuration.runtime_label;

    invoke(
        server,
        map.function(fixture.call_choose)
            .ok_or_else(|| format!("{label} lowering omitted call_choose"))?,
        &[1, 40, 2],
        &[40],
        &format!("{label}_call_choose_true"),
    )?;
    invoke(
        server,
        map.function(fixture.call_choose)
            .ok_or_else(|| format!("{label} lowering omitted call_choose"))?,
        &[0, 40, 2],
        &[2],
        &format!("{label}_call_choose_false"),
    )?;
    invoke(
        server,
        map.function(fixture.call_choose)
            .ok_or_else(|| format!("{label} lowering omitted call_choose"))?,
        &[1, i32::MIN, i32::MAX],
        &[i32::MIN],
        &format!("{label}_call_choose_i32_min"),
    )?;
    invoke(
        server,
        map.function(fixture.call_choose)
            .ok_or_else(|| format!("{label} lowering omitted call_choose"))?,
        &[0, i32::MIN, i32::MAX],
        &[i32::MAX],
        &format!("{label}_call_choose_i32_max"),
    )?;
    invoke(
        server,
        map.function(fixture.sum_down)
            .ok_or_else(|| format!("{label} lowering omitted sum_down"))?,
        &[4, 10],
        &[20],
        &format!("{label}_sum_down"),
    )?;
    invoke(
        server,
        map.function(fixture.countdown_swap)
            .ok_or_else(|| format!("{label} lowering omitted countdown_swap"))?,
        &[3, 10, 20],
        &[20, 10],
        &format!("{label}_countdown_swap"),
    )?;
    Ok(())
}

fn exercise_initialization_collision(
    server: &mut TestServer,
    configuration: CoreConfiguration,
) -> Result<(), String> {
    let sentinel = initialization_sentinel(configuration);
    server
        .command(&format!("data remove {sentinel}"))
        .map_err(|error| error.to_string())?;
    for key in ["first", "second"] {
        server
            .command(&format!(
                "execute store success storage mdl_test:stage5h collisions.{}_{key}_success byte 1 store result storage mdl_test:stage5h collisions.{}_{key}_result int 1 run function {}:__mdl/load",
                configuration.runtime_label,
                configuration.runtime_label,
                configuration.namespace,
            ))
            .map_err(|error| error.to_string())?;
    }
    let marker = format!(
        "MDL_STAGE5H_{}_COLLISION_REJECTED",
        configuration.runtime_label.to_ascii_uppercase()
    );
    expect_marker(
        server,
        &format!(
            "execute if data storage mdl_test:stage5h collisions{{{}_first_success:0b,{}_first_result:0,{}_second_success:0b,{}_second_result:0}} unless data {sentinel} run say {marker}",
            configuration.runtime_label,
            configuration.runtime_label,
            configuration.runtime_label,
            configuration.runtime_label,
        ),
        &marker,
    )
}

fn invoke(
    server: &mut TestServer,
    function: &LoweredFunction,
    arguments: &[i32],
    expected_results: &[i32],
    observation: &str,
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
        .command(&format!(
            "execute store success storage mdl_test:stage5h observations.{observation}_success byte 1 store result storage mdl_test:stage5h observations.{observation}_result int 1 run function {}",
            function.entry_resource()
        ))
        .map_err(|error| error.to_string())?;
    for ((_, slot), expected) in function.result_homes().iter().zip(expected_results) {
        expect_score(server, slot, *expected)?;
    }
    let marker = format!("MDL_STAGE5H_{}", observation.to_ascii_uppercase());
    expect_marker(
        server,
        &format!(
            "execute if data storage mdl_test:stage5h observations{{{observation}_success:1b,{observation}_result:1}} run say {marker}"
        ),
        &marker,
    )
}

fn initialization_sentinel(configuration: CoreConfiguration) -> String {
    format!(
        "storage {}:__mdl/init/v0/{} \"initialized\"",
        configuration.namespace,
        encode_hex(configuration.objective)
    )
}

fn encode_hex(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.bytes() {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
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
        write_retained_file(sandbox, &name, contents)?;
    }
    Ok(())
}

fn write_retained_file(sandbox: &ServerSandbox, name: &str, contents: &str) -> Result<(), String> {
    fs::write(sandbox.root().join(name), contents)
        .map_err(|error| format!("write retained {name}: {error}"))
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

fn accepted_coalescing_merges(report: &str) -> u64 {
    report
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find_map(|field| field.strip_prefix("merges="))
        })
        .filter_map(|merges| merges.parse::<u64>().ok())
        .sum()
}

fn lowering_options_for(namespace: &str, objective: &str) -> Result<LoweringOptions, String> {
    LoweringOptions::new(
        JavaEditionTarget::V26_2,
        PackNamespace::new(namespace).map_err(|error| error.to_string())?,
        ObjectiveName::new(objective).map_err(|error| error.to_string())?,
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
