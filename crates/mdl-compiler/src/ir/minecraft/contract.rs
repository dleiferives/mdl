/// A set of Minecraft execution-context components.
///
/// The raw bits are private so only context components understood by the compiler
/// can be represented.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContextMask(u8);

impl ContextMask {
    /// No context component.
    pub const NONE: Self = Self(0);
    /// Command executor identity.
    pub const EXECUTOR: Self = Self(1 << 0);
    /// Execution position.
    pub const POSITION: Self = Self(1 << 1);
    /// Execution rotation.
    pub const ROTATION: Self = Self(1 << 2);
    /// Execution dimension.
    pub const DIMENSION: Self = Self(1 << 3);
    /// Entity anchor.
    pub const ANCHOR: Self = Self(1 << 4);

    /// Returns the union of two component sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns whether every component in `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether the sets share at least one component.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Returns whether the set contains no context components.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A set of broad Minecraft effect categories.
///
/// These bits are intentionally private and are not place-level alias analysis.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EffectCategories(u8);

impl EffectCategories {
    /// No known effect category.
    pub const NONE: Self = Self(0);
    /// Reads scoreboard state.
    pub const SCORE_READ: Self = Self(1 << 0);
    /// Writes scoreboard state or objective declarations.
    pub const SCORE_WRITE: Self = Self(1 << 1);
    /// Reads command storage.
    pub const STORAGE_READ: Self = Self(1 << 2);
    /// Writes command storage.
    pub const STORAGE_WRITE: Self = Self(1 << 3);
    /// Resolves an entity selector.
    pub const ENTITY_QUERY: Self = Self(1 << 4);
    /// Performs explicit target control flow.
    pub const CONTROL: Self = Self(1 << 5);

    /// Returns the union of two category sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns whether every category in `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns whether the sets share at least one category.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Returns whether the set is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Known broad effects or an explicit unknown barrier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EffectSummary {
    /// Exhaustive broad categories for the modeled command.
    Known(EffectCategories),
    /// Effects cannot be bounded by Stage 3.
    Unknown,
}

impl EffectSummary {
    const fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Known(left), Self::Known(right)) => Self::Known(left.union(right)),
            (Self::Known(_) | Self::Unknown, Self::Unknown) | (Self::Unknown, Self::Known(_)) => {
                Self::Unknown
            }
        }
    }
}

/// Known context reads/changes or an explicit unknown barrier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextSummary {
    /// Conservatively known context use.
    Known {
        /// Components inspected while resolving the command.
        reads: ContextMask,
        /// Components transformed for nested execution.
        changes: ContextMask,
    },
    /// Context dependence cannot be bounded by Stage 3.
    Unknown,
}

impl ContextSummary {
    const NONE: Self = Self::Known {
        reads: ContextMask::NONE,
        changes: ContextMask::NONE,
    };

    const fn union(self, other: Self) -> Self {
        match (self, other) {
            (
                Self::Known {
                    reads: left_reads,
                    changes: left_changes,
                },
                Self::Known {
                    reads: right_reads,
                    changes: right_changes,
                },
            ) => Self::Known {
                reads: left_reads.union(right_reads),
                changes: left_changes.union(right_changes),
            },
            (Self::Known { .. } | Self::Unknown, Self::Unknown)
            | (Self::Unknown, Self::Known { .. }) => Self::Unknown,
        }
    }
}

/// Conservative per-input context fan-out.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ForkClass {
    /// No modifier can multiply one incoming context.
    Never,
    /// A modifier can retain zero or one context per input.
    AtMostOne,
    /// A modifier may produce an unbounded number of contexts per input.
    Unbounded,
    /// Fan-out cannot be bounded by Stage 3.
    Unknown,
}

impl ForkClass {
    const fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Unbounded, _) | (_, Self::Unbounded) => Self::Unbounded,
            (Self::AtMostOne, _) | (_, Self::AtMostOne) => Self::AtMostOne,
            (Self::Never, Self::Never) => Self::Never,
        }
    }
}

/// Conservative facts derived from one command's syntax.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommandContract {
    effects: EffectSummary,
    context: ContextSummary,
    fork: ForkClass,
}

impl CommandContract {
    const fn new(effects: EffectSummary, context: ContextSummary, fork: ForkClass) -> Self {
        Self {
            effects,
            context,
            fork,
        }
    }

    /// Returns broad known effects or unknown.
    #[must_use]
    pub const fn effects(self) -> EffectSummary {
        self.effects
    }

