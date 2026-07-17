use crate::ir::minecraft::{
    CallableRef, Cardinality, CommandKind, Condition, DataCommand, DataSource, ExecuteModifierKind,
    ExternalTagRequirement, FunctionTagEntryKind, FunctionTagId, InternalCallableRef, McFunctionId,
    ReturnCommand, ScoreCommand, StoreChannel, StoreDestination,
};

use super::ReturnValueClass;

/// Closed cost-relevant classification of scoreboard commands.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScoreCommandClass {
    ObjectiveAddDummy,
    PlayersSet,
    PlayersAdd,
    PlayersRemove,
    PlayersGet,
    PlayersReset,
    PlayersOperation,
}

impl ScoreCommandClass {
    /// Classifies every structured scoreboard command without inspecting text.
    #[must_use]
    pub const fn classify(command: &ScoreCommand) -> Self {
        match command {
            ScoreCommand::ObjectiveAddDummy { .. } => Self::ObjectiveAddDummy,
            ScoreCommand::PlayersSet { .. } => Self::PlayersSet,
            ScoreCommand::PlayersAdd { .. } => Self::PlayersAdd,
            ScoreCommand::PlayersRemove { .. } => Self::PlayersRemove,
            ScoreCommand::PlayersGet { .. } => Self::PlayersGet,
            ScoreCommand::PlayersReset { .. } => Self::PlayersReset,
            ScoreCommand::PlayersOperation { .. } => Self::PlayersOperation,
        }
    }
}

/// Closed cost-relevant classification of command-storage operations.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DataCommandClass {
    Get,
    Remove,
    ModifyValue,
    ModifyFrom,
}

impl DataCommandClass {
    /// Classifies every structured command-storage command.
    #[must_use]
    pub const fn classify(command: &DataCommand) -> Self {
        match command {
            DataCommand::Get { .. } => Self::Get,
            DataCommand::Remove { .. } => Self::Remove,
            DataCommand::Modify {
                source: DataSource::Value(_),
                ..
            } => Self::ModifyValue,
            DataCommand::Modify {
                source: DataSource::From(_),
                ..
            } => Self::ModifyFrom,
        }
    }
}

/// Closed cost-relevant classification of execute conditions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ConditionClass {
    ScoreMatches,
    ScoreCompare,
    DataExists,
    EntityExists(Cardinality),
    InternalFunction(McFunctionId),
}

impl ConditionClass {
    /// Classifies every structured execute condition.
    #[must_use]
    pub const fn classify(condition: &Condition) -> Self {
        match condition {
            Condition::ScoreMatches(_, _) => Self::ScoreMatches,
            Condition::ScoreCompare(_, _, _) => Self::ScoreCompare,
            Condition::DataExists(_) => Self::DataExists,
            Condition::EntityExists(selector) => Self::EntityExists(selector.cardinality()),
            Condition::Function(function) => Self::InternalFunction(*function),
        }
    }
}

/// Closed class of an execute-store destination.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StoreDestinationClass {
    Score,
    Storage,
}

impl StoreDestinationClass {
    #[must_use]
    const fn classify(destination: &StoreDestination) -> Self {
        match destination {
            StoreDestination::Score(_) => Self::Score,
            StoreDestination::Storage { .. } => Self::Storage,
        }
    }
}

/// Closed cost-relevant classification of one execute stage.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExecuteModifierClass {
    As(Cardinality),
    At(Cardinality),
    In,
    Positioned,
    Rotated,
    Anchored,
    Align,
    If(ConditionClass),
    Unless(ConditionClass),
    Store(StoreChannel, StoreDestinationClass),
}

impl ExecuteModifierClass {
    /// Classifies every structured execute modifier.
    #[must_use]
    pub const fn classify(modifier: &ExecuteModifierKind) -> Self {
        match modifier {
            ExecuteModifierKind::As(selector) => Self::As(selector.cardinality()),
            ExecuteModifierKind::At(selector) => Self::At(selector.cardinality()),
            ExecuteModifierKind::In(_) => Self::In,
            ExecuteModifierKind::Positioned(_) => Self::Positioned,
            ExecuteModifierKind::Rotated(_) => Self::Rotated,
            ExecuteModifierKind::Anchored(_) => Self::Anchored,
            ExecuteModifierKind::Align(_) => Self::Align,
            ExecuteModifierKind::If(condition) => Self::If(ConditionClass::classify(condition)),
            ExecuteModifierKind::Unless(condition) => {
                Self::Unless(ConditionClass::classify(condition))
            }
            ExecuteModifierKind::Store(channel, destination) => {
                Self::Store(*channel, StoreDestinationClass::classify(destination))
            }
        }
    }
}

