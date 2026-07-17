//! Program-owned semantic entity-query occurrences and provenance.

use std::num::NonZeroU32;

use super::{CoreProgram, EntityQueryId, ProgramError};
use crate::entity::EntityLimitError;
use crate::ir::semantic::{EntityKind, EntityTag, StaticEntityQuery};
use crate::source::OriginId;

/// One source-ordered construction step for a semantic entity query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntityQueryStep {
    /// Start a query for one nominal entity kind.
    Entities {
        /// Selected nominal kind.
        kind: EntityKind,
        /// Provenance of the complete root call.
        origin: OriginId,
        /// Provenance of the nominal-kind argument.
        kind_origin: OriginId,
    },
    /// Add one exact entity-tag filter.
    WithTag {
        /// Validated semantic tag.
        tag: EntityTag,
        /// Provenance of the complete refinement call.
        origin: OriginId,
        /// Provenance of the literal tag value.
        value_origin: OriginId,
    },
    /// Add one positive maximum result count.
    Limit {
        /// Requested positive maximum.
        maximum: NonZeroU32,
        /// Provenance of the complete refinement call.
        origin: OriginId,
        /// Provenance of the integer argument.
        value_origin: OriginId,
    },
}

impl EntityQueryStep {
    /// Returns the complete step provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        match self {
            Self::Entities { origin, .. }
            | Self::WithTag { origin, .. }
            | Self::Limit { origin, .. } => *origin,
        }
    }

    /// Returns the operand provenance carried by this step.
    #[must_use]
    pub const fn value_origin(&self) -> OriginId {
        match self {
            Self::Entities { kind_origin, .. } => *kind_origin,
            Self::WithTag { value_origin, .. } | Self::Limit { value_origin, .. } => *value_origin,
        }
    }
}

/// One canonical semantic query paired with every source construction step.
#[derive(Clone, Debug)]
pub struct EntityQueryDecl {
    semantic: StaticEntityQuery,
    steps: Box<[EntityQueryStep]>,
}

impl EntityQueryDecl {
    /// Creates a candidate query declaration.
    ///
    /// [`CoreProgram::declare_entity_query`] validates that replaying `steps`
    /// exactly reproduces `semantic` before the declaration enters a program.
    #[must_use]
    pub fn new(semantic: StaticEntityQuery, steps: Vec<EntityQueryStep>) -> Self {
        Self {
            semantic,
            steps: steps.into_boxed_slice(),
        }
    }

    /// Creates a canonical declaration for a compiler-generated query occurrence.
    ///
    /// Source lowering should prefer [`EntityQueryDecl::new`] with its exact step
    /// origins. This constructor is for generated IR that has one aggregate origin.
    #[must_use]
    pub fn from_semantic(semantic: StaticEntityQuery, origin: OriginId) -> Self {
        let mut steps = vec![EntityQueryStep::Entities {
            kind: semantic.kind(),
            origin,
            kind_origin: origin,
        }];
        steps.extend(
            semantic
                .tags()
                .iter()
                .cloned()
                .map(|tag| EntityQueryStep::WithTag {
                    tag,
                    origin,
                    value_origin: origin,
                }),
        );
        if let Some(maximum) = semantic.maximum().and_then(NonZeroU32::new) {
            steps.push(EntityQueryStep::Limit {
                maximum,
                origin,
                value_origin: origin,
            });
        }
        Self::new(semantic, steps)
    }

    /// Returns the canonical, origin-free query semantics.
    #[must_use]
    pub const fn semantic(&self) -> &StaticEntityQuery {
        &self.semantic
    }

    /// Returns every source construction step in exact order.
    #[must_use]
    pub fn steps(&self) -> &[EntityQueryStep] {
        &self.steps
    }

    /// Returns the query root provenance when the declaration is well formed.
    #[must_use]
    pub fn origin(&self) -> Option<OriginId> {
        match self.steps.first() {
            Some(EntityQueryStep::Entities { origin, .. }) => Some(*origin),
            Some(EntityQueryStep::WithTag { .. } | EntityQueryStep::Limit { .. }) | None => None,
        }
    }

    /// Returns the literal origin that established the canonical effective limit.
    ///
    /// Repeated limits remain in the occurrence trace. The first occurrence of the
    /// smallest requested maximum is the one that determines the canonical plan.
    #[must_use]
    pub fn effective_limit_value_origin(&self) -> Option<OriginId> {
        let mut effective: Option<(u32, OriginId)> = None;
        for step in &self.steps {
            let EntityQueryStep::Limit {
                maximum,
                value_origin,
                ..
            } = step
            else {
                continue;
            };
            if effective.is_none_or(|(current, _)| maximum.get() < current) {
                effective = Some((maximum.get(), *value_origin));
            }
        }
        effective.map(|(_, origin)| origin)
    }

    pub(crate) fn is_well_formed(&self) -> bool {
        replay_steps(&self.steps).is_some_and(|replayed| replayed == self.semantic)
    }
}

fn replay_steps(steps: &[EntityQueryStep]) -> Option<StaticEntityQuery> {
    let mut steps = steps.iter();
    let EntityQueryStep::Entities { kind, .. } = steps.next()? else {
        return None;
    };
    let mut query = StaticEntityQuery::entities(*kind);
    for step in steps {
        match step {
            EntityQueryStep::Entities { .. } => return None,
            EntityQueryStep::WithTag { tag, .. } => {
                query = query.with_tag(tag.clone());
            }
            EntityQueryStep::Limit { maximum, .. } => {
                query = query.limit(maximum.get()).ok()?;
            }
        }
    }
    Some(query)
}

