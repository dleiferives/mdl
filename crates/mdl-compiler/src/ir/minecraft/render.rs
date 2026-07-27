use std::error::Error;
use std::fmt;
use std::fmt::Write as _;

use crate::ir::core::Operand;
use crate::source::OriginId;

use super::{
    CallableRef, CommandId, CommandKind, CommandNode, Condition, DataCommand, DataModifyMode,
    DataSource, ExecuteModifierKind, ExternalCallableRef, FunctionTagId, InternalCallableRef,
    MacroCommand, MacroSegment, McFunction, McFunctionId, MinecraftProgram, NbtValue,
    ReturnCommand, ScoreCommand, ScoreComparison, ScoreHolders, ScoreOperation, ScoreRef,
    ScoreSelection, StorageNumericType, StoragePath, StoreChannel, StoreDestination,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RenderError {
    CommandTooLong { units: usize, max: usize },
    InvalidInternalFunction(McFunctionId),
    InvalidInternalTag(FunctionTagId),
    FormattingFailed,
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandTooLong { units, max } => {
                write!(
                    formatter,
                    "command has {units} UTF-16 units; maximum is {max}"
                )
            }
            Self::InvalidInternalFunction(function) => {
                write!(formatter, "invalid internal function {function:?}")
            }
            Self::InvalidInternalTag(tag) => write!(formatter, "invalid internal tag {tag:?}"),
            Self::FormattingFailed => formatter.write_str("target formatting failed unexpectedly"),
        }
    }
}

impl Error for RenderError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionRenderError {
    pub(crate) command: CommandId,
    pub(crate) origin: OriginId,
    pub(crate) error: RenderError,
}

pub(super) struct CommandSink {
    bytes: Vec<u8>,
    line_units: usize,
    max_line_units: usize,
    error: Option<RenderError>,
}

impl CommandSink {
    fn new(max_line_units: usize) -> Self {
        Self {
            bytes: vec![],
            line_units: 0,
            max_line_units,
            error: None,
        }
    }

    fn push_checked(&mut self, value: &str) -> Result<(), RenderError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let added = value.encode_utf16().count();
        let units = self.line_units.saturating_add(added);
        if units > self.max_line_units {
            let error = RenderError::CommandTooLong {
                units,
                max: self.max_line_units,
            };
            self.error = Some(error.clone());
            return Err(error);
        }
        self.bytes.extend_from_slice(value.as_bytes());
        self.line_units = units;
        Ok(())
    }

    fn write_arguments(&mut self, arguments: fmt::Arguments<'_>) -> Result<(), RenderError> {
        if self.write_fmt(arguments).is_err() {
            Err(self.error.clone().unwrap_or(RenderError::FormattingFailed))
        } else {
            Ok(())
        }
    }

    fn finish_command(&mut self) {
        self.bytes.push(b'\n');
        self.line_units = 0;
    }

    fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl fmt::Write for CommandSink {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.push_checked(value).map_err(|_| fmt::Error)
    }
}

pub(crate) fn render_function(
    program: &MinecraftProgram,
    function: &McFunction,
) -> Result<Vec<u8>, FunctionRenderError> {
    let max = usize::try_from(program.target().spec().max_logical_command_utf16_units())
        .unwrap_or(usize::MAX);
    let mut sink = CommandSink::new(max);
    for (command, node) in function.body().commands() {
        if let Err(error) = render_command(program, node, &mut sink) {
            return Err(FunctionRenderError {
                command,
                origin: node.origin(),
                error,
            });
        }
        sink.finish_command();
    }
    Ok(sink.into_bytes())
}