    /// Returns known context use or unknown.
    #[must_use]
    pub const fn context(self) -> ContextSummary {
        self.context
    }

    /// Returns conservative per-input fan-out.
    #[must_use]
    pub const fn fork(self) -> ForkClass {
        self.fork
    }
}

use super::{
    CommandKind, CommandNode, Condition, DataCommand, DataSource, ExecuteModifierKind,
    ReturnCommand, ScoreCommand, ScoreHolders, ScoreRef, Selector, SingleScoreHolder,
    StoreDestination,
};

impl CommandNode {
    /// Derives conservative command facts directly from immutable syntax.
    #[must_use]
    pub fn contract(&self) -> CommandContract {
        contract_for_kind(self.kind())
    }
}

fn contract_for_kind(kind: &CommandKind) -> CommandContract {
    match kind {
        CommandKind::Score(command) => score_contract(command),
        CommandKind::Data(command) => data_contract(command),
        CommandKind::Execute(command) => {
            let mut contract = command.run().contract();
            for modifier in command.modifiers().as_slice() {
                contract = compose_modifier(contract, modifier.kind());
            }
            contract
        }
        CommandKind::Function(_) | CommandKind::Raw(_) => CommandContract::new(
            EffectSummary::Unknown,
            ContextSummary::Unknown,
            ForkClass::Unknown,
        ),
        CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail) => CommandContract::new(
            EffectSummary::Known(EffectCategories::CONTROL),
            ContextSummary::NONE,
            ForkClass::Never,
        ),
        CommandKind::Return(ReturnCommand::Run(command)) => {
            let nested = command.contract();
            CommandContract::new(
                nested
                    .effects
                    .union(EffectSummary::Known(EffectCategories::CONTROL)),
                nested.context,
                nested.fork,
            )
        }
    }
}

fn score_contract(command: &ScoreCommand) -> CommandContract {
    let (effects, reads) = match command {
        ScoreCommand::ObjectiveAddDummy { .. } => {
            (EffectCategories::SCORE_WRITE, ContextMask::NONE)
        }
        ScoreCommand::PlayersSet { target, .. } | ScoreCommand::PlayersReset { target } => (
            EffectCategories::SCORE_WRITE.union(holder_effects(target.holders())),
            holder_context(target.holders()),
        ),
        ScoreCommand::PlayersAdd { target, .. } | ScoreCommand::PlayersRemove { target, .. } => (
            EffectCategories::SCORE_READ
                .union(EffectCategories::SCORE_WRITE)
                .union(holder_effects(target.holders())),
            holder_context(target.holders()),
        ),
        ScoreCommand::PlayersGet { score } => (
            EffectCategories::SCORE_READ.union(single_holder_effects(score)),
            single_holder_context(score),
        ),
        ScoreCommand::PlayersOperation { target, source, .. } => (
            EffectCategories::SCORE_READ
                .union(EffectCategories::SCORE_WRITE)
                .union(holder_effects(target.holders()))
                .union(holder_effects(source.holders())),
            holder_context(target.holders()).union(holder_context(source.holders())),
        ),
    };
    CommandContract::new(
        EffectSummary::Known(effects),
        ContextSummary::Known {
            reads,
            changes: ContextMask::NONE,
        },
        ForkClass::Never,
    )
}

fn data_contract(command: &DataCommand) -> CommandContract {
    let effects = match command {
        DataCommand::Get { .. } => EffectCategories::STORAGE_READ,
        DataCommand::Remove { .. }
        | DataCommand::Modify {
            source: DataSource::Value(_),
            ..
        } => EffectCategories::STORAGE_WRITE,
        DataCommand::Modify {
            source: DataSource::From(_),
            ..
        } => EffectCategories::STORAGE_READ.union(EffectCategories::STORAGE_WRITE),
    };
    CommandContract::new(
        EffectSummary::Known(effects),
        ContextSummary::NONE,
        ForkClass::Never,
    )
}

