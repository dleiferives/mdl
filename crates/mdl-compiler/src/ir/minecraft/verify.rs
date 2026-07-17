use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::EntityId;
use crate::source::{OriginId, SourceContext};

use super::{
    CallableRef, CommandKind, CommandNode, Condition, DataCommand, DataSource, ExecuteModifierKind,
    ExternalCallableRef, FunctionTagEntryKind, FunctionTagId, FunctionTagMerge,
    FunctionTagResourceId, InternalCallableRef, MAX_COMMAND_DEPTH, MAX_NBT_DEPTH, McFunctionId,
    MinecraftProgram, NbtValue, NbtValueRef, PackPath, ReturnCommand, SayMessage, UnsafeRawCommand,
};

#[derive(Default)]
struct Verifier {
    findings: Vec<Diagnostic>,
}

impl Verifier {
    fn report(&mut self, code: &'static str, message: impl Into<String>, origin: OriginId) {
        self.findings.push(Diagnostic::new(code, message, origin));
    }

    fn finish(self) -> Result<(), Diagnostics> {
        match Diagnostics::from_findings(self.findings) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(()),
        }
    }
}

/// Verifies a complete Minecraft target program in deterministic structural layers.
///
/// Function recursion, heterogeneous NBT lists, and empty function bodies are legal.
///
/// # Errors
///
/// Returns every safely independent structural finding in stable layer order.
pub fn verify_program(
    program: &MinecraftProgram,
    sources: &SourceContext,
) -> Result<(), Diagnostics> {
    let mut verifier = Verifier::default();

    verify_local_shapes(program, &mut verifier);
    let symbols = verify_resources_and_paths(program, &mut verifier);
    verify_references(program, &symbols, &mut verifier);
    verify_tags(program, &mut verifier);
    verify_origins(program, sources, &mut verifier);

    verifier.finish()
}

struct Symbols {
    functions: HashMap<super::FunctionResourceId, McFunctionId>,
    tags: HashMap<FunctionTagResourceId, FunctionTagId>,
}

fn verify_local_shapes(program: &MinecraftProgram, verifier: &mut Verifier) {
    for (function, data) in program.functions() {
        for (command, node) in data.body().commands() {
            verify_command_shape(
                node,
                &format!("function {function:?} command {command:?}"),
                program.target(),
                verifier,
            );
        }
    }
}

fn verify_command_shape(
    command: &CommandNode,
    location: &str,
    target: crate::target::JavaEditionTarget,
    verifier: &mut Verifier,
) {
    let depth = command.depth();
    if depth > MAX_COMMAND_DEPTH {
        verifier.report(
            "minecraft.command-depth",
            format!("{location} has depth {depth}; maximum is {MAX_COMMAND_DEPTH}"),
            command.origin(),
        );
    }

    match command.kind() {
        CommandKind::Data(data) => verify_data_shape(data, command.origin(), verifier),
        CommandKind::Execute(execute) => {
            if execute.modifiers().is_empty() {
                verifier.report(
                    "minecraft.empty-execute",
                    format!("{location} has no execute modifiers"),
                    command.origin(),
                );
            }
            verify_command_shape(execute.run(), "nested execute command", target, verifier);
        }
        CommandKind::Return(ReturnCommand::Run(nested)) => {
            verify_command_shape(nested, "nested return-run command", target, verifier);
        }
        CommandKind::Score(_)
        | CommandKind::Teleport(_)
        | CommandKind::Function(_)
        | CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail) => {}
        CommandKind::Say(say) => {
            if let Err(error) = SayMessage::new_for_target(say.message().as_str(), target) {
                verifier.report(
                    "minecraft.invalid-say-message",
                    format!("{location} contains invalid say message: {error}"),
                    command.origin(),
                );
            }
        }
        CommandKind::Raw(raw) => {
            if let Err(error) = UnsafeRawCommand::new_for_target(raw.as_str(), target) {
                verifier.report(
                    "minecraft.invalid-raw-line",
                    format!("{location} contains invalid raw command: {error}"),
                    command.origin(),
                );
            }
        }
    }
}

fn verify_data_shape(data: &DataCommand, origin: OriginId, verifier: &mut Verifier) {
    if let DataCommand::Modify {
        source: DataSource::Value(value),
        ..
    } = data
    {
        verify_nbt(value, origin, verifier);
    }
}

