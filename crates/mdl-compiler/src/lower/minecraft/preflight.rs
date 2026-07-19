//! Retained, independently verifiable target-recipe selection.

use crate::analysis::minecraft::{
    CommandLimitAssumptions, CommandOutcome, CommandStepCost, CommandStepCounts, CountBound,
};
use crate::diagnostic::{Diagnostic, DiagnosticLabel, Diagnostics};
use crate::entity::{EntityLimitError, EntityVec};
use crate::ir::core::{
    CoreOp, CoreProgram, ExternalOpId, ExternalSemanticBinding, MinecraftOperationAttributes,
    MinecraftOperationDecl, MinecraftOperationId, RunModifierInstance, RunScopeId,
};
use crate::ir::minecraft::{
    CommandContract, CommandKind, ContextMask, ContextSummary, DataCommand, DataModifyMode,
    DataSource, EffectCategories, EffectSummary, ExecuteCommand, ExecuteModifier,
    ExecuteModifierKind, ExecuteModifiers, ForkClass, JavaDecimal, NativeCommandOutcome, NbtPath,
    NbtPathKey, NbtPathSegment, SayCommand, SayMessage, Selector, StorageId, StoragePath,
    TargetAnchor, TargetAxes, TargetLocalPosition, TargetPosition, TargetRotation,
    TargetRotationAxis, TargetWorldAxis, TargetWorldPosition, TeleportCommand,
};
use crate::ir::semantic::{
    AmbientContextRequirements, ContextRequirement, EntityKind, ForkBound, FunctionBehavior,
    MAX_PACKAGE_RUN_MODIFIERS, MinecraftCommandOutcomeBehavior, MinecraftSemanticDescriptor,
    MinecraftSemanticKey, ObservableEffect, TransitiveWork, WorldEffect, minecraft_descriptor,
};
use crate::source::OriginId;
use crate::target::JavaEditionTarget;

use super::analysis::SemanticInventory;
use super::api::CommandLimitEvidence;

const SAY_COMMAND_PREFIX: &str = "say ";
const SAY_LOCAL_OUTCOMES: &[CommandOutcome] = &[CommandOutcome::Continue];
const TELEPORT_LOCAL_OUTCOMES: &[CommandOutcome] = &[
    CommandOutcome::Continue,
    CommandOutcome::NoResult,
    CommandOutcome::Fail,
];

/// One recipe's cross-layer semantic and primary-command physical contract.
///
/// Recipe selection owns this projection so the semantic registry, structured
/// target contract, and target cost classifier are compared with one expectation
/// instead of repeating recipe facts at each verification boundary. Any required
/// adjacent setup is verified as part of the constructed fragment during
/// reconciliation and remains visible to whole-target cost analysis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RecipeContractProjection {
    semantic_key: MinecraftSemanticKey,
    semantic_behavior: FunctionBehavior,
    semantic_outcome: MinecraftCommandOutcomeBehavior,
    target_effects: EffectSummary,
    target_context: ContextSummary,
    target_fork: ForkClass,
    target_native_outcome: NativeCommandOutcome,
    local_counts: CommandStepCounts,
    local_maximum_chain_expansion: CountBound,
    local_outcomes: &'static [CommandOutcome],
}

impl RecipeContractProjection {
    /// Checks the target-independent registry after instantiating its ambient
    /// requirement for this operation occurrence's receiver kind.
    pub(crate) fn matches_semantic_descriptor(
        self,
        descriptor: &MinecraftSemanticDescriptor,
        receiver_kind: EntityKind,
    ) -> bool {
        let baseline = descriptor.behavior_for_executor_kind(receiver_kind);
        descriptor.key() == self.semantic_key
            && baseline.world_effect() == self.semantic_behavior.world_effect()
            && baseline.observable_effect() == self.semantic_behavior.observable_effect()
            && baseline.fork_bound() == self.semantic_behavior.fork_bound()
            && baseline.transitive_work() == self.semantic_behavior.transitive_work()
            && baseline.contains_unsafe_unknown()
                == self.semantic_behavior.contains_unsafe_unknown()
            && descriptor.outcome_behavior() == self.semantic_outcome
    }

    /// Checks facts derived from the immutable structured target command.
    pub(crate) fn matches_target_contract(self, actual: CommandContract) -> bool {
        actual.effects() == self.target_effects
            && actual.context() == self.target_context
            && actual.fork() == self.target_fork
            && actual.native_outcome() == self.target_native_outcome
    }

    /// Checks the local target-cost classification of the constructed command.
    pub(crate) fn matches_local_cost(self, actual: &CommandStepCost) -> bool {
        actual.counts() == self.local_counts
            && actual.maximum_chain_expansion() == self.local_maximum_chain_expansion
            && actual.outcomes() == self.local_outcomes
    }
}

/// Exact target-specific construction recipe selected for a semantic operation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MinecraftRecipeId {
    /// Java Edition 26.2's native structured `say` command.
    Java26_2Say,
    /// Java Edition 26.2 structured `teleport @s <position>`.
    Java26_2TeleportCurrentExecutor,
    /// Java Edition 26.2 `execute at @s run teleport @s ~...`.
    Java26_2MoveCurrentExecutorBy,
    /// Java Edition 26.2 main-hand literal written-book page read.
    Java26_2ReadMainHandWrittenBookLiteralPage,
}

impl MinecraftRecipeId {
    /// Returns the recipe family supported for one normalized meaning and target.
    ///
    /// The current one-key/one-target product is total. This deliberately becomes
    /// fallible only when a real future key/target pair is unsupported.
    #[must_use]
    pub const fn for_semantic_key(target: JavaEditionTarget, key: MinecraftSemanticKey) -> Self {
        match (target, key) {
            (JavaEditionTarget::V26_2, MinecraftSemanticKey::Say) => Self::Java26_2Say,
            (JavaEditionTarget::V26_2, MinecraftSemanticKey::TeleportCurrentExecutor) => {
                Self::Java26_2TeleportCurrentExecutor
            }
            (JavaEditionTarget::V26_2, MinecraftSemanticKey::MoveCurrentExecutorBy) => {
                Self::Java26_2MoveCurrentExecutorBy
            }
            (
                JavaEditionTarget::V26_2,
                MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage,
            ) => Self::Java26_2ReadMainHandWrittenBookLiteralPage,
        }
    }

