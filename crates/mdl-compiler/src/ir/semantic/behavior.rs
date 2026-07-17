//! Closed, target-independent summaries of function behavior.
//!
//! These facts deliberately describe semantic requirements and conservative
//! upper bounds rather than target recipes or measured command costs.  Each
//! domain has an explicit unknown top so opaque behavior cannot accidentally be
//! treated as an ordinary finite or effect-free operation.

use std::num::NonZeroU64;

use super::EntityKind;

/// Requirement for one component of Minecraft's incoming execution context.
///
/// `None` is the lattice bottom: the component is not consumed.  Two equal
/// `Required` facts join to that same requirement; unequal requirements lose the
/// precise value and join to `Unknown`.  `Unknown` means that some behavior may
/// consume the component, but the closed semantic model cannot describe the
/// required value precisely.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ContextRequirement<T> {
    /// The context component is not required.
    #[default]
    None,
    /// The context component must have this semantic value.
    Required(T),
    /// The component may be required in a way the closed model cannot describe.
    Unknown,
}

impl<T> ContextRequirement<T> {
    /// Returns the precise required value, if one is known.
    #[must_use]
    pub const fn required(&self) -> Option<&T> {
        match self {
            Self::Required(value) => Some(value),
            Self::None | Self::Unknown => None,
        }
    }

    /// Returns whether this fact imposes no requirement.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns whether the requirement is outside the closed semantic model.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

impl<T: Eq> ContextRequirement<T> {
    /// Computes the least conservative fact that contains both requirements.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::None, requirement) | (requirement, Self::None) => requirement,
            (Self::Required(left), Self::Required(right)) if left == right => Self::Required(left),
            (Self::Required(_), Self::Required(_)) | (Self::Unknown, _) | (_, Self::Unknown) => {
                Self::Unknown
            }
        }
    }
}

/// Ambient execution-frame components a function may consume from its caller.
///
/// Requirements are component-wise because Minecraft modifiers can establish
/// one component while preserving the others.  In particular, `execute as`
/// can discharge an executor requirement without proving anything about the
/// incoming position, rotation, dimension, or anchor.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct AmbientContextRequirements {
    executor: ContextRequirement<EntityKind>,
    position: ContextRequirement<()>,
    rotation: ContextRequirement<()>,
    dimension: ContextRequirement<()>,
    anchor: ContextRequirement<()>,
}

impl AmbientContextRequirements {
    /// No ambient execution-frame component is consumed.
    pub const NONE: Self = Self::new(
        ContextRequirement::None,
        ContextRequirement::None,
        ContextRequirement::None,
        ContextRequirement::None,
        ContextRequirement::None,
    );

    /// Every ambient component may be consumed by opaque behavior.
    pub const UNKNOWN: Self = Self::new(
        ContextRequirement::Unknown,
        ContextRequirement::Unknown,
        ContextRequirement::Unknown,
        ContextRequirement::Unknown,
        ContextRequirement::Unknown,
    );

    /// Creates one complete set of component-wise requirements.
    #[must_use]
    pub const fn new(
        executor: ContextRequirement<EntityKind>,
        position: ContextRequirement<()>,
        rotation: ContextRequirement<()>,
        dimension: ContextRequirement<()>,
        anchor: ContextRequirement<()>,
    ) -> Self {
        Self {
            executor,
            position,
            rotation,
            dimension,
            anchor,
        }
    }

    /// Returns the incoming-executor requirement.
    #[must_use]
    pub const fn executor(self) -> ContextRequirement<EntityKind> {
        self.executor
    }

    /// Returns the incoming-position requirement.
    #[must_use]
    pub const fn position(self) -> ContextRequirement<()> {
        self.position
    }

    /// Returns the incoming-rotation requirement.
    #[must_use]
    pub const fn rotation(self) -> ContextRequirement<()> {
        self.rotation
    }

    /// Returns the incoming-dimension requirement.
    #[must_use]
    pub const fn dimension(self) -> ContextRequirement<()> {
        self.dimension
    }

    /// Returns the incoming-anchor requirement.
    #[must_use]
    pub const fn anchor(self) -> ContextRequirement<()> {
        self.anchor
    }

