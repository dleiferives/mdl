//! Ordered source-HIR execution-context transfer.

use super::hir::{HirContextStep, HirExecutionContext, HirRunModifier, SourceRunId};
use crate::ir::semantic::{
    AmbientContextRequirements, ContextRequirement, EntityCapability, ExecutionContext,
};
use crate::source::OriginId;

/// A malformed typed modifier that cannot establish its promised context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ContextTransitionError {
    pub(super) origin: OriginId,
}

/// A body requirement that conflicts with a modifier's established context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ContextRequirementTransferError {
    pub(super) origin: OriginId,
}

/// Returns the execution context visible at an ordinary source-function entry.
#[must_use]
pub(super) const fn function_entry_context() -> HirExecutionContext {
    ExecutionContext::function_entry()
}

/// Applies typed modifiers in exact source order.
///
/// The fold is deliberately separate from parsing/checking and is replayed by HIR
/// verification. Future modifier variants extend this one transfer boundary.
pub(super) fn apply_run_modifiers(
    input: HirExecutionContext,
    run: SourceRunId,
    modifiers: &[HirRunModifier],
) -> Result<HirExecutionContext, ContextTransitionError> {
    let mut context = input;
    for (modifier_index, modifier) in modifiers.iter().enumerate() {
        match modifier {
            HirRunModifier::As { query, origin } => {
                let kind = query.semantic.kind();
                if !kind
                    .capabilities()
                    .contains(EntityCapability::CommandExecutor)
                {
                    return Err(ContextTransitionError { origin: *origin });
                }
                context = context.establish_executor(
                    kind,
                    HirContextStep {
                        run,
                        modifier_index,
                        origin: *origin,
                    },
                );
            }
            HirRunModifier::At { origin, .. } | HirRunModifier::AtExecutor { origin, .. } => {
                let step = HirContextStep {
                    run,
                    modifier_index,
                    origin: *origin,
                };
                context = context
                    .establish_position(step)
                    .establish_rotation(step)
                    .establish_dimension(step);
            }
            HirRunModifier::Positioned { origin, .. } | HirRunModifier::Align { origin, .. } => {
                context = context.establish_position(HirContextStep {
                    run,
                    modifier_index,
                    origin: *origin,
                });
            }
            HirRunModifier::Rotated { origin, .. } => {
                context = context.establish_rotation(HirContextStep {
                    run,
                    modifier_index,
                    origin: *origin,
                });
            }
            HirRunModifier::In { origin, .. } => {
                let step = HirContextStep {
                    run,
                    modifier_index,
                    origin: *origin,
                };
                context = context.establish_position(step).establish_dimension(step);
            }
            HirRunModifier::Anchored { origin, .. } => {
                context = context.establish_anchor(HirContextStep {
                    run,
                    modifier_index,
                    origin: *origin,
                });
            }
        }
    }
    Ok(context)
}

/// Transfers a run body's ambient requirements back through its modifiers.
///
/// This is the requirement-side dual of [`apply_run_modifiers`]. Processing in
/// reverse is essential: a later modifier may satisfy a body requirement, but it
/// cannot retroactively satisfy a query evaluated by an earlier modifier.
pub(super) fn transfer_run_requirements(
    mut requirements: AmbientContextRequirements,
    modifiers: &[HirRunModifier],
) -> Result<AmbientContextRequirements, ContextRequirementTransferError> {
    for modifier in modifiers.iter().rev() {
        match modifier {
            HirRunModifier::As { query, origin } => {
                match requirements.executor() {
                    ContextRequirement::Required(required) if required != query.semantic.kind() => {
                        return Err(ContextRequirementTransferError { origin: *origin });
                    }
                    ContextRequirement::None
                    | ContextRequirement::Required(_)
                    | ContextRequirement::Unknown => {}
                }

                // `execute as` establishes the current executor for everything
                // after it. Resolving the selector itself still consumes the
                // current position and dimension under the supported contract.
                requirements = requirements.with_executor(ContextRequirement::None).join(
                    AmbientContextRequirements::NONE
                        .with_position(ContextRequirement::Required(()))
                        .with_dimension(ContextRequirement::Required(())),
                );
            }
            HirRunModifier::At { origin, .. } => {
                requirements = requirements
                    .with_position(ContextRequirement::None)
                    .with_rotation(ContextRequirement::None)
                    .with_dimension(ContextRequirement::None)
                    .join(
                        AmbientContextRequirements::NONE
                            .with_position(ContextRequirement::Required(()))
                            .with_dimension(ContextRequirement::Required(())),
                    );
                let _ = origin;
            }
            HirRunModifier::AtExecutor { kind, .. } => {
                requirements = requirements
                    .with_position(ContextRequirement::None)
                    .with_rotation(ContextRequirement::None)
                    .with_dimension(ContextRequirement::None)
                    .join(
                        AmbientContextRequirements::NONE
                            .with_executor(ContextRequirement::Required(*kind)),
                    );
            }
            HirRunModifier::Positioned { position, .. } => {
                requirements = requirements.with_position(ContextRequirement::None);
                match position {
                    crate::ir::semantic::PositionSpec::World(position)
                        if position.reads_position() =>
                    {
                        requirements = requirements.with_position(ContextRequirement::Required(()));
                    }
                    crate::ir::semantic::PositionSpec::Local(_) => {
                        requirements = requirements
                            .with_position(ContextRequirement::Required(()))
                            .with_rotation(ContextRequirement::Required(()))
                            .with_anchor(ContextRequirement::Required(()));
                    }
                    crate::ir::semantic::PositionSpec::World(_) => {}
                }
            }
            HirRunModifier::Rotated { rotation, .. } => {
                requirements = requirements.with_rotation(if rotation.reads_rotation() {
                    ContextRequirement::Required(())
                } else {
                    ContextRequirement::None
                });
            }
            HirRunModifier::In { .. } => {
                requirements = requirements
                    .with_position(ContextRequirement::Required(()))
                    .with_dimension(ContextRequirement::Required(()));
            }
            HirRunModifier::Anchored { .. } => {
                requirements = requirements.with_anchor(ContextRequirement::None);
            }
            HirRunModifier::Align { .. } => {
                requirements = requirements.with_position(ContextRequirement::Required(()));
            }
        }
    }
    Ok(requirements)
}

