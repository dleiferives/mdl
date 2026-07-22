//! Closed source-semantic identities that do not imply a Core representation.

use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;

mod behavior;
mod context;
mod message;
mod minecraft;
mod query;
mod spatial;

pub use behavior::{
    AmbientContextRequirements, ContextRequirement, ForkBound, FunctionBehavior, ObservableEffect,
    TransitiveWork, WorldEffect,
};
pub use context::{ContextFact, ExecutionContext};
pub use message::{MessageLiteral, MessageLiteralError};
pub use minecraft::{
    MinecraftAmbientContextRule, MinecraftAttributeKind, MinecraftCommandOutcomeBehavior,
    MinecraftSemanticDescriptor, MinecraftSemanticKey, MinecraftSemanticSignature,
    MinecraftSourceMethodRule, MinecraftValidatorKind, SourceReceiverRule, minecraft_descriptor,
    minecraft_source_methods, resolve_minecraft_method,
};
pub use query::{EntityTag, EntityTagError, StaticEntityQuery};
pub use spatial::{
    Axes, BlockPosition, DimensionKey, EntityAnchor, FiniteDecimal, FiniteDecimalError,
    LocalPosition, MAX_FINITE_DECIMAL_BYTES, MAX_FINITE_DECIMAL_DIGITS, PositionSpec,
    RelativeWorldOffset, RotationAxis, RotationSpec, WorldAxis, WorldPosition,
};

/// Maximum ordered execution-context modifiers accepted in one source/Core scope.
pub const MAX_RUN_MODIFIERS_PER_SCOPE: usize = 256;

/// Maximum ordered execution-context modifier occurrences accepted in one package.
pub const MAX_PACKAGE_RUN_MODIFIERS: usize = 100_000;

/// An ordinary runtime scalar type shared by source semantics and Core lowering.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeValueType {
    /// A Boolean value.
    Bool,
    /// A signed 32-bit integer value.
    Int32,
    /// A Java-compatible immutable runtime string.
    String,
}

impl fmt::Display for RuntimeValueType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bool => "Bool",
            Self::Int32 => "Int32",
            Self::String => "String",
        })
    }
}

/// A nominal Minecraft entity kind understood by the source language.
///
/// Kinds are semantic identities. Two kinds may eventually use the same target
/// representation without becoming interchangeable source types.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntityKind {
    /// A Minecraft armor stand.
    ArmorStand,
}

impl EntityKind {
    /// Resolves one reserved source spelling to its nominal semantic identity.
    #[must_use]
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "ArmorStand" => Some(Self::ArmorStand),
            _ => None,
        }
    }

    /// Returns the reserved source spelling for this nominal kind.
    #[must_use]
    pub const fn source_name(self) -> &'static str {
        match self {
            Self::ArmorStand => "ArmorStand",
        }
    }

    /// Returns the closed capabilities inferred for this nominal kind.
    #[must_use]
    pub const fn capabilities(self) -> EntityCapabilities {
        match self {
            Self::ArmorStand => EntityCapabilities::ARMOR_STAND,
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.source_name())
    }
}

/// A nominal Minecraft block-entity kind understood by the source language.
///
/// Deliberately a separate closed enum from [`EntityKind`], not a shared
/// variant space: block entities are never Minecraft command executors and
/// have no analogous capability set, so unifying the two would force
/// irrelevant entity concepts onto blocks. See
/// `notes/compiler/block-entity-nbt-paths.md` §2.2.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum BlockEntityKind {
    /// A Minecraft chest.
    Chest,
}

impl BlockEntityKind {
    /// Resolves one reserved source spelling to its nominal semantic identity.
    #[must_use]
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "Chest" => Some(Self::Chest),
            _ => None,
        }
    }

    /// Returns the reserved source spelling for this nominal kind.
    #[must_use]
    pub const fn source_name(self) -> &'static str {
        match self {
            Self::Chest => "Chest",
        }
    }
}

impl fmt::Display for BlockEntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.source_name())
    }
}

/// A source-visible semantic capability inferred for an entity kind.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EntityCapability {
    /// The entity kind can establish Minecraft's current command executor.
    CommandExecutor,
    /// The entity exposes a native inventory/hand item surface.
    InventoryHolder,
}

impl fmt::Display for EntityCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CommandExecutor => "CommandExecutor",
            Self::InventoryHolder => "InventoryHolder",
        })
    }
}

