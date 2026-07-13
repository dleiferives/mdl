use crate::source::OriginId;

use super::{
    CommandNode, DimensionId, FiniteF64, McFunctionId, ScoreRange, ScoreRef, Selector, StoragePath,
};

/// One ordered `execute` command and its nested command.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecuteCommand {
    modifiers: ExecuteModifiers,
    run: Box<CommandNode>,
}

impl ExecuteCommand {
    /// Constructs an execute chain from a proven-nonempty modifier sequence.
    #[must_use]
    pub fn new(modifiers: ExecuteModifiers, run: CommandNode) -> Self {
        Self {
            modifiers,
            run: Box::new(run),
        }
    }

    /// Returns modifiers in target order.
    #[must_use]
    pub const fn modifiers(&self) -> &ExecuteModifiers {
        &self.modifiers
    }

    /// Returns the nested command.
    #[must_use]
    pub const fn run(&self) -> &CommandNode {
        &self.run
    }
}

/// A proven-nonempty ordered sequence of execute modifiers.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecuteModifiers(Box<[ExecuteModifier]>);

impl ExecuteModifiers {
    #[cfg(test)]
    pub(super) fn from_unchecked(modifiers: Vec<ExecuteModifier>) -> Self {
        Self(modifiers.into_boxed_slice())
    }

    /// Constructs a sequence from a required first modifier and ordered remainder.
    #[must_use]
    pub fn new(first: ExecuteModifier, remainder: Vec<ExecuteModifier>) -> Self {
        let mut modifiers = Vec::with_capacity(remainder.len() + 1);
        modifiers.push(first);
        modifiers.extend(remainder);
        Self(modifiers.into_boxed_slice())
    }

    /// Returns modifiers in target order.
    #[must_use]
    pub fn as_slice(&self) -> &[ExecuteModifier] {
        &self.0
    }

    /// Returns the nonzero number of modifiers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the sequence is empty.
    ///
    /// This is always false for safely constructed values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One execute modifier with local provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecuteModifier {
    kind: ExecuteModifierKind,
    origin: OriginId,
}

impl ExecuteModifier {
    /// Constructs a modifier with provenance.
    #[must_use]
    pub const fn new(kind: ExecuteModifierKind, origin: OriginId) -> Self {
        Self { kind, origin }
    }

    /// Returns the modifier semantics.
    #[must_use]
    pub const fn kind(&self) -> &ExecuteModifierKind {
        &self.kind
    }

    /// Returns the modifier provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// The closed initial execute-modifier vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub enum ExecuteModifierKind {
    /// Change executor for every selected entity.
    As(Selector),
    /// Change position/rotation/dimension for every selected entity.
    At(Selector),
    /// Change dimension and rescale position.
    In(DimensionId),
    /// Keep contexts satisfying a condition.
    If(Condition),
    /// Keep contexts not satisfying a condition.
    Unless(Condition),
    /// Store the nested command outcome.
    Store(StoreChannel, StoreDestination),
}

/// A closed execute predicate.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    /// Match one score against an inclusive integer range.
    ScoreMatches(ScoreRef, ScoreRange),
    /// Compare two scalar scores.
    ScoreCompare(ScoreRef, ScoreComparison, ScoreRef),
    /// Test whether a static storage path exists.
    DataExists(StoragePath),
    /// Test whether a selector resolves to at least one entity without forking.
    EntityExists(Selector),
    /// Run one internal function and match when at least one invocation returns nonzero.
    Function(McFunctionId),
}

/// One native `execute if score` comparison operator.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScoreComparison {
    /// `=`
    Equal,
    /// `<`
    LessThan,
    /// `<=`
    LessOrEqual,
    /// `>`
    GreaterThan,
    /// `>=`
    GreaterOrEqual,
}

/// Which nested-command outcome is stored.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StoreChannel {
    /// Store the integer result.
    Result,
    /// Store success as zero or one.
    Success,
}

