//! Closed target-independent Minecraft operation and source-method registry.

use super::{
    AmbientContextRequirements, ContextRequirement, EntityCapability, EntityKind, ForkBound,
    FunctionBehavior, ObservableEffect, SemanticType, TransitiveWork, WorldEffect,
};

macro_rules! define_minecraft_semantic_keys {
    ($($(#[$metadata:meta])* $variant:ident),+ $(,)?) => {
        /// Target-independent identity of a normalized typed Minecraft operation.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub enum MinecraftSemanticKey {
            $($(#[$metadata])* $variant),+
        }

        impl MinecraftSemanticKey {
            /// Every semantic key in stable declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
        }
    };
}

define_minecraft_semantic_keys! {
    /// Broadcast a plain message as Minecraft's current executor.
    Say,
    /// Teleport the current executor relative to the current execution frame.
    TeleportCurrentExecutor,
    /// Move the current executor by an offset relative to the executor itself.
    MoveCurrentExecutorBy,
    /// Read one static literal-text page from the current executor's main-hand written book.
    ReadMainHandWrittenBookLiteralPage,
}

/// A target-independent compile-time attribute carried by a Minecraft operation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MinecraftAttributeKind {
    /// One already-validated [`super::MessageLiteral`].
    MessageLiteral,
    /// One exact static position.
    PositionSpec,
    /// One exact receiver-relative world offset.
    RelativeWorldOffset,
    /// One statically validated zero-based written-book page index.
    WrittenBookPageIndex,
}

/// Complete target-independent value and attribute shape of an operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MinecraftSemanticSignature {
    operands: &'static [SemanticType],
    attributes: &'static [MinecraftAttributeKind],
    results: &'static [SemanticType],
}

impl MinecraftSemanticSignature {
    const fn new(
        operands: &'static [SemanticType],
        attributes: &'static [MinecraftAttributeKind],
        results: &'static [SemanticType],
    ) -> Self {
        Self {
            operands,
            attributes,
            results,
        }
    }

    /// Runtime or compiler-known value operands, in semantic order.
    #[must_use]
    pub const fn operands(self) -> &'static [SemanticType] {
        self.operands
    }

    /// Closed compile-time attributes, in semantic order.
    #[must_use]
    pub const fn attributes(self) -> &'static [MinecraftAttributeKind] {
        self.attributes
    }

    /// Source-semantic value results, in semantic order.
    #[must_use]
    pub const fn results(self) -> &'static [SemanticType] {
        self.results
    }
}

/// How an operation obtains target-independent ambient execution context.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MinecraftAmbientContextRule {
    /// Require the current executor to have the operation instance's receiver kind.
    CurrentExecutorKind,
}

impl MinecraftAmbientContextRule {
    /// Instantiates this rule for one already-checked executor kind.
    #[must_use]
    pub const fn for_executor_kind(self, kind: EntityKind) -> AmbientContextRequirements {
        match self {
            Self::CurrentExecutorKind => {
                AmbientContextRequirements::NONE.with_executor(ContextRequirement::Required(kind))
            }
        }
    }
}

/// How source semantics expose the native command's success and result values.
///
/// Exact native values and continuation behavior remain properties of a selected
/// target recipe. This field only records the target-independent source contract.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MinecraftCommandOutcomeBehavior {
    /// The operation is `Void`; native command success/result values are discarded.
    Discarded,
}

/// Closed validation family for operation-specific static data.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MinecraftValidatorKind {
    /// Validate the operation's sole attribute through [`super::MessageLiteral`].
    MessageLiteral,
    /// Validate one exact position attribute.
    PositionSpec,
    /// Validate one exact relative offset attribute.
    RelativeWorldOffset,
    WrittenBookPageIndex,
}

/// Shared target-independent meaning of one normalized Minecraft operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MinecraftSemanticDescriptor {
    key: MinecraftSemanticKey,
    signature: MinecraftSemanticSignature,
    ambient_context: MinecraftAmbientContextRule,
    world_effect: WorldEffect,
    observable_effect: ObservableEffect,
    fork_behavior: ForkBound,
    work_behavior: TransitiveWork,
    outcome_behavior: MinecraftCommandOutcomeBehavior,
    validator: MinecraftValidatorKind,
    documentation: &'static str,
}

