use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::{EntityLimitError, EntityVec};
use crate::source::OriginId;
use crate::target::JavaEditionTarget;

use super::{
    CommandId, CommandNode, FunctionBody, FunctionResourceId, FunctionTag, FunctionTagEntry,
    FunctionTagId, FunctionTagMerge, FunctionTagResourceId, McFunction, McFunctionId,
    MinecraftProgram,
};

#[derive(Debug)]
struct PendingFunction {
    resource: FunctionResourceId,
    origin: OriginId,
    body: Option<FunctionBody>,
}

#[derive(Debug)]
struct PendingFunctionTag {
    resource: FunctionTagResourceId,
    origin: OriginId,
    merge: FunctionTagMerge,
    entries: Option<Vec<FunctionTagEntry>>,
}

/// Builds one complete immutable Minecraft target program.
#[derive(Debug)]
pub struct MinecraftProgramBuilder {
    target: JavaEditionTarget,
    functions: EntityVec<McFunctionId, PendingFunction>,
    function_tags: EntityVec<FunctionTagId, PendingFunctionTag>,
    functions_by_resource: HashMap<FunctionResourceId, McFunctionId>,
    tags_by_resource: HashMap<FunctionTagResourceId, FunctionTagId>,
}

impl MinecraftProgramBuilder {
    /// Starts a program for one closed target.
    #[must_use]
    pub fn new(target: JavaEditionTarget) -> Self {
        Self {
            target,
            functions: EntityVec::new(),
            function_tags: EntityVec::new(),
            functions_by_resource: HashMap::new(),
            tags_by_resource: HashMap::new(),
        }
    }

    /// Declares an owned function and returns its stable typed identity.
    ///
    /// # Errors
    ///
    /// Rejects a duplicate function resource or exhausted entity identity space.
    pub fn declare_function(
        &mut self,
        resource: FunctionResourceId,
        origin: OriginId,
    ) -> Result<McFunctionId, BuildError> {
        if self.functions_by_resource.contains_key(&resource) {
            return Err(BuildError::DuplicateFunctionResource(resource));
        }
        let id = self
            .functions
            .push(PendingFunction {
                resource: resource.clone(),
                origin,
                body: None,
            })
            .map_err(|EntityLimitError| BuildError::EntityLimit)?;
        self.functions_by_resource.insert(resource, id);
        Ok(id)
    }

    /// Declares an owned function tag and returns its stable typed identity.
    ///
    /// # Errors
    ///
    /// Rejects a duplicate tag resource or exhausted entity identity space.
    pub fn declare_function_tag(
        &mut self,
        resource: FunctionTagResourceId,
        origin: OriginId,
        merge: FunctionTagMerge,
    ) -> Result<FunctionTagId, BuildError> {
        if self.tags_by_resource.contains_key(&resource) {
            return Err(BuildError::DuplicateFunctionTagResource(resource));
        }
        let id = self
            .function_tags
            .push(PendingFunctionTag {
                resource: resource.clone(),
                origin,
                merge,
                entries: None,
            })
            .map_err(|EntityLimitError| BuildError::EntityLimit)?;
        self.tags_by_resource.insert(resource, id);
        Ok(id)
    }

    /// Starts the one definition for a declared function.
    ///
    /// Dropping the returned builder without finishing leaves the declaration
    /// undefined and permits another attempt.
    ///
    /// # Errors
    ///
    /// Rejects an absent function ID or a function already defined.
    pub fn begin_function(
        &mut self,
        function: McFunctionId,
    ) -> Result<FunctionBodyBuilder<'_>, BuildError> {
        let pending = self
            .functions
            .get_mut(function)
            .ok_or(BuildError::InvalidFunction(function))?;
        if pending.body.is_some() {
            return Err(BuildError::FunctionAlreadyDefined(function));
        }
        Ok(FunctionBodyBuilder {
            slot: &mut pending.body,
            commands: EntityVec::new(),
        })
    }

    /// Starts the one definition for a declared function tag.
    ///
    /// Dropping the returned builder without finishing leaves the declaration
    /// undefined and permits another attempt.
    ///
    /// # Errors
    ///
    /// Rejects an absent tag ID or a tag already defined.
    pub fn begin_function_tag(
        &mut self,
        tag: FunctionTagId,
    ) -> Result<FunctionTagBuilder<'_>, BuildError> {
        let pending = self
            .function_tags
            .get_mut(tag)
            .ok_or(BuildError::InvalidFunctionTag(tag))?;
        if pending.entries.is_some() {
            return Err(BuildError::FunctionTagAlreadyDefined(tag));
        }
        Ok(FunctionTagBuilder {
            slot: &mut pending.entries,
            entries: vec![],
        })
    }

    /// Consumes pending state and produces a complete immutable program.
    ///
    /// # Errors
    ///
    /// Returns every declaration that still lacks a definition in deterministic
    /// function-then-tag declaration order.
    pub fn finish(self) -> Result<MinecraftProgram, Diagnostics> {
        let mut findings = vec![];
        let mut function_values = Vec::with_capacity(self.functions.len());
        for (function, pending) in self.functions.into_iter() {
            let Some(body) = pending.body else {
                findings.push(Diagnostic::new(
                    "minecraft.undefined-function",
                    format!(
                        "internal function {function:?} ({}) has no definition",
                        pending.resource
                    ),
                    pending.origin,
                ));
                continue;
            };
            function_values.push(McFunction::new(pending.resource, pending.origin, body));
        }

        let mut tag_values = Vec::with_capacity(self.function_tags.len());
        for (tag, pending) in self.function_tags.into_iter() {
            let Some(entries) = pending.entries else {
                findings.push(Diagnostic::new(
                    "minecraft.undefined-tag",
                    format!(
                        "internal function tag {tag:?} ({}) has no definition",
                        pending.resource
                    ),
                    pending.origin,
                ));
                continue;
            };
            tag_values.push(FunctionTag::new(
                pending.resource,
                pending.origin,
                pending.merge,
                entries,
            ));
        }

        match Diagnostics::from_findings(findings) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(MinecraftProgram::new(
                self.target,
                EntityVec::from_constrained_values(function_values),
                EntityVec::from_constrained_values(tag_values),
            )),
        }
    }
}