    /// Replaces the executor component, for ordered context transfer.
    #[must_use]
    pub const fn with_executor(mut self, requirement: ContextRequirement<EntityKind>) -> Self {
        self.executor = requirement;
        self
    }

    /// Replaces the position component, for ordered context transfer.
    #[must_use]
    pub const fn with_position(mut self, requirement: ContextRequirement<()>) -> Self {
        self.position = requirement;
        self
    }

    /// Replaces the rotation component, for ordered context transfer.
    #[must_use]
    pub const fn with_rotation(mut self, requirement: ContextRequirement<()>) -> Self {
        self.rotation = requirement;
        self
    }

    /// Replaces the dimension component, for ordered context transfer.
    #[must_use]
    pub const fn with_dimension(mut self, requirement: ContextRequirement<()>) -> Self {
        self.dimension = requirement;
        self
    }

    /// Replaces the anchor component, for ordered context transfer.
    #[must_use]
    pub const fn with_anchor(mut self, requirement: ContextRequirement<()>) -> Self {
        self.anchor = requirement;
        self
    }

    /// Computes the component-wise least conservative common summary.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            executor: self.executor.join(other.executor),
            position: self.position.join(other.position),
            rotation: self.rotation.join(other.rotation),
            dimension: self.dimension.join(other.dimension),
            anchor: self.anchor.join(other.anchor),
        }
    }
}

/// Conservative world-state effects reachable from a function.
///
/// Reads and writes form independent axes.  `ReadWrite` is therefore the join of
/// `Read` and `Write`, while `Unknown` is a distinct top for opaque behavior.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum WorldEffect {
    /// No world read or mutation is reachable.
    #[default]
    None,
    /// World state may be observed but is not mutated.
    Read,
    /// World state may be mutated without a modeled read dependency.
    Write,
    /// Both world reads and writes may occur.
    ReadWrite,
    /// Opaque behavior may have effects outside the closed model.
    Unknown,
}

impl WorldEffect {
    /// Computes the least conservative effect containing both operands.
    #[must_use]
    pub const fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::None, effect) | (effect, Self::None) => effect,
            (Self::Read, Self::Read) => Self::Read,
            (Self::Write, Self::Write) => Self::Write,
            (Self::ReadWrite, _)
            | (_, Self::ReadWrite)
            | (Self::Read, Self::Write)
            | (Self::Write, Self::Read) => Self::ReadWrite,
        }
    }

    /// Returns whether the summary permits a world-state read.
    #[must_use]
    pub const fn may_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite | Self::Unknown)
    }

    /// Returns whether the summary permits a world-state mutation.
    #[must_use]
    pub const fn may_write(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite | Self::Unknown)
    }

    /// Returns whether the effect is outside the closed semantic model.
    #[must_use]
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Conservative externally visible output reachable from a function.
///
/// This axis is independent of ordinary world-state reads and writes. For example,
/// `say` is observable without being modeled as a world mutation.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ObservableEffect {
    /// No externally visible output is reachable.
    #[default]
    None,
    /// Compiler-modeled externally visible output may occur.
    Observable,
    /// Opaque behavior may produce output outside the closed model.
    Unknown,
}

impl ObservableEffect {
    /// Computes the least conservative effect containing both operands.
    #[must_use]
    pub const fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Observable, _) | (_, Self::Observable) => Self::Observable,
            (Self::None, Self::None) => Self::None,
        }
    }

    /// Returns whether externally visible output may occur.
    #[must_use]
    pub const fn may_be_observable(self) -> bool {
        matches!(self, Self::Observable | Self::Unknown)
    }

    /// Returns whether output behavior is outside the closed semantic model.
    #[must_use]
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Conservative upper bound on one ordinary fork-checked redirect reachable from a
/// function.
///
/// `None` is the identity and means no redirect is reachable.  It is deliberately
/// distinct from `Finite(1)`, which records that a redirect exists but has a
/// proven at-most-one expansion.  `NoFiniteUpperBound` retains modeled fork
/// semantics with no finite proof; `Unknown` denotes opaque fork behavior. Separate
/// commands and calls join by their maximum individual expansion; only modifiers
/// inside one ordered redirect chain compose multiplicatively.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum ForkBound {
    /// No contextual redirect is reachable.
    #[default]
    None,
    /// Redirects are modeled and expand to no more than this positive bound.
    Finite(NonZeroU64),
    /// Modeled behavior has no proven finite upper bound.
    NoFiniteUpperBound,
    /// Opaque behavior may fork in a way the closed model cannot describe.
    Unknown,
}

