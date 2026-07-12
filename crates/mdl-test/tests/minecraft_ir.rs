use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use mdl_compiler::datapack::{EmissionOptions, EmissionOutput, emit_datapack};
use mdl_compiler::ir::minecraft::{
    AtMostOneSelector, CallableRef, CommandKind, CommandNode, Condition, DataCommand,
    DataModifyMode, DataSource, DimensionId, ExecuteCommand, ExecuteModifier, ExecuteModifierKind,
    ExecuteModifiers, ExternalCallableRef, ExternalTagRequirement, FakeScoreHolder, FiniteF64,
    FunctionCall, FunctionResourceId, FunctionTagEntry, FunctionTagMerge, FunctionTagResourceId,
    InternalCallableRef, MinecraftProgramBuilder, NbtKey, NbtPath, NbtPathKey, NbtPathSegment,
    NbtValue, NonNegativeI32, ObjectiveName, ReturnCommand, ScoreCommand, ScoreComparison,
    ScoreHolders, ScoreOperation, ScoreRange, ScoreRef, ScoreSelection, SingleScoreHolder,
    StorageId, StorageNumericType, StoragePath, StoreChannel, StoreDestination, UnboundedSelector,
    UnsafeRawCommand,
};
use mdl_compiler::source::{Origin, OriginId, SourceContext};
use mdl_compiler::target::JavaEditionTarget;
use mdl_test::{ServerConfig, ServerSandbox, TestServer};

const DONE_MARKER: &str = "MDL_STAGE3_DONE";
const LIMIT_PREFIX: &str = "data modify storage mdl:state \"utf16_limit\" set value \"";

#[test]
#[ignore = "requires the official Minecraft 26.2 server JAR and Java 25"]
fn direct_minecraft_ir_runs_on_vanilla_26_2() {
    if let Err(error) = run_conformance() {
        panic!("{error}");
    }
}

fn run_conformance() -> Result<(), String> {
    let server_jar = env::var_os("MDL_SERVER_JAR")
        .map(PathBuf::from)
        .ok_or_else(|| "set MDL_SERVER_JAR to the official Minecraft 26.2 server JAR".to_owned())?;
    let java = env::var_os("MDL_JAVA").map_or_else(|| PathBuf::from("java"), PathBuf::from);
    let config = ServerConfig::new(java, server_jar);
    let (output, trace) = build_pack()?;
    let (second, second_trace) = build_pack()?;
    if output.pack() != second.pack() {
        return Err("repeated Stage 3 emission was not byte-identical".to_owned());
    }
    if trace != second_trace {
        return Err("repeated Stage 3 trace emission was not identical".to_owned());
    }

    let preserve_success = env::var_os("MDL_KEEP_TEST_DIR").is_some();
    let sandbox = ServerSandbox::create(preserve_success).map_err(|error| error.to_string())?;
    sandbox
        .install_datapack(
            "mdl_stage3",
            output
                .pack()
                .files()
                .iter()
                .map(|file| (file.path().as_str(), file.bytes())),
        )
        .map_err(|error| error.to_string())?;
    fs::write(sandbox.root().join("stage3-trace.txt"), trace)
        .map_err(|error| format!("write retained trace: {error}"))?;

    let mut server = sandbox.start(&config).map_err(|error| error.to_string())?;
    let root = server.root().to_path_buf();
    let result = exercise_pack(&mut server);
    if let Err(error) = result {
        server.preserve_sandbox();
        return Err(format!(
            "{error}\nStage 3 conformance sandbox preserved at {}",
            root.display()
        ));
    }
    server.shutdown().map_err(|error| error.to_string())?;
    Ok(())
}