/// Builds one linear function body while reserving its exact definition slot.
pub struct FunctionBodyBuilder<'a> {
    slot: &'a mut Option<FunctionBody>,
    commands: EntityVec<CommandId, CommandNode>,
}

impl FunctionBodyBuilder<'_> {
    /// Appends one physical command and returns its dense function-local identity.
    ///
    /// # Errors
    ///
    /// Returns [`BuildError::EntityLimit`] if command IDs are exhausted.
    pub fn push(&mut self, command: CommandNode) -> Result<CommandId, BuildError> {
        self.commands
            .push(command)
            .map_err(|EntityLimitError| BuildError::EntityLimit)
    }

    /// Installs the complete body into its reserved declaration slot.
    pub fn finish(self) {
        let Self { slot, commands } = self;
        *slot = Some(FunctionBody::from_commands(commands));
    }
}

/// Builds one ordered function-tag definition while reserving its exact slot.
pub struct FunctionTagBuilder<'a> {
    slot: &'a mut Option<Vec<FunctionTagEntry>>,
    entries: Vec<FunctionTagEntry>,
}

impl FunctionTagBuilder<'_> {
    /// Appends one entry in target order.
    pub fn push(&mut self, entry: FunctionTagEntry) {
        self.entries.push(entry);
    }

    /// Installs the complete ordered entries into the reserved declaration slot.
    pub fn finish(self) {
        let Self { slot, entries } = self;
        *slot = Some(entries);
    }
}

/// A local Minecraft program-construction error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildError {
    /// A function resource was declared twice.
    DuplicateFunctionResource(FunctionResourceId),
    /// A function-tag resource was declared twice.
    DuplicateFunctionTagResource(FunctionTagResourceId),
    /// A function ID does not belong to this pending store.
    InvalidFunction(McFunctionId),
    /// A function-tag ID does not belong to this pending store.
    InvalidFunctionTag(FunctionTagId),
    /// The function already has a completed definition.
    FunctionAlreadyDefined(McFunctionId),
    /// The function tag already has a completed definition.
    FunctionTagAlreadyDefined(FunctionTagId),
    /// A dense compiler ID space was exhausted.
    EntityLimit,
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateFunctionResource(resource) => {
                write!(formatter, "function resource {resource} was declared twice")
            }
            Self::DuplicateFunctionTagResource(resource) => {
                write!(
                    formatter,
                    "function-tag resource {resource} was declared twice"
                )
            }
            Self::InvalidFunction(function) => {
                write!(formatter, "invalid pending function ID {function:?}")
            }
            Self::InvalidFunctionTag(tag) => {
                write!(formatter, "invalid pending function-tag ID {tag:?}")
            }
            Self::FunctionAlreadyDefined(function) => {
                write!(formatter, "function {function:?} is already defined")
            }
            Self::FunctionTagAlreadyDefined(tag) => {
                write!(formatter, "function tag {tag:?} is already defined")
            }
            Self::EntityLimit => formatter.write_str("Minecraft entity ID space exhausted"),
        }
    }
}

impl Error for BuildError {}

