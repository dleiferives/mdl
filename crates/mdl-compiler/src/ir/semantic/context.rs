//! Target-independent facts about Minecraft's current execution frame.

use super::EntityKind;

/// Availability and provenance of one execution-context component.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextFact<T, P> {
    /// No valid value is available in this lexical context.
    Unavailable,
    /// The component is inherited from the incoming Minecraft command frame.
    Inherited,
    /// A typed modifier established the component at an exact plan step.
    Established {
        /// Semantic value established for the component.
        value: T,
        /// Identity of the modifier step that established it.
        by: P,
    },
}

impl<T, P> ContextFact<T, P> {
    /// Returns the established value and proof when this is not ambient/absent.
    #[must_use]
    pub const fn established(&self) -> Option<(&T, &P)> {
        match self {
            Self::Established { value, by } => Some((value, by)),
            Self::Unavailable | Self::Inherited => None,
        }
    }
}

/// The five ambient components consumed and transformed by Minecraft commands.
///
/// `P` is an IR-specific exact modifier-step identity. It remains generic so the
/// semantic model does not depend on source or Core entity IDs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExecutionContext<P> {
    executor: ContextFact<EntityKind, P>,
    position: ContextFact<(), P>,
    rotation: ContextFact<(), P>,
    dimension: ContextFact<(), P>,
    anchor: ContextFact<(), P>,
}

impl<P> ExecutionContext<P> {
    /// Context at the entry of an ordinary source function.
    ///
    /// Minecraft supplies an incoming frame, but an arbitrary function invocation
    /// does not prove the presence or nominal kind of `@s`.
    #[must_use]
    pub const fn function_entry() -> Self {
        Self {
            executor: ContextFact::Unavailable,
            position: ContextFact::Inherited,
            rotation: ContextFact::Inherited,
            dimension: ContextFact::Inherited,
            anchor: ContextFact::Inherited,
        }
    }

    /// Returns the current executor fact.
    #[must_use]
    pub const fn executor(&self) -> &ContextFact<EntityKind, P> {
        &self.executor
    }

    /// Returns the current position fact.
    #[must_use]
    pub const fn position(&self) -> &ContextFact<(), P> {
        &self.position
    }

    /// Returns the current rotation fact.
    #[must_use]
    pub const fn rotation(&self) -> &ContextFact<(), P> {
        &self.rotation
    }

    /// Returns the current dimension fact.
    #[must_use]
    pub const fn dimension(&self) -> &ContextFact<(), P> {
        &self.dimension
    }

    /// Returns the current anchor fact.
    #[must_use]
    pub const fn anchor(&self) -> &ContextFact<(), P> {
        &self.anchor
    }

    /// Replaces only the executor, exactly matching Minecraft `execute as`.
    #[must_use]
    pub fn establish_executor(mut self, kind: EntityKind, by: P) -> Self {
        self.executor = ContextFact::Established { value: kind, by };
        self
    }

    /// Replaces the position component.
    #[must_use]
    pub fn establish_position(mut self, by: P) -> Self {
        self.position = ContextFact::Established { value: (), by };
        self
    }

    /// Replaces the rotation component.
    #[must_use]
    pub fn establish_rotation(mut self, by: P) -> Self {
        self.rotation = ContextFact::Established { value: (), by };
        self
    }

    /// Replaces the dimension component.
    #[must_use]
    pub fn establish_dimension(mut self, by: P) -> Self {
        self.dimension = ContextFact::Established { value: (), by };
        self
    }

    /// Replaces the anchor component.
    #[must_use]
    pub fn establish_anchor(mut self, by: P) -> Self {
        self.anchor = ContextFact::Established { value: (), by };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{ContextFact, ExecutionContext};
    use crate::ir::semantic::EntityKind;

    #[test]
    fn execute_as_replaces_only_the_executor() {
        let input = ExecutionContext::<u8>::function_entry();
        let output = input.establish_executor(EntityKind::ArmorStand, 7);
        assert_eq!(
            output.executor(),
            &ContextFact::Established {
                value: EntityKind::ArmorStand,
                by: 7
            }
        );
        assert_eq!(output.position(), input.position());
        assert_eq!(output.rotation(), input.rotation());
        assert_eq!(output.dimension(), input.dimension());
        assert_eq!(output.anchor(), input.anchor());
    }
}