/// Typed internal or external callable class.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CallableClass {
    InternalFunction(McFunctionId),
    InternalTag(FunctionTagId),
    ExternalFunction,
    ExternalTag,
}

impl CallableClass {
    /// Classifies function and function-tag references without path heuristics.
    #[must_use]
    pub const fn classify(target: &CallableRef) -> Self {
        match target {
            CallableRef::Internal(InternalCallableRef::Function(function)) => {
                Self::InternalFunction(*function)
            }
            CallableRef::Internal(InternalCallableRef::Tag(tag)) => Self::InternalTag(*tag),
            CallableRef::External(crate::ir::minecraft::ExternalCallableRef::Function(_)) => {
                Self::ExternalFunction
            }
            CallableRef::External(crate::ir::minecraft::ExternalCallableRef::Tag(_)) => {
                Self::ExternalTag
            }
        }
    }
}

/// Exhaustive typed classification of one owned function-tag entry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FunctionTagEntryClass {
    InternalFunction(McFunctionId),
    InternalTag(FunctionTagId),
    ExternalFunction(ExternalTagRequirement),
    ExternalTag(ExternalTagRequirement),
}

impl FunctionTagEntryClass {
    /// Classifies every internal and deployment-provided tag entry.
    #[must_use]
    pub const fn classify(entry: &FunctionTagEntryKind) -> Self {
        match entry {
            FunctionTagEntryKind::Internal(InternalCallableRef::Function(function)) => {
                Self::InternalFunction(*function)
            }
            FunctionTagEntryKind::Internal(InternalCallableRef::Tag(tag)) => {
                Self::InternalTag(*tag)
            }
            FunctionTagEntryKind::External {
                target: crate::ir::minecraft::ExternalCallableRef::Function(_),
                requirement,
            } => Self::ExternalFunction(*requirement),
            FunctionTagEntryKind::External {
                target: crate::ir::minecraft::ExternalCallableRef::Tag(_),
                requirement,
            } => Self::ExternalTag(*requirement),
        }
    }
}

/// Closed classification of native return forms.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ReturnCommandClass {
    Value(ReturnValueClass),
    Fail,
    Run(Box<CommandStepClass>),
}

impl ReturnCommandClass {
    /// Classifies every return form, retaining nested structured syntax.
    #[must_use]
    pub fn classify(command: &ReturnCommand) -> Self {
        match command {
            ReturnCommand::Value(0) => Self::Value(ReturnValueClass::Zero),
            ReturnCommand::Value(_) => Self::Value(ReturnValueClass::NonZero),
            ReturnCommand::Fail => Self::Fail,
            ReturnCommand::Run(command) => {
                Self::Run(Box::new(CommandStepClass::classify(command.kind())))
            }
        }
    }
}

/// Exhaustive cost-relevant syntax class for one structured command node.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CommandStepClass {
    Score(ScoreCommandClass),
    Data(DataCommandClass),
    Say,
    Teleport,
    Execute {
        modifiers: Box<[ExecuteModifierClass]>,
        run: Box<CommandStepClass>,
    },
    Function(CallableClass),
    Return(ReturnCommandClass),
    Raw,
}