/// A compact closed set of inferred [`EntityCapability`] values.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityCapabilities(u8);

impl EntityCapabilities {
    const COMMAND_EXECUTOR_BIT: u8 = 1 << 0;
    const INVENTORY_HOLDER_BIT: u8 = 1 << 1;
    const ARMOR_STAND: Self = Self(Self::COMMAND_EXECUTOR_BIT | Self::INVENTORY_HOLDER_BIT);

    /// Returns whether the set contains `capability`.
    #[must_use]
    pub const fn contains(self, capability: EntityCapability) -> bool {
        let bit = match capability {
            EntityCapability::CommandExecutor => Self::COMMAND_EXECUTOR_BIT,
            EntityCapability::InventoryHolder => Self::INVENTORY_HOLDER_BIT,
        };
        self.0 & bit != 0
    }
}

/// Conservative static multiplicity of an entity query.
///
/// Construction keeps the representation canonical: a finite upper bound of one
/// is [`QueryCardinality::AT_MOST_ONE`], while `Bounded` always means two or more.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct QueryCardinality(QueryCardinalityKind);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum QueryCardinalityKind {
    ExactlyOne,
    AtMostOne,
    Bounded(NonZeroU32),
    Unbounded,
}

impl QueryCardinality {
    /// A query proven to select exactly one entity.
    pub const EXACTLY_ONE: Self = Self(QueryCardinalityKind::ExactlyOne);
    /// A query proven to select zero or one entity.
    pub const AT_MOST_ONE: Self = Self(QueryCardinalityKind::AtMostOne);
    /// A query with no proven finite upper bound.
    pub const UNBOUNDED: Self = Self(QueryCardinalityKind::Unbounded);

    /// Creates a canonical finite upper-bound fact.
    ///
    /// A bound of one becomes [`QueryCardinality::AT_MOST_ONE`].
    ///
    /// # Errors
    ///
    /// Returns [`QueryCardinalityError::ZeroBound`] for zero, because a statically
    /// empty selection is not an entity query cardinality.
    pub const fn bounded(maximum: u32) -> Result<Self, QueryCardinalityError> {
        let Some(maximum) = NonZeroU32::new(maximum) else {
            return Err(QueryCardinalityError::ZeroBound);
        };
        Ok(Self::bounded_nonzero(maximum))
    }

    pub(super) const fn bounded_nonzero(maximum: NonZeroU32) -> Self {
        if maximum.get() == 1 {
            Self::AT_MOST_ONE
        } else {
            Self(QueryCardinalityKind::Bounded(maximum))
        }
    }

    /// Returns the proven finite maximum, or `None` when no finite bound is known.
    #[must_use]
    pub const fn maximum(self) -> Option<u32> {
        match self.0 {
            QueryCardinalityKind::ExactlyOne | QueryCardinalityKind::AtMostOne => Some(1),
            QueryCardinalityKind::Bounded(maximum) => Some(maximum.get()),
            QueryCardinalityKind::Unbounded => None,
        }
    }

    /// Returns whether the fact permits an empty query result.
    #[must_use]
    pub const fn may_be_empty(self) -> bool {
        !matches!(self.0, QueryCardinalityKind::ExactlyOne)
    }

    /// Returns whether the query can invoke scalar work at most once.
    #[must_use]
    pub const fn is_at_most_one(self) -> bool {
        matches!(
            self.0,
            QueryCardinalityKind::ExactlyOne | QueryCardinalityKind::AtMostOne
        )
    }

    /// Applies a positive query `limit`, preserving stronger existing knowledge.
    ///
    /// # Errors
    ///
    /// Returns [`QueryCardinalityError::ZeroBound`] for a zero limit.
    pub const fn limit(self, maximum: u32) -> Result<Self, QueryCardinalityError> {
        let requested = match Self::bounded(maximum) {
            Ok(requested) => requested,
            Err(error) => return Err(error),
        };
        match self.0 {
            QueryCardinalityKind::ExactlyOne | QueryCardinalityKind::AtMostOne => Ok(self),
            QueryCardinalityKind::Bounded(current) => {
                let requested = match requested.0 {
                    QueryCardinalityKind::ExactlyOne | QueryCardinalityKind::AtMostOne => 1,
                    QueryCardinalityKind::Bounded(requested) => requested.get(),
                    QueryCardinalityKind::Unbounded => u32::MAX,
                };
                Self::bounded(if current.get() < requested {
                    current.get()
                } else {
                    requested
                })
            }
            QueryCardinalityKind::Unbounded => Ok(requested),
        }
    }

