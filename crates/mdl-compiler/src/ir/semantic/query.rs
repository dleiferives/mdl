//! Compiler-owned static entity-query plans.

use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;

use super::{EntityKind, EntityQueryType, QueryCardinality, QueryCardinalityError};

/// One compile-time entity tag accepted by the first static query slice.
///
/// The initial spelling is deliberately restricted to Brigadier's unquoted-word
/// alphabet so target emission never needs to guess selector-string escaping.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EntityTag(Box<str>);

impl EntityTag {
    /// Validates and owns one non-empty entity tag.
    ///
    /// # Errors
    ///
    /// Returns [`EntityTagError::Empty`] for an empty tag, or
    /// [`EntityTagError::InvalidCharacter`] for a character outside ASCII letters,
    /// digits, `_`, `-`, `.`, and `+`.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, EntityTagError> {
        let value = value.into();
        if value.is_empty() {
            return Err(EntityTagError::Empty);
        }
        for (byte, character) in value.char_indices() {
            if !is_unquoted_word_character(character) {
                return Err(EntityTagError::InvalidCharacter { byte, character });
            }
        }
        Ok(Self(value))
    }

    /// Returns the validated tag spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntityTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

const fn is_unquoted_word_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | '+')
}

/// Invalid source data for a static entity tag.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityTagError {
    /// The tag was empty.
    Empty,
    /// The tag contained a character that cannot be emitted as a safe unquoted word.
    InvalidCharacter {
        /// UTF-8 byte offset of the invalid character.
        byte: usize,
        /// Invalid Unicode scalar value.
        character: char,
    },
}

impl fmt::Display for EntityTagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("entity tags cannot be empty"),
            Self::InvalidCharacter { byte, character } => write!(
                formatter,
                "entity tag contains invalid character {character:?} at UTF-8 byte {byte}"
            ),
        }
    }
}

impl Error for EntityTagError {}

/// An immutable, compiler-known entity query rather than a runtime collection.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StaticEntityQuery {
    kind: EntityKind,
    tags: Vec<EntityTag>,
    limit: Option<NonZeroU32>,
}

impl StaticEntityQuery {
    /// Starts an unbounded query for one nominal entity kind.
    #[must_use]
    pub fn entities(kind: EntityKind) -> Self {
        Self {
            kind,
            tags: vec![],
            limit: None,
        }
    }

    /// Returns the nominal selected kind.
    #[must_use]
    pub const fn kind(&self) -> EntityKind {
        self.kind
    }

    /// Returns tag filters in source order.
    #[must_use]
    pub fn tags(&self) -> &[EntityTag] {
        &self.tags
    }

    /// Returns the canonical positive limit, if one is present.
    #[must_use]
    pub const fn maximum(&self) -> Option<u32> {
        match self.limit {
            Some(limit) => Some(limit.get()),
            None => None,
        }
    }

    /// Returns the semantic query type derived from the complete plan.
    #[must_use]
    pub fn ty(&self) -> EntityQueryType {
        let cardinality = self.limit.map_or(QueryCardinality::UNBOUNDED, |limit| {
            QueryCardinality::bounded_nonzero(limit)
        });
        EntityQueryType::new(self.kind, cardinality)
    }

    /// Appends one typed tag filter while preserving source order.
    #[must_use]
    pub fn with_tag(mut self, tag: EntityTag) -> Self {
        self.push_tag(tag);
        self
    }

    pub(crate) fn push_tag(&mut self, tag: EntityTag) {
        self.tags.push(tag);
    }

    /// Refines the query with a positive static maximum.
    ///
    /// Repeated limits retain the strongest (smallest) upper bound, so the plan has
    /// one canonical target limit while its type monotonically gains information.
    ///
    /// # Errors
    ///
    /// Returns [`QueryCardinalityError::ZeroBound`] for a zero limit.
    pub fn limit(mut self, maximum: u32) -> Result<Self, QueryCardinalityError> {
        self.refine_limit(maximum)?;
        Ok(self)
    }

    pub(crate) fn refine_limit(&mut self, maximum: u32) -> Result<(), QueryCardinalityError> {
        let Some(maximum) = NonZeroU32::new(maximum) else {
            return Err(QueryCardinalityError::ZeroBound);
        };
        self.limit = Some(self.limit.map_or(maximum, |current| current.min(maximum)));
        Ok(())
    }
}

impl fmt::Display for StaticEntityQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "entities({})", self.kind)?;
        for tag in &self.tags {
            write!(formatter, ".with_tag({:?})", tag.as_str())?;
        }
        if let Some(limit) = self.limit {
            write!(formatter, ".limit({limit})")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{EntityKind, QueryCardinality, QueryCardinalityError};
    use super::{EntityTag, EntityTagError, StaticEntityQuery};

    #[test]
    fn tags_use_the_reviewed_unquoted_brigadier_alphabet() {
        for accepted in ["stage7", "A-Z_09", "a.b+c-d"] {
            assert_eq!(EntityTag::new(accepted).unwrap().as_str(), accepted);
        }
        assert_eq!(EntityTag::new(""), Err(EntityTagError::Empty));
        assert_eq!(
            EntityTag::new("two words"),
            Err(EntityTagError::InvalidCharacter {
                byte: 3,
                character: ' '
            })
        );
        assert_eq!(
            EntityTag::new("tag/child"),
            Err(EntityTagError::InvalidCharacter {
                byte: 3,
                character: '/'
            })
        );
    }

    #[test]
    fn query_plan_preserves_filters_and_refines_cardinality() {
        let query = StaticEntityQuery::entities(EntityKind::ArmorStand)
            .with_tag(EntityTag::new("first").unwrap())
            .with_tag(EntityTag::new("second").unwrap())
            .limit(10)
            .unwrap()
            .limit(1)
            .unwrap();
        assert_eq!(query.kind(), EntityKind::ArmorStand);
        assert_eq!(query.tags()[0].as_str(), "first");
        assert_eq!(query.tags()[1].as_str(), "second");
        assert_eq!(query.maximum(), Some(1));
        assert_eq!(query.ty().cardinality(), QueryCardinality::AT_MOST_ONE);
        assert_eq!(
            query.to_string(),
            "entities(ArmorStand).with_tag(\"first\").with_tag(\"second\").limit(1)"
        );
    }

    #[test]
    fn query_plan_rejects_zero_limit() {
        assert_eq!(
            StaticEntityQuery::entities(EntityKind::ArmorStand).limit(0),
            Err(QueryCardinalityError::ZeroBound)
        );
    }
}
