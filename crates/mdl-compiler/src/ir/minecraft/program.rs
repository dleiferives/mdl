use crate::entity::{EntityVec, entity_id};
use crate::source::OriginId;
use crate::target::JavaEditionTarget;

use super::{CommandNode, FunctionResourceId, FunctionTagResourceId};

entity_id!(
    /// Identity of a function within one Minecraft program.
    pub struct McFunctionId;
);

entity_id!(
    /// Identity of a function tag within one Minecraft program.
    pub struct FunctionTagId;
);

entity_id!(
    /// Identity of a top-level command within one function body.
    pub struct CommandId;
);

/// A callable resource owned by the current program.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InternalCallableRef {
    /// One owned function.
    Function(McFunctionId),
    /// One owned function tag.
    Tag(FunctionTagId),
}

/// A callable resource supplied by the deployment environment.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ExternalCallableRef {
    /// One external function.
    Function(FunctionResourceId),
    /// One external function tag.
    Tag(FunctionTagResourceId),
}

/// An explicitly internal or external callable target.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CallableRef {
    /// An ID-checked resource owned by this program.
    Internal(InternalCallableRef),
    /// A validated resource whose existence is a deployment responsibility.
    External(ExternalCallableRef),
}

impl From<InternalCallableRef> for CallableRef {
    fn from(target: InternalCallableRef) -> Self {
        Self::Internal(target)
    }
}

impl From<ExternalCallableRef> for CallableRef {
    fn from(target: ExternalCallableRef) -> Self {
        Self::External(target)
    }
}

/// Whether an external function-tag entry may be absent at load time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExternalTagRequirement {
    /// Absence is a pack loading error.
    Required,
    /// Absence is tolerated by the target loader.
    Optional,
}

/// How an owned function-tag file combines with lower-priority packs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FunctionTagMerge {
    /// Append entries to the resolved lower-priority tag.
    Append,
    /// Replace lower-priority entries.
    Replace,
}

/// The semantic kind of one ordered function-tag entry.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum FunctionTagEntryKind {
    /// An owned target, always required by construction.
    Internal(InternalCallableRef),
    /// A deployment-provided target with explicit absence policy.
    External {
        /// External function or tag resource.
        target: ExternalCallableRef,
        /// Required versus optional load behavior.
        requirement: ExternalTagRequirement,
    },
}

/// One ordered function-tag entry with compiler provenance.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FunctionTagEntry {
    kind: FunctionTagEntryKind,
    origin: OriginId,
}

impl FunctionTagEntry {
    /// Constructs an always-required reference to an owned target.
    #[must_use]
    pub const fn internal(target: InternalCallableRef, origin: OriginId) -> Self {
        Self {
            kind: FunctionTagEntryKind::Internal(target),
            origin,
        }
    }

    /// Constructs a reference to a deployment-provided target.
    #[must_use]
    pub const fn external(
        target: ExternalCallableRef,
        requirement: ExternalTagRequirement,
        origin: OriginId,
    ) -> Self {
        Self {
            kind: FunctionTagEntryKind::External {
                target,
                requirement,
            },
            origin,
        }
    }

    /// Returns the entry semantics.
    #[must_use]
    pub const fn kind(&self) -> &FunctionTagEntryKind {
        &self.kind
    }

    /// Returns the entry provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// One immutable linear function body.
#[derive(Clone, Debug)]
pub struct FunctionBody {
    commands: EntityVec<CommandId, CommandNode>,
}

impl FunctionBody {
    pub(super) const fn from_commands(commands: EntityVec<CommandId, CommandNode>) -> Self {
        Self { commands }
    }

    /// Returns a command by function-local identity.
    #[must_use]
    pub fn command(&self, command: CommandId) -> Option<&CommandNode> {
        self.commands.get(command)
    }

    /// Iterates commands in physical target order.
    #[must_use]
    pub fn commands(&self) -> impl ExactSizeIterator<Item = (CommandId, &CommandNode)> + '_ {
        self.commands.iter()
    }

    /// Returns the number of top-level physical commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Returns whether the function emits zero commands.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// One owned function resource and its complete body.
#[derive(Clone, Debug)]
pub struct McFunction {
    resource: FunctionResourceId,
    origin: OriginId,
    body: FunctionBody,
}

impl McFunction {
    pub(super) const fn new(
        resource: FunctionResourceId,
        origin: OriginId,
        body: FunctionBody,
    ) -> Self {
        Self {
            resource,
            origin,
            body,
        }
    }

    /// Returns the serialized resource identity.
    #[must_use]
    pub const fn resource(&self) -> &FunctionResourceId {
        &self.resource
    }