impl CoreProgram {
    /// Stores one immutable, verified semantic entity-query occurrence.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramError::InvalidEntityQueryDeclaration`] when the ordered
    /// steps do not reproduce the canonical semantics, or an entity-limit error
    /// when the query ID space is exhausted.
    pub fn declare_entity_query(
        &mut self,
        query: EntityQueryDecl,
    ) -> Result<EntityQueryId, ProgramError> {
        if !query.is_well_formed() {
            return Err(ProgramError::InvalidEntityQueryDeclaration);
        }
        self.entity_queries
            .push(query)
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Returns one semantic entity-query declaration.
    #[must_use]
    pub fn entity_query(&self, query: EntityQueryId) -> Option<&EntityQueryDecl> {
        self.entity_queries.get(query)
    }

    /// Iterates semantic entity queries in stable identity order.
    #[must_use]
    pub fn entity_queries(
        &self,
    ) -> impl ExactSizeIterator<Item = (EntityQueryId, &EntityQueryDecl)> + '_ {
        self.entity_queries.iter()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{EntityQueryDecl, EntityQueryStep};
    use crate::entity::EntityId;
    use crate::ir::core::{CoreProgram, ProgramError, verify_program};
    use crate::ir::semantic::{EntityKind, EntityTag, StaticEntityQuery};
    use crate::source::{OriginId, SourceContext};

    fn occurrence() -> EntityQueryDecl {
        let tag = EntityTag::new("stage7").unwrap();
        let semantic = StaticEntityQuery::entities(EntityKind::ArmorStand)
            .with_tag(tag.clone())
            .limit(10)
            .unwrap()
            .limit(1)
            .unwrap();
        EntityQueryDecl::new(
            semantic,
            vec![
                EntityQueryStep::Entities {
                    kind: EntityKind::ArmorStand,
                    origin: OriginId::UNKNOWN,
                    kind_origin: OriginId::UNKNOWN,
                },
                EntityQueryStep::WithTag {
                    tag,
                    origin: OriginId::UNKNOWN,
                    value_origin: OriginId::UNKNOWN,
                },
                EntityQueryStep::Limit {
                    maximum: NonZeroU32::new(10).unwrap(),
                    origin: OriginId::UNKNOWN,
                    value_origin: OriginId::UNKNOWN,
                },
                EntityQueryStep::Limit {
                    maximum: NonZeroU32::new(1).unwrap(),
                    origin: OriginId::UNKNOWN,
                    value_origin: OriginId::UNKNOWN,
                },
            ],
        )
    }

    #[test]
    fn declaration_replays_every_step_and_retains_effective_limit_origin() {
        let mut program = CoreProgram::new();
        let mut occurrence = occurrence();
        let EntityQueryStep::Limit { value_origin, .. } = &mut occurrence.steps[2] else {
            unreachable!();
        };
        *value_origin = OriginId::from_index(1);
        let EntityQueryStep::Limit { value_origin, .. } = &mut occurrence.steps[3] else {
            unreachable!();
        };
        *value_origin = OriginId::from_index(2);
        let mut steps = occurrence.steps.into_vec();
        steps.push(EntityQueryStep::Limit {
            maximum: NonZeroU32::new(1).unwrap(),
            origin: OriginId::from_index(3),
            value_origin: OriginId::from_index(3),
        });
        occurrence.steps = steps.into_boxed_slice();
        let query = program.declare_entity_query(occurrence).unwrap();
        let declaration = program.entity_query(query).unwrap();
        assert_eq!(declaration.steps().len(), 5);
        assert_eq!(declaration.semantic().maximum(), Some(1));
        assert_eq!(
            declaration.effective_limit_value_origin(),
            Some(OriginId::from_index(2))
        );
    }

    #[test]
    fn declaration_rejects_missing_or_repeated_roots_and_semantic_drift() {
        let mut missing = occurrence();
        missing.steps = missing.steps[1..].to_vec().into_boxed_slice();
        assert_eq!(
            CoreProgram::new().declare_entity_query(missing),
            Err(ProgramError::InvalidEntityQueryDeclaration)
        );

        let mut repeated = occurrence();
        let root = repeated.steps[0].clone();
        repeated.steps[1] = root;
        assert_eq!(
            CoreProgram::new().declare_entity_query(repeated),
            Err(ProgramError::InvalidEntityQueryDeclaration)
        );

        let mut drifted = occurrence();
        drifted.semantic = StaticEntityQuery::entities(EntityKind::ArmorStand);
        assert_eq!(
            CoreProgram::new().declare_entity_query(drifted),
            Err(ProgramError::InvalidEntityQueryDeclaration)
        );
    }

    #[test]
    fn whole_program_verifier_replays_inventory_and_checks_step_origins() {
        let sources = SourceContext::new();
        let mut malformed = CoreProgram::new();
        let query = malformed.declare_entity_query(occurrence()).unwrap();
        malformed
            .entity_queries
            .get_mut(query)
            .unwrap()
            .steps
            .swap(0, 1);
        assert!(
            verify_program(&malformed, &sources)
                .unwrap_err()
                .contains_code("core.invalid-entity-query")
        );

        let mut foreign_origin = CoreProgram::new();
        let query = foreign_origin.declare_entity_query(occurrence()).unwrap();
        foreign_origin.entity_queries.get_mut(query).unwrap().steps[0] =
            EntityQueryStep::Entities {
                kind: EntityKind::ArmorStand,
                origin: OriginId::from_index(99),
                kind_origin: OriginId::UNKNOWN,
            };
        assert!(
            verify_program(&foreign_origin, &sources)
                .unwrap_err()
                .contains_code("core.invalid-origin")
        );
    }
}