    /// Returns the complete semantic/target contract promised by this recipe for
    /// one already-checked receiver kind.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "the closed recipe registry keeps each complete projection adjacent"
    )]
    pub(crate) fn contract_projection(self, receiver_kind: EntityKind) -> RecipeContractProjection {
        match self {
            Self::Java26_2Say => RecipeContractProjection {
                semantic_key: MinecraftSemanticKey::Say,
                semantic_behavior: FunctionBehavior::new(
                    AmbientContextRequirements::NONE
                        .with_executor(ContextRequirement::Required(receiver_kind)),
                    WorldEffect::None,
                    ObservableEffect::Observable,
                    ForkBound::None,
                    TransitiveWork::Finite,
                    false,
                ),
                semantic_outcome: MinecraftCommandOutcomeBehavior::Discarded,
                target_effects: EffectSummary::Known(EffectCategories::OUTPUT),
                target_context: ContextSummary::Known {
                    reads: ContextMask::EXECUTOR,
                    changes: ContextMask::NONE,
                },
                target_fork: ForkClass::NEVER,
                target_native_outcome: NativeCommandOutcome::Exact(1),
                local_counts: CommandStepCounts::new(1, 0, 0, 0),
                local_maximum_chain_expansion: CountBound::exact(0),
                local_outcomes: SAY_LOCAL_OUTCOMES,
            },
            Self::Java26_2TeleportCurrentExecutor => RecipeContractProjection {
                semantic_key: MinecraftSemanticKey::TeleportCurrentExecutor,
                semantic_behavior: FunctionBehavior::new(
                    AmbientContextRequirements::NONE
                        .with_executor(ContextRequirement::Required(receiver_kind)),
                    WorldEffect::Write,
                    ObservableEffect::None,
                    ForkBound::None,
                    TransitiveWork::Finite,
                    false,
                ),
                semantic_outcome: MinecraftCommandOutcomeBehavior::Discarded,
                target_effects: EffectSummary::Known(EffectCategories::ENTITY_WRITE),
                target_context: ContextSummary::Known {
                    reads: ContextMask::EXECUTOR.union(ContextMask::DIMENSION),
                    changes: ContextMask::NONE,
                },
                target_fork: ForkClass::NEVER,
                target_native_outcome: NativeCommandOutcome::Unknown,
                local_counts: CommandStepCounts::new(1, 0, 0, 0),
                local_maximum_chain_expansion: CountBound::exact(0),
                local_outcomes: TELEPORT_LOCAL_OUTCOMES,
            },
            Self::Java26_2MoveCurrentExecutorBy => RecipeContractProjection {
                semantic_key: MinecraftSemanticKey::MoveCurrentExecutorBy,
                semantic_behavior: FunctionBehavior::new(
                    AmbientContextRequirements::NONE
                        .with_executor(ContextRequirement::Required(receiver_kind)),
                    WorldEffect::Write,
                    ObservableEffect::None,
                    ForkBound::None,
                    TransitiveWork::Finite,
                    false,
                ),
                semantic_outcome: MinecraftCommandOutcomeBehavior::Discarded,
                target_effects: EffectSummary::Known(
                    EffectCategories::ENTITY_QUERY.union(EffectCategories::ENTITY_WRITE),
                ),
                target_context: ContextSummary::Known {
                    reads: ContextMask::EXECUTOR
                        .union(ContextMask::POSITION)
                        .union(ContextMask::DIMENSION),
                    changes: ContextMask::POSITION
                        .union(ContextMask::ROTATION)
                        .union(ContextMask::DIMENSION),
                },
                target_fork: ForkClass::AT_MOST_ONE,
                target_native_outcome: NativeCommandOutcome::Unknown,
                local_counts: CommandStepCounts::new(2, 1, 0, 0),
                local_maximum_chain_expansion: CountBound::finite(0, 1)
                    .expect("zero-to-one is a valid finite cost interval"),
                local_outcomes: TELEPORT_LOCAL_OUTCOMES,
            },
            Self::Java26_2ReadMainHandWrittenBookLiteralPage => RecipeContractProjection {
                semantic_key: MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage,
                semantic_behavior: FunctionBehavior::new(
                    AmbientContextRequirements::NONE
                        .with_executor(ContextRequirement::Required(receiver_kind)),
                    WorldEffect::Read,
                    ObservableEffect::None,
                    ForkBound::None,
                    TransitiveWork::Finite,
                    false,
                ),
                semantic_outcome: MinecraftCommandOutcomeBehavior::Discarded,
                target_effects: EffectSummary::Known(
                    EffectCategories::ENTITY_QUERY.union(EffectCategories::STORAGE_WRITE),
                ),
                target_context: ContextSummary::Known {
                    reads: ContextMask::EXECUTOR,
                    changes: ContextMask::NONE,
                },
                target_fork: ForkClass::NEVER,
                target_native_outcome: NativeCommandOutcome::Unknown,
                local_counts: CommandStepCounts::new(1, 0, 0, 1),
                local_maximum_chain_expansion: CountBound::exact(0),
                local_outcomes: SAY_LOCAL_OUTCOMES,
            },
        }
    }
}

/// One validated recipe selection for a specific Core semantic-operation instance.
///
/// The target-facing message is retained here so construction consumes the value
/// that preflight validated instead of parsing or reselecting from Core later.
#[allow(
    clippy::enum_variant_names,
    reason = "selected variants deliberately carry target-version recipe identity"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SelectedSemanticRecipe {
    /// Java Edition 26.2 native `say`, with its exact semantic declaration identity.
    Java26_2Say {
        operation: MinecraftOperationId,
        message: SayMessage,
    },
    Java26_2Teleport {
        operation: MinecraftOperationId,
        destination: TargetPosition,
    },
    Java26_2MoveBy {
        operation: MinecraftOperationId,
        destination: TargetPosition,
    },
    Java26_2BookPage {
        operation: MinecraftOperationId,
        page_index: u8,
    },
}

/// One preflight-selected Java 26.2 run-modifier recipe and validated argument.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SelectedRunModifierRecipe {
    As(crate::ir::minecraft::Selector),
    At(crate::ir::minecraft::Selector),
    AtExecutor,
    Positioned(TargetPosition),
    Rotated(TargetRotation),
    In(crate::ir::minecraft::DimensionId),
    Anchored(TargetAnchor),
    Align(TargetAxes),
}