fn verify_nbt(value: &NbtValue, origin: OriginId, verifier: &mut Verifier) {
    let depth = value.depth();
    if depth > MAX_NBT_DEPTH {
        verifier.report(
            "minecraft.nbt-depth",
            format!("NBT depth {depth} exceeds compiler limit {MAX_NBT_DEPTH}"),
            origin,
        );
    }
    match value.as_ref() {
        NbtValueRef::List(values) => {
            for child in values {
                verify_nbt(child, origin, verifier);
            }
        }
        NbtValueRef::Compound(entries) => {
            let mut previous = None;
            for (key, child) in entries {
                if let Some(previous_key) = previous {
                    if previous_key == key {
                        verifier.report(
                            "minecraft.duplicate-nbt-key",
                            format!("duplicate NBT key {:?}", key.as_str()),
                            origin,
                        );
                    } else if previous_key > key {
                        verifier.report(
                            "minecraft.noncanonical-nbt-order",
                            format!("NBT key {:?} is out of canonical order", key.as_str()),
                            origin,
                        );
                    }
                }
                previous = Some(key);
                verify_nbt(child, origin, verifier);
            }
        }
        NbtValueRef::Byte(_)
        | NbtValueRef::Short(_)
        | NbtValueRef::Int(_)
        | NbtValueRef::Long(_)
        | NbtValueRef::Float(_)
        | NbtValueRef::Double(_)
        | NbtValueRef::String(_) => {}
    }
}