fn exercise_pack(server: &mut TestServer) -> Result<(), String> {
    server
        .command("forceload add 0 0")
        .map_err(|error| error.to_string())?;
    server
        .command("kill @e")
        .map_err(|error| error.to_string())?;
    server
        .command("summon armor_stand 0 100 0")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log("Summoned new Armor Stand")
        .map_err(|error| error.to_string())?;
    expect_marker(server, "execute if entity @e run say MDL_ENTITY_READY")?;
    let checkpoint = server.log_checkpoint();
    server
        .command("function mdl:run")
        .map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(DONE_MARKER)
        .map_err(|error| error.to_string())?;
    server
        .check_datapack_logs_since(checkpoint)
        .map_err(|error| error.to_string())?;

    let scores = [
        ("#a", 20),
        ("#assign", 20),
        ("#add", 4),
        ("#sub", 7),
        ("#mul", 12),
        ("#div", 6),
        ("#mod", 2),
        ("#min", 3),
        ("#max", 3),
        ("#swapl", 9),
        ("#swapr", 5),
        ("#match", 1),
        ("#equal", 1),
        ("#less", 1),
        ("#less_equal", 1),
        ("#greater", 1),
        ("#greater_equal", 1),
        ("#unless", 1),
        ("#data_condition", 1),
        ("#entity_condition", 1),
        ("#contexts", 1),
        ("#calls", 2),
        ("#return_value", 7),
        ("#return_success", 0),
        ("#return_run", 20),
        ("#final", 42),
    ];
    for (holder, expected) in scores {
        expect_score(server, holder, expected)?;
    }
    expect_marker(
        server,
        concat!(
            "execute if data storage mdl:state {",
            "copied:{a:1,c:3},ordered:[0,1,2,3],",
            "stored_byte:20b,stored_short:20s,stored_int:20,stored_long:20L,",
            "stored_float:20.0f,stored_double:20.0d,stored_success:1b",
            "} run say MDL_STORAGE_EXACT"
        ),
    )?;
    Ok(())
}

fn expect_score(server: &mut TestServer, holder: &str, expected: i32) -> Result<(), String> {
    server
        .command(&format!("scoreboard players get {holder} mdl.reg"))
        .map_err(|error| error.to_string())?;
    let line = server
        .wait_for_command_log(&format!("{holder} has"))
        .map_err(|error| error.to_string())?;
    let expected_text = format!("{holder} has {expected} [mdl.reg]");
    if line.contains(&expected_text) {
        Ok(())
    } else {
        Err(format!(
            "expected scoreboard output containing {expected_text:?}, got {line:?}"
        ))
    }
}

fn expect_marker(server: &mut TestServer, command: &str) -> Result<(), String> {
    let marker = command
        .rsplit_once("run say ")
        .map(|(_, marker)| marker)
        .ok_or_else(|| format!("validation command has no marker: {command}"))?;
    server.command(command).map_err(|error| error.to_string())?;
    server
        .wait_for_command_log(marker)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn build_pack() -> Result<(EmissionOutput, String), String> {
    let mut sources = SourceContext::new();
    let origin = sources
        .add_origin(Origin::Unknown)
        .map_err(|error| error.to_string())?;
    let nested_origin = sources
        .add_origin(Origin::Unknown)
        .map_err(|error| error.to_string())?;
    let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);

    let init = declare(&mut builder, "mdl:init", origin)?;
    let tick = declare(&mut builder, "mdl:tick", origin)?;
    let leaf = declare(&mut builder, "mdl:leaf", origin)?;
    let return_value = declare(&mut builder, "mdl:return_value", origin)?;
    let return_fail = declare(&mut builder, "mdl:return_fail", origin)?;
    let return_run = declare(&mut builder, "mdl:return_run", origin)?;
    let empty = declare(&mut builder, "mdl:empty", origin)?;
    let utf16_limit = declare(&mut builder, "mdl:utf16_limit", origin)?;
    let run = declare(&mut builder, "mdl:run", origin)?;

    let load_tag = declare_tag(&mut builder, "minecraft:load", origin)?;
    let tick_tag = declare_tag(&mut builder, "minecraft:tick", origin)?;
    let base_tag = declare_tag(&mut builder, "mdl:base", origin)?;
    let nested_tag = declare_tag(&mut builder, "mdl:nested", origin)?;

    define(
        &mut builder,
        init,
        vec![node(
            CommandKind::Score(ScoreCommand::ObjectiveAddDummy {
                objective: objective(),
            }),
            origin,
        )?],
    )?;
    define(&mut builder, tick, vec![])?;
    define(&mut builder, leaf, vec![score_add("#calls", 1, origin)?])?;
    define(
        &mut builder,
        return_value,
        vec![node(CommandKind::Return(ReturnCommand::Value(7)), origin)?],
    )?;
    define(
        &mut builder,
        return_fail,
        vec![node(CommandKind::Return(ReturnCommand::Fail), origin)?],
    )?;
    define(
        &mut builder,
        return_run,
        vec![node(
            CommandKind::Return(ReturnCommand::run(node(
                CommandKind::Score(ScoreCommand::PlayersGet { score: score("#a") }),
                nested_origin,
            )?)),
            origin,
        )?],
    )?;
    define(&mut builder, empty, vec![])?;
    let exact_limit_payload = "x".repeat(2_000_000 - LIMIT_PREFIX.len() - 1);
    define(
        &mut builder,
        utf16_limit,
        vec![data_modify(
            "utf16_limit",
            DataModifyMode::Set,
            DataSource::Value(NbtValue::string_owned(exact_limit_payload)),
            origin,
        )?],
    )?;
    define(
        &mut builder,
        run,
        run_commands(
            leaf,
            return_value,
            return_fail,
            return_run,
            nested_tag,
            origin,
            nested_origin,
        )?,
    )?;

    define_tag(
        &mut builder,
        load_tag,
        vec![FunctionTagEntry::internal(
            InternalCallableRef::Function(init),
            origin,
        )],
    )?;
    define_tag(
        &mut builder,
        tick_tag,
        vec![FunctionTagEntry::internal(
            InternalCallableRef::Function(tick),
            origin,
        )],
    )?;
    define_tag(
        &mut builder,
        base_tag,
        vec![FunctionTagEntry::internal(
            InternalCallableRef::Function(leaf),
            origin,
        )],
    )?;
    define_tag(
        &mut builder,
        nested_tag,
        vec![
            FunctionTagEntry::internal(InternalCallableRef::Tag(base_tag), origin),
            FunctionTagEntry::internal(InternalCallableRef::Function(leaf), origin),
            FunctionTagEntry::external(
                ExternalCallableRef::Function(
                    FunctionResourceId::parse("other:optional")
                        .map_err(|error| error.to_string())?,
                ),
                ExternalTagRequirement::Optional,
                origin,
            ),
        ],
    )?;

    let program = builder.finish().map_err(|error| error.to_string())?;
    let output = emit_datapack(
        &program,
        &sources,
        &EmissionOptions::new("MDL Stage 3 conformance"),
    )
    .map_err(|error| error.to_string())?;
    validate_trace(&program, &output)?;
    let limit_file = output
        .pack()
        .file("data/mdl/function/utf16_limit.mcfunction")
        .ok_or_else(|| "missing UTF-16 boundary fixture".to_owned())?;
    if limit_file.bytes().len() != 2_000_001 {
        return Err(format!(
            "UTF-16 boundary fixture has {} bytes including LF, expected 2000001",
            limit_file.bytes().len()
        ));
    }
    let mut trace = String::new();
    for record in output.trace().records() {
        writeln!(
            trace,
            "{} {:?} line={} command={:?} origin={:?}",
            record.function(),
            record.function_id(),
            record.line(),
            record.command(),
            record.origin()
        )
        .map_err(|error| error.to_string())?;
    }
    Ok((output, trace))
}