fn render_command(
    program: &MinecraftProgram,
    command: &CommandNode,
    sink: &mut CommandSink,
) -> Result<(), RenderError> {
    match command.kind() {
        CommandKind::Score(command) => render_score(command, sink),
        CommandKind::Data(command) => render_data(command, sink),
        CommandKind::Say(command) => {
            sink.push_checked("say ")?;
            sink.push_checked(command.message().as_str())
        }
        CommandKind::Teleport(command) => {
            sink.push_checked("teleport @s ")?;
            sink.write_arguments(format_args!("{}", command.destination()))
        }
        CommandKind::Execute(command) => {
            sink.push_checked("execute")?;
            for modifier in command.modifiers().as_slice() {
                sink.push_checked(" ")?;
                render_modifier(program, modifier.kind(), sink)?;
            }
            sink.push_checked(" run ")?;
            render_command(program, command.run(), sink)
        }
        CommandKind::Function(call) => {
            sink.push_checked("function ")?;
            render_callable(program, call.target(), sink)
        }
        CommandKind::Return(ReturnCommand::Value(value)) => {
            sink.write_arguments(format_args!("return {value}"))
        }
        CommandKind::Return(ReturnCommand::Fail) => sink.push_checked("return fail"),
        CommandKind::Return(ReturnCommand::Run(command)) => {
            sink.push_checked("return run ")?;
            render_command(program, command, sink)
        }
        CommandKind::Raw(command) => sink.push_checked(command.as_str()),
        CommandKind::Macro(command) => render_macro(command, sink),
        CommandKind::FunctionWithStorage(call) => {
            sink.push_checked("function ")?;
            render_callable(program, &call.target, sink)?;
            sink.push_checked(" with storage ")?;
            sink.write_arguments(format_args!(
                "{} {}",
                call.storage.storage(),
                call.storage.path()
            ))
        }
        CommandKind::AdvancementRevoke(command) => {
            sink.push_checked("advancement revoke @s only ")?;
            sink.write_arguments(format_args!("{}", command.resource()))
        }
        CommandKind::Schedule(command) => {
            sink.push_checked("schedule function ")?;
            render_internal_callable(
                program,
                InternalCallableRef::Function(command.target()),
                sink,
            )?;
            sink.push_checked(" ")?;
            sink.write_arguments(format_args!("{}t", command.delay_ticks()))?;
            match command.mode() {
                crate::ir::core::ScheduleMode::Append => sink.push_checked(" append"),
                crate::ir::core::ScheduleMode::Replace => sink.push_checked(" replace"),
            }
        }
        CommandKind::ScheduleClear(command) => {
            sink.push_checked("schedule clear ")?;
            render_internal_callable(
                program,
                InternalCallableRef::Function(command.target()),
                sink,
            )
        }
        CommandKind::ItemReplaceBlock(command) => {
            let position = command.position();
            let Operand::Const(slot) = command.slot() else {
                panic!(
                    "runtime container slot rendered outside a macro context; \
                     PS-11C extract_crossings handles $(key) substitution"
                );
            };
            let Operand::Const(item_id) = command.item_id() else {
                panic!(
                    "runtime item id rendered outside a macro context; \
                     PS-11C extract_crossings handles $(key) substitution"
                );
            };
            let Operand::Const(count) = command.count() else {
                panic!(
                    "runtime count rendered outside a macro context; \
                     PS-11C extract_crossings handles $(key) substitution"
                );
            };
            sink.write_arguments(format_args!(
                "item replace block {} {} {} container.{slot} with {item_id} {count}",
                position.x, position.y, position.z,
            ))
        }
    }
}

fn render_score(command: &ScoreCommand, sink: &mut CommandSink) -> Result<(), RenderError> {
    match command {
        ScoreCommand::ObjectiveAddDummy { objective } => {
            sink.write_arguments(format_args!("scoreboard objectives add {objective} dummy"))
        }
        ScoreCommand::PlayersSet { target, value } => {
            sink.push_checked("scoreboard players set ")?;
            render_selection(target, sink)?;
            sink.write_arguments(format_args!(" {value}"))
        }
        ScoreCommand::PlayersAdd { target, amount } => {
            sink.push_checked("scoreboard players add ")?;
            render_selection(target, sink)?;
            sink.write_arguments(format_args!(" {amount}"))
        }
        ScoreCommand::PlayersRemove { target, amount } => {
            sink.push_checked("scoreboard players remove ")?;
            render_selection(target, sink)?;
            sink.write_arguments(format_args!(" {amount}"))
        }
        ScoreCommand::PlayersGet { score } => {
            sink.push_checked("scoreboard players get ")?;
            render_score_ref(score, sink)
        }
        ScoreCommand::PlayersReset { target } => {
            sink.push_checked("scoreboard players reset ")?;
            render_selection(target, sink)
        }
        ScoreCommand::PlayersOperation { target, op, source } => {
            sink.push_checked("scoreboard players operation ")?;
            render_selection(target, sink)?;
            sink.push_checked(" ")?;
            sink.push_checked(score_operation_token(*op))?;
            sink.push_checked(" ")?;
            render_selection(source, sink)
        }
    }
}