/// Stable target recipe identity for one selected run modifier.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RunModifierRecipeId {
    Java26_2As,
    Java26_2At,
    Java26_2AtExecutor,
    Java26_2Positioned,
    Java26_2Rotated,
    Java26_2In,
    Java26_2Anchored,
    Java26_2Align,
}

impl SelectedRunModifierRecipe {
    pub(crate) const fn recipe_id(&self) -> RunModifierRecipeId {
        match self {
            Self::As(_) => RunModifierRecipeId::Java26_2As,
            Self::At(_) => RunModifierRecipeId::Java26_2At,
            Self::AtExecutor => RunModifierRecipeId::Java26_2AtExecutor,
            Self::Positioned(_) => RunModifierRecipeId::Java26_2Positioned,
            Self::Rotated(_) => RunModifierRecipeId::Java26_2Rotated,
            Self::In(_) => RunModifierRecipeId::Java26_2In,
            Self::Anchored(_) => RunModifierRecipeId::Java26_2Anchored,
            Self::Align(_) => RunModifierRecipeId::Java26_2Align,
        }
    }
    pub(crate) fn target_kind(&self) -> ExecuteModifierKind {
        match self {
            Self::As(selector) => ExecuteModifierKind::As(selector.clone()),
            Self::At(selector) => ExecuteModifierKind::At(selector.clone()),
            Self::AtExecutor => ExecuteModifierKind::At(
                crate::ir::minecraft::AtMostOneSelector::SelfExecutor.into(),
            ),
            Self::Positioned(position) => ExecuteModifierKind::Positioned(position.clone()),
            Self::Rotated(rotation) => ExecuteModifierKind::Rotated(rotation.clone()),
            Self::In(dimension) => ExecuteModifierKind::In(dimension.clone()),
            Self::Anchored(anchor) => ExecuteModifierKind::Anchored(*anchor),
            Self::Align(axes) => ExecuteModifierKind::Align(*axes),
        }
    }
}

impl SelectedSemanticRecipe {
    pub(crate) fn contract_projection(
        &self,
        declaration: &MinecraftOperationDecl,
    ) -> RecipeContractProjection {
        let mut projection = self
            .recipe_id()
            .contract_projection(declaration.receiver_kind());
        projection.semantic_behavior = FunctionBehavior::new(
            declaration
                .attributes()
                .ambient_requirements(declaration.receiver_kind()),
            minecraft_descriptor(declaration.key()).world_effect(),
            minecraft_descriptor(declaration.key()).observable_effect(),
            minecraft_descriptor(declaration.key()).fork_behavior(),
            minecraft_descriptor(declaration.key()).work_behavior(),
            false,
        );
        if let Self::Java26_2Teleport { destination, .. } = self {
            projection.target_context = ContextSummary::Known {
                reads: ContextMask::EXECUTOR
                    .union(ContextMask::DIMENSION)
                    .union(target_position_reads(destination)),
                changes: ContextMask::NONE,
            };
        }
        projection
    }
    /// Returns the closed construction recipe identity.
    pub(crate) const fn recipe_id(&self) -> MinecraftRecipeId {
        match self {
            Self::Java26_2Say { .. } => MinecraftRecipeId::Java26_2Say,
            Self::Java26_2Teleport { .. } => MinecraftRecipeId::Java26_2TeleportCurrentExecutor,
            Self::Java26_2MoveBy { .. } => MinecraftRecipeId::Java26_2MoveCurrentExecutorBy,
            Self::Java26_2BookPage { .. } => {
                MinecraftRecipeId::Java26_2ReadMainHandWrittenBookLiteralPage
            }
        }
    }

    /// Returns the exact Core semantic-operation declaration selected by this recipe.
    pub(crate) const fn operation(&self) -> MinecraftOperationId {
        match self {
            Self::Java26_2Say { operation, .. }
            | Self::Java26_2BookPage { operation, .. }
            | Self::Java26_2Teleport { operation, .. }
            | Self::Java26_2MoveBy { operation, .. } => *operation,
        }
    }

    /// Returns the target-independent semantic meaning implemented by this recipe.
    pub(crate) const fn semantic_key(&self) -> MinecraftSemanticKey {
        match self {
            Self::Java26_2Say { .. } => MinecraftSemanticKey::Say,
            Self::Java26_2Teleport { .. } => MinecraftSemanticKey::TeleportCurrentExecutor,
            Self::Java26_2MoveBy { .. } => MinecraftSemanticKey::MoveCurrentExecutorBy,
            Self::Java26_2BookPage { .. } => {
                MinecraftSemanticKey::ReadMainHandWrittenBookLiteralPage
            }
        }
    }

    /// Returns the preflight-validated native `say` message.
    pub(crate) const fn say_message(&self) -> Option<&SayMessage> {
        match self {
            Self::Java26_2Say { message, .. } => Some(message),
            Self::Java26_2Teleport { .. }
            | Self::Java26_2MoveBy { .. }
            | Self::Java26_2BookPage { .. } => None,
        }
    }

    /// Constructs the exact structured target command selected and validated by
    /// preflight. Construction must consume this method rather than reselecting.
    pub(crate) fn command_kind(&self) -> CommandKind {
        match self {
            Self::Java26_2Say { message, .. } => CommandKind::Say(SayCommand::new(message.clone())),
            Self::Java26_2Teleport { destination, .. } => {
                CommandKind::Teleport(TeleportCommand::current_executor(destination.clone()))
            }
            Self::Java26_2MoveBy { destination, .. } => {
                let nested = crate::ir::minecraft::CommandNode::new(
                    CommandKind::Teleport(TeleportCommand::current_executor(destination.clone())),
                    OriginId::UNKNOWN,
                )
                .expect("single structured teleport has bounded depth");
                let at_self = ExecuteModifier::new(
                    ExecuteModifierKind::At(
                        crate::ir::minecraft::AtMostOneSelector::SelfExecutor.into(),
                    ),
                    OriginId::UNKNOWN,
                );
                CommandKind::Execute(ExecuteCommand::new(
                    ExecuteModifiers::new(at_self, vec![]),
                    nested,
                ))
            }
            Self::Java26_2BookPage { page_index, .. } => CommandKind::Data(DataCommand::Modify {
                target: StoragePath::new(
                    StorageId::parse("mdl:preflight").expect("static ID is valid"),
                    NbtPath::new(
                        NbtPathSegment::Key(
                            NbtPathKey::new("book_page").expect("static key is valid"),
                        ),
                        vec![],
                    ),
                ),
                mode: DataModifyMode::Set,
                source: DataSource::Entity {
                    selector: Selector::from(crate::ir::minecraft::AtMostOneSelector::SelfExecutor),
                    path: written_book_literal_page_path(*page_index),
                },
            }),
        }
    }

