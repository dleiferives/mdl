use std::fmt;

use super::{
    CommandKind, CommandNode, Condition, DataCommand, DataSource, ExecuteModifierKind,
    FunctionTagEntry, FunctionTagEntryKind, InternalCallableRef, MinecraftProgram, ReturnCommand,
    ScoreCommand,
};

/// Deterministic diagnostics-only text for valid or malformed Minecraft IR.
#[derive(Clone, Copy, Debug, Default)]
pub struct MinecraftDebugDumper;

impl MinecraftDebugDumper {
    /// Dumps a complete target program without invoking target serialization.
    #[must_use]
    pub fn program(program: &MinecraftProgram) -> String {
        let mut output = String::new();
        if write_program(program, &mut output).is_err() {
            return String::from("<infallible String formatting failed>");
        }
        output
    }

    /// Dumps one command tree without resolving IDs or origins.
    #[must_use]
    pub fn command(command: &CommandNode) -> String {
        let mut output = String::new();
        if write_command(command, 0, &mut output).is_err() {
            return String::from("<infallible String formatting failed>");
        }
        output
    }
}

fn write_program(program: &MinecraftProgram, output: &mut impl fmt::Write) -> fmt::Result {
    writeln!(output, "minecraft.program target={:?}", program.target())?;
    for (function, data) in program.functions() {
        writeln!(
            output,
            "function {function:?} resource={} origin={:?}",
            data.resource(),
            data.origin()
        )?;
        for (command, node) in data.body().commands() {
            writeln!(output, "  command {command:?}")?;
            write_command(node, 2, output)?;
        }
    }
    for (tag, data) in program.function_tags() {
        writeln!(
            output,
            "tag {tag:?} resource={} origin={:?} merge={:?}",
            data.resource(),
            data.origin(),
            data.merge()
        )?;
        for (index, entry) in data.entries().iter().enumerate() {
            write_indent(output, 1)?;
            write!(output, "entry {index} origin={:?} ", entry.origin())?;
            write_tag_entry(entry, output)?;
            output.write_char('\n')?;
        }
    }
    Ok(())
}

fn write_command(
    command: &CommandNode,
    indentation: usize,
    output: &mut impl fmt::Write,
) -> fmt::Result {
    write_indent(output, indentation)?;
    write!(output, "origin={:?} ", command.origin())?;
    match command.kind() {
        CommandKind::Score(score) => write_score(score, output)?,
        CommandKind::Data(data) => write_data(data, output)?,
        CommandKind::Say(command) => {
            write!(output, "say message={:?}", command.message().as_str())?;
        }
        CommandKind::Teleport(command) => {
            write!(
                output,
                "teleport.self destination={:?}",
                command.destination()
            )?;
        }
        CommandKind::Execute(execute) => {
            writeln!(output, "execute")?;
            for (index, modifier) in execute.modifiers().as_slice().iter().enumerate() {
                write_indent(output, indentation + 1)?;
                write!(output, "modifier {index} origin={:?} ", modifier.origin())?;
                write_modifier(modifier.kind(), output)?;
                output.write_char('\n')?;
            }
            write_indent(output, indentation + 1)?;
            writeln!(output, "run")?;
            write_command(execute.run(), indentation + 2, output)?;
            return Ok(());
        }
        CommandKind::Function(call) => write!(output, "function target={:?}", call.target())?,
        CommandKind::Return(ReturnCommand::Value(value)) => write!(output, "return.value {value}")?,
        CommandKind::Return(ReturnCommand::Fail) => output.write_str("return.fail")?,
        CommandKind::Return(ReturnCommand::Run(nested)) => {
            writeln!(output, "return.run")?;
            write_command(nested, indentation + 1, output)?;
            return Ok(());
        }
        CommandKind::Raw(raw) => write!(output, "raw.unknown {:?}", raw.as_str())?,
    }
    output.write_char('\n')
}

