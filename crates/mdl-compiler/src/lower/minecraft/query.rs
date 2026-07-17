//! Exhaustive target conversion for compiler-known semantic entity queries.

use crate::ir::core::{EntityQueryDecl, EntityQueryStep};
use crate::ir::minecraft::{EntitySelector, EntitySelectorError};
use crate::ir::semantic::{EntityKind, StaticEntityQuery};
use crate::source::OriginId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct QueryLoweringFailure {
    pub(super) error: EntitySelectorError,
    pub(super) origin: OriginId,
}

pub(super) fn lower_entity_query(
    query: &EntityQueryDecl,
) -> Result<EntitySelector, QueryLoweringFailure> {
    lower_static_entity_query(query.semantic()).map_err(|error| QueryLoweringFailure {
        origin: failure_origin(query, error),
        error,
    })
}

fn failure_origin(query: &EntityQueryDecl, error: EntitySelectorError) -> OriginId {
    match error {
        EntitySelectorError::LimitOutOfRange(_) | EntitySelectorError::ZeroLimit => query
            .effective_limit_value_origin()
            .or_else(|| query.origin())
            .unwrap_or(OriginId::UNKNOWN),
        EntitySelectorError::EmptyTag { index }
        | EntitySelectorError::InvalidTagCharacter { index, .. } => query
            .steps()
            .iter()
            .filter_map(|step| match step {
                EntityQueryStep::WithTag { value_origin, .. } => Some(*value_origin),
                EntityQueryStep::Entities { .. } | EntityQueryStep::Limit { .. } => None,
            })
            .nth(index)
            .or_else(|| query.origin())
            .unwrap_or(OriginId::UNKNOWN),
    }
}

pub(super) fn lower_static_entity_query(
    query: &StaticEntityQuery,
) -> Result<EntitySelector, EntitySelectorError> {
    let tags = query
        .tags()
        .iter()
        .map(|tag| Box::<str>::from(tag.as_str()))
        .collect();
    match query.kind() {
        EntityKind::ArmorStand => EntitySelector::armor_stands(tags, query.maximum()),
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{failure_origin, lower_entity_query, lower_static_entity_query};
    use crate::entity::EntityId;
    use crate::ir::core::{EntityQueryDecl, EntityQueryStep};
    use crate::ir::minecraft::EntitySelectorError;
    use crate::ir::semantic::{EntityKind, EntityTag, StaticEntityQuery};
    use crate::source::OriginId;

    #[test]
    fn conversion_preserves_kind_tags_and_limit_exactly() {
        let query = StaticEntityQuery::entities(EntityKind::ArmorStand)
            .with_tag(EntityTag::new("first").unwrap())
            .with_tag(EntityTag::new("second").unwrap())
            .limit(1)
            .unwrap();
        let selector = lower_static_entity_query(&query).unwrap();
        assert_eq!(
            selector.to_string(),
            "@e[type=minecraft:armor_stand,tag=first,tag=second,limit=1]"
        );
    }

    #[test]
    fn target_range_validation_stays_at_the_shared_conversion_boundary() {
        let query = StaticEntityQuery::entities(EntityKind::ArmorStand)
            .limit(i32::MAX as u32 + 1)
            .unwrap();
        assert_eq!(
            lower_static_entity_query(&query),
            Err(EntitySelectorError::LimitOutOfRange(i32::MAX as u32 + 1))
        );
    }

    #[test]
    fn declaration_conversion_attributes_limit_failure_to_the_effective_literal() {
        let query = StaticEntityQuery::entities(EntityKind::ArmorStand)
            .limit(u32::MAX)
            .unwrap();
        let value_origin = OriginId::from_index(7);
        let declaration = EntityQueryDecl::new(
            query,
            vec![
                EntityQueryStep::Entities {
                    kind: EntityKind::ArmorStand,
                    origin: OriginId::from_index(1),
                    kind_origin: OriginId::from_index(2),
                },
                EntityQueryStep::Limit {
                    maximum: NonZeroU32::new(u32::MAX).unwrap(),
                    origin: OriginId::from_index(6),
                    value_origin,
                },
            ],
        );
        let failure = lower_entity_query(&declaration).unwrap_err();
        assert_eq!(
            failure.error,
            EntitySelectorError::LimitOutOfRange(u32::MAX)
        );
        assert_eq!(failure.origin, value_origin);
    }

    #[test]
    fn indexed_tag_failure_selects_the_matching_refinement_origin() {
        let first_origin = OriginId::from_index(3);
        let second_origin = OriginId::from_index(7);
        let first = EntityTag::new("first").unwrap();
        let second = EntityTag::new("second").unwrap();
        let declaration = EntityQueryDecl::new(
            StaticEntityQuery::entities(EntityKind::ArmorStand)
                .with_tag(first.clone())
                .with_tag(second.clone()),
            vec![
                EntityQueryStep::Entities {
                    kind: EntityKind::ArmorStand,
                    origin: OriginId::from_index(1),
                    kind_origin: OriginId::from_index(2),
                },
                EntityQueryStep::WithTag {
                    tag: first,
                    origin: OriginId::from_index(3),
                    value_origin: first_origin,
                },
                EntityQueryStep::WithTag {
                    tag: second,
                    origin: OriginId::from_index(6),
                    value_origin: second_origin,
                },
            ],
        );
        assert_eq!(
            failure_origin(
                &declaration,
                EntitySelectorError::InvalidTagCharacter {
                    index: 1,
                    character: ':',
                },
            ),
            second_origin
        );
    }
}