    pub(crate) const fn book_page_index(&self) -> Option<u8> {
        match self {
            Self::Java26_2BookPage { page_index, .. } => Some(*page_index),
            Self::Java26_2Say { .. }
            | Self::Java26_2Teleport { .. }
            | Self::Java26_2MoveBy { .. } => None,
        }
    }
}

pub(crate) fn written_book_literal_page_path(page_index: u8) -> NbtPath {
    NbtPath::new(
        NbtPathSegment::Key(NbtPathKey::new("equipment").expect("static key is valid")),
        vec![
            NbtPathSegment::Key(NbtPathKey::new("mainhand").expect("static key is valid")),
            NbtPathSegment::Key(NbtPathKey::new("components").expect("static key is valid")),
            NbtPathSegment::Key(
                NbtPathKey::new("minecraft:written_book_content").expect("static key is valid"),
            ),
            NbtPathSegment::Key(NbtPathKey::new("pages").expect("static key is valid")),
            NbtPathSegment::Index(i32::from(page_index)),
            NbtPathSegment::Key(NbtPathKey::new("raw").expect("static key is valid")),
        ],
    )
}

fn target_position_reads(position: &TargetPosition) -> ContextMask {
    match position {
        TargetPosition::World(position)
            if [&position.x, &position.y, &position.z]
                .into_iter()
                .any(|axis| matches!(axis, TargetWorldAxis::Relative(_))) =>
        {
            ContextMask::POSITION
        }
        TargetPosition::World(_) => ContextMask::NONE,
        TargetPosition::Local(_) => ContextMask::POSITION
            .union(ContextMask::ROTATION)
            .union(ContextMask::ANCHOR),
    }
}

/// Complete retained target decisions made before physical resource allocation.
///
/// The recipe table is dense over the Core external-declaration inventory.
/// Unreachable and nonsemantic declarations deliberately contain `None`.
#[derive(Clone, Debug)]
pub(crate) struct TargetPreflight {
    target: JavaEditionTarget,
    command_limits: CommandLimitEvidence,
    selected_semantic_recipes: EntityVec<ExternalOpId, Option<SelectedSemanticRecipe>>,
    selected_run_modifiers: EntityVec<RunScopeId, Box<[SelectedRunModifierRecipe]>>,
}

impl TargetPreflight {
    /// Selects and validates every entry-reachable typed Minecraft operation.
    ///
    /// This does not allocate target functions or construct target IR. The supplied
    /// command-limit evidence is the successful product of the preceding legality
    /// audit and is retained without weakening or normalization.
    pub(crate) fn new(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        target: JavaEditionTarget,
        command_limits: CommandLimitEvidence,
    ) -> Result<Self, Diagnostics> {
        validate_command_limit_target(target, command_limits)?;
        let selected_semantic_recipes = select_reachable_recipes(core, inventory, target)?;
        let selected_run_modifiers = select_run_modifier_recipes(core)?;
        Ok(Self {
            target,
            command_limits,
            selected_semantic_recipes,
            selected_run_modifiers,
        })
    }

    /// Returns the exact Java Edition target used for every retained selection.
    pub(crate) const fn target(&self) -> JavaEditionTarget {
        self.target
    }

    /// Returns the successful legality evidence retained alongside target recipes.
    pub(crate) const fn command_limit_evidence(&self) -> CommandLimitEvidence {
        self.command_limits
    }

    /// Returns the selected semantic recipe for an external declaration.
    ///
    /// `None` means either the identity is foreign, the declaration is not a typed
    /// Minecraft operation, or it has no entry-reachable occurrence.
    pub(crate) fn selected_recipe(
        &self,
        operation: ExternalOpId,
    ) -> Option<&SelectedSemanticRecipe> {
        self.selected_semantic_recipes
            .get(operation)
            .and_then(Option::as_ref)
    }

    pub(crate) fn selected_run_modifier(
        &self,
        scope: RunScopeId,
        modifier_index: usize,
    ) -> Option<&SelectedRunModifierRecipe> {
        self.selected_run_modifiers.get(scope)?.get(modifier_index)
    }

    /// Returns the number of dense external-operation slots retained by preflight.
    pub(crate) fn recipe_slot_count(&self) -> usize {
        self.selected_semantic_recipes.len()
    }