impl MinecraftSemanticDescriptor {
    /// Returns this descriptor's normalized semantic identity.
    #[must_use]
    pub const fn key(self) -> MinecraftSemanticKey {
        self.key
    }

    /// Returns the operation's complete operand/attribute/result shape.
    #[must_use]
    pub const fn signature(self) -> MinecraftSemanticSignature {
        self.signature
    }

    /// Returns the operation's ambient execution-context rule.
    #[must_use]
    pub const fn ambient_context(self) -> MinecraftAmbientContextRule {
        self.ambient_context
    }

    /// Returns the operation's ordinary world-state effect.
    #[must_use]
    pub const fn world_effect(self) -> WorldEffect {
        self.world_effect
    }

    /// Returns the operation's externally observable-output effect.
    #[must_use]
    pub const fn observable_effect(self) -> ObservableEffect {
        self.observable_effect
    }

    /// Returns the operation's intrinsic redirect behavior.
    #[must_use]
    pub const fn fork_behavior(self) -> ForkBound {
        self.fork_behavior
    }

    /// Returns the operation's target-independent work class.
    #[must_use]
    pub const fn work_behavior(self) -> TransitiveWork {
        self.work_behavior
    }

    /// Returns how source semantics expose native command outcomes.
    #[must_use]
    pub const fn outcome_behavior(self) -> MinecraftCommandOutcomeBehavior {
        self.outcome_behavior
    }

    /// Returns the closed validator family for static operation data.
    #[must_use]
    pub const fn validator(self) -> MinecraftValidatorKind {
        self.validator
    }

    /// Returns concise user-facing semantic documentation.
    #[must_use]
    pub const fn documentation(self) -> &'static str {
        self.documentation
    }

    /// Instantiates the descriptor's complete function-behavior contribution.
    #[must_use]
    pub const fn behavior_for_executor_kind(self, kind: EntityKind) -> FunctionBehavior {
        FunctionBehavior::new(
            self.ambient_context.for_executor_kind(kind),
            self.world_effect,
            self.observable_effect,
            self.fork_behavior,
            self.work_behavior,
            false,
        )
    }
}

const SAY_ATTRIBUTES: &[MinecraftAttributeKind] = &[MinecraftAttributeKind::MessageLiteral];
const TELEPORT_ATTRIBUTES: &[MinecraftAttributeKind] = &[MinecraftAttributeKind::PositionSpec];
const MOVE_BY_ATTRIBUTES: &[MinecraftAttributeKind] =
    &[MinecraftAttributeKind::RelativeWorldOffset];
const BOOK_PAGE_ATTRIBUTES: &[MinecraftAttributeKind] =
    &[MinecraftAttributeKind::WrittenBookPageIndex];
const STRING_RESULT: &[SemanticType] = &[SemanticType::Runtime(super::RuntimeValueType::String)];

const SAY_DESCRIPTOR: MinecraftSemanticDescriptor = MinecraftSemanticDescriptor {
    key: MinecraftSemanticKey::Say,
    signature: MinecraftSemanticSignature::new(&[], SAY_ATTRIBUTES, &[]),
    ambient_context: MinecraftAmbientContextRule::CurrentExecutorKind,
    world_effect: WorldEffect::None,
    observable_effect: ObservableEffect::Observable,
    fork_behavior: ForkBound::None,
    work_behavior: TransitiveWork::Finite,
    outcome_behavior: MinecraftCommandOutcomeBehavior::Discarded,
    validator: MinecraftValidatorKind::MessageLiteral,
    documentation: "Broadcast a plain message as the current Minecraft executor.",
};

const TELEPORT_DESCRIPTOR: MinecraftSemanticDescriptor = MinecraftSemanticDescriptor {
    key: MinecraftSemanticKey::TeleportCurrentExecutor,
    signature: MinecraftSemanticSignature::new(&[], TELEPORT_ATTRIBUTES, &[]),
    ambient_context: MinecraftAmbientContextRule::CurrentExecutorKind,
    world_effect: WorldEffect::Write,
    observable_effect: ObservableEffect::None,
    fork_behavior: ForkBound::None,
    work_behavior: TransitiveWork::Finite,
    outcome_behavior: MinecraftCommandOutcomeBehavior::Discarded,
    validator: MinecraftValidatorKind::PositionSpec,
    documentation: "Teleport the current executor relative to the current execution frame.",
};