fn verify_resources_and_paths(program: &MinecraftProgram, verifier: &mut Verifier) -> Symbols {
    let mut functions = HashMap::new();
    let mut tags = HashMap::new();
    let mut paths: HashMap<PackPath, (&'static str, OriginId)> = HashMap::new();

    for (function, data) in program.functions() {
        if let Some(previous) = functions.insert(data.resource().clone(), function) {
            verifier.report(
                "minecraft.duplicate-function-resource",
                format!(
                    "function {function:?} duplicates resource {} first owned by {previous:?}",
                    data.resource()
                ),
                data.origin(),
            );
        }
        insert_path(
            data.resource().pack_path(program.target()),
            "function",
            data.origin(),
            &mut paths,
            verifier,
        );
    }
    for (tag, data) in program.function_tags() {
        if let Some(previous) = tags.insert(data.resource().clone(), tag) {
            verifier.report(
                "minecraft.duplicate-tag-resource",
                format!(
                    "function tag {tag:?} duplicates resource {} first owned by {previous:?}",
                    data.resource()
                ),
                data.origin(),
            );
        }
        insert_path(
            data.resource().pack_path(program.target()),
            "function tag",
            data.origin(),
            &mut paths,
            verifier,
        );
    }

    Symbols { functions, tags }
}

fn insert_path(
    path: PackPath,
    kind: &'static str,
    origin: OriginId,
    paths: &mut HashMap<PackPath, (&'static str, OriginId)>,
    verifier: &mut Verifier,
) {
    if let Some((previous_kind, _)) = paths.get(&path) {
        verifier.report(
            "minecraft.artifact-path-collision",
            format!("{kind} path {path} collides with earlier {previous_kind}"),
            origin,
        );
    } else {
        paths.insert(path, (kind, origin));
    }
}

fn verify_references(program: &MinecraftProgram, symbols: &Symbols, verifier: &mut Verifier) {
    for (function, data) in program.functions() {
        for (command, node) in data.body().commands() {
            verify_command_references(
                node,
                program,
                symbols,
                &format!("function {function:?} command {command:?}"),
                verifier,
            );
        }
    }

    for (tag, data) in program.function_tags() {
        for (index, entry) in data.entries().iter().enumerate() {
            match entry.kind() {
                FunctionTagEntryKind::Internal(target) => verify_internal_callable(
                    *target,
                    program,
                    &format!("function tag {tag:?} entry {index}"),
                    entry.origin(),
                    verifier,
                ),
                FunctionTagEntryKind::External { target, .. } => verify_external_alias(
                    target,
                    symbols,
                    &format!("function tag {tag:?} entry {index}"),
                    entry.origin(),
                    verifier,
                ),
            }
        }
    }
}

fn verify_command_references(
    command: &CommandNode,
    program: &MinecraftProgram,
    symbols: &Symbols,
    location: &str,
    verifier: &mut Verifier,
) {
    match command.kind() {
        CommandKind::Function(call) => match call.target() {
            CallableRef::Internal(target) => {
                verify_internal_callable(*target, program, location, command.origin(), verifier);
            }
            CallableRef::External(target) => {
                verify_external_alias(target, symbols, location, command.origin(), verifier);
            }
        },
        CommandKind::Execute(execute) => {
            for (index, modifier) in execute.modifiers().as_slice().iter().enumerate() {
                match modifier.kind() {
                    ExecuteModifierKind::If(condition) | ExecuteModifierKind::Unless(condition) => {
                        verify_condition_references(
                            condition,
                            program,
                            &format!("{location} execute modifier {index}"),
                            modifier.origin(),
                            verifier,
                        );
                    }
                    ExecuteModifierKind::As(_)
                    | ExecuteModifierKind::At(_)
                    | ExecuteModifierKind::In(_)
                    | ExecuteModifierKind::Positioned(_)
                    | ExecuteModifierKind::Rotated(_)
                    | ExecuteModifierKind::Anchored(_)
                    | ExecuteModifierKind::Align(_)
                    | ExecuteModifierKind::Store(_, _) => {}
                }
            }
            verify_command_references(
                execute.run(),
                program,
                symbols,
                "nested execute command",
                verifier,
            );
        }
        CommandKind::Return(ReturnCommand::Run(nested)) => verify_command_references(
            nested,
            program,
            symbols,
            "nested return-run command",
            verifier,
        ),
        CommandKind::Score(_)
        | CommandKind::Data(_)
        | CommandKind::Say(_)
        | CommandKind::Teleport(_)
        | CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail)
        | CommandKind::Raw(_) => {}
    }
}

fn verify_condition_references(
    condition: &Condition,
    program: &MinecraftProgram,
    location: &str,
    origin: OriginId,
    verifier: &mut Verifier,
) {
    match condition {
        Condition::Function(function) => verify_internal_callable(
            InternalCallableRef::Function(*function),
            program,
            location,
            origin,
            verifier,
        ),
        Condition::ScoreMatches(_, _)
        | Condition::ScoreCompare(_, _, _)
        | Condition::DataExists(_)
        | Condition::EntityExists(_) => {}
    }
}

fn verify_internal_callable(
    target: InternalCallableRef,
    program: &MinecraftProgram,
    location: &str,
    origin: OriginId,
    verifier: &mut Verifier,
) {
    let exists = match target {
        InternalCallableRef::Function(function) => program.function(function).is_some(),
        InternalCallableRef::Tag(tag) => program.function_tag(tag).is_some(),
    };
    if !exists {
        verifier.report(
            "minecraft.invalid-internal-reference",
            format!("{location} refers to absent internal target {target:?}"),
            origin,
        );
    }
}

fn verify_external_alias(
    target: &ExternalCallableRef,
    symbols: &Symbols,
    location: &str,
    origin: OriginId,
    verifier: &mut Verifier,
) {
    let aliases = match target {
        ExternalCallableRef::Function(resource) => symbols.functions.contains_key(resource),
        ExternalCallableRef::Tag(resource) => symbols.tags.contains_key(resource),
    };
    if aliases {
        verifier.report(
            "minecraft.external-aliases-owned-resource",
            format!("{location} spells owned target {target:?} as external"),
            origin,
        );
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum DirectTagTarget {
    Internal(InternalCallableRef),
    External(ExternalCallableRef),
}

fn verify_tags(program: &MinecraftProgram, verifier: &mut Verifier) {
    let mut edges = vec![vec![]; program.function_tags().len()];

    for (tag, data) in program.function_tags() {
        if data.merge() == FunctionTagMerge::Replace && is_shared_function_tag(data.resource()) {
            verifier.report(
                "minecraft.replace-shared-function-tag",
                format!("shared function tag {} cannot use replace", data.resource()),
                data.origin(),
            );
        }

        let mut direct = HashSet::new();
        for entry in data.entries() {
            let target = match entry.kind() {
                FunctionTagEntryKind::Internal(target) => {
                    if let InternalCallableRef::Tag(target_tag) = target {
                        if program.function_tag(*target_tag).is_some() {
                            edges[id_index(tag)].push(*target_tag);
                        }
                    }
                    DirectTagTarget::Internal(*target)
                }
                FunctionTagEntryKind::External { target, .. } => {
                    DirectTagTarget::External(target.clone())
                }
            };
            if !direct.insert(target.clone()) {
                verifier.report(
                    "minecraft.duplicate-tag-entry",
                    format!("function tag {tag:?} contains duplicate direct target {target:?}"),
                    entry.origin(),
                );
            }
        }
    }

    verify_tag_cycles(program, &edges, verifier);
}

fn verify_tag_cycles(
    program: &MinecraftProgram,
    edges: &[Vec<FunctionTagId>],
    verifier: &mut Verifier,
) {
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    let mut colors = vec![WHITE; edges.len()];

    for (start, data) in program.function_tags() {
        if colors[id_index(start)] != WHITE {
            continue;
        }
        colors[id_index(start)] = GRAY;
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, next_edge)) = stack.last_mut() {
            let node_index = id_index(*node);
            if *next_edge == edges[node_index].len() {
                colors[node_index] = BLACK;
                stack.pop();
                continue;
            }
            let target = edges[node_index][*next_edge];
            *next_edge += 1;
            match colors[id_index(target)] {
                WHITE => {
                    colors[id_index(target)] = GRAY;
                    stack.push((target, 0));
                }
                GRAY => verifier.report(
                    "minecraft.function-tag-cycle",
                    format!("owned function-tag cycle includes {node:?} -> {target:?}"),
                    program
                        .function_tag(*node)
                        .map_or(data.origin(), super::FunctionTag::origin),
                ),
                BLACK => {}
                _ => unreachable!("tag color is internally constrained"),
            }
        }
    }
}

fn is_shared_function_tag(resource: &FunctionTagResourceId) -> bool {
    resource.namespace().as_str() == "minecraft"
        && matches!(resource.path().as_str(), "load" | "tick")
}

fn id_index<I: EntityId>(id: I) -> usize {
    usize::try_from(id.index()).unwrap_or(usize::MAX)
}

fn verify_origins(program: &MinecraftProgram, sources: &SourceContext, verifier: &mut Verifier) {
    for (function, data) in program.functions() {
        verify_origin(
            data.origin(),
            sources,
            &format!("function {function:?}"),
            verifier,
        );
        for (command, node) in data.body().commands() {
            verify_command_origins(
                node,
                sources,
                &format!("function {function:?} command {command:?}"),
                verifier,
            );
        }
    }
    for (tag, data) in program.function_tags() {
        verify_origin(
            data.origin(),
            sources,
            &format!("function tag {tag:?}"),
            verifier,
        );
        for (index, entry) in data.entries().iter().enumerate() {
            verify_origin(
                entry.origin(),
                sources,
                &format!("function tag {tag:?} entry {index}"),
                verifier,
            );
        }
    }
}

fn verify_command_origins(
    command: &CommandNode,
    sources: &SourceContext,
    location: &str,
    verifier: &mut Verifier,
) {
    verify_origin(command.origin(), sources, location, verifier);
    match command.kind() {
        CommandKind::Execute(execute) => {
            for (index, modifier) in execute.modifiers().as_slice().iter().enumerate() {
                verify_origin(
                    modifier.origin(),
                    sources,
                    &format!("{location} modifier {index}"),
                    verifier,
                );
            }
            verify_command_origins(execute.run(), sources, "nested execute command", verifier);
        }
        CommandKind::Return(ReturnCommand::Run(nested)) => {
            verify_command_origins(nested, sources, "nested return-run command", verifier);
        }
        CommandKind::Score(_)
        | CommandKind::Data(_)
        | CommandKind::Say(_)
        | CommandKind::Teleport(_)
        | CommandKind::Function(_)
        | CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail)
        | CommandKind::Raw(_) => {}
    }
}

