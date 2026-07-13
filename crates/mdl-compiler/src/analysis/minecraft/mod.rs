//! Static execution-cost analysis for structured Minecraft programs.

mod analyze;
mod cost;
mod graph;
mod local;
mod report;
mod solve;
mod syntax;

pub use analyze::analyze_target_execution;
pub(crate) use analyze::analyze_verified_target_execution;
pub use cost::{
    AnalysisArithmeticCaps, AnalysisArithmeticCapsError, CommandLimitAssumptions,
    CommandLimitAssumptionsError, CommandLimitStatus, CommandOutcome, CommandStepCost,
    CommandStepCounts, CostRegionId, CountBound, CountBoundInvariantError, CountUpper,
    CountUpperKind, FunctionLocalSummary, FunctionOutcomeCost, InternalCallRole, InternalCallSite,
    NoFiniteBoundReason, ResolvedTargetExecutionRoot, ReturnValueClass, RootEntryRegion,
    RootExecutionSummary, TargetExecutionRoot, UnknownCostReason,
};
pub use report::{
    CostRegionSummary, TargetExecutionAnalysisCompletion, TargetExecutionAnalysisFailure,
    TargetExecutionAnalysisLimitKind, TargetExecutionAnalysisLimits, TargetExecutionAnalysisPhase,
    TargetExecutionAnalysisStats, TargetExecutionCensus, TargetExecutionCostReport,
};
pub use syntax::{
    CallableClass, CommandStepClass, ConditionClass, DataCommandClass, ExecuteModifierClass,
    FunctionTagEntryClass, ReturnCommandClass, ScoreCommandClass, StoreDestinationClass,
};