const MOVE_BY_DESCRIPTOR: MinecraftSemanticDescriptor = MinecraftSemanticDescriptor {
    key: MinecraftSemanticKey::MoveCurrentExecutorBy,
    signature: MinecraftSemanticSignature::new(&[], MOVE_BY_ATTRIBUTES, &[]),
    ambient_context: MinecraftAmbientContextRule::CurrentExecutorKind,
    world_effect: WorldEffect::Write,
    observable_effect: ObservableEffect::None,
    fork_behavior: ForkBound::None,
    work_behavior: TransitiveWork::Finite,
    outcome_behavior: MinecraftCommandOutcomeBehavior::Discarded,
    validator: MinecraftValidatorKind::RelativeWorldOffset,
    documentation: "Move the current executor by a receiver-relative world offset.",
};

const BOOK_PAGE_DESCRIPTOR: MinecraftSemanticDescriptor = MinecraftSemanticDescriptor {
    key: MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage,
    signature: MinecraftSemanticSignature::new(&[], BOOK_PAGE_ATTRIBUTES, STRING_RESULT),
    ambient_context: MinecraftAmbientContextRule::CurrentExecutorKind,
    world_effect: WorldEffect::Read,
    observable_effect: ObservableEffect::None,
    fork_behavior: ForkBound::None,
    work_behavior: TransitiveWork::Finite,
    outcome_behavior: MinecraftCommandOutcomeBehavior::Discarded,
    validator: MinecraftValidatorKind::WrittenBookPageIndex,
    documentation: "Read one static literal-text page from the current executor's main-hand written book; missing or unsupported content yields an empty string.",
};

/// Returns the authoritative descriptor for `key`.
///
/// The exhaustive match intentionally prevents a new semantic key from compiling
/// until its target-independent meaning is supplied.
#[must_use]
pub const fn minecraft_descriptor(
    key: MinecraftSemanticKey,
) -> &'static MinecraftSemanticDescriptor {
    match key {
        MinecraftSemanticKey::Say => &SAY_DESCRIPTOR,
        MinecraftSemanticKey::TeleportCurrentExecutor => &TELEPORT_DESCRIPTOR,
        MinecraftSemanticKey::MoveCurrentExecutorBy => &MOVE_BY_DESCRIPTOR,
        MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage => &BOOK_PAGE_DESCRIPTOR,
    }
}

/// Receiver constraint and normalization applied before semantic lookup.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceReceiverRule {
    /// Use the already-current executor and require its nominal kind to have a
    /// particular semantic capability.
    CurrentExecutor {
        required_capability: EntityCapability,
    },
}

impl SourceReceiverRule {
    /// Returns whether `receiver` satisfies this closed receiver rule.
    #[must_use]
    pub fn accepts(self, receiver: SemanticType) -> bool {
        match (self, receiver) {
            (
                Self::CurrentExecutor {
                    required_capability,
                },
                SemanticType::Executor(executor),
            ) => executor.kind().capabilities().contains(required_capability),
            (Self::CurrentExecutor { .. }, _) => false,
        }
    }
}

/// One compiler-owned source method spelling and its normalization.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MinecraftSourceMethodRule {
    name: &'static str,
    receiver: SourceReceiverRule,
    semantic_key: MinecraftSemanticKey,
}

impl MinecraftSourceMethodRule {
    /// Returns the source member spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.name
    }

    /// Returns the receiver constraint and normalization.
    #[must_use]
    pub const fn receiver(self) -> SourceReceiverRule {
        self.receiver
    }

    /// Returns the normalized operation identity.
    #[must_use]
    pub const fn semantic_key(self) -> MinecraftSemanticKey {
        self.semantic_key
    }
}