    /// Returns whether `self` may be implicitly forgotten to the weaker `target` fact.
    #[must_use]
    pub const fn can_weaken_to(self, target: Self) -> bool {
        match target.0 {
            QueryCardinalityKind::Unbounded => true,
            QueryCardinalityKind::ExactlyOne => {
                matches!(self.0, QueryCardinalityKind::ExactlyOne)
            }
            QueryCardinalityKind::AtMostOne => matches!(
                self.0,
                QueryCardinalityKind::ExactlyOne | QueryCardinalityKind::AtMostOne
            ),
            QueryCardinalityKind::Bounded(target) => {
                let source = match self.0 {
                    QueryCardinalityKind::ExactlyOne | QueryCardinalityKind::AtMostOne => 1,
                    QueryCardinalityKind::Bounded(source) => source.get(),
                    QueryCardinalityKind::Unbounded => return false,
                };
                source <= target.get()
            }
        }
    }
}

impl fmt::Display for QueryCardinality {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            QueryCardinalityKind::ExactlyOne => formatter.write_str("ExactlyOne"),
            QueryCardinalityKind::AtMostOne => formatter.write_str("AtMostOne"),
            QueryCardinalityKind::Bounded(maximum) => {
                write!(formatter, "Bounded({maximum})")
            }
            QueryCardinalityKind::Unbounded => formatter.write_str("Unbounded"),
        }
    }
}

/// Conservative invocation interval for one structured contextual region.
///
/// This is distinct from entity-query cardinality: filters and failed redirects
/// can make a region execute exactly zero times, while ordered modifier chains
/// compose multiple selection bounds multiplicatively.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct InvocationBounds {
    lower: u64,
    upper: Option<u64>,
}

impl InvocationBounds {
    /// One invocation for every incoming context.
    pub const EXACTLY_ONCE: Self = Self {
        lower: 1,
        upper: Some(1),
    };

    /// Creates a closed interval after checking its ordering.
    #[must_use]
    pub const fn new(lower: u64, upper: Option<u64>) -> Option<Self> {
        match upper {
            Some(upper) if lower > upper => None,
            _ => Some(Self { lower, upper }),
        }
    }

    /// Converts one entity selection fact into per-input invocation bounds.
    #[must_use]
    pub const fn from_query_cardinality(cardinality: QueryCardinality) -> Self {
        Self {
            lower: if cardinality.may_be_empty() { 0 } else { 1 },
            upper: match cardinality.maximum() {
                Some(maximum) => Some(maximum as u64),
                None => None,
            },
        }
    }

    /// Returns the proven minimum invocation count.
    #[must_use]
    pub const fn lower(self) -> u64 {
        self.lower
    }

    /// Returns the proven finite maximum, or `None` when no finite bound is known.
    #[must_use]
    pub const fn upper(self) -> Option<u64> {
        self.upper
    }

    /// Returns whether the region invokes its body no more than once.
    #[must_use]
    pub const fn is_at_most_once(self) -> bool {
        matches!(self.upper, Some(0 | 1))
    }

    /// Sequentially composes two context multipliers.
    ///
    /// Overflow of a finite upper product loses the finite proof rather than
    /// wrapping. A saturated lower bound remains conservative: it still states a
    /// count that every execution meeting the stronger mathematical bound exceeds.
    #[must_use]
    pub const fn multiply(self, next: Self) -> Self {
        let upper = match (self.upper, next.upper) {
            (Some(left), Some(right)) => left.checked_mul(right),
            (None, _) | (_, None) => None,
        };
        Self {
            lower: self.lower.saturating_mul(next.lower),
            upper,
        }
    }
}

impl fmt::Display for InvocationBounds {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.upper {
            Some(upper) => write!(formatter, "{}..={upper}", self.lower),
            None => write!(formatter, "{}..", self.lower),
        }
    }
}

/// Invalid construction of a semantic query-cardinality fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryCardinalityError {
    /// A query limit or bound was zero.
    ZeroBound,
}

impl fmt::Display for QueryCardinalityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroBound => formatter.write_str("entity-query bounds must be positive"),
        }
    }
}