fn verify_origin(
    origin: OriginId,
    sources: &SourceContext,
    location: &str,
    verifier: &mut Verifier,
) {
    if sources.origin(origin).is_none() {
        verifier.report(
            "minecraft.invalid-origin",
            format!("{location} has invalid origin {origin:?}"),
            OriginId::UNKNOWN,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::{EntityId, EntityVec};
    use crate::ir::minecraft::{
        CallableRef, CommandKind, CommandNode, Condition, ExecuteCommand, ExecuteModifier,
        ExecuteModifierKind, ExecuteModifiers, FunctionBody, FunctionCall, FunctionResourceId,
        FunctionTag, FunctionTagEntry, FunctionTagMerge, FunctionTagResourceId,
        InternalCallableRef, McFunction, McFunctionId, MinecraftProgram, MinecraftProgramBuilder,
        RawCommandError, ReturnCommand, SayCommand, SayMessage, UnsafeRawCommand,
    };
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    use super::verify_program;

    fn program_with(command: CommandNode) -> MinecraftProgram {
        let mut commands = EntityVec::new();
        commands.push(command).unwrap();
        let body = FunctionBody::from_commands(commands);
        let mut functions = EntityVec::new();
        functions
            .push(McFunction::new(
                FunctionResourceId::parse("mdl:test").unwrap(),
                OriginId::UNKNOWN,
                body,
            ))
            .unwrap();
        MinecraftProgram::new(JavaEditionTarget::V26_2, functions, EntityVec::new())
    }

    #[test]
    fn valid_empty_and_recursive_programs_verify() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:recursive").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Function(FunctionCall::new(CallableRef::Internal(
                    InternalCallableRef::Function(function),
                ))),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        assert!(verify_program(&program, &SourceContext::new()).is_ok());
    }

    #[test]
    fn malformed_local_shapes_accumulate_without_rendering_or_panic() {
        let invalid_origin = <OriginId as EntityId>::from_index(90);
        let raw = UnsafeRawCommand::from_unchecked("# invalid");
        assert_eq!(
            UnsafeRawCommand::new(raw.as_str()),
            Err(RawCommandError::ReservedPrefix('#'))
        );
        let nested = CommandNode::from_unchecked(CommandKind::Raw(raw), invalid_origin);
        let execute = ExecuteCommand::new(ExecuteModifiers::from_unchecked(vec![]), nested);
        let command = CommandNode::from_unchecked(CommandKind::Execute(execute), invalid_origin);
        let program = program_with(command);

        let diagnostics = verify_program(&program, &SourceContext::new()).unwrap_err();
        assert!(diagnostics.contains_code("minecraft.empty-execute"));
        assert!(diagnostics.contains_code("minecraft.invalid-raw-line"));
        assert!(diagnostics.contains_code("minecraft.invalid-origin"));
    }

    #[test]
    fn malformed_say_message_is_reported_without_rendering() {
        let message = SayMessage::from_unchecked("hello @s");
        let command = CommandNode::from_unchecked(
            CommandKind::Say(SayCommand::new(message)),
            OriginId::UNKNOWN,
        );

        let diagnostics =
            verify_program(&program_with(command), &SourceContext::new()).unwrap_err();
        assert!(diagnostics.contains_code("minecraft.invalid-say-message"));
    }

    #[test]
    fn broken_internal_reference_is_reported_fallibly() {
        let invalid = <McFunctionId as EntityId>::from_index(99);
        let command = CommandNode::new(
            CommandKind::Function(FunctionCall::new(CallableRef::Internal(
                InternalCallableRef::Function(invalid),
            ))),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let diagnostics =
            verify_program(&program_with(command), &SourceContext::new()).unwrap_err();
        assert!(diagnostics.contains_code("minecraft.invalid-internal-reference"));
    }

    #[test]
    fn broken_internal_function_condition_is_reported_fallibly() {
        let invalid = <McFunctionId as EntityId>::from_index(99);
        let modifier = ExecuteModifier::new(
            ExecuteModifierKind::Unless(Condition::Function(invalid)),
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

        let diagnostics =
            verify_program(&program_with(command), &SourceContext::new()).unwrap_err();
        assert!(diagnostics.contains_code("minecraft.invalid-internal-reference"));
    }

    #[test]
    fn duplicate_entries_shared_replace_and_owned_cycles_are_rejected() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let load = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("minecraft:load").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Replace,
            )
            .unwrap();
        let other = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:other").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        {
            let mut entries = builder.begin_function_tag(load).unwrap();
            let target = InternalCallableRef::Tag(other);
            entries.push(FunctionTagEntry::internal(target, OriginId::UNKNOWN));
            entries.push(FunctionTagEntry::internal(target, OriginId::UNKNOWN));
            entries.finish();
        }
        {
            let mut entries = builder.begin_function_tag(other).unwrap();
            entries.push(FunctionTagEntry::internal(
                InternalCallableRef::Tag(load),
                OriginId::UNKNOWN,
            ));
            entries.finish();
        }
        let program = builder.finish().unwrap();
        let diagnostics = verify_program(&program, &SourceContext::new()).unwrap_err();

        assert!(diagnostics.contains_code("minecraft.replace-shared-function-tag"));
        assert!(diagnostics.contains_code("minecraft.duplicate-tag-entry"));
        assert!(diagnostics.contains_code("minecraft.function-tag-cycle"));
    }

    #[test]
    fn same_kind_duplicate_resources_are_detected_in_malformed_programs() {
        let body = || FunctionBody::from_commands(EntityVec::new());
        let resource = FunctionResourceId::parse("mdl:duplicate").unwrap();
        let mut functions = EntityVec::new();
        functions
            .push(McFunction::new(resource.clone(), OriginId::UNKNOWN, body()))
            .unwrap();
        functions
            .push(McFunction::new(resource, OriginId::UNKNOWN, body()))
            .unwrap();
        let program = MinecraftProgram::new(
            JavaEditionTarget::V26_2,
            functions,
            EntityVec::<super::super::FunctionTagId, FunctionTag>::new(),
        );
        let diagnostics = verify_program(&program, &SourceContext::new()).unwrap_err();
        assert!(diagnostics.contains_code("minecraft.duplicate-function-resource"));
        assert!(diagnostics.contains_code("minecraft.artifact-path-collision"));
    }
}
