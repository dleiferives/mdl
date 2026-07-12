use std::fmt;

use super::ContextMask;

/// A statically known upper bound on selector results.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Cardinality {
    /// Zero or one result.
    AtMostOne,
    /// No useful static upper bound.
    Unbounded,
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
        Cardinality::AtMostOne
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
        Cardinality::Unbounded
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
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Selector {
    /// A selector returning at most one entity.
    AtMostOne(AtMostOneSelector),
    /// A selector with no useful static upper bound.
    Unbounded(UnboundedSelector),
}

impl Selector {
    /// Returns the selector's static result bound.
    #[must_use]
    pub const fn cardinality(self) -> Cardinality {
        match self {
            Self::AtMostOne(selector) => selector.cardinality(),
            Self::Unbounded(selector) => selector.cardinality(),
        }
    }

    /// Returns a conservative set of context components read while resolving the
    /// selector.
    #[must_use]
    pub const fn context_reads(self) -> ContextMask {
        match self {
            Self::AtMostOne(selector) => selector.context_reads(),
            Self::Unbounded(selector) => selector.context_reads(),
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

impl fmt::Display for Selector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtMostOne(selector) => selector.fmt(formatter),
            Self::Unbounded(selector) => selector.fmt(formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AtMostOneSelector, Cardinality, Selector, UnboundedSelector};
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
            Cardinality::AtMostOne
        );
        assert_eq!(
            Selector::from(UnboundedSelector::AllEntities).cardinality(),
            Cardinality::Unbounded
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
}