fn write_score(score: &ScoreCommand, output: &mut impl fmt::Write) -> fmt::Result {
    match score {
        ScoreCommand::ObjectiveAddDummy { objective } => {
            write!(output, "score.objective_add_dummy objective={objective}")
        }
        ScoreCommand::PlayersSet { target, value } => {
            write!(output, "score.players_set target={target:?} value={value}")
        }
        ScoreCommand::PlayersAdd { target, amount } => {
            write!(
                output,
                "score.players_add target={target:?} amount={amount}"
            )
        }
        ScoreCommand::PlayersRemove { target, amount } => {
            write!(
                output,
                "score.players_remove target={target:?} amount={amount}"
            )
        }
        ScoreCommand::PlayersGet { score } => {
            write!(output, "score.players_get score={score:?}")
        }
        ScoreCommand::PlayersReset { target } => {
            write!(output, "score.players_reset target={target:?}")
        }
        ScoreCommand::PlayersOperation { target, op, source } => write!(
            output,
            "score.players_operation target={target:?} op={op:?} source={source:?}"
        ),
    }
}

fn write_data(data: &DataCommand, output: &mut impl fmt::Write) -> fmt::Result {
    match data {
        DataCommand::Get { source, scale } => {
            write!(output, "data.get source={source:?} scale={scale:?}")
        }
        DataCommand::Remove { target } => write!(output, "data.remove target={target:?}"),
        DataCommand::Modify {
            target,
            mode,
            source: DataSource::Value(value),
        } => write!(
            output,
            "data.modify target={target:?} mode={mode:?} value={value:?}"
        ),
        DataCommand::Modify {
            target,
            mode,
            source: DataSource::From(source),
        } => write!(
            output,
            "data.modify target={target:?} mode={mode:?} from={source:?}"
        ),
        DataCommand::Modify {
            target,
            mode,
            source: DataSource::StringSlice { source, start, end },
        } => write!(
            output,
            "data.modify target={target:?} mode={mode:?} string={source:?} start={start} end={end:?}"
        ),
        DataCommand::Modify {
            target,
            mode,
            source: DataSource::Entity { selector, path },
        } => write!(
            output,
            "data.modify target={target:?} mode={mode:?} entity={selector:?} path={path:?}"
        ),
    }
}

fn write_modifier(modifier: &ExecuteModifierKind, output: &mut impl fmt::Write) -> fmt::Result {
    match modifier {
        ExecuteModifierKind::As(selector) => write!(output, "as {selector:?}"),
        ExecuteModifierKind::At(selector) => write!(output, "at {selector:?}"),
        ExecuteModifierKind::In(dimension) => write!(output, "in {dimension}"),
        ExecuteModifierKind::Positioned(position) => write!(output, "positioned {position:?}"),
        ExecuteModifierKind::Rotated(rotation) => write!(output, "rotated {rotation:?}"),
        ExecuteModifierKind::Anchored(anchor) => write!(output, "anchored {anchor:?}"),
        ExecuteModifierKind::Align(axes) => write!(output, "align {}", axes.as_str()),
        ExecuteModifierKind::If(condition) => {
            output.write_str("if ")?;
            write_condition(condition, output)
        }
        ExecuteModifierKind::Unless(condition) => {
            output.write_str("unless ")?;
            write_condition(condition, output)
        }
        ExecuteModifierKind::Store(channel, destination) => {
            write!(output, "store {channel:?} {destination:?}")
        }
    }
}

fn write_condition(condition: &Condition, output: &mut impl fmt::Write) -> fmt::Result {
    match condition {
        Condition::ScoreMatches(_, _)
        | Condition::ScoreCompare(_, _, _)
        | Condition::DataExists(_)
        | Condition::DataMatches(_, _)
        | Condition::EntityExists(_)
        | Condition::Function(_) => write!(output, "{condition:?}"),
    }
}

