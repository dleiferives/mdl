use super::{CoreProgram, FunctionId, ProgramError};
use crate::entity::EntityLimitError;
use crate::source::OriginId;

/// A validated `namespace:path` item-resource-id spelling (PS-15 Slice 1:
/// item ID only, no count/component predicates). Validation happens in the
/// checker, before Core ever sees the string — Core stores the already-
/// checked spelling as a plain wrapper, the same discipline
/// `ir/core/entity_nbt.rs` uses for NBT keys, so Core never depends on
/// `ir::minecraft`'s target-facing resource-id types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemMatch(Box<str>);

impl ItemMatch {
    /// Wraps an already-validated item-resource-id spelling.
    #[must_use]
    pub fn new(id: impl Into<Box<str>>) -> Self {
        Self(id.into())
    }

    /// Returns the validated `namespace:path` spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Closed advancement-trigger vocabulary (Slice 1: one variant). Growing
/// this to a second trigger is "add a variant", matching every other
/// closed-vocabulary table in this compiler (`MinecraftSemanticKey`,
/// `SchemaNode`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Criterion {
    /// `minecraft:inventory_changed`, matched against a fixed set of item IDs.
    InventoryChanged {
        /// At least one item resource id (checker-enforced non-empty).
        items: Vec<ItemMatch>,
    },
}

/// One program-owned advancement declaration: a criterion and the reward
/// function vanilla invokes when it is satisfied.
///
/// `reward` is a full `FunctionId`, not source-level identity — the reward
/// body itself is an ordinary Core function like any other; only this
/// declaration marks it as an advancement's reward.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdvancementDecl {
    reward: FunctionId,
    criterion: Criterion,
    origin: OriginId,
}

impl AdvancementDecl {
    /// Returns the reward function invoked when the criterion is satisfied.
    #[must_use]
    pub const fn reward(&self) -> FunctionId {
        self.reward
    }

    /// Returns the closed trigger criterion.
    #[must_use]
    pub const fn criterion(&self) -> &Criterion {
        &self.criterion
    }

    /// Returns declaration provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

impl CoreProgram {
    /// Declares one advancement in stable order.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown reward function, a reward with
    /// nonzero parameters or results (a reward function always has the same
    /// zero-parameter `Void` shape as an ordinary MDL event-handler body),
    /// or exhaustion of the advancement identity space.
    pub fn declare_advancement(
        &mut self,
        reward: FunctionId,
        criterion: Criterion,
        origin: OriginId,
    ) -> Result<super::AdvancementId, ProgramError> {
        let declaration = self
            .functions
            .get(reward)
            .ok_or(ProgramError::InvalidAdvancementReward { function: reward })?;
        if !declaration.parameters.is_empty() || !declaration.results.is_empty() {
            return Err(ProgramError::InvalidAdvancementReward { function: reward });
        }
        self.advancements
            .push(AdvancementDecl {
                reward,
                criterion,
                origin,
            })
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Returns an advancement declaration, or `None` for a foreign identity.
    #[must_use]
    pub fn advancement(&self, advancement: super::AdvancementId) -> Option<&AdvancementDecl> {
        self.advancements.get(advancement)
    }

    /// Iterates advancement declarations in stable declaration order.
    #[must_use]
    pub fn advancements(
        &self,
    ) -> impl ExactSizeIterator<Item = (super::AdvancementId, &AdvancementDecl)> + '_ {
        self.advancements.iter()
    }
}
