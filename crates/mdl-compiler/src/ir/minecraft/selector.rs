use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;

use super::ContextMask;

/// A statically known upper bound on selector results.
///
/// Construction keeps the representation canonical: a finite maximum of one is
/// [`Cardinality::AT_MOST_ONE`], while the private `Bounded` representation always
/// means two or more. Cardinality is derived from validated selector syntax rather
/// than constructed independently by target consumers.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Cardinality(CardinalityKind);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum CardinalityKind {
    AtMostOne,
    Bounded(NonZeroU32),
    Unbounded,
}

impl Cardinality {
    /// A selector proven to return zero or one entity.
    pub const AT_MOST_ONE: Self = Self(CardinalityKind::AtMostOne);
    /// A selector with no proven finite upper bound.
    pub const UNBOUNDED: Self = Self(CardinalityKind::Unbounded);

    const fn bounded_nonzero(maximum: NonZeroU32) -> Self {
        if maximum.get() == 1 {
            Self::AT_MOST_ONE
        } else {
            Self(CardinalityKind::Bounded(maximum))
        }
    }

    /// Returns the proven finite maximum, or `None` when no finite bound is known.
    #[must_use]
    pub const fn maximum(self) -> Option<u32> {
        match self.0 {
            CardinalityKind::AtMostOne => Some(1),
            CardinalityKind::Bounded(maximum) => Some(maximum.get()),
            CardinalityKind::Unbounded => None,
        }
    }

    /// Returns whether this selector is proven to return at most one entity.
    #[must_use]
    pub const fn is_at_most_one(self) -> bool {
        matches!(self.0, CardinalityKind::AtMostOne)
    }

    /// Returns whether no finite selector bound is known.
    #[must_use]
    pub const fn is_unbounded(self) -> bool {
        matches!(self.0, CardinalityKind::Unbounded)
    }
}

/// Nominal entity type encoded by the first structured `@e[...]` selector slice.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SelectedEntityKind {
    /// `minecraft:armor_stand`.
    ArmorStand,
    /// `minecraft:player`.
    Player,
}

/// One owned, validated structured entity selector.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EntitySelector {
    kind: SelectedEntityKind,
    tags: Box<[Box<str>]>,
    limit: Option<NonZeroU32>,
}

impl EntitySelector {
    /// Builds a structured selector selecting armor stands.
    ///
    /// # Errors
    ///
    /// Rejects empty or unsafe unquoted tags, a zero limit, and limits above
    /// Minecraft's signed 32-bit selector bound.
    pub fn armor_stands(
        tags: Vec<Box<str>>,
        limit: Option<u32>,
    ) -> Result<Self, EntitySelectorError> {
        Self::with_kind(SelectedEntityKind::ArmorStand, tags, limit)
    }

    /// Builds a structured selector selecting players.
    ///
    /// # Errors
    ///
    /// Rejects empty or unsafe unquoted tags, a zero limit, and limits above
    /// Minecraft's signed 32-bit selector bound.
    pub fn players(tags: Vec<Box<str>>, limit: Option<u32>) -> Result<Self, EntitySelectorError> {
        Self::with_kind(SelectedEntityKind::Player, tags, limit)
    }

    fn with_kind(
        kind: SelectedEntityKind,
        tags: Vec<Box<str>>,
        limit: Option<u32>,
    ) -> Result<Self, EntitySelectorError> {
        for (index, tag) in tags.iter().enumerate() {
            validate_tag(index, tag)?;
        }
        let limit = match limit {
            Some(0) => return Err(EntitySelectorError::ZeroLimit),
            Some(value) if value > i32::MAX as u32 => {
                return Err(EntitySelectorError::LimitOutOfRange(value));
            }
            Some(value) => NonZeroU32::new(value),
            None => None,
        };
        Ok(Self {
            kind,
            tags: tags.into_boxed_slice(),
            limit,
        })
    }

    /// Returns the selector's nominal entity kind.
    #[must_use]
    pub const fn kind(&self) -> SelectedEntityKind {
        self.kind
    }

    /// Returns validated tag filters in target order.
    #[must_use]
    pub fn tags(&self) -> &[Box<str>] {
        &self.tags
    }

    /// Returns the positive selector limit, if present.
    #[must_use]
    pub const fn limit(&self) -> Option<u32> {
        match self.limit {
            Some(limit) => Some(limit.get()),
            None => None,
        }
    }

    /// Returns the target-analysis cardinality class.
    #[must_use]
    pub const fn cardinality(&self) -> Cardinality {
        match self.limit {
            Some(limit) => Cardinality::bounded_nonzero(limit),
            None => Cardinality::UNBOUNDED,
        }
    }

    /// Returns context read while resolving `@e[...]`.
    #[must_use]
    pub const fn context_reads(&self) -> ContextMask {
        ContextMask::POSITION.union(ContextMask::DIMENSION)
    }
}