fn write_tag_entry(entry: &FunctionTagEntry, output: &mut impl fmt::Write) -> fmt::Result {
    match entry.kind() {
        FunctionTagEntryKind::Internal(target) => {
            output.write_str("internal ")?;
            write_internal_callable(*target, output)
        }
        FunctionTagEntryKind::External {
            target,
            requirement,
        } => write!(
            output,
            "external target={target:?} requirement={requirement:?}"
        ),
    }
}

fn write_internal_callable(
    target: InternalCallableRef,
    output: &mut impl fmt::Write,
) -> fmt::Result {
    match target {
        InternalCallableRef::Function(function) => write!(output, "function={function:?}"),
        InternalCallableRef::Tag(tag) => write!(output, "tag={tag:?}"),
    }
}

fn write_indent(output: &mut impl fmt::Write, indentation: usize) -> fmt::Result {
    for _ in 0..indentation {
        output.write_str("  ")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::MinecraftDebugDumper;
    use crate::entity::EntityId;
    use crate::ir::minecraft::{
        CallableRef, CommandKind, CommandNode, Condition, ExecuteCommand, ExecuteModifier,
        ExecuteModifierKind, ExecuteModifiers, FunctionCall, InternalCallableRef, McFunctionId,
        ReturnCommand, SayCommand, SayMessage,
    };
    use crate::source::OriginId;

    #[test]
    fn malformed_ids_and_origins_dump_without_lookup_or_panic() {
        let invalid_function = <McFunctionId as EntityId>::from_index(900);
        let invalid_origin = <OriginId as EntityId>::from_index(901);
        let command = CommandNode::new(
            CommandKind::Function(FunctionCall::new(CallableRef::Internal(
                InternalCallableRef::Function(invalid_function),
            ))),
            invalid_origin,
        )
        .unwrap();

        let dump = MinecraftDebugDumper::command(&command);
        assert!(dump.contains("OriginId(901)"));
        assert!(dump.contains("McFunctionId(900)"));
    }

    #[test]
    fn raw_commands_are_visibly_unknown_barriers() {
        let command = CommandNode::new(
            CommandKind::Raw(crate::ir::minecraft::UnsafeRawCommand::new("say marker").unwrap()),
            OriginId::UNKNOWN,
        )
        .unwrap();
        assert_eq!(
            MinecraftDebugDumper::command(&command),
            "origin=OriginId(0) raw.unknown \"say marker\"\n"
        );
    }

    #[test]
    fn structured_say_is_visibly_typed() {
        let command = CommandNode::new(
            CommandKind::Say(SayCommand::new(SayMessage::new("hello").unwrap())),
            OriginId::UNKNOWN,
        )
        .unwrap();

        assert_eq!(
            MinecraftDebugDumper::command(&command),
            "origin=OriginId(0) say message=\"hello\"\n"
        );
    }

    #[test]
    fn internal_function_condition_dumps_typed_identity_exactly() {
        let function = <McFunctionId as EntityId>::from_index(900);
        let modifier = ExecuteModifier::new(
            ExecuteModifierKind::If(Condition::Function(function)),
            OriginId::UNKNOWN,
        );
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(modifier, vec![]),
                CommandNode::new(
                    CommandKind::Return(ReturnCommand::Value(1)),
                    OriginId::UNKNOWN,
                )
                .unwrap(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();

        assert_eq!(
            MinecraftDebugDumper::command(&command),
            concat!(
                "origin=OriginId(0) execute\n",
                "  modifier 0 origin=OriginId(0) if Function(McFunctionId(900))\n",
                "  run\n",
                "    origin=OriginId(0) return.value 1\n",
            )
        );
    }

    #[test]
    fn principal_node_sizes_stay_below_recorded_initial_guardrails() {
        assert!(size_of::<CommandNode>() <= 160);
        assert!(size_of::<crate::ir::minecraft::ExecuteModifier>() <= 160);
        assert!(size_of::<crate::ir::minecraft::NbtValue>() <= 48);
        assert_eq!(size_of::<McFunctionId>(), 4);
    }
}