fn validate_trace(
    program: &mdl_compiler::ir::minecraft::MinecraftProgram,
    output: &EmissionOutput,
) -> Result<(), String> {
    for (function_id, function) in program.functions() {
        let trace = output
            .trace()
            .function(function_id)
            .ok_or_else(|| format!("missing trace for {}", function.resource()))?;
        if trace.len() != function.body().len() {
            return Err(format!(
                "trace/body length mismatch for {}: {} versus {}",
                function.resource(),
                trace.len(),
                function.body().len()
            ));
        }
        let path = function.resource().pack_path(program.target());
        let file = output
            .pack()
            .file(path.as_str())
            .ok_or_else(|| format!("missing artifact for {}", function.resource()))?;
        let physical_lines = file.bytes().split_inclusive(|byte| *byte == b'\n').count();
        if physical_lines != function.body().len() {
            return Err(format!(
                "physical line/body mismatch for {}: {physical_lines} versus {}",
                function.resource(),
                function.body().len()
            ));
        }
        for (command, node) in function.body().commands() {
            if trace.origin(command) != Some(node.origin()) {
                return Err(format!(
                    "origin mismatch for {} command {command:?}",
                    function.resource()
                ));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn run_commands(
    leaf: mdl_compiler::ir::minecraft::McFunctionId,
    return_value: mdl_compiler::ir::minecraft::McFunctionId,
    return_fail: mdl_compiler::ir::minecraft::McFunctionId,
    return_run: mdl_compiler::ir::minecraft::McFunctionId,
    nested_tag: mdl_compiler::ir::minecraft::FunctionTagId,
    origin: OriginId,
    nested_origin: OriginId,
) -> Result<Vec<CommandNode>, String> {
    let mut commands = vec![];
    for (holder, value) in [
        ("#a", 20),
        ("#b", 3),
        ("#assign", 0),
        ("#add", 1),
        ("#sub", 10),
        ("#mul", 4),
        ("#div", 20),
        ("#mod", 20),
        ("#min", 10),
        ("#max", 1),
        ("#swapl", 5),
        ("#swapr", 9),
        ("#match", 0),
        ("#equal", 0),
        ("#less", 0),
        ("#less_equal", 0),
        ("#greater", 0),
        ("#greater_equal", 0),
        ("#unless", 0),
        ("#data_condition", 0),
        ("#entity_condition", 0),
        ("#contexts", 0),
        ("#calls", 0),
        ("#temp", 1),
        ("#final", 42),
    ] {
        commands.push(score_set(holder, value, origin)?);
    }
    commands.push(score_add("#a", 2, origin)?);
    commands.push(score_remove("#a", 2, origin)?);
    commands.push(node(
        CommandKind::Score(ScoreCommand::PlayersGet { score: score("#a") }),
        origin,
    )?);
    commands.push(node(
        CommandKind::Score(ScoreCommand::PlayersReset {
            target: selection("#temp"),
        }),
        origin,
    )?);
    for (target, operation, source) in [
        ("#assign", ScoreOperation::Assign, "#a"),
        ("#add", ScoreOperation::Add, "#b"),
        ("#sub", ScoreOperation::Subtract, "#b"),
        ("#mul", ScoreOperation::Multiply, "#b"),
        ("#div", ScoreOperation::Divide, "#b"),
        ("#mod", ScoreOperation::Modulo, "#b"),
        ("#min", ScoreOperation::Min, "#b"),
        ("#max", ScoreOperation::Max, "#b"),
        ("#swapl", ScoreOperation::Swap, "#swapr"),
    ] {
        commands.push(node(
            CommandKind::Score(ScoreCommand::PlayersOperation {
                target: selection(target),
                op: operation,
                source: selection(source),
            }),
            origin,
        )?);
    }
    commands.push(node(
        CommandKind::Score(ScoreCommand::PlayersSet {
            target: ScoreSelection::new(
                ScoreHolders::Selector(UnboundedSelector::AllEntities.into()),
                objective(),
            ),
            value: 11,
        }),
        origin,
    )?);

    commands.push(conditional(
        ExecuteModifierKind::If(Condition::ScoreMatches(score("#a"), ScoreRange::exact(20))),
        score_add("#match", 1, nested_origin)?,
        origin,
    )?);
    for (counter, left, comparison, right) in [
        ("#equal", "#assign", ScoreComparison::Equal, "#a"),
        ("#less", "#b", ScoreComparison::LessThan, "#a"),
        ("#less_equal", "#b", ScoreComparison::LessOrEqual, "#a"),
        ("#greater", "#a", ScoreComparison::GreaterThan, "#b"),
        (
            "#greater_equal",
            "#a",
            ScoreComparison::GreaterOrEqual,
            "#b",
        ),
    ] {
        commands.push(conditional(
            ExecuteModifierKind::If(Condition::ScoreCompare(
                score(left),
                comparison,
                score(right),
            )),
            score_add(counter, 1, nested_origin)?,
            origin,
        )?);
    }
    commands.push(conditional(
        ExecuteModifierKind::Unless(Condition::ScoreMatches(score("#a"), ScoreRange::exact(0))),
        score_add("#unless", 1, nested_origin)?,
        origin,
    )?);

    commands.extend(data_commands(origin)?);
    commands.push(conditional(
        ExecuteModifierKind::If(Condition::DataExists(storage("copied"))),
        score_add("#data_condition", 1, nested_origin)?,
        origin,
    )?);
    commands.push(conditional(
        ExecuteModifierKind::If(Condition::EntityExists(
            UnboundedSelector::AllEntities.into(),
        )),
        score_add("#entity_condition", 1, nested_origin)?,
        origin,
    )?);
    commands.push(execute(
        vec![
            ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
            ExecuteModifierKind::At(AtMostOneSelector::SelfExecutor.into()),
            ExecuteModifierKind::In(
                DimensionId::parse("minecraft:overworld").map_err(|error| error.to_string())?,
            ),
        ],
        score_add("#contexts", 1, nested_origin)?,
        origin,
    )?);

    commands.push(call(InternalCallableRef::Function(leaf), origin)?);
    commands.push(call(InternalCallableRef::Tag(nested_tag), origin)?);
    commands.push(store_score_call(
        StoreChannel::Result,
        "#return_value",
        return_value,
        origin,
    )?);
    commands.push(store_score_call(
        StoreChannel::Success,
        "#return_success",
        return_fail,
        origin,
    )?);
    commands.push(store_score_call(
        StoreChannel::Result,
        "#return_run",
        return_run,
        origin,
    )?);
    for (key, numeric_type) in [
        ("stored_byte", StorageNumericType::Byte),
        ("stored_short", StorageNumericType::Short),
        ("stored_int", StorageNumericType::Int),
        ("stored_long", StorageNumericType::Long),
        ("stored_float", StorageNumericType::Float),
        ("stored_double", StorageNumericType::Double),
    ] {
        commands.push(execute(
            vec![ExecuteModifierKind::Store(
                StoreChannel::Result,
                StoreDestination::Storage {
                    target: storage(key),
                    numeric_type,
                    scale: FiniteF64::new(1.0).map_err(|error| error.to_string())?,
                },
            )],
            node(
                CommandKind::Score(ScoreCommand::PlayersGet { score: score("#a") }),
                nested_origin,
            )?,
            origin,
        )?);
    }
    commands.push(execute(
        vec![ExecuteModifierKind::Store(
            StoreChannel::Success,
            StoreDestination::Storage {
                target: storage("stored_success"),
                numeric_type: StorageNumericType::Byte,
                scale: FiniteF64::new(1.0).map_err(|error| error.to_string())?,
            },
        )],
        node(
            CommandKind::Score(ScoreCommand::PlayersGet { score: score("#a") }),
            nested_origin,
        )?,
        origin,
    )?);
    commands.push(raw(&format!("say {DONE_MARKER}"), origin)?);
    Ok(commands)
}

fn data_commands(origin: OriginId) -> Result<Vec<CommandNode>, String> {
    let ordered = NbtValue::list(vec![NbtValue::int(1), NbtValue::int(2)])
        .map_err(|error| error.to_string())?;
    let source = NbtValue::compound(vec![
        (NbtKey::new("b"), NbtValue::int(2)),
        (NbtKey::new("a"), NbtValue::int(1)),
    ])
    .map_err(|error| error.to_string())?;
    let merge = NbtValue::compound(vec![(NbtKey::new("c"), NbtValue::int(3))])
        .map_err(|error| error.to_string())?;
    let commands = vec![
        data_modify(
            "ordered",
            DataModifyMode::Set,
            DataSource::Value(ordered),
            origin,
        )?,
        data_modify(
            "ordered",
            DataModifyMode::Append,
            DataSource::Value(NbtValue::int(3)),
            origin,
        )?,
        data_modify(
            "ordered",
            DataModifyMode::Prepend,
            DataSource::Value(NbtValue::int(0)),
            origin,
        )?,
        data_modify(
            "source",
            DataModifyMode::Set,
            DataSource::Value(source),
            origin,
        )?,
        data_modify(
            "copied",
            DataModifyMode::Set,
            DataSource::From(storage("source")),
            origin,
        )?,
        data_modify(
            "copied",
            DataModifyMode::Merge,
            DataSource::Value(merge),
            origin,
        )?,
        node(
            CommandKind::Data(DataCommand::Get {
                source: storage("copied"),
                scale: None,
            }),
            origin,
        )?,
        node(
            CommandKind::Data(DataCommand::Get {
                source: storage("copied.a"),
                scale: Some(FiniteF64::new(2.0).map_err(|error| error.to_string())?),
            }),
            origin,
        )?,
        node(
            CommandKind::Data(DataCommand::Remove {
                target: storage("copied.b"),
            }),
            origin,
        )?,
    ];
    Ok(commands)
}

fn declare(
    builder: &mut MinecraftProgramBuilder,
    resource: &str,
    origin: OriginId,
) -> Result<mdl_compiler::ir::minecraft::McFunctionId, String> {
    builder
        .declare_function(
            FunctionResourceId::parse(resource).map_err(|error| error.to_string())?,
            origin,
        )
        .map_err(|error| error.to_string())
}

fn declare_tag(
    builder: &mut MinecraftProgramBuilder,
    resource: &str,
    origin: OriginId,
) -> Result<mdl_compiler::ir::minecraft::FunctionTagId, String> {
    builder
        .declare_function_tag(
            FunctionTagResourceId::parse(resource).map_err(|error| error.to_string())?,
            origin,
            FunctionTagMerge::Append,
        )
        .map_err(|error| error.to_string())
}

fn define(
    builder: &mut MinecraftProgramBuilder,
    function: mdl_compiler::ir::minecraft::McFunctionId,
    commands: Vec<CommandNode>,
) -> Result<(), String> {
    let mut body = builder
        .begin_function(function)
        .map_err(|error| error.to_string())?;
    for command in commands {
        body.push(command).map_err(|error| error.to_string())?;
    }
    body.finish();
    Ok(())
}

fn define_tag(
    builder: &mut MinecraftProgramBuilder,
    tag: mdl_compiler::ir::minecraft::FunctionTagId,
    entries: Vec<FunctionTagEntry>,
) -> Result<(), String> {
    let mut definition = builder
        .begin_function_tag(tag)
        .map_err(|error| error.to_string())?;
    for entry in entries {
        definition.push(entry);
    }
    definition.finish();
    Ok(())
}

fn objective() -> ObjectiveName {
    ObjectiveName::new("mdl.reg").unwrap()
}

fn score(holder: &str) -> ScoreRef {
    ScoreRef::new(
        SingleScoreHolder::from(FakeScoreHolder::new(holder).unwrap()),
        objective(),
    )
}

fn selection(holder: &str) -> ScoreSelection {
    ScoreSelection::new(
        ScoreHolders::from(FakeScoreHolder::new(holder).unwrap()),
        objective(),
    )
}

fn storage(path: &str) -> StoragePath {
    let segments = path
        .split('.')
        .map(|key| NbtPathSegment::Key(NbtPathKey::new(key).unwrap()))
        .collect::<Vec<_>>();
    StoragePath::new(
        StorageId::parse("mdl:state").unwrap(),
        NbtPath::from_segments(segments).unwrap(),
    )
}

fn node(kind: CommandKind, origin: OriginId) -> Result<CommandNode, String> {
    CommandNode::new(kind, origin).map_err(|error| error.to_string())
}

fn raw(line: &str, origin: OriginId) -> Result<CommandNode, String> {
    node(
        CommandKind::Raw(UnsafeRawCommand::new(line).map_err(|error| error.to_string())?),
        origin,
    )
}

fn score_set(holder: &str, value: i32, origin: OriginId) -> Result<CommandNode, String> {
    node(
        CommandKind::Score(ScoreCommand::PlayersSet {
            target: selection(holder),
            value,
        }),
        origin,
    )
}

fn score_add(holder: &str, amount: i32, origin: OriginId) -> Result<CommandNode, String> {
    node(
        CommandKind::Score(ScoreCommand::PlayersAdd {
            target: selection(holder),
            amount: NonNegativeI32::new(amount).map_err(|error| error.to_string())?,
        }),
        origin,
    )
}

fn score_remove(holder: &str, amount: i32, origin: OriginId) -> Result<CommandNode, String> {
    node(
        CommandKind::Score(ScoreCommand::PlayersRemove {
            target: selection(holder),
            amount: NonNegativeI32::new(amount).map_err(|error| error.to_string())?,
        }),
        origin,
    )
}

fn data_modify(
    path: &str,
    mode: DataModifyMode,
    source: DataSource,
    origin: OriginId,
) -> Result<CommandNode, String> {
    node(
        CommandKind::Data(DataCommand::Modify {
            target: storage(path),
            mode,
            source,
        }),
        origin,
    )
}

fn execute(
    modifiers: Vec<ExecuteModifierKind>,
    run: CommandNode,
    origin: OriginId,
) -> Result<CommandNode, String> {
    let mut modifiers = modifiers.into_iter();
    let first = modifiers
        .next()
        .ok_or_else(|| "execute helper requires a modifier".to_owned())?;
    let remainder = modifiers
        .map(|kind| ExecuteModifier::new(kind, origin))
        .collect();
    node(
        CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(ExecuteModifier::new(first, origin), remainder),
            run,
        )),
        origin,
    )
}

fn conditional(
    modifier: ExecuteModifierKind,
    run: CommandNode,
    origin: OriginId,
) -> Result<CommandNode, String> {
    execute(vec![modifier], run, origin)
}

fn call(target: InternalCallableRef, origin: OriginId) -> Result<CommandNode, String> {
    node(
        CommandKind::Function(FunctionCall::new(CallableRef::from(target))),
        origin,
    )
}

fn store_score_call(
    channel: StoreChannel,
    holder: &str,
    target: mdl_compiler::ir::minecraft::McFunctionId,
    origin: OriginId,
) -> Result<CommandNode, String> {
    execute(
        vec![ExecuteModifierKind::Store(
            channel,
            StoreDestination::Score(score(holder)),
        )],
        call(InternalCallableRef::Function(target), origin)?,
        origin,
    )
}