impl ForkBound {
    /// Creates a finite redirect bound, rejecting zero.
    #[must_use]
    pub const fn finite(maximum: u64) -> Option<Self> {
        match NonZeroU64::new(maximum) {
            Some(maximum) => Some(Self::Finite(maximum)),
            None => None,
        }
    }

    /// Returns the modeled finite redirect bound, if a redirect is present.
    ///
    /// `None` returns `None` because it records the absence of a redirect, not a
    /// finite redirect with a bound of one.
    #[must_use]
    pub const fn finite_maximum(self) -> Option<u64> {
        match self {
            Self::Finite(maximum) => Some(maximum.get()),
            Self::None | Self::NoFiniteUpperBound | Self::Unknown => None,
        }
    }

    /// Returns whether no contextual redirect is reachable.
    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    /// Returns whether all modeled expansions are proven at most one.
    ///
    /// The no-redirect identity also satisfies this predicate.
    #[must_use]
    pub const fn is_at_most_once(self) -> bool {
        match self {
            Self::None => true,
            Self::Finite(maximum) => maximum.get() == 1,
            Self::NoFiniteUpperBound | Self::Unknown => false,
        }
    }

    /// Returns whether fork behavior is outside the closed semantic model.
    #[must_use]
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Computes the least conservative bound containing both operands.
    #[must_use]
    pub const fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::NoFiniteUpperBound, _) | (_, Self::NoFiniteUpperBound) => {
                Self::NoFiniteUpperBound
            }
            (Self::None, bound) | (bound, Self::None) => bound,
            (Self::Finite(left), Self::Finite(right)) => {
                if left.get() >= right.get() {
                    Self::Finite(left)
                } else {
                    Self::Finite(right)
                }
            }
        }
    }

    /// Composes two modifiers in one ordered redirect chain multiplicatively.
    ///
    /// The no-redirect fact is the identity.  Overflow of two finite bounds loses
    /// the finite proof rather than wrapping, and opaque behavior always remains
    /// opaque.
    #[must_use]
    pub const fn multiply(self, next: Self) -> Self {
        match (self, next) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::None, bound) | (bound, Self::None) => bound,
            (Self::NoFiniteUpperBound, _) | (_, Self::NoFiniteUpperBound) => {
                Self::NoFiniteUpperBound
            }
            (Self::Finite(left), Self::Finite(right)) => {
                match left.get().checked_mul(right.get()) {
                    Some(product) => match NonZeroU64::new(product) {
                        Some(product) => Self::Finite(product),
                        None => Self::NoFiniteUpperBound,
                    },
                    None => Self::NoFiniteUpperBound,
                }
            }
        }
    }
}

/// Conservative classification of all work transitively reachable from a function.
///
/// The finite class intentionally does not claim an exact command count.  Exact
/// target costs belong to later lowering and target analysis.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum TransitiveWork {
    /// No command-like work is reachable.
    #[default]
    Zero,
    /// Reachable work has a finite upper bound.
    Finite,
    /// Modeled work has no proven finite upper bound.
    NoFiniteUpperBound,
    /// Opaque behavior prevents a modeled work bound.
    Unknown,
}

impl TransitiveWork {
    /// Computes the least conservative work class containing both operands.
    #[must_use]
    pub const fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::NoFiniteUpperBound, _) | (_, Self::NoFiniteUpperBound) => {
                Self::NoFiniteUpperBound
            }
            (Self::Finite, _) | (_, Self::Finite) => Self::Finite,
            (Self::Zero, Self::Zero) => Self::Zero,
        }
    }

    /// Returns whether work behavior is outside the closed semantic model.
    #[must_use]
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// Closed behavior summary inferred for one checked source function.
///
/// Summaries form a product lattice.  Joining is component-wise, except that the
/// explicit unsafe marker is combined by logical OR.  The marker distinguishes a
/// summary widened by an unsafe opaque operation from ordinary conservative loss
/// of a finite proof.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct FunctionBehavior {
    required_ambient_context: AmbientContextRequirements,
    world_effect: WorldEffect,
    observable_effect: ObservableEffect,
    fork_bound: ForkBound,
    transitive_work: TransitiveWork,
    contains_unsafe_unknown: bool,
}