fn render_selection(selection: &ScoreSelection, sink: &mut CommandSink) -> Result<(), RenderError> {
    match selection.holders() {
        ScoreHolders::Fake(holder) => sink.write_arguments(format_args!("{holder}"))?,
        ScoreHolders::Selector(selector) => sink.write_arguments(format_args!("{selector}"))?,
        ScoreHolders::AllTracked => sink.push_checked("*")?,
    }
    sink.write_arguments(format_args!(" {}", selection.objective()))
}

fn render_score_ref(score: &ScoreRef, sink: &mut CommandSink) -> Result<(), RenderError> {
    sink.write_arguments(format_args!("{score}"))
}

const fn score_operation_token(operation: ScoreOperation) -> &'static str {
    match operation {
        ScoreOperation::Assign => "=",
        ScoreOperation::Add => "+=",
        ScoreOperation::Subtract => "-=",
        ScoreOperation::Multiply => "*=",
        ScoreOperation::Divide => "/=",
        ScoreOperation::Modulo => "%=",
        ScoreOperation::Min => "<",
        ScoreOperation::Max => ">",
        ScoreOperation::Swap => "><",
    }
}

fn render_data(command: &DataCommand, sink: &mut CommandSink) -> Result<(), RenderError> {
    match command {
        DataCommand::Get { source, scale } => {
            sink.push_checked("data get ")?;
            render_storage_path(source, sink)?;
            if let Some(scale) = scale {
                sink.write_arguments(format_args!(" {scale}"))?;
            }
            Ok(())
        }
        DataCommand::Remove { target } => {
            sink.push_checked("data remove ")?;
            render_storage_path(target, sink)
        }
        DataCommand::Modify {
            target,
            mode,
            source,
        } => {
            sink.push_checked("data modify ")?;
            render_storage_path(target, sink)?;
            sink.write_arguments(format_args!(" {} ", data_modify_token(*mode)))?;
            match source {
                DataSource::Value(value) => {
                    sink.push_checked("value ")?;
                    render_nbt(value, sink)
                }
                DataSource::From(path) => {
                    sink.push_checked("from ")?;
                    render_storage_path(path, sink)
                }
                DataSource::StringSlice { source, start, end } => {
                    sink.push_checked("string ")?;
                    render_storage_path(source, sink)?;
                    sink.write_arguments(format_args!(" {start}"))?;
                    if let Some(end) = end {
                        sink.write_arguments(format_args!(" {end}"))?;
                    }
                    Ok(())
                }
                DataSource::Entity { selector, path } => {
                    sink.write_arguments(format_args!("from entity {selector} "))?;
                    sink.write_arguments(format_args!("{path}"))
                }
                DataSource::Block { position, path } => {
                    sink.write_arguments(format_args!(
                        "from block {} {} {} ",
                        position.x, position.y, position.z
                    ))?;
                    sink.write_arguments(format_args!("{path}"))
                }
            }
        }
    }
}

const fn data_modify_token(mode: DataModifyMode) -> &'static str {
    match mode {
        DataModifyMode::Set => "set",
        DataModifyMode::Merge => "merge",
        DataModifyMode::Append => "append",
        DataModifyMode::Prepend => "prepend",
    }
}

fn render_storage_path(path: &StoragePath, sink: &mut CommandSink) -> Result<(), RenderError> {
    sink.write_arguments(format_args!("storage {} {}", path.storage(), path.path()))
}

fn render_nbt(value: &NbtValue, sink: &mut CommandSink) -> Result<(), RenderError> {
    if value.write_snbt(sink).is_err() {
        Err(sink.error.clone().unwrap_or(RenderError::FormattingFailed))
    } else {
        Ok(())
    }
}