    /// Returns declaration provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Returns the complete linear body.
    #[must_use]
    pub const fn body(&self) -> &FunctionBody {
        &self.body
    }
}

/// One owned function-tag resource and its complete ordered entries.
#[derive(Clone, Debug)]
pub struct FunctionTag {
    resource: FunctionTagResourceId,
    origin: OriginId,
    merge: FunctionTagMerge,
    entries: Box<[FunctionTagEntry]>,
}

impl FunctionTag {
    pub(super) fn new(
        resource: FunctionTagResourceId,
        origin: OriginId,
        merge: FunctionTagMerge,
        entries: Vec<FunctionTagEntry>,
    ) -> Self {
        Self {
            resource,
            origin,
            merge,
            entries: entries.into_boxed_slice(),
        }
    }

    /// Returns the serialized resource identity.
    #[must_use]
    pub const fn resource(&self) -> &FunctionTagResourceId {
        &self.resource
    }

    /// Returns declaration provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Returns append-versus-replace semantics.
    #[must_use]
    pub const fn merge(&self) -> FunctionTagMerge {
        self.merge
    }

    /// Returns entries in target order.
    #[must_use]
    pub fn entries(&self) -> &[FunctionTagEntry] {
        &self.entries
    }
}

/// A complete immutable Minecraft target program.
#[derive(Debug)]
pub struct MinecraftProgram {
    target: JavaEditionTarget,
    functions: EntityVec<McFunctionId, McFunction>,
    function_tags: EntityVec<FunctionTagId, FunctionTag>,
}

impl MinecraftProgram {
    pub(super) const fn new(
        target: JavaEditionTarget,
        functions: EntityVec<McFunctionId, McFunction>,
        function_tags: EntityVec<FunctionTagId, FunctionTag>,
    ) -> Self {
        Self {
            target,
            functions,
            function_tags,
        }
    }

    /// Returns the closed compilation target.
    #[must_use]
    pub const fn target(&self) -> JavaEditionTarget {
        self.target
    }

    /// Looks up one owned function.
    #[must_use]
    pub fn function(&self, function: McFunctionId) -> Option<&McFunction> {
        self.functions.get(function)
    }

    /// Iterates owned functions in declaration order.
    #[must_use]
    pub fn functions(&self) -> impl ExactSizeIterator<Item = (McFunctionId, &McFunction)> + '_ {
        self.functions.iter()
    }

    /// Looks up one owned function tag.
    #[must_use]
    pub fn function_tag(&self, tag: FunctionTagId) -> Option<&FunctionTag> {
        self.function_tags.get(tag)
    }

    /// Iterates owned function tags in declaration order.
    #[must_use]
    pub fn function_tags(
        &self,
    ) -> impl ExactSizeIterator<Item = (FunctionTagId, &FunctionTag)> + '_ {
        self.function_tags.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CallableRef, ExternalCallableRef, ExternalTagRequirement, FunctionTagEntry,
        FunctionTagEntryKind, InternalCallableRef,
    };
    use crate::entity::{EntityId, EntityVec};
    use crate::ir::minecraft::{FunctionResourceId, FunctionTagResourceId};
    use crate::source::OriginId;

    #[test]
    fn program_entity_ids_are_dense_and_domain_distinct() {
        let mut functions = EntityVec::<super::McFunctionId, _>::new();
        let mut tags = EntityVec::<super::FunctionTagId, _>::new();
        let function = functions.push("function").unwrap();
        let tag = tags.push("tag").unwrap();

        assert_eq!(function.index(), 0);
        assert_eq!(tag.index(), 0);
        assert_eq!(functions.get(function), Some(&"function"));
        assert_eq!(tags.get(tag), Some(&"tag"));
    }

    #[test]
    fn callable_ownership_and_tag_optionality_are_structural() {
        let external_function =
            ExternalCallableRef::Function(FunctionResourceId::parse("other:entry").unwrap());
        let external_tag =
            ExternalCallableRef::Tag(FunctionTagResourceId::parse("other:entries").unwrap());
        let callable = CallableRef::from(external_function.clone());
        assert!(matches!(callable, CallableRef::External(_)));

        let entry = FunctionTagEntry::external(
            external_tag,
            ExternalTagRequirement::Optional,
            OriginId::UNKNOWN,
        );
        assert!(matches!(
            entry.kind(),
            FunctionTagEntryKind::External {
                requirement: ExternalTagRequirement::Optional,
                ..
            }
        ));

        let internal = FunctionTagEntry::internal(
            InternalCallableRef::Function(super::McFunctionId::from_index(0)),
            OriginId::UNKNOWN,
        );
        assert!(matches!(
            internal.kind(),
            FunctionTagEntryKind::Internal(InternalCallableRef::Function(_))
        ));
    }

    #[test]
    fn final_function_body_distinguishes_empty_from_missing() {
        let body = super::FunctionBody::from_commands(EntityVec::new());

        assert!(body.is_empty());
        assert_eq!(body.len(), 0);
        assert_eq!(body.commands().count(), 0);
    }
}
