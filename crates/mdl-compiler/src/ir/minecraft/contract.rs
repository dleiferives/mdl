use std::num::NonZeroU64;

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
pub struct EffectCategories(u16);

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
    /// Produces externally observable player/server output.
    pub const OUTPUT: Self = Self(1 << 6);
    /// Mutates entity state such as position or dimension.
    pub const ENTITY_WRITE: Self = Self(1 << 7);
    /// Reads a block/block-entity at a fixed position (BE-1). Distinct from
    /// `ENTITY_QUERY`: a block position is never ambiguous/forking the way
    /// a selector predicate can be.
    pub const BLOCK_QUERY: Self = Self(1 << 8);
    /// Mutates per-player advancement state. Distinct from `ENTITY_WRITE`:
    /// advancement progress is not entity NBT/position state, and optimizer
    /// passes must not assume the two commute or alias the same way.
    pub const ADVANCEMENT_WRITE: Self = Self(1 << 9);

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
///
/// Like selector cardinality, this is privately represented so `Bounded(1)` cannot
/// coexist with the canonical [`ForkClass::AT_MOST_ONE`] fact.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ForkClass(ForkClassKind);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum ForkClassKind {
    Never,
    AtMostOne,
    Bounded(NonZeroU64),
    Unbounded,
    Unknown,
}

impl ForkClass {
    /// No modifier can multiply one incoming context.
    pub const NEVER: Self = Self(ForkClassKind::Never);
    /// A modifier can retain zero or one context per input.
    pub const AT_MOST_ONE: Self = Self(ForkClassKind::AtMostOne);
    /// A modifier may produce an unbounded number of contexts per input.
    pub const UNBOUNDED: Self = Self(ForkClassKind::Unbounded);
    /// Fan-out cannot be bounded by Stage 3.
    pub const UNKNOWN: Self = Self(ForkClassKind::Unknown);

    const fn bounded(maximum: u64) -> Self {
        let Some(maximum) = NonZeroU64::new(maximum) else {
            return Self::AT_MOST_ONE;
        };
        if maximum.get() == 1 {
            Self::AT_MOST_ONE
        } else {
            Self(ForkClassKind::Bounded(maximum))
        }
    }

    /// Returns the proven maximum output contexts per input when finite.
    ///
    /// Both `Never` and `AtMostOne` have a numeric upper bound of one; their
    /// distinct classes retain whether a modifier can filter the input context.
    #[must_use]
    pub const fn maximum_per_input(self) -> Option<u64> {
        match self.0 {
            ForkClassKind::Never | ForkClassKind::AtMostOne => Some(1),
            ForkClassKind::Bounded(maximum) => Some(maximum.get()),
            ForkClassKind::Unbounded | ForkClassKind::Unknown => None,
        }
    }

    const fn combine(self, other: Self) -> Self {
        match (self.0, other.0) {
            (ForkClassKind::Unknown, _) | (_, ForkClassKind::Unknown) => Self::UNKNOWN,
            (ForkClassKind::Unbounded, _) | (_, ForkClassKind::Unbounded) => Self::UNBOUNDED,
            (ForkClassKind::Bounded(left), ForkClassKind::Bounded(right)) => {
                match left.get().checked_mul(right.get()) {
                    Some(product) => Self::bounded(product),
                    None => Self::UNKNOWN,
                }
            }
            (ForkClassKind::Bounded(bound), ForkClassKind::Never | ForkClassKind::AtMostOne)
            | (ForkClassKind::Never | ForkClassKind::AtMostOne, ForkClassKind::Bounded(bound)) => {
                Self(ForkClassKind::Bounded(bound))
            }
            (ForkClassKind::AtMostOne, _) | (_, ForkClassKind::AtMostOne) => Self::AT_MOST_ONE,
            (ForkClassKind::Never, ForkClassKind::Never) => Self::NEVER,
        }
    }
}