    /// Iterates every dense recipe slot in stable external-declaration order.
    pub(crate) fn selected_recipes(
        &self,
    ) -> impl ExactSizeIterator<Item = (ExternalOpId, Option<&SelectedSemanticRecipe>)> + '_ {
        self.selected_semantic_recipes
            .iter()
            .map(|(operation, recipe)| (operation, recipe.as_ref()))
    }

    /// Independently rederives reachability, recipes, target validation, and
    /// command-limit evidence, then compares them with the retained product.
    pub(crate) fn verify(
        &self,
        core: &CoreProgram,
        inventory: &SemanticInventory,
    ) -> Result<(), Diagnostics> {
        validate_command_limit_target(self.target(), self.command_limits)?;
        let expected = select_reachable_recipes(core, inventory, self.target())?;
        let expected_run_modifiers = select_run_modifier_recipes(core)?;
        let mut findings = Vec::new();

        if self.recipe_slot_count() != expected.len() {
            findings.push(Diagnostic::new(
                "lower.preflight.recipe-table-shape",
                format!(
                    "retained recipe table has {} slots; Core requires {}",
                    self.recipe_slot_count(),
                    expected.len()
                ),
                OriginId::UNKNOWN,
            ));
        }

        for ((operation, actual_recipe), (expected_operation, expected_recipe)) in
            self.selected_recipes().zip(expected.iter())
        {
            debug_assert_eq!(operation, expected_operation);
            if actual_recipe != expected_recipe.as_ref() {
                findings.push(Diagnostic::new(
                    "lower.preflight.recipe-mismatch",
                    format!(
                        "retained recipe for {operation:?} is {actual_recipe:?}; independently selected {expected_recipe:?}"
                    ),
                    external_diagnostic_origin(core, operation),
                ));
            }
        }
        let run_modifiers_match = self.selected_run_modifiers.len() == expected_run_modifiers.len()
            && self
                .selected_run_modifiers
                .iter()
                .zip(expected_run_modifiers.iter())
                .all(|((left_id, left), (right_id, right))| left_id == right_id && left == right);
        if !run_modifiers_match {
            findings.push(Diagnostic::new(
                "lower.preflight.run-modifier-recipe-mismatch",
                "retained run-modifier recipes differ from independent preflight selection",
                OriginId::UNKNOWN,
            ));
        }

        match super::audit::audit_legality(
            core,
            inventory,
            self.target,
            self.command_limits.configured_assumptions(),
        ) {
            Ok(recomputed) if recomputed == self.command_limits => {}
            Ok(recomputed) => findings.push(Diagnostic::new(
                "lower.preflight.command-limit-evidence",
                format!(
                    "retained command-limit evidence {0:?} does not match independently recomputed {recomputed:?}",
                    self.command_limits
                ),
                OriginId::UNKNOWN,
            )),
            Err(diagnostics) => findings.push(
                Diagnostic::new(
                    "lower.preflight.command-limit-evidence",
                    "retained command-limit evidence no longer reproduces successful legality",
                    OriginId::UNKNOWN,
                )
                .with_note(diagnostics.to_string()),
            ),
        }

        match Diagnostics::from_findings(findings) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub(crate) fn replace_command_limit_evidence_for_test(
        &mut self,
        command_limits: CommandLimitEvidence,
    ) {
        self.command_limits = command_limits;
    }
}

fn validate_command_limit_target(
    target: JavaEditionTarget,
    command_limits: CommandLimitEvidence,
) -> Result<(), Diagnostics> {
    let expected = CommandLimitAssumptions::for_target(target);
    if command_limits.target_defaults() == expected {
        return Ok(());
    }
    Err(Diagnostics::from_findings(vec![Diagnostic::new(
        "lower.preflight.command-limit-target",
        format!(
            "command-limit evidence records target defaults {:?}, but {target:?} requires {expected:?}",
            command_limits.target_defaults()
        ),
        OriginId::UNKNOWN,
    )])
    .expect("one target-evidence finding always forms diagnostics"))
}