impl CommandStepClass {
    /// Classifies the entire recursive Stage 3 command vocabulary exhaustively.
    #[must_use]
    pub fn classify(kind: &CommandKind) -> Self {
        match kind {
            CommandKind::Score(command) => Self::Score(ScoreCommandClass::classify(command)),
            CommandKind::Data(command) => Self::Data(DataCommandClass::classify(command)),
            CommandKind::Say(_) => Self::Say,
            CommandKind::Teleport(_) => Self::Teleport,
            CommandKind::Execute(command) => Self::Execute {
                modifiers: command
                    .modifiers()
                    .as_slice()
                    .iter()
                    .map(|modifier| ExecuteModifierClass::classify(modifier.kind()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                run: Box::new(Self::classify(command.run().kind())),
            },
            CommandKind::Function(command) => {
                Self::Function(CallableClass::classify(command.target()))
            }
            CommandKind::Return(command) => Self::Return(ReturnCommandClass::classify(command)),
            CommandKind::Raw(_) => Self::Raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityId;
    use crate::ir::minecraft::{
        AtMostOneSelector, CommandNode, EntitySelector, ExecuteModifier, ExternalCallableRef,
        FunctionResourceId, FunctionTagEntry, FunctionTagResourceId, SayCommand, SayMessage,
        Selector, UnboundedSelector, UnsafeRawCommand,
    };
    use crate::source::OriginId;

    use super::*;

    #[test]
    fn callable_and_tag_entry_classes_preserve_typed_ownership() {
        let function = McFunctionId::from_index(3);
        let tag = FunctionTagId::from_index(4);
        for (target, expected) in [
            (
                CallableRef::Internal(InternalCallableRef::Function(function)),
                CallableClass::InternalFunction(function),
            ),
            (
                CallableRef::Internal(InternalCallableRef::Tag(tag)),
                CallableClass::InternalTag(tag),
            ),
            (
                ExternalCallableRef::Function(FunctionResourceId::parse("other:f").unwrap()).into(),
                CallableClass::ExternalFunction,
            ),
            (
                ExternalCallableRef::Tag(FunctionTagResourceId::parse("other:t").unwrap()).into(),
                CallableClass::ExternalTag,
            ),
        ] {
            assert_eq!(CallableClass::classify(&target), expected);
        }

        let entry = FunctionTagEntry::external(
            ExternalCallableRef::Tag(FunctionTagResourceId::parse("other:t").unwrap()),
            ExternalTagRequirement::Optional,
            OriginId::UNKNOWN,
        );
        assert_eq!(
            FunctionTagEntryClass::classify(entry.kind()),
            FunctionTagEntryClass::ExternalTag(ExternalTagRequirement::Optional)
        );
    }

    #[test]
    fn recursive_command_and_modifier_classes_keep_semantic_distinctions() {
        let function = McFunctionId::from_index(7);
        let raw = CommandNode::new(
            CommandKind::Raw(UnsafeRawCommand::new("say opaque").unwrap()),
            OriginId::UNKNOWN,
        )
        .unwrap();
        assert_eq!(
            ReturnCommandClass::classify(&ReturnCommand::run(raw)),
            ReturnCommandClass::Run(Box::new(CommandStepClass::Raw))
        );
        assert_eq!(
            ReturnCommandClass::classify(&ReturnCommand::Value(0)),
            ReturnCommandClass::Value(ReturnValueClass::Zero)
        );
        assert_eq!(
            ReturnCommandClass::classify(&ReturnCommand::Value(-1)),
            ReturnCommandClass::Value(ReturnValueClass::NonZero)
        );
        assert_eq!(
            CommandStepClass::classify(&CommandKind::Say(SayCommand::new(
                SayMessage::new("hello").unwrap(),
            ))),
            CommandStepClass::Say
        );

        let classes = [
            ExecuteModifierKind::As(Selector::from(UnboundedSelector::AllEntities)),
            ExecuteModifierKind::At(Selector::from(AtMostOneSelector::SelfExecutor)),
            ExecuteModifierKind::As(
                EntitySelector::armor_stands(vec![], Some(2))
                    .unwrap()
                    .into(),
            ),
            ExecuteModifierKind::If(Condition::Function(function)),
        ]
        .map(|modifier| ExecuteModifierClass::classify(&modifier));
        assert_eq!(classes[0], ExecuteModifierClass::As(Cardinality::UNBOUNDED));
        assert_eq!(
            classes[1],
            ExecuteModifierClass::At(Cardinality::AT_MOST_ONE)
        );
        let ExecuteModifierClass::As(bounded) = classes[2] else {
            panic!("expected bounded as modifier class");
        };
        assert_eq!(bounded.maximum(), Some(2));
        assert_eq!(
            classes[3],
            ExecuteModifierClass::If(ConditionClass::InternalFunction(function))
        );

        let modifier = ExecuteModifier::new(
            ExecuteModifierKind::Unless(Condition::EntityExists(Selector::from(
                UnboundedSelector::AllPlayers,
            ))),
            OriginId::UNKNOWN,
        );
        assert_eq!(
            ExecuteModifierClass::classify(modifier.kind()),
            ExecuteModifierClass::Unless(ConditionClass::EntityExists(Cardinality::UNBOUNDED))
        );
    }
}