const SOURCE_METHODS: &[MinecraftSourceMethodRule] = &[
    MinecraftSourceMethodRule {
        name: "say",
        receiver: SourceReceiverRule::CurrentExecutor {
            required_capability: EntityCapability::CommandExecutor,
        },
        semantic_key: MinecraftSemanticKey::Say,
    },
    MinecraftSourceMethodRule {
        name: "teleport",
        receiver: SourceReceiverRule::CurrentExecutor {
            required_capability: EntityCapability::CommandExecutor,
        },
        semantic_key: MinecraftSemanticKey::TeleportCurrentExecutor,
    },
    MinecraftSourceMethodRule {
        name: "move_by",
        receiver: SourceReceiverRule::CurrentExecutor {
            required_capability: EntityCapability::CommandExecutor,
        },
        semantic_key: MinecraftSemanticKey::MoveCurrentExecutorBy,
    },
    MinecraftSourceMethodRule {
        name: "main_hand_written_book_literal_page_or_empty",
        receiver: SourceReceiverRule::CurrentExecutor {
            required_capability: EntityCapability::InventoryHolder,
        },
        semantic_key: MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage,
    },
];

/// Returns every compiler-owned Minecraft source method in declaration order.
#[must_use]
pub const fn minecraft_source_methods() -> &'static [MinecraftSourceMethodRule] {
    SOURCE_METHODS
}

/// Resolves a Minecraft method for an already-typed receiver.
///
/// This performs only closed registry lookup. Lexical capture validity, argument
/// checking, and diagnostics remain responsibilities of the frontend checker.
#[must_use]
pub fn resolve_minecraft_method(
    receiver: SemanticType,
    name: &str,
) -> Option<&'static MinecraftSourceMethodRule> {
    SOURCE_METHODS
        .iter()
        .find(|rule| rule.name == name && rule.receiver.accepts(receiver))
}

#[cfg(test)]
mod tests {
    use super::{
        MinecraftAmbientContextRule, MinecraftAttributeKind, MinecraftCommandOutcomeBehavior,
        MinecraftSemanticDescriptor, MinecraftSemanticKey, MinecraftSemanticSignature,
        MinecraftSourceMethodRule, MinecraftValidatorKind, SourceReceiverRule,
        minecraft_descriptor, minecraft_source_methods, resolve_minecraft_method,
    };
    use crate::ir::semantic::{
        ContextRequirement, EntityCapability, EntityKind, EntityRefType, ExecutorType, ForkBound,
        ObservableEffect, RuntimeValueType, SemanticType, TransitiveWork, WorldEffect,
    };
    use std::collections::BTreeSet;

    #[test]
    fn every_key_has_one_complete_descriptor() {
        let mut described = BTreeSet::new();
        for &key in MinecraftSemanticKey::ALL {
            let descriptor = minecraft_descriptor(key);
            assert_eq!(descriptor.key(), key);
            assert!(described.insert(descriptor.key()));
            assert!(!descriptor.documentation().trim().is_empty());
        }
        assert_eq!(described.len(), MinecraftSemanticKey::ALL.len());
    }

    #[test]
    fn say_contract_is_target_independent_and_complete() {
        let descriptor = *minecraft_descriptor(MinecraftSemanticKey::Say);
        assert_eq!(descriptor.signature().operands(), &[]);
        assert_eq!(
            descriptor.signature().attributes(),
            &[MinecraftAttributeKind::MessageLiteral]
        );
        assert_eq!(descriptor.signature().results(), &[]);
        assert_eq!(
            descriptor.ambient_context(),
            MinecraftAmbientContextRule::CurrentExecutorKind
        );
        assert_eq!(descriptor.world_effect(), WorldEffect::None);
        assert_eq!(descriptor.observable_effect(), ObservableEffect::Observable);
        assert_eq!(descriptor.fork_behavior(), ForkBound::None);
        assert_eq!(descriptor.work_behavior(), TransitiveWork::Finite);
        assert_eq!(
            descriptor.outcome_behavior(),
            MinecraftCommandOutcomeBehavior::Discarded
        );
        assert_eq!(
            descriptor.validator(),
            MinecraftValidatorKind::MessageLiteral
        );

        let behavior = descriptor.behavior_for_executor_kind(EntityKind::ArmorStand);
        assert_eq!(
            behavior.required_ambient_context().executor(),
            ContextRequirement::Required(EntityKind::ArmorStand)
        );
        assert_eq!(behavior.world_effect(), WorldEffect::None);
        assert_eq!(behavior.observable_effect(), ObservableEffect::Observable);
        assert!(!behavior.contains_unsafe_unknown());
    }