impl fmt::Display for EntitySelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("@e[type=")?;
        formatter.write_str(match self.kind {
            SelectedEntityKind::ArmorStand => "minecraft:armor_stand",
            SelectedEntityKind::Player => "minecraft:player",
        })?;
        for tag in &self.tags {
            write!(formatter, ",tag={tag}")?;
        }
        if let Some(limit) = self.limit {
            write!(formatter, ",limit={limit}")?;
        }
        formatter.write_str("]")
    }
}

/// Invalid construction of a structured entity selector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntitySelectorError {
    /// A selector tag was empty.
    EmptyTag {
        /// Zero-based tag position in selector order.
        index: usize,
    },
    /// A selector tag contained an unsafe unquoted character.
    InvalidTagCharacter {
        /// Zero-based tag position in selector order.
        index: usize,
        /// Invalid character.
        character: char,
    },
    /// A selector limit was zero.
    ZeroLimit,
    /// A selector limit exceeded Minecraft's signed 32-bit parser range.
    LimitOutOfRange(u32),
}

impl fmt::Display for EntitySelectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTag { index } => {
                write!(formatter, "selector tag {index} cannot be empty")
            }
            Self::InvalidTagCharacter { index, character } => {
                write!(
                    formatter,
                    "selector tag {index} contains invalid character {character:?}"
                )
            }
            Self::ZeroLimit => formatter.write_str("selector limit must be positive"),
            Self::LimitOutOfRange(limit) => {
                write!(formatter, "selector limit {limit} exceeds Int32")
            }
        }
    }
}

impl Error for EntitySelectorError {}

fn validate_tag(index: usize, tag: &str) -> Result<(), EntitySelectorError> {
    if tag.is_empty() {
        return Err(EntitySelectorError::EmptyTag { index });
    }
    for character in tag.chars() {
        if !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')) {
            return Err(EntitySelectorError::InvalidTagCharacter { index, character });
        }
    }
    Ok(())
}

/// A closed selector guaranteed to return at most one entity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AtMostOneSelector {
    /// The current executor (`@s`).
    SelfExecutor,
    /// The nearest player (`@p`).
    NearestPlayer,
    /// A random player (`@r`).
    RandomPlayer,
}

impl AtMostOneSelector {
    /// Returns the selector's static result bound.
    #[must_use]
    pub const fn cardinality(self) -> Cardinality {
        Cardinality::AT_MOST_ONE
    }

    /// Returns a conservative set of execution-context components read while
    /// resolving the selector.
    #[must_use]
    pub const fn context_reads(self) -> ContextMask {
        match self {
            Self::SelfExecutor => ContextMask::EXECUTOR,
            Self::NearestPlayer | Self::RandomPlayer => {
                ContextMask::POSITION.union(ContextMask::DIMENSION)
            }
        }
    }
}

impl fmt::Display for AtMostOneSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SelfExecutor => "@s",
            Self::NearestPlayer => "@p",
            Self::RandomPlayer => "@r",
        })
    }
}

/// A closed selector with no useful static result bound.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnboundedSelector {
    /// Every player (`@a`).
    AllPlayers,
    /// Every selected entity (`@e`).
    AllEntities,
}

impl UnboundedSelector {
    /// Returns the selector's static result bound.
    #[must_use]
    pub const fn cardinality(self) -> Cardinality {
        Cardinality::UNBOUNDED
    }

    /// Returns a conservative set of execution-context components read while
    /// resolving the selector.
    #[must_use]
    pub const fn context_reads(self) -> ContextMask {
        ContextMask::POSITION.union(ContextMask::DIMENSION)
    }
}

impl fmt::Display for UnboundedSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AllPlayers => "@a",
            Self::AllEntities => "@e",
        })
    }
}

/// Any selector in the closed Stage 3 subset.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Selector {
    /// A selector returning at most one entity.
    AtMostOne(AtMostOneSelector),
    /// A selector with no useful static upper bound.
    Unbounded(UnboundedSelector),
    /// A structured `@e[...]` selector with typed filters.
    Entity(EntitySelector),
}

impl Selector {
    /// Returns the selector's static result bound.
    #[must_use]
    pub const fn cardinality(&self) -> Cardinality {
        match self {
            Self::AtMostOne(selector) => selector.cardinality(),
            Self::Unbounded(selector) => selector.cardinality(),
            Self::Entity(selector) => selector.cardinality(),
        }
    }

    /// Returns a conservative set of context components read while resolving the
    /// selector.
    #[must_use]
    pub const fn context_reads(&self) -> ContextMask {
        match self {
            Self::AtMostOne(selector) => selector.context_reads(),
            Self::Unbounded(selector) => selector.context_reads(),
            Self::Entity(selector) => selector.context_reads(),
        }
    }
}