fn compose_modifier(nested: CommandContract, modifier: &ExecuteModifierKind) -> CommandContract {
    let modifier_contract = match modifier {
        ExecuteModifierKind::As(selector) => CommandContract::new(
            EffectSummary::Known(EffectCategories::ENTITY_QUERY),
            ContextSummary::Known {
                reads: selector.context_reads(),
                changes: ContextMask::EXECUTOR,
            },
            selector_fork(*selector),
        ),
        ExecuteModifierKind::At(selector) => CommandContract::new(
            EffectSummary::Known(EffectCategories::ENTITY_QUERY),
            ContextSummary::Known {
                reads: selector.context_reads(),
                changes: ContextMask::POSITION
                    .union(ContextMask::ROTATION)
                    .union(ContextMask::DIMENSION),
            },
            selector_fork(*selector),
        ),
        ExecuteModifierKind::In(_) => CommandContract::new(
            EffectSummary::Known(EffectCategories::NONE),
            ContextSummary::Known {
                reads: ContextMask::POSITION.union(ContextMask::DIMENSION),
                changes: ContextMask::POSITION.union(ContextMask::DIMENSION),
            },
            ForkClass::Never,
        ),
        ExecuteModifierKind::If(condition) | ExecuteModifierKind::Unless(condition) => {
            condition_contract(condition)
        }
        ExecuteModifierKind::Store(_, destination) => store_contract(destination),
    };
    CommandContract::new(
        nested.effects.union(modifier_contract.effects),
        nested.context.union(modifier_contract.context),
        nested.fork.combine(modifier_contract.fork),
    )
}

fn condition_contract(condition: &Condition) -> CommandContract {
    match condition {
        Condition::Function(_) => CommandContract::new(
            EffectSummary::Unknown,
            ContextSummary::Unknown,
            ForkClass::Unknown,
        ),
        Condition::ScoreMatches(score, _) => known_condition_contract(
            EffectCategories::SCORE_READ.union(single_holder_effects(score)),
            single_holder_context(score),
        ),
        Condition::ScoreCompare(left, _, right) => known_condition_contract(
            EffectCategories::SCORE_READ
                .union(single_holder_effects(left))
                .union(single_holder_effects(right)),
            single_holder_context(left).union(single_holder_context(right)),
        ),
        Condition::DataExists(_) => {
            known_condition_contract(EffectCategories::STORAGE_READ, ContextMask::NONE)
        }
        Condition::EntityExists(selector) => {
            known_condition_contract(EffectCategories::ENTITY_QUERY, selector.context_reads())
        }
    }
}

const fn known_condition_contract(
    effects: EffectCategories,
    reads: ContextMask,
) -> CommandContract {
    CommandContract::new(
        EffectSummary::Known(effects),
        ContextSummary::Known {
            reads,
            changes: ContextMask::NONE,
        },
        ForkClass::Never,
    )
}

fn store_contract(destination: &StoreDestination) -> CommandContract {
    let (effects, reads) = match destination {
        StoreDestination::Score(score) => (
            EffectCategories::SCORE_WRITE.union(single_holder_effects(score)),
            single_holder_context(score),
        ),
        StoreDestination::Storage { .. } => (EffectCategories::STORAGE_WRITE, ContextMask::NONE),
    };
    CommandContract::new(
        EffectSummary::Known(effects),
        ContextSummary::Known {
            reads,
            changes: ContextMask::NONE,
        },
        ForkClass::Never,
    )
}

const fn selector_fork(selector: Selector) -> ForkClass {
    match selector.cardinality() {
        super::Cardinality::AtMostOne => ForkClass::AtMostOne,
        super::Cardinality::Unbounded => ForkClass::Unbounded,
    }
}

fn holder_effects(holders: &ScoreHolders) -> EffectCategories {
    match holders {
        ScoreHolders::Selector(_) => EffectCategories::ENTITY_QUERY,
        ScoreHolders::Fake(_) | ScoreHolders::AllTracked => EffectCategories::NONE,
    }
}

fn holder_context(holders: &ScoreHolders) -> ContextMask {
    match holders {
        ScoreHolders::Selector(selector) => selector.context_reads(),
        ScoreHolders::Fake(_) | ScoreHolders::AllTracked => ContextMask::NONE,
    }
}

fn single_holder_effects(score: &ScoreRef) -> EffectCategories {
    match score.holder() {
        SingleScoreHolder::Selector(_) => EffectCategories::ENTITY_QUERY,
        SingleScoreHolder::Fake(_) => EffectCategories::NONE,
    }
}