/// Guaranteed native success/result behavior of one command invocation.
///
/// This is deliberately separate from source-language result types and from the
/// cost analyzer's control-flow outcomes. `Exact(value)` means the native command
/// succeeds and produces exactly `value`; `Unknown` makes no success or result
/// claim.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NativeCommandOutcome {
    /// Successful native completion with an exact integer result.
    Exact(i32),
    /// Native success or result is not modeled exactly.
    Unknown,
}

impl NativeCommandOutcome {
    /// Projects an exact successful native result, when one is known.
    #[must_use]
    pub const fn exact_result(self) -> Option<i32> {
        match self {
            Self::Exact(result) => Some(result),
            Self::Unknown => None,
        }
    }
}

/// Conservative facts derived from one command's syntax.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CommandContract {
    effects: EffectSummary,
    context: ContextSummary,
    fork: ForkClass,
    native_outcome: NativeCommandOutcome,
}

impl CommandContract {
    const fn new(effects: EffectSummary, context: ContextSummary, fork: ForkClass) -> Self {
        Self {
            effects,
            context,
            fork,
            native_outcome: NativeCommandOutcome::Unknown,
        }
    }

    const fn with_native_outcome(mut self, native_outcome: NativeCommandOutcome) -> Self {
        self.native_outcome = native_outcome;
        self
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

    /// Returns guaranteed native success/result behavior.
    #[must_use]
    pub const fn native_outcome(self) -> NativeCommandOutcome {
        self.native_outcome
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
        self.kind().contract()
    }
}

impl CommandKind {
    /// Derives conservative command facts directly from immutable syntax.
    #[must_use]
    pub fn contract(&self) -> CommandContract {
        contract_for_kind(self)
    }
}

fn contract_for_kind(kind: &CommandKind) -> CommandContract {
    match kind {
        CommandKind::Score(command) => score_contract(command),
        CommandKind::Data(command) => data_contract(command),
        CommandKind::Say(_) => CommandContract::new(
            EffectSummary::Known(EffectCategories::OUTPUT),
            ContextSummary::Known {
                reads: ContextMask::EXECUTOR,
                changes: ContextMask::NONE,
            },
            ForkClass::NEVER,
        )
        .with_native_outcome(NativeCommandOutcome::Exact(1)),
        CommandKind::Teleport(command) => {
            let reads = teleport_context_reads(command.destination());
            CommandContract::new(
                EffectSummary::Known(EffectCategories::ENTITY_WRITE),
                ContextSummary::Known {
                    reads,
                    changes: ContextMask::NONE,
                },
                ForkClass::NEVER,
            )
        }
        CommandKind::Execute(command) => {
            let mut contract = command.run().contract();
            for modifier in command.modifiers().as_slice() {
                contract = compose_modifier(contract, modifier.kind());
            }
            contract
        }
        CommandKind::Function(_)
        | CommandKind::Raw(_)
        | CommandKind::Macro(_)
        | CommandKind::FunctionWithStorage(_) => CommandContract::new(
            EffectSummary::Unknown,
            ContextSummary::Unknown,
            ForkClass::UNKNOWN,
        ),
        // Native outcome (the exact success count `advancement revoke` returns)
        // is deliberately left at the `new` default of `Unknown` — it has not
        // been measured against the pinned server yet, unlike every other fact
        // here, which the compiler knows precisely because it only ever
        // generates this exact self/only shape.
        CommandKind::AdvancementRevoke(_) => CommandContract::new(
            EffectSummary::Known(EffectCategories::ADVANCEMENT_WRITE),
            ContextSummary::Known {
                reads: ContextMask::EXECUTOR,
                changes: ContextMask::NONE,
            },
            ForkClass::NEVER,
        ),
        CommandKind::Return(ReturnCommand::Value(_) | ReturnCommand::Fail) => CommandContract::new(
            EffectSummary::Known(EffectCategories::CONTROL),
            ContextSummary::NONE,
            ForkClass::NEVER,
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
        ForkClass::NEVER,
    )
}

fn data_contract(command: &DataCommand) -> CommandContract {
    let (effects, reads) = match command {
        DataCommand::Get { .. } => (EffectCategories::STORAGE_READ, ContextMask::NONE),
        DataCommand::Remove { .. }
        | DataCommand::Modify {
            source: DataSource::Value(_),
            ..
        } => (EffectCategories::STORAGE_WRITE, ContextMask::NONE),
        DataCommand::Modify {
            source: DataSource::From(_),
            ..
        }
        | DataCommand::Modify {
            source: DataSource::StringSlice { .. },
            ..
        } => (
            EffectCategories::STORAGE_READ.union(EffectCategories::STORAGE_WRITE),
            ContextMask::NONE,
        ),
        DataCommand::Modify {
            source: DataSource::Entity { selector, .. },
            ..
        } => (
            EffectCategories::ENTITY_QUERY.union(EffectCategories::STORAGE_WRITE),
            selector.context_reads(),
        ),
        DataCommand::Modify {
            source: DataSource::Block { .. },
            ..
        } => (
            EffectCategories::BLOCK_QUERY.union(EffectCategories::STORAGE_WRITE),
            ContextMask::NONE,
        ),
    };
    CommandContract::new(
        EffectSummary::Known(effects),
        ContextSummary::Known {
            reads,
            changes: ContextMask::NONE,
        },
        ForkClass::NEVER,
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
            selector_fork(selector),
        ),
        ExecuteModifierKind::At(selector) => CommandContract::new(
            EffectSummary::Known(EffectCategories::ENTITY_QUERY),
            ContextSummary::Known {
                reads: selector.context_reads(),
                changes: ContextMask::POSITION
                    .union(ContextMask::ROTATION)
                    .union(ContextMask::DIMENSION),
            },
            selector_fork(selector),
        ),
        ExecuteModifierKind::In(_) => CommandContract::new(
            EffectSummary::Known(EffectCategories::NONE),
            ContextSummary::Known {
                reads: ContextMask::POSITION.union(ContextMask::DIMENSION),
                changes: ContextMask::POSITION.union(ContextMask::DIMENSION),
            },
            ForkClass::NEVER,
        ),
        ExecuteModifierKind::Positioned(position) => CommandContract::new(
            EffectSummary::Known(EffectCategories::NONE),
            ContextSummary::Known {
                reads: position_context_reads(position),
                changes: ContextMask::POSITION,
            },
            ForkClass::NEVER,
        ),
        ExecuteModifierKind::Rotated(rotation) => CommandContract::new(
            EffectSummary::Known(EffectCategories::NONE),
            ContextSummary::Known {
                reads: rotation_context_reads(rotation),
                changes: ContextMask::ROTATION,
            },
            ForkClass::NEVER,
        ),
        ExecuteModifierKind::Anchored(_) => CommandContract::new(
            EffectSummary::Known(EffectCategories::NONE),
            ContextSummary::Known {
                reads: ContextMask::NONE,
                changes: ContextMask::ANCHOR,
            },
            ForkClass::NEVER,
        ),
        ExecuteModifierKind::Align(_) => CommandContract::new(
            EffectSummary::Known(EffectCategories::NONE),
            ContextSummary::Known {
                reads: ContextMask::POSITION,
                changes: ContextMask::POSITION,
            },
            ForkClass::NEVER,
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

fn position_context_reads(position: &super::TargetPosition) -> ContextMask {
    match position {
        super::TargetPosition::World(position) => {
            let relative = [&position.x, &position.y, &position.z]
                .into_iter()
                .any(|axis| matches!(axis, super::TargetWorldAxis::Relative(_)));
            if relative {
                ContextMask::POSITION
            } else {
                ContextMask::NONE
            }
        }
        super::TargetPosition::Local(_) => ContextMask::POSITION
            .union(ContextMask::ROTATION)
            .union(ContextMask::ANCHOR),
    }
}

fn rotation_context_reads(rotation: &super::TargetRotation) -> ContextMask {
    if matches!(rotation.yaw, super::TargetRotationAxis::Relative(_))
        || matches!(rotation.pitch, super::TargetRotationAxis::Relative(_))
    {
        ContextMask::ROTATION
    } else {
        ContextMask::NONE
    }
}

fn teleport_context_reads(position: &super::TargetPosition) -> ContextMask {
    ContextMask::EXECUTOR
        .union(ContextMask::DIMENSION)
        .union(position_context_reads(position))
}

fn condition_contract(condition: &Condition) -> CommandContract {
    match condition {
        Condition::Function(_) => CommandContract::new(
            EffectSummary::Unknown,
            ContextSummary::Unknown,
            ForkClass::UNKNOWN,
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
        Condition::DataExists(_) | Condition::DataMatches(_, _) => {
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
        ForkClass::NEVER,
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
        ForkClass::NEVER,
    )
}

const fn selector_fork(selector: &Selector) -> ForkClass {
    match selector.cardinality().maximum() {
        Some(1) => ForkClass::AT_MOST_ONE,
        Some(maximum) => ForkClass::bounded(maximum as u64),
        None => ForkClass::UNBOUNDED,
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
    use super::{
        ContextMask, ContextSummary, EffectCategories, EffectSummary, ForkClass,
        NativeCommandOutcome,
    };
    use crate::entity::EntityId;
    use crate::ir::minecraft::{
        AtMostOneSelector, CommandKind, CommandNode, Condition, EntitySelector, ExecuteCommand,
        ExecuteModifier, ExecuteModifierKind, ExecuteModifiers, FunctionCall, McFunctionId,
        ObjectiveName, SayCommand, SayMessage, ScoreCommand, ScoreHolders, ScoreSelection,
        Selector, UnboundedSelector,
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
        assert_eq!(contract.fork(), ForkClass::NEVER);
        assert!(matches!(
            contract.effects(),
            EffectSummary::Known(effects)
                if effects.contains(EffectCategories::SCORE_WRITE)
                    && effects.contains(EffectCategories::ENTITY_QUERY)
        ));
    }

    #[test]
    fn say_contract_is_known_observable_executor_local_and_exact() {
        let kind = CommandKind::Say(SayCommand::new(SayMessage::new("hello").unwrap()));
        let contract = kind.contract();

        assert_eq!(contract.fork(), ForkClass::NEVER);
        assert_eq!(contract.native_outcome(), NativeCommandOutcome::Exact(1));
        assert_eq!(contract.native_outcome().exact_result(), Some(1));
        assert!(matches!(
            contract.effects(),
            EffectSummary::Known(effects) if effects == EffectCategories::OUTPUT
        ));
        assert!(matches!(
            contract.context(),
            ContextSummary::Known { reads, changes }
                if reads == ContextMask::EXECUTOR && changes == ContextMask::NONE
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
        assert_eq!(command.contract().fork(), ForkClass::NEVER);
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
        assert_eq!(contract.fork(), ForkClass::UNKNOWN);
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
        assert_eq!(contract.fork(), ForkClass::UNBOUNDED);
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
    fn bounded_selector_fanout_is_preserved_and_composed_multiplicatively() {
        let as_two = ExecuteModifier::new(
            ExecuteModifierKind::As(
                EntitySelector::armor_stands(vec!["first".into()], Some(2))
                    .unwrap()
                    .into(),
            ),
            OriginId::UNKNOWN,
        );
        let at_three = ExecuteModifier::new(
            ExecuteModifierKind::At(
                EntitySelector::armor_stands(vec!["second".into()], Some(3))
                    .unwrap()
                    .into(),
            ),
            OriginId::UNKNOWN,
        );
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(as_two, vec![at_three]),
                known_leaf(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();

        let fork = command.contract().fork();
        assert_eq!(fork.maximum_per_input(), Some(6));
        assert_ne!(fork, ForkClass::AT_MOST_ONE);
        assert_ne!(fork, ForkClass::UNBOUNDED);
    }

    #[test]
    fn fork_bound_overflow_becomes_unknown_instead_of_wrapping() {
        let overflow = ForkClass::bounded(u64::MAX).combine(ForkClass::bounded(2));

        assert_eq!(overflow, ForkClass::UNKNOWN);
        assert_eq!(overflow.maximum_per_input(), None);
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
        assert_eq!(call.contract().fork(), ForkClass::UNKNOWN);
        assert_eq!(call.contract().context(), ContextSummary::Unknown);
    }
}