#[cfg(test)]
mod tests {
    use super::{BuildError, MinecraftProgramBuilder};
    use crate::entity::{EntityId, EntityVec};
    use crate::ir::minecraft::{
        CallableRef, CommandKind, CommandNode, FunctionCall, FunctionResourceId, FunctionTagEntry,
        FunctionTagMerge, FunctionTagResourceId, InternalCallableRef, SayCommand, SayMessage,
    };
    use crate::source::OriginId;
    use crate::target::JavaEditionTarget;

    #[test]
    fn declarations_are_kind_distinct_and_same_kind_unique() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function_resource = FunctionResourceId::parse("mdl:same").unwrap();
        let tag_resource = FunctionTagResourceId::parse("mdl:same").unwrap();
        builder
            .declare_function(function_resource.clone(), OriginId::UNKNOWN)
            .unwrap();
        builder
            .declare_function_tag(
                tag_resource.clone(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();

        assert!(matches!(
            builder.declare_function(function_resource, OriginId::UNKNOWN),
            Err(BuildError::DuplicateFunctionResource(_))
        ));
        assert!(matches!(
            builder.declare_function_tag(tag_resource, OriginId::UNKNOWN, FunctionTagMerge::Append),
            Err(BuildError::DuplicateFunctionTagResource(_))
        ));
    }

    #[test]
    fn forward_recursive_definitions_and_dense_commands_finish_without_pending_state() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let first = builder
            .declare_function(
                FunctionResourceId::parse("mdl:first").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let second = builder
            .declare_function(
                FunctionResourceId::parse("mdl:second").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:entries").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();

        {
            let mut body = builder.begin_function(first).unwrap();
            let command = CommandNode::new(
                CommandKind::Function(FunctionCall::new(CallableRef::Internal(
                    InternalCallableRef::Function(second),
                ))),
                OriginId::UNKNOWN,
            )
            .unwrap();
            assert_eq!(body.push(command).unwrap().index(), 0);
            body.finish();
        }
        {
            let mut body = builder.begin_function(second).unwrap();
            let command = CommandNode::new(
                CommandKind::Function(FunctionCall::new(CallableRef::Internal(
                    InternalCallableRef::Function(first),
                ))),
                OriginId::UNKNOWN,
            )
            .unwrap();
            body.push(command).unwrap();
            body.finish();
        }
        {
            let mut entries = builder.begin_function_tag(tag).unwrap();
            entries.push(FunctionTagEntry::internal(
                InternalCallableRef::Function(first),
                OriginId::UNKNOWN,
            ));
            entries.finish();
        }

        assert!(matches!(
            builder.begin_function(first),
            Err(BuildError::FunctionAlreadyDefined(found)) if found == first
        ));
        let program = builder.finish().unwrap();
        assert_eq!(program.functions().count(), 2);
        assert_eq!(program.function_tags().count(), 1);
        assert_eq!(program.function(first).unwrap().body().len(), 1);
    }

    #[test]
    fn dropped_scoped_builder_can_retry_and_empty_body_is_complete() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:empty").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        drop(builder.begin_function(function).unwrap());
        builder.begin_function(function).unwrap().finish();

        let program = builder.finish().unwrap();
        assert!(program.function(function).unwrap().body().is_empty());
    }

    #[test]
    fn function_body_builder_retains_typed_say_without_raw_text() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:say").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        let command = body
            .push(
                CommandNode::new(
                    CommandKind::Say(SayCommand::new(SayMessage::new("hello").unwrap())),
                    OriginId::UNKNOWN,
                )
                .unwrap(),
            )
            .unwrap();
        body.finish();

        let program = builder.finish().unwrap();
        let node = program
            .function(function)
            .unwrap()
            .body()
            .command(command)
            .unwrap();
        let CommandKind::Say(say) = node.kind() else {
            panic!("typed say was not retained");
        };
        assert_eq!(say.message().as_str(), "hello");
    }

    #[test]
    fn finish_accumulates_every_missing_definition_in_stable_order() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        builder
            .declare_function(
                FunctionResourceId::parse("mdl:missing").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:missing").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();

        let diagnostics = builder.finish().unwrap_err();
        let codes = diagnostics
            .findings()
            .iter()
            .map(crate::diagnostic::Diagnostic::code)
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            vec!["minecraft.undefined-function", "minecraft.undefined-tag"]
        );
    }

    #[test]
    fn invalid_ids_are_rejected_without_indexing() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let invalid_function = <super::super::McFunctionId as EntityId>::from_index(4);
        let invalid_tag = <super::super::FunctionTagId as EntityId>::from_index(4);
        assert!(matches!(
            builder.begin_function(invalid_function),
            Err(BuildError::InvalidFunction(_))
        ));
        assert!(matches!(
            builder.begin_function_tag(invalid_tag),
            Err(BuildError::InvalidFunctionTag(_))
        ));

        let empty: EntityVec<super::super::CommandId, ()> = EntityVec::new();
        assert!(empty.is_empty());
    }
}