fn single_holder_context(score: &ScoreRef) -> ContextMask {
    match score.holder() {
        SingleScoreHolder::Selector(selector) => selector.context_reads(),
        SingleScoreHolder::Fake(_) => ContextMask::NONE,
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextMask, ContextSummary, EffectCategories, EffectSummary, ForkClass};
    use crate::entity::EntityId;
    use crate::ir::minecraft::{
        AtMostOneSelector, CommandKind, CommandNode, Condition, ExecuteCommand, ExecuteModifier,
        ExecuteModifierKind, ExecuteModifiers, FunctionCall, McFunctionId, ObjectiveName,
        ScoreCommand, ScoreHolders, ScoreSelection, Selector, UnboundedSelector,
    };
    use crate::source::OriginId;

    #[test]
    fn context_masks_expose_named_set_operations_only() {
        let spatial = ContextMask::POSITION.union(ContextMask::DIMENSION);

        assert!(spatial.contains(ContextMask::POSITION));
        assert!(spatial.contains(ContextMask::DIMENSION));
        assert!(!spatial.contains(ContextMask::ROTATION));
        assert!(spatial.intersects(ContextMask::DIMENSION));
        assert!(!spatial.intersects(ContextMask::EXECUTOR));
        assert!(ContextMask::NONE.is_empty());
    }

    fn raw_leaf() -> CommandNode {
        CommandNode::new(
            CommandKind::Raw(crate::ir::minecraft::UnsafeRawCommand::new("say unknown").unwrap()),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    fn known_leaf() -> CommandNode {
        let target = ScoreSelection::new(
            ScoreHolders::from(Selector::from(UnboundedSelector::AllPlayers)),
            ObjectiveName::new("mdl.reg").unwrap(),
        );
        CommandNode::new(
            CommandKind::Score(ScoreCommand::PlayersSet { target, value: 1 }),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    #[test]
    fn native_bulk_score_selection_queries_entities_but_never_forks() {
        let contract = known_leaf().contract();
        assert_eq!(contract.fork(), ForkClass::Never);
        assert!(matches!(
            contract.effects(),
            EffectSummary::Known(effects)
                if effects.contains(EffectCategories::SCORE_WRITE)
                    && effects.contains(EffectCategories::ENTITY_QUERY)
        ));
    }

    #[test]
    fn entity_predicate_does_not_inherit_selector_fanout() {
        let modifier = ExecuteModifier::new(
            ExecuteModifierKind::If(Condition::EntityExists(Selector::from(
                UnboundedSelector::AllEntities,
            ))),
            OriginId::UNKNOWN,
        );
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(modifier, vec![]),
                known_leaf(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        assert_eq!(command.contract().fork(), ForkClass::Never);
    }

    #[test]
    fn internal_function_predicate_is_an_unknown_barrier() {
        let function = <McFunctionId as EntityId>::from_index(7);
        let modifier = ExecuteModifier::new(
            ExecuteModifierKind::If(Condition::Function(function)),
            OriginId::UNKNOWN,
        );
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(modifier, vec![]),
                known_leaf(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let contract = command.contract();

        assert_eq!(contract.effects(), EffectSummary::Unknown);
        assert_eq!(contract.context(), ContextSummary::Unknown);
        assert_eq!(contract.fork(), ForkClass::Unknown);
    }

    #[test]
    fn as_and_at_fanout_are_role_sensitive_and_contextual() {
        let as_all = ExecuteModifier::new(
            ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
            OriginId::UNKNOWN,
        );
        let at_self = ExecuteModifier::new(
            ExecuteModifierKind::At(AtMostOneSelector::SelfExecutor.into()),
            OriginId::UNKNOWN,
        );
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(as_all, vec![at_self]),
                known_leaf(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let contract = command.contract();
        assert_eq!(contract.fork(), ForkClass::Unbounded);
        assert!(matches!(
            contract.context(),
            ContextSummary::Known { reads, changes }
                if reads.intersects(ContextMask::DIMENSION)
                    && changes.contains(ContextMask::EXECUTOR)
                    && changes.contains(ContextMask::POSITION)
                    && changes.contains(ContextMask::ROTATION)
                    && changes.contains(ContextMask::DIMENSION)
        ));
    }

    #[test]
    fn raw_and_calls_are_unknown_barriers() {
        assert_eq!(raw_leaf().contract().effects(), EffectSummary::Unknown);
        let call = CommandNode::new(
            CommandKind::Function(FunctionCall::new(
                crate::ir::minecraft::ExternalCallableRef::Function(
                    crate::ir::minecraft::FunctionResourceId::parse("mdl:external").unwrap(),
                )
                .into(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        assert_eq!(call.contract().fork(), ForkClass::Unknown);
        assert_eq!(call.contract().context(), ContextSummary::Unknown);
    }
}