fn render_modifier(
    program: &MinecraftProgram,
    modifier: &ExecuteModifierKind,
    sink: &mut CommandSink,
) -> Result<(), RenderError> {
    match modifier {
        ExecuteModifierKind::As(selector) => sink.write_arguments(format_args!("as {selector}")),
        ExecuteModifierKind::At(selector) => sink.write_arguments(format_args!("at {selector}")),
        ExecuteModifierKind::In(dimension) => sink.write_arguments(format_args!("in {dimension}")),
        ExecuteModifierKind::Positioned(position) => {
            sink.write_arguments(format_args!("positioned {position}"))
        }
        ExecuteModifierKind::Rotated(rotation) => {
            sink.write_arguments(format_args!("rotated {rotation}"))
        }
        ExecuteModifierKind::Anchored(anchor) => sink.write_arguments(format_args!(
            "anchored {}",
            match anchor {
                super::TargetAnchor::Feet => "feet",
                super::TargetAnchor::Eyes => "eyes",
            }
        )),
        ExecuteModifierKind::Align(axes) => {
            sink.write_arguments(format_args!("align {}", axes.as_str()))
        }
        ExecuteModifierKind::If(condition) => {
            sink.push_checked("if ")?;
            render_condition(program, condition, sink)
        }
        ExecuteModifierKind::Unless(condition) => {
            sink.push_checked("unless ")?;
            render_condition(program, condition, sink)
        }
        ExecuteModifierKind::Store(channel, destination) => {
            sink.write_arguments(format_args!("store {} ", store_channel_token(*channel)))?;
            render_store_destination(destination, sink)
        }
    }
}

fn render_condition(
    program: &MinecraftProgram,
    condition: &Condition,
    sink: &mut CommandSink,
) -> Result<(), RenderError> {
    match condition {
        Condition::ScoreMatches(score, range) => {
            sink.push_checked("score ")?;
            render_score_ref(score, sink)?;
            sink.write_arguments(format_args!(" matches {range}"))
        }
        Condition::ScoreCompare(left, comparison, right) => {
            sink.push_checked("score ")?;
            render_score_ref(left, sink)?;
            sink.write_arguments(format_args!(" {} ", score_comparison_token(*comparison)))?;
            render_score_ref(right, sink)
        }
        Condition::DataExists(path) => {
            sink.push_checked("data ")?;
            render_storage_path(path, sink)
        }
        Condition::DataMatches(storage, pattern) => {
            sink.write_arguments(format_args!("data storage {storage} "))?;
            render_nbt(pattern, sink)
        }
        Condition::EntityExists(selector) => {
            sink.write_arguments(format_args!("entity {selector}"))
        }
        Condition::Function(function) => {
            sink.push_checked("function ")?;
            render_internal_callable(program, InternalCallableRef::Function(*function), sink)
        }
    }
}

const fn score_comparison_token(comparison: ScoreComparison) -> &'static str {
    match comparison {
        ScoreComparison::Equal => "=",
        ScoreComparison::LessThan => "<",
        ScoreComparison::LessOrEqual => "<=",
        ScoreComparison::GreaterThan => ">",
        ScoreComparison::GreaterOrEqual => ">=",
    }
}

const fn store_channel_token(channel: StoreChannel) -> &'static str {
    match channel {
        StoreChannel::Result => "result",
        StoreChannel::Success => "success",
    }
}

fn render_store_destination(
    destination: &StoreDestination,
    sink: &mut CommandSink,
) -> Result<(), RenderError> {
    match destination {
        StoreDestination::Score(score) => {
            sink.push_checked("score ")?;
            render_score_ref(score, sink)
        }
        StoreDestination::Storage {
            target,
            numeric_type,
            scale,
        } => sink.write_arguments(format_args!(
            "storage {} {} {} {scale}",
            target.storage(),
            target.path(),
            storage_numeric_token(*numeric_type)
        )),
    }
}

const fn storage_numeric_token(numeric_type: StorageNumericType) -> &'static str {
    match numeric_type {
        StorageNumericType::Byte => "byte",
        StorageNumericType::Short => "short",
        StorageNumericType::Int => "int",
        StorageNumericType::Long => "long",
        StorageNumericType::Float => "float",
        StorageNumericType::Double => "double",
    }
}

fn render_callable(
    program: &MinecraftProgram,
    target: &CallableRef,
    sink: &mut CommandSink,
) -> Result<(), RenderError> {
    match target {
        CallableRef::Internal(target) => render_internal_callable(program, *target, sink),
        CallableRef::External(ExternalCallableRef::Function(resource)) => {
            sink.write_arguments(format_args!("{resource}"))
        }
        CallableRef::External(ExternalCallableRef::Tag(resource)) => {
            sink.write_arguments(format_args!("#{resource}"))
        }
    }
}