fn select_reachable_recipes(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    target: JavaEditionTarget,
) -> Result<EntityVec<ExternalOpId, Option<SelectedSemanticRecipe>>, Diagnostics> {
    let mut selections =
        EntityVec::from_constrained_values(core.external_ops().map(|_| None).collect::<Vec<_>>());
    let mut findings = Vec::new();

    for (function, declaration) in core.functions() {
        let Some(body) = declaration.body() else {
            findings.push(Diagnostic::new(
                "lower.preflight.missing-definition",
                format!("cannot select target recipes for undefined function {function:?}"),
                declaration.origin(),
            ));
            continue;
        };
        let Some(function_inventory) = inventory.function(function) else {
            findings.push(Diagnostic::new(
                "lower.preflight.missing-semantic-inventory",
                format!("semantic inventory has no entry for {function:?}"),
                declaration.origin(),
            ));
            continue;
        };

        for instruction in function_inventory.reachable_instructions().iter().copied() {
            let Some(instruction_data) = body.instruction(instruction) else {
                findings.push(Diagnostic::new(
                    "lower.preflight.missing-instruction",
                    format!(
                        "semantic inventory names absent instruction {instruction:?} in {function:?}"
                    ),
                    declaration.origin(),
                ));
                continue;
            };
            let CoreOp::External(external) = instruction_data.op() else {
                continue;
            };
            let Some(external_declaration) = core.external_op(*external) else {
                findings.push(Diagnostic::new(
                    "lower.preflight.invalid-external-operation",
                    format!("reachable instruction refers to absent external {external:?}"),
                    instruction_data.origin(),
                ));
                continue;
            };
            let ExternalSemanticBinding::MinecraftOperation(operation) =
                external_declaration.binding()
            else {
                continue;
            };
            let Some(slot) = selections.get_mut(*external) else {
                findings.push(Diagnostic::new(
                    "lower.preflight.recipe-table-shape",
                    format!("reachable external {external:?} is outside the dense recipe table"),
                    instruction_data.origin(),
                ));
                continue;
            };
            if slot.is_some() {
                continue;
            }
            let Some(operation_declaration) = core.minecraft_operation(operation) else {
                findings.push(Diagnostic::new(
                    "lower.preflight.invalid-minecraft-operation",
                    format!(
                        "reachable external {external:?} refers to absent typed operation {operation:?}"
                    ),
                    external_declaration.origin(),
                ));
                continue;
            };
            match select_recipe(target, operation, operation_declaration) {
                Ok(recipe) => *slot = Some(recipe),
                Err(finding) => findings.push(finding),
            }
        }
    }

    match Diagnostics::from_findings(findings) {
        Some(diagnostics) => Err(diagnostics),
        None => Ok(selections),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive preflight boundary validates every closed modifier recipe"
)]
fn select_run_modifier_recipes(
    core: &CoreProgram,
) -> Result<EntityVec<RunScopeId, Box<[SelectedRunModifierRecipe]>>, Diagnostics> {
    let mut output = EntityVec::new();
    let mut findings = Vec::new();
    let mut selected_occurrences = 0usize;
    for (scope, declaration) in core.run_scopes() {
        selected_occurrences = selected_occurrences.saturating_add(declaration.modifiers().len());
        if selected_occurrences > MAX_PACKAGE_RUN_MODIFIERS {
            findings.push(Diagnostic::new(
                "lower.preflight.run-recipe-budget",
                format!("package has more than {MAX_PACKAGE_RUN_MODIFIERS} run-modifier recipes"),
                declaration.origin(),
            ));
            break;
        }
        let mut recipes = Vec::with_capacity(declaration.modifiers().len());
        for modifier in declaration.modifiers() {
            let origin = modifier.origin();
            let selected = match modifier {
                RunModifierInstance::AsEntityQuery { query, .. }
                | RunModifierInstance::AtEntityQuery { query, .. } => {
                    let Some(query) = core.entity_query(*query) else {
                        findings.push(Diagnostic::new(
                            "lower.preflight.missing-run-query",
                            "run modifier references a missing entity query",
                            origin,
                        ));
                        continue;
                    };
                    match super::query::lower_entity_query(query) {
                        Ok(selector)
                            if matches!(modifier, RunModifierInstance::AsEntityQuery { .. }) =>
                        {
                            SelectedRunModifierRecipe::As(selector.into())
                        }
                        Ok(selector) => SelectedRunModifierRecipe::At(selector.into()),
                        Err(failure) => {
                            findings.push(Diagnostic::new(
                                "lower.preflight.invalid-run-query",
                                format!(
                                    "run query is not representable by Java 26.2: {}",
                                    failure.error
                                ),
                                failure.origin,
                            ));
                            continue;
                        }
                    }
                }
                RunModifierInstance::AtExecutor { .. } => SelectedRunModifierRecipe::AtExecutor,
                RunModifierInstance::Positioned { position, .. } => {
                    match lower_semantic_position(position) {
                        Ok(position) => SelectedRunModifierRecipe::Positioned(position),
                        Err(error) => {
                            findings.push(spatial_recipe_diagnostic(error, origin));
                            continue;
                        }
                    }
                }
                RunModifierInstance::Rotated { rotation, .. } => {
                    let axis = |value: &crate::ir::semantic::RotationAxis| -> Result<
                        TargetRotationAxis,
                        crate::ir::minecraft::JavaDecimalError,
                    > {
                        Ok(match value {
                            crate::ir::semantic::RotationAxis::Absolute(value) => {
                                TargetRotationAxis::Absolute(lower_java_decimal(value)?)
                            }
                            crate::ir::semantic::RotationAxis::Relative(value) => {
                                TargetRotationAxis::Relative(lower_java_decimal(value)?)
                            }
                        })
                    };
                    match (axis(&rotation.yaw), axis(&rotation.pitch)) {
                        (Ok(yaw), Ok(pitch)) => {
                            SelectedRunModifierRecipe::Rotated(TargetRotation { yaw, pitch })
                        }
                        (Err(error), _) | (_, Err(error)) => {
                            findings.push(spatial_recipe_diagnostic(error, origin));
                            continue;
                        }
                    }
                }
                RunModifierInstance::In { dimension, .. } => {
                    match crate::ir::minecraft::DimensionId::parse(dimension.resource()) {
                        Ok(dimension) => SelectedRunModifierRecipe::In(dimension),
                        Err(error) => {
                            findings.push(Diagnostic::new(
                                "lower.preflight.invalid-dimension",
                                error.to_string(),
                                origin,
                            ));
                            continue;
                        }
                    }
                }
                RunModifierInstance::Anchored { anchor, .. } => {
                    SelectedRunModifierRecipe::Anchored(match anchor {
                        crate::ir::semantic::EntityAnchor::Feet => TargetAnchor::Feet,
                        crate::ir::semantic::EntityAnchor::Eyes => TargetAnchor::Eyes,
                    })
                }
                RunModifierInstance::Align { axes, .. } => {
                    let Some(axes) = TargetAxes::new(axes.bits()) else {
                        findings.push(Diagnostic::new(
                            "lower.preflight.invalid-axes",
                            "run align axes became empty",
                            origin,
                        ));
                        continue;
                    };
                    SelectedRunModifierRecipe::Align(axes)
                }
            };
            recipes.push(selected);
        }
        if recipes.len() == declaration.modifiers().len() {
            let allocated =
                output
                    .push(recipes.into_boxed_slice())
                    .map_err(|EntityLimitError| {
                        Diagnostics::from_findings(vec![Diagnostic::new(
                            "lower.preflight.run-recipe-limit",
                            "run-modifier recipe identity space exhausted",
                            declaration.origin(),
                        )])
                        .expect("one finding")
                    })?;
            if allocated != scope {
                findings.push(Diagnostic::new(
                    "lower.preflight.run-recipe-density",
                    "run-modifier recipe table is not dense",
                    declaration.origin(),
                ));
            }
        }
    }
    match Diagnostics::from_findings(findings) {
        Some(diagnostics) => Err(diagnostics),
        None => Ok(output),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one exhaustive operation preflight owns recipe validation and diagnostics"
)]
fn select_recipe(
    target: JavaEditionTarget,
    operation: MinecraftOperationId,
    declaration: &MinecraftOperationDecl,
) -> Result<SelectedSemanticRecipe, Diagnostic> {
    let recipe = MinecraftRecipeId::for_semantic_key(target, declaration.key());
    let projection = recipe.contract_projection(declaration.receiver_kind());
    if !projection.matches_semantic_descriptor(
        minecraft_descriptor(declaration.key()),
        declaration.receiver_kind(),
    ) {
        return Err(Diagnostic::new(
            "lower.preflight.recipe-semantic-contract",
            format!(
                "target recipe {recipe:?} disagrees with the instantiated {:?} semantic descriptor",
                declaration.key()
            ),
            declaration.origins().call(),
        ));
    }

    match (recipe, declaration.attributes()) {
        (
            MinecraftRecipeId::Java26_2Say,
            MinecraftOperationAttributes::Say {
                message,
                message_origin,
            },
        ) => {
            let origins = declaration.origins();
            let native_message =
                SayMessage::from_message_literal(message, target).map_err(|error| {
                    Diagnostic::new(
                        "lower.preflight.invalid-say-message",
                        format!("typed say cannot be emitted for {target:?}: {error}"),
                        *message_origin,
                    )
                    .with_primary_label("this message is not representable by the selected recipe")
                    .with_supporting_label(
                        DiagnosticLabel::new(origins.call())
                            .with_message("this typed say call requires the target recipe"),
                    )
                })?;

            let rendered_units = SAY_COMMAND_PREFIX
                .encode_utf16()
                .count()
                .saturating_add(native_message.as_str().encode_utf16().count());
            let maximum = usize::try_from(target.spec().max_logical_command_utf16_units())
                .expect("target logical command bound fits usize");
            if rendered_units > maximum {
                return Err(Diagnostic::new(
                    "lower.preflight.command-too-long",
                    format!(
                        "rendered typed say has {rendered_units} UTF-16 units; {target:?} permits {maximum}"
                    ),
                    *message_origin,
                )
                .with_primary_label("this message makes the complete command too long")
                .with_supporting_label(
                    DiagnosticLabel::new(origins.call())
                        .with_message("complete command length is checked at this call"),
                ));
            }

            Ok(SelectedSemanticRecipe::Java26_2Say {
                operation,
                message: native_message,
            })
        }
        (
            MinecraftRecipeId::Java26_2TeleportCurrentExecutor,
            MinecraftOperationAttributes::Teleport {
                position,
                component_origins,
            },
        ) => {
            let destination = lower_semantic_position(position).map_err(|error| {
                Diagnostic::new(
                    "lower.preflight.invalid-teleport-position",
                    format!("typed teleport position is not representable by {target:?}: {error}"),
                    component_origins[0],
                )
                .with_primary_label("this exact coordinate is rejected by the selected Java parser")
            })?;
            let selected = SelectedSemanticRecipe::Java26_2Teleport {
                operation,
                destination,
            };
            validate_selected_recipe(&selected, declaration)?;
            Ok(selected)
        }
        (
            MinecraftRecipeId::Java26_2MoveCurrentExecutorBy,
            MinecraftOperationAttributes::MoveBy {
                offset,
                component_origins,
            },
        ) => {
            let destination = TargetPosition::World(TargetWorldPosition {
                x: TargetWorldAxis::Relative(
                    lower_java_decimal(&offset.x)
                        .map_err(|error| spatial_recipe_diagnostic(error, component_origins[0]))?,
                ),
                y: TargetWorldAxis::Relative(
                    lower_java_decimal(&offset.y)
                        .map_err(|error| spatial_recipe_diagnostic(error, component_origins[1]))?,
                ),
                z: TargetWorldAxis::Relative(
                    lower_java_decimal(&offset.z)
                        .map_err(|error| spatial_recipe_diagnostic(error, component_origins[2]))?,
                ),
            });
            let selected = SelectedSemanticRecipe::Java26_2MoveBy {
                operation,
                destination,
            };
            validate_selected_recipe(&selected, declaration)?;
            Ok(selected)
        }
        (
            MinecraftRecipeId::Java26_2ReadMainHandWrittenBookLiteralPage,
            MinecraftOperationAttributes::BookPage { page_index, .. },
        ) => {
            let selected = SelectedSemanticRecipe::Java26_2BookPage {
                operation,
                page_index: *page_index,
            };
            validate_selected_recipe(&selected, declaration)?;
            Ok(selected)
        }
        _ => Err(Diagnostic::new(
            "lower.preflight.recipe-attribute-shape",
            format!(
                "target recipe {recipe:?} does not accept {:?} attributes",
                declaration.key()
            ),
            declaration.origins().call(),
        )),
    }
}