impl Error for QueryCardinalityError {}

/// The type of one exact semantic entity reference.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityRefType {
    kind: EntityKind,
}

impl EntityRefType {
    /// Creates a reference type for `kind`.
    #[must_use]
    pub const fn new(kind: EntityKind) -> Self {
        Self { kind }
    }

    /// Returns the nominal entity kind.
    #[must_use]
    pub const fn kind(self) -> EntityKind {
        self.kind
    }
}

/// The type of a compiler-known entity query plan.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityQueryType {
    kind: EntityKind,
    cardinality: QueryCardinality,
}

impl EntityQueryType {
    /// Creates a query type with independent nominal-kind and cardinality facts.
    #[must_use]
    pub const fn new(kind: EntityKind, cardinality: QueryCardinality) -> Self {
        Self { kind, cardinality }
    }

    /// Returns the selected nominal entity kind.
    #[must_use]
    pub const fn kind(self) -> EntityKind {
        self.kind
    }

    /// Returns the query's conservative cardinality fact.
    #[must_use]
    pub const fn cardinality(self) -> QueryCardinality {
        self.cardinality
    }

    /// Refines this type through a positive static query limit.
    ///
    /// # Errors
    ///
    /// Returns [`QueryCardinalityError::ZeroBound`] for a zero limit.
    pub const fn limit(self, maximum: u32) -> Result<Self, QueryCardinalityError> {
        let cardinality = match self.cardinality.limit(maximum) {
            Ok(cardinality) => cardinality,
            Err(error) => return Err(error),
        };
        Ok(Self::new(self.kind, cardinality))
    }

    /// Returns whether this type can be implicitly weakened to `target`.
    #[must_use]
    pub fn can_weaken_to(self, target: Self) -> bool {
        self.kind == target.kind && self.cardinality.can_weaken_to(target.cardinality)
    }
}

/// A non-escaping proof of Minecraft's current executor within one run invocation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExecutorType {
    kind: EntityKind,
}

impl ExecutorType {
    /// Creates an executor capability for `kind`.
    #[must_use]
    pub const fn new(kind: EntityKind) -> Self {
        Self { kind }
    }

    /// Returns the nominal kind proven for the current executor.
    #[must_use]
    pub const fn kind(self) -> EntityKind {
        self.kind
    }
}

/// Minecraft command-outcome meanings kept distinct from ordinary integers and booleans.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommandOutcomeType {
    /// Whether a command succeeded according to its Minecraft contract.
    Success,
    /// The integer result produced by a command according to its Minecraft contract.
    Result,
}

/// A complete closed source-semantic type key for the Stage 7 slice.
///
/// Most variants are compiler-known values with no general Core/scoreboard
/// representation. Only [`SemanticType::Runtime`] can enter ordinary scalar Core.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SemanticType {
    /// An ordinary runtime scalar value.
    Runtime(RuntimeValueType),
    /// One exact entity reference; no general Stage 7 physical representation exists.
    EntityRef(EntityRefType),
    /// A compiler-known static query plan and its cardinality.
    EntityQuery(EntityQueryType),
    /// A lexical proof of the current executor, not a stored entity handle.
    Executor(ExecutorType),
    /// A compile-time literal accepted by Minecraft's `message` command argument.
    MessageLiteral,
    /// A typed Minecraft command outcome.
    CommandOutcome(CommandOutcomeType),
}

impl SemanticType {
    /// Returns the ordinary runtime type, if this semantic value is representable in Core.
    #[must_use]
    pub const fn runtime(self) -> Option<RuntimeValueType> {
        match self {
            Self::Runtime(ty) => Some(ty),
            Self::EntityRef(_)
            | Self::EntityQuery(_)
            | Self::Executor(_)
            | Self::MessageLiteral
            | Self::CommandOutcome(_) => None,
        }
    }
}