fn render_internal_callable(
    program: &MinecraftProgram,
    target: InternalCallableRef,
    sink: &mut CommandSink,
) -> Result<(), RenderError> {
    match target {
        InternalCallableRef::Function(function) => {
            let resource = program
                .function(function)
                .ok_or(RenderError::InvalidInternalFunction(function))?
                .resource();
            sink.write_arguments(format_args!("{resource}"))
        }
        InternalCallableRef::Tag(tag) => {
            let resource = program
                .function_tag(tag)
                .ok_or(RenderError::InvalidInternalTag(tag))?
                .resource();
            sink.write_arguments(format_args!("#{resource}"))
        }
    }
}

fn render_macro(command: &MacroCommand, sink: &mut CommandSink) -> Result<(), RenderError> {
    // A macro command node spans several physical lines. The caller
    // (`render_function`) terminates the command node with one trailing newline, so
    // this only inserts separators *between* the macro's own lines — a trailing
    // `finish_command` here would emit a spurious blank line.
    for (index, line) in command.lines.iter().enumerate() {
        if index > 0 {
            sink.finish_command();
        }
        if line.has_variables() {
            sink.push_checked("$")?;
        }
        for segment in &line.segments {
            match segment {
                MacroSegment::Literal(text) => sink.push_checked(text)?,
                MacroSegment::Variable(id) => {
                    let variable = command
                        .arguments
                        .get(*id)
                        .ok_or(RenderError::FormattingFailed)?;
                    sink.push_checked("$(")?;
                    sink.push_checked(&variable.key)?;
                    sink.push_checked(")")?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CommandSink, RenderError, render_function};
    use crate::ir::minecraft::{
        CallableRef, CommandKind, CommandNode, Condition, DataCommand, DataModifyMode, DataSource,
        ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers,
        ExternalCallableRef, FakeScoreHolder, FiniteF64, FunctionCall, FunctionResourceId,
        FunctionWithStorage, JavaDecimal, MacroArguments, MacroCommand, MacroLine, MacroSegment,
        MacroVariable, MacroVariableId, MinecraftProgramBuilder, NbtKey, NbtPath, NbtPathKey,
        NbtPathSegment, NbtValue, NonNegativeI32, ObjectiveName, ReturnCommand, SayCommand,
        SayMessage, ScoreCommand, ScoreComparison, ScoreHolders, ScoreOperation, ScoreRange,
        ScoreRef, ScoreSelection, SingleScoreHolder, StorageId, StorageNumericType, StoragePath,
        StoreChannel, StoreDestination, SyntaxSlot, TargetAnchor, TargetAxes, TargetLocalPosition,
        TargetPosition, TargetRotation, TargetRotationAxis, TargetWorldAxis, TargetWorldPosition,
        TeleportCommand, UnboundedSelector,
    };
    use crate::source::OriginId;
    use crate::target::JavaEditionTarget;

    fn score(holder: &str) -> ScoreRef {
        ScoreRef::new(
            SingleScoreHolder::from(FakeScoreHolder::new(holder).unwrap()),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
    }

    fn selection(holder: &str) -> ScoreSelection {
        ScoreSelection::new(
            ScoreHolders::from(FakeScoreHolder::new(holder).unwrap()),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
    }

    fn storage(key: &str) -> StoragePath {
        StoragePath::new(
            StorageId::parse("mdl:state").unwrap(),
            NbtPath::new(NbtPathSegment::Key(NbtPathKey::new(key).unwrap()), vec![]),
        )
    }

    fn node(kind: CommandKind) -> CommandNode {
        CommandNode::new(kind, OriginId::UNKNOWN).unwrap()
    }

    fn render(commands: Vec<CommandNode>) -> String {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:test").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        for command in commands {
            body.push(command).unwrap();
        }
        body.finish();
        let program = builder.finish().unwrap();
        let bytes = render_function(&program, program.function(function).unwrap()).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn command_sink_counts_utf16_and_excludes_lf() {
        let mut sink = CommandSink::new(4);
        sink.push_checked("a😀b").unwrap();
        sink.finish_command();
        sink.push_checked("four").unwrap();
        sink.finish_command();
        assert_eq!(sink.into_bytes(), b"a\xF0\x9F\x98\x80b\nfour\n");

        let mut over = CommandSink::new(3);
        assert_eq!(
            over.push_checked("a😀b"),
            Err(RenderError::CommandTooLong { units: 4, max: 3 })
        );
    }

    #[test]
    fn scoreboard_and_data_families_render_exactly() {
        let compound = NbtValue::compound(vec![(NbtKey::new("x"), NbtValue::int(1))]).unwrap();
        let commands = vec![
            node(CommandKind::Score(ScoreCommand::ObjectiveAddDummy {
                objective: ObjectiveName::new("mdl.reg").unwrap(),
            })),
            node(CommandKind::Score(ScoreCommand::PlayersSet {
                target: selection("#a"),
                value: -2,
            })),
            node(CommandKind::Score(ScoreCommand::PlayersAdd {
                target: selection("#a"),
                amount: NonNegativeI32::new(3).unwrap(),
            })),
            node(CommandKind::Score(ScoreCommand::PlayersOperation {
                target: selection("#a"),
                op: ScoreOperation::Swap,
                source: ScoreSelection::new(
                    ScoreHolders::from(crate::ir::minecraft::Selector::from(
                        UnboundedSelector::AllPlayers,
                    )),
                    ObjectiveName::new("mdl.reg").unwrap(),
                ),
            })),
            node(CommandKind::Data(DataCommand::Get {
                source: storage("x"),
                scale: Some(FiniteF64::new(2.5).unwrap()),
            })),
            node(CommandKind::Data(DataCommand::Modify {
                target: storage("value"),
                mode: DataModifyMode::Set,
                source: DataSource::Value(compound),
            })),
        ];
        assert_eq!(
            render(commands),
            concat!(
                "scoreboard objectives add mdl.reg dummy\n",
                "scoreboard players set #a mdl.reg -2\n",
                "scoreboard players add #a mdl.reg 3\n",
                "scoreboard players operation #a mdl.reg >< @a mdl.reg\n",
                "data get storage mdl:state \"x\" 2.5\n",
                "data modify storage mdl:state \"value\" set value {\"x\":1}\n"
            )
        );
    }

    #[test]
    fn structured_say_renders_literal_text_without_raw_command_ir() {
        let say = node(CommandKind::Say(SayCommand::new(
            SayMessage::new("hello  π 😀 \\\"quoted\\\"").unwrap(),
        )));

        assert_eq!(render(vec![say]), "say hello  π 😀 \\\"quoted\\\"\n");
    }

    #[test]
    fn structured_spatial_commands_render_canonical_atoms() {
        let decimal = |value| JavaDecimal::new(value).unwrap();
        let destination = TargetPosition::World(TargetWorldPosition {
            x: TargetWorldAxis::Relative(decimal("10.500")),
            y: TargetWorldAxis::Relative(decimal("0")),
            z: TargetWorldAxis::Absolute(decimal("-2")),
        });
        let teleport = node(CommandKind::Teleport(TeleportCommand::current_executor(
            destination,
        )));
        let execute = node(CommandKind::Execute(ExecuteCommand::new(
            ExecuteModifiers::new(
                ExecuteModifier::new(
                    ExecuteModifierKind::Positioned(TargetPosition::Local(TargetLocalPosition {
                        left: decimal("0"),
                        up: decimal("1"),
                        forward: decimal("2"),
                    })),
                    OriginId::UNKNOWN,
                ),
                vec![
                    ExecuteModifier::new(
                        ExecuteModifierKind::Rotated(TargetRotation {
                            yaw: TargetRotationAxis::Relative(decimal("90")),
                            pitch: TargetRotationAxis::Relative(decimal("0")),
                        }),
                        OriginId::UNKNOWN,
                    ),
                    ExecuteModifier::new(
                        ExecuteModifierKind::Anchored(TargetAnchor::Eyes),
                        OriginId::UNKNOWN,
                    ),
                    ExecuteModifier::new(
                        ExecuteModifierKind::Align(TargetAxes::new(5).unwrap()),
                        OriginId::UNKNOWN,
                    ),
                ],
            ),
            teleport,
        )));
        assert_eq!(
            render(vec![execute]),
            "execute positioned ^ ^1 ^2 rotated ~90 ~ anchored eyes align xz run teleport @s ~10.5 ~ -2\n"
        );
    }

    #[test]
    fn execute_store_conditions_calls_and_returns_render_exactly() {
        let nested = node(CommandKind::Function(FunctionCall::new(CallableRef::from(
            ExternalCallableRef::Function(FunctionResourceId::parse("other:run").unwrap()),
        ))));
        let modifiers = ExecuteModifiers::new(
            ExecuteModifier::new(
                ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
                OriginId::UNKNOWN,
            ),
            vec![
                ExecuteModifier::new(
                    ExecuteModifierKind::If(Condition::ScoreMatches(
                        score("#a"),
                        ScoreRange::between(1, 3).unwrap(),
                    )),
                    OriginId::UNKNOWN,
                ),
                ExecuteModifier::new(
                    ExecuteModifierKind::Unless(Condition::ScoreCompare(
                        score("#a"),
                        ScoreComparison::GreaterOrEqual,
                        score("#b"),
                    )),
                    OriginId::UNKNOWN,
                ),
                ExecuteModifier::new(
                    ExecuteModifierKind::Store(
                        StoreChannel::Success,
                        StoreDestination::Storage {
                            target: storage("ok"),
                            numeric_type: StorageNumericType::Byte,
                            scale: FiniteF64::new(1.0).unwrap(),
                        },
                    ),
                    OriginId::UNKNOWN,
                ),
            ],
        );
        let execute = node(CommandKind::Execute(ExecuteCommand::new(modifiers, nested)));
        let return_run = node(CommandKind::Return(ReturnCommand::run(node(
            CommandKind::Score(ScoreCommand::PlayersGet { score: score("#a") }),
        ))));
        assert_eq!(
            render(vec![execute, return_run]),
            concat!(
                "execute as @e if score #a mdl.reg matches 1..3 unless score #a mdl.reg >= #b mdl.reg store success storage mdl:state \"ok\" byte 1 run function other:run\n",
                "return run scoreboard players get #a mdl.reg\n"
            )
        );
    }

    #[test]
    fn macro_and_function_with_storage_render_exactly() {
        // A macro helper body: one $-prefixed line substituting an NBT index, and a
        // plain line that needs no substitution (so it is not $-prefixed).
        let arguments = MacroArguments::new(vec![MacroVariable {
            key: "index".to_owned(),
            slot: SyntaxSlot::NbtIndex,
            source: storage("index"),
        }])
        .unwrap();
        let macro_command = MacroCommand::new(
            vec![
                MacroLine {
                    segments: vec![MacroSegment::Literal("say plain line".to_owned())],
                },
                MacroLine {
                    segments: vec![
                        MacroSegment::Literal("say page ".to_owned()),
                        MacroSegment::Variable(MacroVariableId(0)),
                    ],
                },
            ],
            arguments,
            OriginId::UNKNOWN,
        )
        .unwrap();

        // The calling convention: `function <id> with storage <storage> <path>`.
        let call = node(CommandKind::FunctionWithStorage(FunctionWithStorage::new(
            CallableRef::from(ExternalCallableRef::Function(
                FunctionResourceId::parse("mdl:helper").unwrap(),
            )),
            storage("args"),
        )));

        assert_eq!(
            render(vec![node(CommandKind::Macro(macro_command)), call]),
            concat!(
                "say plain line\n",
                "$say page $(index)\n",
                "function mdl:helper with storage mdl:state \"args\"\n",
            )
        );
    }

    #[test]
    fn internal_function_condition_resolves_a_forward_reference_exactly() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let caller = builder
            .declare_function(
                FunctionResourceId::parse("mdl:caller").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let target = builder
            .declare_function(
                FunctionResourceId::parse("mdl:target").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        {
            let condition = ExecuteModifier::new(
                ExecuteModifierKind::If(Condition::Function(target)),
                OriginId::UNKNOWN,
            );
            let execute = node(CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(condition, vec![]),
                node(CommandKind::Return(ReturnCommand::Value(7))),
            )));
            let mut body = builder.begin_function(caller).unwrap();
            body.push(execute).unwrap();
            body.finish();
        }
        {
            let mut body = builder.begin_function(target).unwrap();
            body.push(node(CommandKind::Return(ReturnCommand::Value(1))))
                .unwrap();
            body.finish();
        }

        let program = builder.finish().unwrap();
        let bytes = render_function(&program, program.function(caller).unwrap()).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "execute if function mdl:target run return 7\n"
        );
    }
}
