//! Checked Core-to-Minecraft lowering.

// Retained temporarily as the byte-for-byte Stage 4 unit oracle. Production
// lowering uses the immutable assignment -> transfer -> resource -> plan graph.
#[cfg(test)]
mod allocate;
mod analysis;
mod api;
mod assignment;
mod audit;
mod call;
mod coalescing;
mod construct;
mod control;
mod crossings;
mod demand;
mod edge_transfer;
mod emit;
mod liveness;
mod names;
mod physical_preflight;
mod placement;
mod plan;
mod preflight;
mod query;
mod realization;
mod recipe;
mod resources;
mod scalar;
mod support;
mod transfer;

use std::error::Error;
use std::fmt;

use crate::ir::minecraft::{ObjectiveName, PackNamespace};
use crate::target::JavaEditionTarget;

pub use crate::analysis::minecraft::{CommandLimitAssumptions, CommandLimitAssumptionsError};
pub use api::{
    ActivationContract, ActivationDepthContract, CommandLimitContract, CommandLimitEvidence,
    ExecutionContract, LoweredCommand, LoweredFunction, LoweredRunModifier, LoweringFailure,
    LoweringMap, LoweringOutput, LoweringPhase, RegisterSlot, lower_to_minecraft,
};
pub(crate) use names::GeneratedNames;
pub use plan::{LoweringDecisionReport, LoweringDecisionStatistics};
pub use preflight::{MinecraftRecipeId, RunModifierRecipeId};
pub(crate) use preflight::{SelectedSemanticRecipe, TargetPreflight};
pub use support::{MinecraftApiSupport, dump_supported_minecraft_api, supported_minecraft_api};

/// Optimization policy for Core-to-Minecraft physical planning.
///
/// This is separate from Core optimization: callers may preserve Core exactly while
/// still selecting how the verified Core program is physically represented.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum MinecraftOptimizationLevel {
    /// Preserve the complete Stage 4 physicalization as a differential oracle.
    None,
    /// Enable the reviewed baseline Minecraft-planning policy.
    Baseline,
}

/// Explicit target and private-name ownership for one lowering run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoweringOptions {
    target: JavaEditionTarget,
    namespace: PackNamespace,
    register_objective: ObjectiveName,
    command_limit_assumptions: CommandLimitAssumptions,
    optimization_level: MinecraftOptimizationLevel,
}

impl LoweringOptions {
    /// Validates options that constrain the complete generated pack.
    ///
    /// # Errors
    ///
    /// Rejects the vanilla `minecraft` namespace, which is not compiler-owned.
    pub fn new(
        target: JavaEditionTarget,
        namespace: PackNamespace,
        register_objective: ObjectiveName,
    ) -> Result<Self, LoweringOptionsError> {
        if namespace.as_str() == "minecraft" {
            return Err(LoweringOptionsError::ReservedPackNamespace);
        }
        Ok(Self {
            target,
            namespace,
            register_objective,
            command_limit_assumptions: CommandLimitAssumptions::for_target(target),
            optimization_level: MinecraftOptimizationLevel::None,
        })
    }

    /// Returns the selected immutable Minecraft target.
    #[must_use]
    pub const fn target(&self) -> JavaEditionTarget {
        self.target
    }

    /// Returns the compiler-owned non-vanilla pack namespace.
    #[must_use]
    pub const fn namespace(&self) -> &PackNamespace {
        &self.namespace
    }

    /// Returns the objective exclusively reserved for generated register homes.
    #[must_use]
    pub const fn register_objective(&self) -> &ObjectiveName {
        &self.register_objective
    }

    /// Returns the assumed server hard limits used by target analysis and recipe
    /// safety comparisons.
    #[must_use]
    pub const fn command_limit_assumptions(&self) -> CommandLimitAssumptions {
        self.command_limit_assumptions
    }

    /// Returns the selected Minecraft physical-planning policy.
    #[must_use]
    pub const fn optimization_level(&self) -> MinecraftOptimizationLevel {
        self.optimization_level
    }

    /// Overrides the target-default command limits with one already validated value.
    ///
    /// This records a deployment assumption; it does not emit gamerule mutations.
    /// A consumer relying on a `ProvenWithin` result must deploy with actual values
    /// no lower than these assumptions, or analyze again with the actual values.
    #[must_use]
    pub fn with_command_limit_assumptions(mut self, assumptions: CommandLimitAssumptions) -> Self {
        self.command_limit_assumptions = assumptions;
        self
    }

    /// Selects the Minecraft physical-planning policy explicitly.
    ///
    /// [`Self::new`] defaults to [`MinecraftOptimizationLevel::None`] so existing
    /// callers retain the complete Stage 4 output.
    #[must_use]
    pub const fn with_optimization_level(mut self, level: MinecraftOptimizationLevel) -> Self {
        self.optimization_level = level;
        self
    }

    pub(crate) const fn generated_names(&self) -> GeneratedNames<'_> {
        GeneratedNames::new(self)
    }

    fn validate(&self) -> Result<(), LoweringOptionsError> {
        if self.namespace.as_str() == "minecraft" {
            Err(LoweringOptionsError::ReservedPackNamespace)
        } else {
            Ok(())
        }
    }
}

/// Invalid private-pack ownership requested for lowering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoweringOptionsError {
    /// Generated private resources may not occupy the vanilla namespace.
    ReservedPackNamespace,
}

impl fmt::Display for LoweringOptionsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReservedPackNamespace => {
                formatter.write_str("the `minecraft` namespace is reserved for vanilla resources")
            }
        }
    }
}

impl Error for LoweringOptionsError {}

#[cfg(test)]
mod tests {
    use super::{
        CommandLimitAssumptions, LoweringOptions, LoweringOptionsError, MinecraftOptimizationLevel,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::target::JavaEditionTarget;

    #[test]
    fn options_reject_the_reserved_pack_namespace() {
        let result = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("minecraft").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        );
        assert_eq!(result, Err(LoweringOptionsError::ReservedPackNamespace));
    }

    #[test]
    fn options_retain_only_explicit_typed_configuration() {
        let options = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap();

        assert_eq!(options.target(), JavaEditionTarget::V26_2);
        assert_eq!(options.namespace().as_str(), "mdl");
        assert_eq!(options.register_objective().as_str(), "mdl.reg");
        assert_eq!(
            options.optimization_level(),
            MinecraftOptimizationLevel::None
        );
        assert_eq!(
            options.command_limit_assumptions(),
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2)
        );
    }

    #[test]
    fn options_accept_one_validated_command_limit_override() {
        let assumptions = CommandLimitAssumptions::new(10, 4).unwrap();
        let options = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
        .with_command_limit_assumptions(assumptions);

        assert_eq!(options.command_limit_assumptions(), assumptions);
    }

    #[test]
    fn options_accept_one_explicit_minecraft_optimization_level() {
        let options = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
        .with_optimization_level(MinecraftOptimizationLevel::Baseline);

        assert_eq!(
            options.optimization_level(),
            MinecraftOptimizationLevel::Baseline
        );
    }
}