impl fmt::Display for SemanticType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(ty) => ty.fmt(formatter),
            Self::EntityRef(ty) => write!(formatter, "EntityRef<{}>", ty.kind()),
            Self::EntityQuery(ty) => write!(
                formatter,
                "EntityQuery<{}, {}>",
                ty.kind(),
                ty.cardinality()
            ),
            Self::Executor(ty) => write!(formatter, "Executor<{}>", ty.kind()),
            Self::MessageLiteral => formatter.write_str("MessageLiteral"),
            Self::CommandOutcome(CommandOutcomeType::Success) => {
                formatter.write_str("CommandSuccess")
            }
            Self::CommandOutcome(CommandOutcomeType::Result) => {
                formatter.write_str("CommandResult")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeValueType;
    use super::{
        CommandOutcomeType, EntityCapability, EntityKind, EntityQueryType, EntityRefType,
        ExecutorType, InvocationBounds, QueryCardinality, QueryCardinalityError, SemanticType,
    };

    #[test]
    fn armor_stand_exposes_only_the_first_slice_capability() {
        let capabilities = EntityKind::ArmorStand.capabilities();
        assert!(capabilities.contains(EntityCapability::CommandExecutor));
    }

    #[test]
    fn query_bounds_are_canonical_and_refine_monotonically() {
        assert_eq!(
            QueryCardinality::bounded(0),
            Err(QueryCardinalityError::ZeroBound)
        );
        assert_eq!(
            QueryCardinality::bounded(1).unwrap(),
            QueryCardinality::AT_MOST_ONE
        );
        assert_eq!(
            QueryCardinality::UNBOUNDED.limit(10).unwrap().maximum(),
            Some(10)
        );
        assert_eq!(
            QueryCardinality::UNBOUNDED.limit(1).unwrap(),
            QueryCardinality::AT_MOST_ONE
        );
        assert_eq!(
            QueryCardinality::bounded(10).unwrap().limit(20).unwrap(),
            QueryCardinality::bounded(10).unwrap()
        );
        assert_eq!(
            QueryCardinality::EXACTLY_ONE.limit(1).unwrap(),
            QueryCardinality::EXACTLY_ONE
        );
    }

    #[test]
    fn weakening_never_strengthens_absence_or_fan_out_facts() {
        let two = QueryCardinality::bounded(2).unwrap();
        let ten = QueryCardinality::bounded(10).unwrap();
        assert!(QueryCardinality::EXACTLY_ONE.can_weaken_to(QueryCardinality::AT_MOST_ONE));
        assert!(QueryCardinality::AT_MOST_ONE.can_weaken_to(ten));
        assert!(two.can_weaken_to(ten));
        assert!(ten.can_weaken_to(QueryCardinality::UNBOUNDED));
        assert!(!QueryCardinality::AT_MOST_ONE.can_weaken_to(QueryCardinality::EXACTLY_ONE));
        assert!(!ten.can_weaken_to(two));
        assert!(!QueryCardinality::UNBOUNDED.can_weaken_to(ten));
    }

    #[test]
    fn invocation_bounds_are_distinct_and_compose_without_wrapping() {
        let optional = InvocationBounds::from_query_cardinality(QueryCardinality::AT_MOST_ONE);
        let bounded =
            InvocationBounds::from_query_cardinality(QueryCardinality::bounded(3).unwrap());
        assert_eq!(optional.lower(), 0);
        assert_eq!(optional.upper(), Some(1));
        assert!(optional.is_at_most_once());
        assert_eq!(optional.multiply(bounded).upper(), Some(3));

        let huge = InvocationBounds::new(0, Some(u64::MAX)).unwrap();
        assert_eq!(huge.multiply(bounded).upper(), None);
        assert!(!huge.multiply(bounded).is_at_most_once());
    }

    #[test]
    fn semantic_keys_keep_nominal_cardinality_and_representation_axes_separate() {
        let kind = EntityKind::ArmorStand;
        let query = EntityQueryType::new(kind, QueryCardinality::UNBOUNDED)
            .limit(1)
            .unwrap();
        assert_eq!(query.kind(), kind);
        assert_eq!(query.cardinality(), QueryCardinality::AT_MOST_ONE);
        assert_eq!(
            SemanticType::EntityQuery(query).to_string(),
            "EntityQuery<ArmorStand, AtMostOne>"
        );
        assert_eq!(
            SemanticType::EntityRef(EntityRefType::new(kind)).runtime(),
            None
        );
        assert_eq!(
            SemanticType::Executor(ExecutorType::new(kind)).runtime(),
            None
        );
        assert_eq!(
            SemanticType::CommandOutcome(CommandOutcomeType::Result).runtime(),
            None
        );
        assert_eq!(
            SemanticType::Runtime(RuntimeValueType::Int32).runtime(),
            Some(RuntimeValueType::Int32)
        );
    }
}