impl FunctionBehavior {
    /// Bottom behavior: no requirements, effects, redirects, work, or unsafe opacity.
    pub const NONE: Self = Self::new(
        AmbientContextRequirements::NONE,
        WorldEffect::None,
        ObservableEffect::None,
        ForkBound::None,
        TransitiveWork::Zero,
        false,
    );

    /// Creates one complete closed behavior summary.
    #[must_use]
    pub const fn new(
        required_ambient_context: AmbientContextRequirements,
        world_effect: WorldEffect,
        observable_effect: ObservableEffect,
        fork_bound: ForkBound,
        transitive_work: TransitiveWork,
        contains_unsafe_unknown: bool,
    ) -> Self {
        Self {
            required_ambient_context,
            world_effect,
            observable_effect,
            fork_bound,
            transitive_work,
            contains_unsafe_unknown,
        }
    }

    /// Returns the ambient execution-frame requirements.
    #[must_use]
    pub const fn required_ambient_context(self) -> AmbientContextRequirements {
        self.required_ambient_context
    }

    /// Returns the reachable world-state effect.
    #[must_use]
    pub const fn world_effect(self) -> WorldEffect {
        self.world_effect
    }

    /// Returns the reachable externally visible output effect.
    #[must_use]
    pub const fn observable_effect(self) -> ObservableEffect {
        self.observable_effect
    }

    /// Returns the reachable contextual-fork bound.
    #[must_use]
    pub const fn fork_bound(self) -> ForkBound {
        self.fork_bound
    }

    /// Returns the transitive work classification.
    #[must_use]
    pub const fn transitive_work(self) -> TransitiveWork {
        self.transitive_work
    }

    /// Returns whether an unsafe opaque operation is transitively reachable.
    #[must_use]
    pub const fn contains_unsafe_unknown(self) -> bool {
        self.contains_unsafe_unknown
    }