fn lower_java_decimal(
    value: &crate::ir::semantic::FiniteDecimal,
) -> Result<JavaDecimal, crate::ir::minecraft::JavaDecimalError> {
    JavaDecimal::new(value.as_str())
}

fn lower_semantic_position(
    position: &crate::ir::semantic::PositionSpec,
) -> Result<TargetPosition, crate::ir::minecraft::JavaDecimalError> {
    fn axis(
        value: &crate::ir::semantic::WorldAxis,
    ) -> Result<TargetWorldAxis, crate::ir::minecraft::JavaDecimalError> {
        Ok(match value {
            crate::ir::semantic::WorldAxis::Absolute(value) => {
                TargetWorldAxis::Absolute(lower_java_decimal(value)?)
            }
            crate::ir::semantic::WorldAxis::Relative(value) => {
                TargetWorldAxis::Relative(lower_java_decimal(value)?)
            }
        })
    }
    Ok(match position {
        crate::ir::semantic::PositionSpec::World(value) => {
            TargetPosition::World(TargetWorldPosition {
                x: axis(&value.x)?,
                y: axis(&value.y)?,
                z: axis(&value.z)?,
            })
        }
        crate::ir::semantic::PositionSpec::Local(value) => {
            TargetPosition::Local(TargetLocalPosition {
                left: lower_java_decimal(&value.left)?,
                up: lower_java_decimal(&value.up)?,
                forward: lower_java_decimal(&value.forward)?,
            })
        }
    })
}

fn spatial_recipe_diagnostic(
    error: crate::ir::minecraft::JavaDecimalError,
    origin: OriginId,
) -> Diagnostic {
    Diagnostic::new(
        "lower.preflight.invalid-spatial-decimal",
        format!("spatial decimal is not representable by Java 26.2: {error}"),
        origin,
    )
    .with_primary_label("this exact decimal is rejected by the selected Java parser")
}

fn validate_selected_recipe(
    selected: &SelectedSemanticRecipe,
    declaration: &MinecraftOperationDecl,
) -> Result<(), Diagnostic> {
    let command = selected.command_kind();
    let projection = selected.contract_projection(declaration);
    if !projection.matches_target_contract(command.contract()) {
        return Err(Diagnostic::new(
            "lower.preflight.recipe-target-contract",
            "selected structured command disagrees with its recipe contract",
            declaration.origins().call(),
        ));
    }
    let cost =
        crate::analysis::minecraft::classify_constructed_command(&command).ok_or_else(|| {
            Diagnostic::new(
                "lower.preflight.recipe-local-cost",
                "selected structured command has no local cost classification",
                declaration.origins().call(),
            )
        })?;
    if !projection.matches_local_cost(&cost) {
        return Err(Diagnostic::new(
            "lower.preflight.recipe-local-cost",
            "selected structured command disagrees with its recipe cost",
            declaration.origins().call(),
        ));
    }
    Ok(())
}

fn external_diagnostic_origin(core: &CoreProgram, external: ExternalOpId) -> OriginId {
    let Some(declaration) = core.external_op(external) else {
        return OriginId::UNKNOWN;
    };
    match declaration.binding() {
        ExternalSemanticBinding::MinecraftOperation(operation) => core
            .minecraft_operation(operation)
            .map_or(declaration.origin(), |operation| operation.origins().call()),
        ExternalSemanticBinding::UnsafeTargetFragment(_)
        | ExternalSemanticBinding::MinecraftRunScope(_) => declaration.origin(),
    }
}