#[cfg(test)]
mod tests {
    use super::{apply_run_modifiers, function_entry_context, transfer_run_requirements};
    use crate::entity::EntityId;
    use crate::frontend::hir::{HirEntityQuery, HirEntityQueryStep, HirRunModifier, SourceRunId};
    use crate::ir::semantic::{
        AmbientContextRequirements, ContextFact, ContextRequirement, EntityKind, StaticEntityQuery,
    };
    use crate::source::OriginId;

    fn query() -> HirEntityQuery {
        HirEntityQuery {
            semantic: StaticEntityQuery::entities(EntityKind::ArmorStand),
            steps: vec![HirEntityQueryStep::Entities {
                kind: EntityKind::ArmorStand,
                origin: OriginId::UNKNOWN,
                kind_origin: OriginId::UNKNOWN,
            }],
        }
    }

    #[test]
    fn repeated_as_is_ordered_and_preserves_the_non_executor_frame() {
        let input = function_entry_context();
        let output = apply_run_modifiers(
            input,
            SourceRunId::from_index(3).unwrap(),
            &[
                HirRunModifier::As {
                    query: query(),
                    origin: OriginId::from_index(1),
                },
                HirRunModifier::As {
                    query: query(),
                    origin: OriginId::from_index(2),
                },
            ],
        )
        .unwrap();

        let ContextFact::Established { by, .. } = output.executor() else {
            panic!("expected an established executor");
        };
        assert_eq!(by.modifier_index, 1);
        assert_eq!(output.position(), input.position());
        assert_eq!(output.rotation(), input.rotation());
        assert_eq!(output.dimension(), input.dimension());
        assert_eq!(output.anchor(), input.anchor());
    }

    #[test]
    fn reverse_transfer_discharges_executor_but_retains_query_frame_reads() {
        let body = AmbientContextRequirements::UNKNOWN
            .with_executor(ContextRequirement::Required(EntityKind::ArmorStand));
        let input = transfer_run_requirements(
            body,
            &[HirRunModifier::As {
                query: query(),
                origin: OriginId::from_index(1),
            }],
        )
        .unwrap();

        assert_eq!(input.executor(), ContextRequirement::None);
        assert_eq!(input.position(), ContextRequirement::Unknown);
        assert_eq!(input.dimension(), ContextRequirement::Unknown);
        assert_eq!(input.rotation(), ContextRequirement::Unknown);
        assert_eq!(input.anchor(), ContextRequirement::Unknown);
    }

    #[test]
    fn query_frame_reads_are_added_to_an_otherwise_empty_body() {
        let input = transfer_run_requirements(
            AmbientContextRequirements::NONE,
            &[HirRunModifier::As {
                query: query(),
                origin: OriginId::from_index(1),
            }],
        )
        .unwrap();

        assert_eq!(input.executor(), ContextRequirement::None);
        assert_eq!(input.position(), ContextRequirement::Required(()));
        assert_eq!(input.dimension(), ContextRequirement::Required(()));
        assert_eq!(input.rotation(), ContextRequirement::None);
        assert_eq!(input.anchor(), ContextRequirement::None);
    }
}
