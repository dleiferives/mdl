//! Typed, server-independent Minecraft scenario contracts.
//!
//! These types describe controlled vanilla experiments and normalized observations.
//! They deliberately do not form a general command language and do not emulate
//! Minecraft.

mod model;
mod names;
mod observation;
mod runner;

pub use model::{
    Anchor, CommandOutcomeExpectation, CompilerPolicy, CompletionExpectation, Coordinate,
    CorePolicy, EntitySelector, ExactLimitContract, ExactLimitKind, Executor, InvocationFrame,
    MinecraftPolicy, MinecraftScenario, OutcomeChannelExpectation, OwnedResource, Position,
    Rotation, ScenarioEntry, ScenarioMode, ScenarioSpec, ScenarioValidationError,
};
pub use names::{GenerationNonce, NameError, ResourceLocation, ScenarioId, TestName};
pub use observation::{
    BlockPosition, EntityCollection, EntityComparison, EntityRecord, NbtParseError, NbtValue,
    NonFiniteNumberError, NormalizedBlockState, NormalizedLogEffect, ObservationComparison,
    ObservationDifference, ObservationKind, ObservationPath, ObservationSet, ObservationSetError,
    ObservationValue, compare_observations,
};
pub use runner::{
    CommandOutcomeObservation, DeploymentFile, DriverCommand, PolicyDeployment, PolicyObservation,
    ScenarioRun, ScenarioRunError, SemanticScenarioCase, run_exact_limit_policy_scenario,
    run_semantic_policy_scenario, run_semantic_policy_suite,
};