impl From<AtMostOneSelector> for Selector {
    fn from(selector: AtMostOneSelector) -> Self {
        Self::AtMostOne(selector)
    }
}

impl From<UnboundedSelector> for Selector {
    fn from(selector: UnboundedSelector) -> Self {
        Self::Unbounded(selector)
    }
}

impl From<EntitySelector> for Selector {
    fn from(selector: EntitySelector) -> Self {
        Self::Entity(selector)
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtMostOne(selector) => selector.fmt(formatter),
            Self::Unbounded(selector) => selector.fmt(formatter),
            Self::Entity(selector) => selector.fmt(formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AtMostOneSelector, Cardinality, EntitySelector, EntitySelectorError, Selector,
        UnboundedSelector,
    };
    use crate::ir::minecraft::ContextMask;

    #[test]
    fn closed_selectors_have_exact_text_and_static_cardinality() {
        let cases = [
            (Selector::from(AtMostOneSelector::SelfExecutor), "@s"),
            (Selector::from(AtMostOneSelector::NearestPlayer), "@p"),
            (Selector::from(AtMostOneSelector::RandomPlayer), "@r"),
            (Selector::from(UnboundedSelector::AllPlayers), "@a"),
            (Selector::from(UnboundedSelector::AllEntities), "@e"),
        ];

        for (selector, text) in cases {
            assert_eq!(selector.to_string(), text);
        }
        assert_eq!(
            Selector::from(AtMostOneSelector::SelfExecutor).cardinality(),
            Cardinality::AT_MOST_ONE
        );
        assert_eq!(
            Selector::from(UnboundedSelector::AllEntities).cardinality(),
            Cardinality::UNBOUNDED
        );
    }

    #[test]
    fn selector_context_reads_are_conservative_and_role_independent() {
        let self_reads = AtMostOneSelector::SelfExecutor.context_reads();
        assert!(self_reads.contains(ContextMask::EXECUTOR));
        assert!(!self_reads.intersects(ContextMask::POSITION));

        for selector in [
            Selector::from(AtMostOneSelector::NearestPlayer),
            Selector::from(AtMostOneSelector::RandomPlayer),
            Selector::from(UnboundedSelector::AllPlayers),
            Selector::from(UnboundedSelector::AllEntities),
        ] {
            let reads = selector.context_reads();
            assert!(reads.contains(ContextMask::POSITION));
            assert!(reads.contains(ContextMask::DIMENSION));
        }
    }

    #[test]
    fn structured_entity_selector_validates_and_renders_every_first_slice_field() {
        let selector =
            EntitySelector::armor_stands(vec!["first".into(), "second".into()], Some(1)).unwrap();
        assert_eq!(
            selector
                .tags()
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<&str>>(),
            ["first", "second"]
        );
        assert_eq!(selector.limit(), Some(1));
        assert_eq!(selector.cardinality(), Cardinality::AT_MOST_ONE);
        assert_eq!(
            selector.to_string(),
            "@e[type=minecraft:armor_stand,tag=first,tag=second,limit=1]"
        );
        assert!(selector.context_reads().contains(ContextMask::POSITION));
        assert!(selector.context_reads().contains(ContextMask::DIMENSION));

        assert_eq!(
            EntitySelector::armor_stands(vec![], Some(0)),
            Err(EntitySelectorError::ZeroLimit)
        );
        assert_eq!(
            EntitySelector::armor_stands(vec![], Some(i32::MAX as u32 + 1)),
            Err(EntitySelectorError::LimitOutOfRange(i32::MAX as u32 + 1))
        );
        assert_eq!(
            EntitySelector::armor_stands(vec![Box::from("valid"), Box::from("not valid")], None,),
            Err(EntitySelectorError::InvalidTagCharacter {
                index: 1,
                character: ' ',
            })
        );
    }

    #[test]
    fn structured_limits_preserve_canonical_finite_cardinality() {
        let one = EntitySelector::armor_stands(vec![], Some(1)).unwrap();
        let seven = EntitySelector::armor_stands(vec![], Some(7)).unwrap();
        let maximum = EntitySelector::armor_stands(vec![], Some(i32::MAX as u32)).unwrap();
        let unlimited = EntitySelector::armor_stands(vec![], None).unwrap();

        assert_eq!(one.cardinality(), Cardinality::AT_MOST_ONE);
        assert!(one.cardinality().is_at_most_one());
        assert_eq!(seven.cardinality().maximum(), Some(7));
        assert!(!seven.cardinality().is_at_most_one());
        assert!(!seven.cardinality().is_unbounded());
        assert_eq!(maximum.cardinality().maximum(), Some(i32::MAX as u32));
        assert_eq!(unlimited.cardinality(), Cardinality::UNBOUNDED);
        assert!(unlimited.cardinality().is_unbounded());
        assert_eq!(unlimited.cardinality().maximum(), None);
    }
}