    /// Computes the component-wise least conservative common summary.
    #[must_use]
    pub fn join(self, other: Self) -> Self {
        Self {
            required_ambient_context: self
                .required_ambient_context
                .join(other.required_ambient_context),
            world_effect: self.world_effect.join(other.world_effect),
            observable_effect: self.observable_effect.join(other.observable_effect),
            fork_bound: self.fork_bound.join(other.fork_bound),
            transitive_work: self.transitive_work.join(other.transitive_work),
            contains_unsafe_unknown: self.contains_unsafe_unknown || other.contains_unsafe_unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AmbientContextRequirements, ContextRequirement, ForkBound, FunctionBehavior,
        ObservableEffect, TransitiveWork, WorldEffect,
    };
    use crate::ir::semantic::EntityKind;

    #[test]
    fn context_requirement_join_preserves_equal_values_and_widens_conflicts() {
        let first = ContextRequirement::Required(1_u8);
        let second = ContextRequirement::Required(2_u8);

        assert_eq!(ContextRequirement::None.join(first), first);
        assert_eq!(first.join(first), first);
        assert_eq!(first.join(second), ContextRequirement::Unknown);
        assert_eq!(
            first.join(ContextRequirement::Unknown),
            ContextRequirement::Unknown
        );
    }

    #[test]
    fn ambient_requirements_join_independently() {
        let executor = AmbientContextRequirements::NONE
            .with_executor(ContextRequirement::Required(EntityKind::ArmorStand));
        let positioned =
            AmbientContextRequirements::NONE.with_position(ContextRequirement::Required(()));
        let joined = executor.join(positioned);

        assert_eq!(
            joined.executor(),
            ContextRequirement::Required(EntityKind::ArmorStand)
        );
        assert_eq!(joined.position(), ContextRequirement::Required(()));
        assert!(joined.rotation().is_none());
        assert_eq!(
            joined.join(AmbientContextRequirements::UNKNOWN),
            AmbientContextRequirements::UNKNOWN
        );
    }

    #[test]
    fn world_effect_join_is_a_read_write_diamond_with_unknown_top() {
        assert_eq!(WorldEffect::None.join(WorldEffect::Read), WorldEffect::Read);
        assert_eq!(
            WorldEffect::Read.join(WorldEffect::Write),
            WorldEffect::ReadWrite
        );
        assert_eq!(
            WorldEffect::ReadWrite.join(WorldEffect::Read),
            WorldEffect::ReadWrite
        );
        assert_eq!(
            WorldEffect::Write.join(WorldEffect::Unknown),
            WorldEffect::Unknown
        );
        assert!(WorldEffect::Unknown.may_read());
        assert!(WorldEffect::Unknown.may_write());
    }

    #[test]
    fn observable_effect_is_independent_with_an_unknown_top() {
        assert_eq!(
            ObservableEffect::None.join(ObservableEffect::Observable),
            ObservableEffect::Observable
        );
        assert_eq!(
            ObservableEffect::Observable.join(ObservableEffect::Unknown),
            ObservableEffect::Unknown
        );
        assert!(!ObservableEffect::None.may_be_observable());
        assert!(ObservableEffect::Observable.may_be_observable());
        assert!(ObservableEffect::Unknown.may_be_observable());
        assert!(ObservableEffect::Unknown.is_unknown());
    }

    #[test]
    fn observable_effect_join_obeys_product_lattice_laws_exhaustively() {
        let effects = [
            ObservableEffect::None,
            ObservableEffect::Observable,
            ObservableEffect::Unknown,
        ];
        for left in effects {
            assert_eq!(left.join(left), left);
            for right in effects {
                assert_eq!(left.join(right), right.join(left));
                for third in effects {
                    assert_eq!(left.join(right).join(third), left.join(right.join(third)));
                }
            }
        }
    }

    #[test]
    fn fork_bounds_join_and_multiply_without_erasing_redirect_presence() {
        let one = ForkBound::finite(1).unwrap();
        let two = ForkBound::finite(2).unwrap();
        let three = ForkBound::finite(3).unwrap();

        assert_eq!(ForkBound::None.join(one), one);
        assert_eq!(two.join(three), three);
        assert_eq!(ForkBound::None.multiply(two), two);
        assert_eq!(two.multiply(three).finite_maximum(), Some(6));
        assert!(ForkBound::None.is_at_most_once());
        assert!(one.is_at_most_once());
        assert_ne!(ForkBound::None, one);
    }

    #[test]
    fn fork_bound_overflow_loses_only_the_finite_proof() {
        let maximum = ForkBound::finite(u64::MAX).unwrap();
        let two = ForkBound::finite(2).unwrap();

        assert_eq!(maximum.multiply(two), ForkBound::NoFiniteUpperBound);
        assert_eq!(maximum.multiply(ForkBound::Unknown), ForkBound::Unknown);
        assert_eq!(ForkBound::finite(0), None);
    }

    #[test]
    fn function_behavior_join_is_componentwise_and_marks_unsafe_transitively() {
        let query = FunctionBehavior::new(
            AmbientContextRequirements::NONE.with_dimension(ContextRequirement::Required(())),
            WorldEffect::Read,
            ObservableEffect::None,
            ForkBound::finite(3).unwrap(),
            TransitiveWork::Finite,
            false,
        );
        let opaque = FunctionBehavior::new(
            AmbientContextRequirements::UNKNOWN,
            WorldEffect::Unknown,
            ObservableEffect::Unknown,
            ForkBound::Unknown,
            TransitiveWork::Unknown,
            true,
        );
        let joined = query.join(opaque);

        assert_eq!(
            joined.required_ambient_context(),
            AmbientContextRequirements::UNKNOWN
        );
        assert_eq!(joined.world_effect(), WorldEffect::Unknown);
        assert_eq!(joined.observable_effect(), ObservableEffect::Unknown);
        assert_eq!(joined.fork_bound(), ForkBound::Unknown);
        assert_eq!(joined.transitive_work(), TransitiveWork::Unknown);
        assert!(joined.contains_unsafe_unknown());
    }
}