    #[test]
    fn source_method_is_separate_and_requires_a_capable_executor() {
        assert_eq!(minecraft_source_methods().len(), 4);
        let executor = SemanticType::Executor(ExecutorType::new(EntityKind::ArmorStand));
        let rule = resolve_minecraft_method(executor, "say").unwrap();
        assert_eq!(rule.semantic_key(), MinecraftSemanticKey::Say);
        assert_eq!(
            rule.receiver(),
            SourceReceiverRule::CurrentExecutor {
                required_capability: EntityCapability::CommandExecutor,
            }
        );

        assert_eq!(resolve_minecraft_method(executor, "unknown"), None);
        assert_eq!(
            resolve_minecraft_method(executor, "teleport").map(|rule| rule.semantic_key()),
            Some(MinecraftSemanticKey::TeleportCurrentExecutor)
        );
        assert_eq!(
            resolve_minecraft_method(executor, "move_by").map(|rule| rule.semantic_key()),
            Some(MinecraftSemanticKey::MoveCurrentExecutorBy)
        );
        assert_eq!(
            resolve_minecraft_method(executor, "main_hand_written_book_literal_page_or_empty")
                .map(|rule| rule.semantic_key()),
            Some(MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage)
        );
        assert_eq!(
            resolve_minecraft_method(
                SemanticType::EntityRef(EntityRefType::new(EntityKind::ArmorStand)),
                "say"
            ),
            None
        );
        assert_eq!(
            resolve_minecraft_method(SemanticType::Runtime(RuntimeValueType::Int32), "say"),
            None
        );
    }

    #[test]
    fn registry_keys_are_unique_and_method_keys_do_not_collide() {
        let keys = MinecraftSemanticKey::ALL
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(keys.len(), MinecraftSemanticKey::ALL.len());

        assert!(method_keys_are_unique(minecraft_source_methods()));
        let duplicate = [minecraft_source_methods()[0], minecraft_source_methods()[0]];
        assert!(!method_keys_are_unique(&duplicate));
    }

    #[test]
    fn descriptor_corruption_checks_cover_signature_behavior_and_documentation() {
        let descriptor = *minecraft_descriptor(MinecraftSemanticKey::Say);
        assert!(validate_say_descriptor(descriptor));

        let wrong_signature = MinecraftSemanticDescriptor {
            signature: MinecraftSemanticSignature::new(&[], &[], &[]),
            ..descriptor
        };
        assert!(!validate_say_descriptor(wrong_signature));

        let wrong_behavior = MinecraftSemanticDescriptor {
            observable_effect: ObservableEffect::None,
            ..descriptor
        };
        assert!(!validate_say_descriptor(wrong_behavior));

        let undocumented = MinecraftSemanticDescriptor {
            documentation: "",
            ..descriptor
        };
        assert!(!validate_say_descriptor(undocumented));
    }

    #[test]
    fn closed_lookup_stays_deterministic_at_scale() {
        let receiver = SemanticType::Executor(ExecutorType::new(EntityKind::ArmorStand));
        for index in 0..100_000 {
            let name = if index % 2 == 0 { "say" } else { "missing" };
            assert_eq!(
                resolve_minecraft_method(receiver, name).is_some(),
                index % 2 == 0
            );
        }
    }

    fn method_keys_are_unique(rules: &[MinecraftSourceMethodRule]) -> bool {
        let mut keys = BTreeSet::new();
        rules
            .iter()
            .all(|rule| keys.insert((rule.name(), rule.receiver())))
    }

    fn validate_say_descriptor(descriptor: MinecraftSemanticDescriptor) -> bool {
        descriptor.key() == MinecraftSemanticKey::Say
            && descriptor.signature().operands().is_empty()
            && descriptor.signature().attributes() == [MinecraftAttributeKind::MessageLiteral]
            && descriptor.signature().results().is_empty()
            && descriptor.ambient_context() == MinecraftAmbientContextRule::CurrentExecutorKind
            && descriptor.world_effect() == WorldEffect::None
            && descriptor.observable_effect() == ObservableEffect::Observable
            && descriptor.fork_behavior() == ForkBound::None
            && descriptor.work_behavior() == TransitiveWork::Finite
            && descriptor.outcome_behavior() == MinecraftCommandOutcomeBehavior::Discarded
            && descriptor.validator() == MinecraftValidatorKind::MessageLiteral
            && !descriptor.documentation().trim().is_empty()
    }
}