#[cfg(test)]
mod tests {
    use super::{MinecraftRecipeId, TargetPreflight};
    use crate::analysis::minecraft::{
        CommandLimitAssumptions, CommandStepCounts, classify_constructed_command,
    };
    use crate::ir::core::{
        CoreProgram, ExternalSemanticBinding, FunctionBuilder, MinecraftOperationAttributes,
        MinecraftOperationOrigins, Terminator, TerminatorKind,
    };
    use crate::ir::minecraft::{CommandKind, NativeCommandOutcome, SayCommand, SayMessage};
    use crate::ir::semantic::{
        EntityKind, FunctionBehavior, MessageLiteral, MinecraftSemanticKey, minecraft_descriptor,
    };
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::api::CommandLimitEvidence;
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    fn evidence() -> CommandLimitEvidence {
        let defaults = CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2);
        CommandLimitEvidence::new(defaults, defaults, 0)
    }

    #[test]
    fn say_recipe_projection_matches_registry_and_constructed_target_analyses() {
        let projection = MinecraftRecipeId::Java26_2Say.contract_projection(EntityKind::ArmorStand);
        assert!(projection.matches_semantic_descriptor(
            minecraft_descriptor(MinecraftSemanticKey::Say),
            EntityKind::ArmorStand,
        ));

        let command = CommandKind::Say(SayCommand::new(SayMessage::new("hello").unwrap()));
        assert!(projection.matches_target_contract(command.contract()));
        let local = classify_constructed_command(&command).unwrap();
        assert!(projection.matches_local_cost(&local));
    }

    #[test]
    fn recipe_projection_detects_semantic_target_and_cost_drift_independently() {
        let projection = MinecraftRecipeId::Java26_2Say.contract_projection(EntityKind::ArmorStand);
        let descriptor = minecraft_descriptor(MinecraftSemanticKey::Say);
        let command = CommandKind::Say(SayCommand::new(SayMessage::new("hello").unwrap()));
        let local = classify_constructed_command(&command).unwrap();

        let semantic_drift = super::RecipeContractProjection {
            semantic_behavior: FunctionBehavior::NONE,
            ..projection
        };
        assert!(!semantic_drift.matches_semantic_descriptor(descriptor, EntityKind::ArmorStand,));

        let target_drift = super::RecipeContractProjection {
            target_native_outcome: NativeCommandOutcome::Unknown,
            ..projection
        };
        assert!(!target_drift.matches_target_contract(command.contract()));

        let cost_drift = super::RecipeContractProjection {
            local_counts: CommandStepCounts::new(2, 0, 0, 0),
            ..projection
        };
        assert!(!cost_drift.matches_local_cost(&local));
    }

    #[test]
    fn selects_only_entry_reachable_semantic_external_operations() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let reachable_semantic = core
            .declare_minecraft_operation(
                MinecraftSemanticKey::Say,
                EntityKind::ArmorStand,
                MinecraftOperationAttributes::Say {
                    message: MessageLiteral::new("reachable").unwrap(),
                    message_origin: OriginId::UNKNOWN,
                },
                MinecraftOperationOrigins::new(
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                ),
            )
            .unwrap();
        let unreachable_semantic = core
            .declare_minecraft_operation(
                MinecraftSemanticKey::Say,
                EntityKind::ArmorStand,
                MinecraftOperationAttributes::Say {
                    message: MessageLiteral::new("unreachable").unwrap(),
                    message_origin: OriginId::UNKNOWN,
                },
                MinecraftOperationOrigins::new(
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                    OriginId::UNKNOWN,
                ),
            )
            .unwrap();
        let reachable_external = core
            .declare_external_op(
                ExternalSemanticBinding::MinecraftOperation(reachable_semantic),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let unreachable_external = core
            .declare_external_op(
                ExternalSemanticBinding::MinecraftOperation(unreachable_semantic),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = core
            .declare_function(Some("main"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder
            .external(reachable_external, vec![], OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let dead = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder.switch_to_block(dead).unwrap();
        builder
            .external(unreachable_external, vec![], OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        core.define_function(function, body).unwrap();

        let inventory = SemanticInventory::new(&core).unwrap();
        let mut preflight =
            TargetPreflight::new(&core, &inventory, JavaEditionTarget::V26_2, evidence()).unwrap();

        assert_eq!(preflight.recipe_slot_count(), 2);
        assert_eq!(
            preflight
                .selected_recipe(reachable_external)
                .unwrap()
                .recipe_id(),
            MinecraftRecipeId::Java26_2Say
        );
        assert!(preflight.selected_recipe(unreachable_external).is_none());
        assert!(preflight.verify(&core, &inventory).is_ok());

        *preflight
            .selected_semantic_recipes
            .get_mut(reachable_external)
            .unwrap() = None;
        assert!(
            preflight
                .verify(&core, &inventory)
                .unwrap_err()
                .contains_code("lower.preflight.recipe-mismatch")
        );
    }

    #[test]
    fn target_rejection_preserves_literal_and_call_provenance() {
        let mut sources = SourceContext::new();
        let file = sources.add_file("test.mdl", "call literal").unwrap();
        let call = sources
            .add_origin(Origin::Source(sources.span(file, 0, 4).unwrap()))
            .unwrap();
        let literal = sources
            .add_origin(Origin::Source(sources.span(file, 5, 12).unwrap()))
            .unwrap();
        let mut core = CoreProgram::new();
        let semantic = core
            .declare_minecraft_operation(
                MinecraftSemanticKey::Say,
                EntityKind::ArmorStand,
                MinecraftOperationAttributes::Say {
                    message: MessageLiteral::new("x".repeat(257)).unwrap(),
                    message_origin: literal,
                },
                MinecraftOperationOrigins::new(call, call, call),
            )
            .unwrap();
        let external = core
            .declare_external_op(
                ExternalSemanticBinding::MinecraftOperation(semantic),
                vec![],
                vec![],
                call,
            )
            .unwrap();
        let function = core
            .declare_function(Some("main"), vec![], vec![], call)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder.external(external, vec![], call).unwrap();
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), call))
            .unwrap();
        let body = builder.finish().unwrap();
        core.define_function(function, body).unwrap();

        let inventory = SemanticInventory::new(&core).unwrap();
        let diagnostics =
            TargetPreflight::new(&core, &inventory, JavaEditionTarget::V26_2, evidence())
                .unwrap_err();
        let finding = diagnostics.findings().first().unwrap();

        assert_eq!(finding.code(), "lower.preflight.invalid-say-message");
        assert_eq!(finding.primary_label().origin(), literal);
        assert_eq!(finding.supporting_labels()[0].origin(), call);
    }
}