/// Numeric NBT tag type accepted by `execute store ... storage`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StorageNumericType {
    /// Byte tag.
    Byte,
    /// Short tag.
    Short,
    /// Integer tag.
    Int,
    /// Long tag.
    Long,
    /// Float tag.
    Float,
    /// Double tag.
    Double,
}

/// A typed destination for one command outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum StoreDestination {
    /// One scalar scoreboard value.
    Score(ScoreRef),
    /// One static storage path and numeric conversion.
    Storage {
        /// Static storage destination.
        target: StoragePath,
        /// NBT numeric tag type.
        numeric_type: StorageNumericType,
        /// Outcome multiplier.
        scale: FiniteF64,
    },
}

#[cfg(test)]
mod tests {
    use super::{
        Condition, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers, ScoreComparison,
        StorageNumericType, StoreChannel, StoreDestination,
    };
    use crate::ir::minecraft::{
        AtMostOneSelector, FakeScoreHolder, FiniteF64, NbtPath, NbtPathKey, NbtPathSegment,
        ObjectiveName, ScoreRef, SingleScoreHolder, StorageId, StoragePath,
    };
    use crate::source::OriginId;

    fn score(holder: &str) -> ScoreRef {
        ScoreRef::new(
            SingleScoreHolder::from(FakeScoreHolder::new(holder).unwrap()),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
    }

    #[test]
    fn execute_modifiers_are_nonempty_and_ordered() {
        let first = ExecuteModifier::new(
            ExecuteModifierKind::As(AtMostOneSelector::SelfExecutor.into()),
            OriginId::UNKNOWN,
        );
        let second = ExecuteModifier::new(
            ExecuteModifierKind::At(AtMostOneSelector::NearestPlayer.into()),
            OriginId::UNKNOWN,
        );
        let modifiers = ExecuteModifiers::new(first, vec![second]);

        assert_eq!(modifiers.len(), 2);
        assert!(!modifiers.is_empty());
        assert!(matches!(
            modifiers.as_slice()[0].kind(),
            ExecuteModifierKind::As(_)
        ));
        assert!(matches!(
            modifiers.as_slice()[1].kind(),
            ExecuteModifierKind::At(_)
        ));
    }

    #[test]
    fn score_comparisons_cover_exactly_the_five_target_operators() {
        let operators = [
            ScoreComparison::Equal,
            ScoreComparison::LessThan,
            ScoreComparison::LessOrEqual,
            ScoreComparison::GreaterThan,
            ScoreComparison::GreaterOrEqual,
        ];
        for operator in operators {
            let condition = Condition::ScoreCompare(score("#a"), operator, score("#b"));
            assert!(matches!(condition, Condition::ScoreCompare(_, found, _) if found == operator));
        }
    }

    #[test]
    fn result_success_and_storage_number_type_are_not_interchangeable() {
        let score_store = ExecuteModifierKind::Store(
            StoreChannel::Result,
            StoreDestination::Score(score("#result")),
        );
        assert!(matches!(
            score_store,
            ExecuteModifierKind::Store(StoreChannel::Result, StoreDestination::Score(_))
        ));

        let storage = StoragePath::new(
            StorageId::parse("mdl:state").unwrap(),
            NbtPath::new(
                NbtPathSegment::Key(NbtPathKey::new("success").unwrap()),
                vec![],
            ),
        );
        let success_store = ExecuteModifierKind::Store(
            StoreChannel::Success,
            StoreDestination::Storage {
                target: storage,
                numeric_type: StorageNumericType::Byte,
                scale: FiniteF64::new(1.0).unwrap(),
            },
        );
        assert!(matches!(
            success_store,
            ExecuteModifierKind::Store(
                StoreChannel::Success,
                StoreDestination::Storage {
                    numeric_type: StorageNumericType::Byte,
                    ..
                }
            )
        ));
    }
}
