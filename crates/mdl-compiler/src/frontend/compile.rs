//! Owned source-to-datapack compilation facade.

use std::error::Error;
use std::fmt;

use super::FrontendLimits;
use super::ast::AstModule;
use super::check::{CheckError, CheckedEntityKind, PackageAstModule, check_package};
use super::hir::{
    CheckedFrontendOutput, HirExternalSemantic, SourceExternalOpId, SourceFunctionId,
    SourceModuleId, SourceRunId,
};
use super::input::{
    ModuleDependency, ModuleInput, ModuleKey, PackageInput, PackageInputError, SourceInput,
};
use super::lexer::{LexerError, lex};
use super::lower::{CoreGenerationFailure, SourceToCoreMap, lower_hir};
use super::parser::{ParserError, parse};
use crate::analysis::minecraft::{
    TargetExecutionAnalysisFailure, TargetExecutionAnalysisLimits, TargetExecutionCostReport,
};
use crate::datapack::{EmissionOptions, EmissionOutput, emit_datapack};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::ir::core::{CoreOp, ExternalSemanticBinding};
use crate::lower::minecraft::{
    LoweredCommand, LoweredFunction, LoweringFailure, LoweringOptions, LoweringOutput,
    lower_to_minecraft,
};
use crate::opt::core::{
    CoreOptimizationFailure, CoreOptimizationOptions, CoreOptimizationOutput, optimize_core,
};
use crate::source::{Origin, OriginError, SourceContext, SourceError};

/// Exact options consumed by one source compilation.
///
/// Each field retains the existing producer's typed option contract. This facade
/// does not duplicate backend configuration or validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilationOptions {
    frontend: FrontendLimits,
    core_optimization: CoreOptimizationOptions,
    minecraft_lowering: LoweringOptions,
    target_analysis: TargetExecutionAnalysisLimits,
    emission: EmissionOptions,
}

impl CompilationOptions {
    /// Composes one already-validated option value for each compilation boundary.
    #[must_use]
    pub const fn new(
        frontend: FrontendLimits,
        core_optimization: CoreOptimizationOptions,
        minecraft_lowering: LoweringOptions,
        target_analysis: TargetExecutionAnalysisLimits,
        emission: EmissionOptions,
    ) -> Self {
        Self {
            frontend,
            core_optimization,
            minecraft_lowering,
            target_analysis,
            emission,
        }
    }

    /// Returns the source-derived frontend and Core-generation resource limits.
    #[must_use]
    pub const fn frontend(&self) -> FrontendLimits {
        self.frontend
    }

    /// Returns the Core optimization options.
    #[must_use]
    pub const fn core_optimization(&self) -> &CoreOptimizationOptions {
        &self.core_optimization
    }

    /// Returns the Core-to-Minecraft lowering options.
    #[must_use]
    pub const fn minecraft_lowering(&self) -> &LoweringOptions {
        &self.minecraft_lowering
    }

    /// Returns the optional target-cost analysis limits.
    #[must_use]
    pub const fn target_analysis(&self) -> TargetExecutionAnalysisLimits {
        self.target_analysis
    }

    /// Returns the datapack emission options.
    #[must_use]
    pub const fn emission(&self) -> &EmissionOptions {
        &self.emission
    }

    /// Consumes the facade options into the exact producer option values.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        FrontendLimits,
        CoreOptimizationOptions,
        LoweringOptions,
        TargetExecutionAnalysisLimits,
        EmissionOptions,
    ) {
        (
            self.frontend,
            self.core_optimization,
            self.minecraft_lowering,
            self.target_analysis,
            self.emission,
        )
    }
}

/// A frontend phase whose checked infrastructure could not produce its ordinary
/// tokens, syntax, or typed-HIR result.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum FrontendInfrastructurePhase {
    /// Bounded lexical analysis.
    Lexing,
    /// Recoverable syntax parsing.
    Parsing,
    /// Name resolution, type checking, and structured flow checking.
    SemanticChecking,
}

impl fmt::Display for FrontendInfrastructurePhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lexing => formatter.write_str("lexing"),
            Self::Parsing => formatter.write_str("parsing"),
            Self::SemanticChecking => formatter.write_str("semantic checking"),
        }
    }
}

/// A closed frontend identity domain that can be exhausted by source-size input.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum FrontendEntityKind {
    /// Source modules in canonical package order.
    Module,
    /// Source functions in declaration order.
    Function,
    /// Source-level external operations in canonical package order.
    ExternalOperation,
    /// Structured contextual run scopes in canonical source order.
    RunScope,
    /// Function-owned parameters and local bindings.
    Local,
}

impl fmt::Display for FrontendEntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Module => formatter.write_str("module"),
            Self::Function => formatter.write_str("function"),
            Self::ExternalOperation => formatter.write_str("external operation"),
            Self::RunScope => formatter.write_str("run scope"),
            Self::Local => formatter.write_str("local"),
        }
    }
}

/// Concrete non-diagnostic failure while constructing a frontend phase product.
///
/// Ordinary invalid user syntax and semantics are represented separately by
/// [`CompilationFailure::Syntax`] and [`CompilationFailure::Semantic`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum FrontendInfrastructureFailure {
    /// Source storage or span access failed during a specific frontend phase.
    Source {
        /// Phase that encountered the failure.
        phase: FrontendInfrastructurePhase,
        /// Original structured source failure.
        error: SourceError,
    },
    /// Provenance allocation or validation failed during a specific frontend phase.
    Origin {
        /// Phase that encountered the failure.
        phase: FrontendInfrastructurePhase,
        /// Original structured provenance failure.
        error: OriginError,
    },
    /// A checked semantic dense-ID domain was exhausted.
    IdentitySpaceExhausted {
        /// Exhausted frontend entity domain.
        entity: FrontendEntityKind,
    },
    /// The semantic checker constructed HIR that its independent verifier rejected.
    InvalidHir {
        /// Deterministic invariant detail produced by the HIR verifier.
        detail: Box<str>,
    },
    /// The semantic producer returned neither a complete product nor diagnostics,
    /// or attempted to return both at once.
    InvalidSemanticOutput,
}

impl fmt::Display for FrontendInfrastructureFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source { phase, error } => {
                write!(formatter, "source handling failed during {phase}: {error}")
            }
            Self::Origin { phase, error } => {
                write!(
                    formatter,
                    "provenance handling failed during {phase}: {error}"
                )
            }
            Self::IdentitySpaceExhausted { entity } => {
                write!(formatter, "frontend {entity} identity space is exhausted")
            }
            Self::InvalidHir { detail } => {
                write!(formatter, "constructed invalid typed HIR: {detail}")
            }
            Self::InvalidSemanticOutput => formatter
                .write_str("semantic checking returned an inconsistent product/diagnostic state"),
        }
    }
}

impl Error for FrontendInfrastructureFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Source { error, .. } => Some(error),
            Self::Origin { error, .. } => Some(error),
            Self::IdentitySpaceExhausted { .. }
            | Self::InvalidHir { .. }
            | Self::InvalidSemanticOutput => None,
        }
    }
}

impl From<LexerError> for FrontendInfrastructureFailure {
    fn from(error: LexerError) -> Self {
        match error {
            LexerError::Source(error) => Self::Source {
                phase: FrontendInfrastructurePhase::Lexing,
                error,
            },
            LexerError::Origin(error) => Self::Origin {
                phase: FrontendInfrastructurePhase::Lexing,
                error,
            },
        }
    }
}

impl From<ParserError> for FrontendInfrastructureFailure {
    fn from(error: ParserError) -> Self {
        match error {
            ParserError::Source(error) => Self::Source {
                phase: FrontendInfrastructurePhase::Parsing,
                error,
            },
            ParserError::Origin(error) => Self::Origin {
                phase: FrontendInfrastructurePhase::Parsing,
                error,
            },
        }
    }
}

impl From<CheckError> for FrontendInfrastructureFailure {
    fn from(error: CheckError) -> Self {
        match error {
            CheckError::Source(error) => Self::Source {
                phase: FrontendInfrastructurePhase::SemanticChecking,
                error,
            },
            CheckError::Origin(error) => Self::Origin {
                phase: FrontendInfrastructurePhase::SemanticChecking,
                error,
            },
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::Module) => {
                Self::IdentitySpaceExhausted {
                    entity: FrontendEntityKind::Module,
                }
            }
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::Function) => {
                Self::IdentitySpaceExhausted {
                    entity: FrontendEntityKind::Function,
                }
            }
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::ExternalOperation) => {
                Self::IdentitySpaceExhausted {
                    entity: FrontendEntityKind::ExternalOperation,
                }
            }
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::RunScope) => {
                Self::IdentitySpaceExhausted {
                    entity: FrontendEntityKind::RunScope,
                }
            }
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::Local) => {
                Self::IdentitySpaceExhausted {
                    entity: FrontendEntityKind::Local,
                }
            }
            CheckError::InvalidHir(error) => Self::InvalidHir {
                detail: error.to_string().into(),
            },
        }
    }
}

/// Complete owned products from one successful source compilation.
#[derive(Debug)]
pub struct CompilationOutput {
    sources: SourceContext,
    checked_frontend: CheckedFrontendOutput,
    source_to_core: SourceToCoreMap,
    core_optimization: CoreOptimizationOutput,
    lowering: LoweringOutput,
    target_analysis: Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure>,
    emission: EmissionOutput,
}

impl CompilationOutput {
    /// Returns source text and provenance retained by every later product.
    #[must_use]
    pub const fn sources(&self) -> &SourceContext {
        &self.sources
    }

    /// Returns the resolved, fully typed source program.
    #[must_use]
    pub const fn checked_frontend(&self) -> &CheckedFrontendOutput {
        &self.checked_frontend
    }

    /// Returns the source-function-to-Core correlation retained across optimization.
    #[must_use]
    pub const fn source_to_core(&self) -> &SourceToCoreMap {
        &self.source_to_core
    }

    /// Returns the verified optimized Core program and its optimization report.
    #[must_use]
    pub const fn core_optimization(&self) -> &CoreOptimizationOutput {
        &self.core_optimization
    }

    /// Returns the complete verified Minecraft lowering product.
    #[must_use]
    pub const fn lowering(&self) -> &LoweringOutput {
        &self.lowering
    }

    /// Returns the nonfatal target-cost analysis result.
    ///
    /// Analysis failure does not invalidate or remove lowering and emission.
    pub const fn target_analysis(
        &self,
    ) -> &Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure> {
        &self.target_analysis
    }

    /// Returns the complete deterministic in-memory datapack and trace.
    #[must_use]
    pub const fn emission(&self) -> &EmissionOutput {
        &self.emission
    }

    /// Looks up one source function's generated entry resource and ordered ABI homes.
    ///
    /// This composes the retained source/Core correlation with the lowering map; it
    /// does not duplicate either map.
    #[must_use]
    pub fn source_function_abi(
        &self,
        source_function: SourceFunctionId,
    ) -> Option<&LoweredFunction> {
        let core_function = self.source_to_core.function(source_function)?;
        self.lowering.map().function(core_function)
    }

    /// Resolves one typed source-semantic operation occurrence to its exact generated
    /// target command.
    ///
    /// The lookup composes retained source/Core and post-construction maps while
    /// defensively checking that the optimized Core instruction still invokes the
    /// recorded typed Minecraft declaration. Unsafe externals, unreachable source
    /// operations, non-direct lowerings, and foreign identities return `None`.
    #[must_use]
    pub fn source_semantic_command(
        &self,
        source_external: SourceExternalOpId,
    ) -> Option<LoweredCommand> {
        let source_operation = self.checked_frontend.external_op(source_external)?;
        if !matches!(
            &source_operation.semantic,
            HirExternalSemantic::MinecraftOperation { .. }
        ) {
            return None;
        }
        let occurrence = self.source_to_core.semantic_operation(source_external)?;
        if self.source_to_core.external_operation(source_external) != Some(occurrence.external()) {
            return None;
        }
        let program = self.core_optimization.program();
        let body = program.function(occurrence.function())?.body()?;
        let attached = body.block_order().iter().any(|block| {
            body.block(*block)
                .is_some_and(|block| block.instructions().contains(&occurrence.instruction()))
        });
        if !attached {
            return None;
        }
        let instruction = body.instruction(occurrence.instruction())?;
        if instruction.origin() != source_operation.origin {
            return None;
        }
        let CoreOp::External(external) = instruction.op() else {
            return None;
        };
        if *external != occurrence.external()
            || !matches!(
                program.external_op(*external)?.binding(),
                ExternalSemanticBinding::MinecraftOperation(_)
            )
        {
            return None;
        }
        self.lowering
            .map()
            .semantic_command(occurrence.function(), occurrence.instruction())
    }

    /// Resolves one source run-modifier occurrence through Core and retained
    /// preflight selection to its exact target execute-modifier placement.
    #[must_use]
    pub fn source_run_modifier(
        &self,
        source_run: SourceRunId,
        modifier_index: usize,
    ) -> Option<(
        crate::ir::core::RunScopeId,
        crate::lower::minecraft::LoweredRunModifier,
    )> {
        let scope = self.source_to_core.run_scope(source_run)?;
        self.core_optimization
            .program()
            .run_scope(scope)?
            .modifiers()
            .get(modifier_index)?;
        let lowered = self.lowering.map().run_modifier(scope, modifier_index)?;
        Some((scope, lowered))
    }

    /// Returns one source function's verified transitive behavior summary.
    ///
    /// This facade keeps inferred ambient requirements and unsafe opacity visible
    /// beside exported ABI information without copying either fact into Core.
    #[must_use]
    pub fn source_function_behavior(
        &self,
        source_function: SourceFunctionId,
    ) -> Option<crate::ir::semantic::FunctionBehavior> {
        self.checked_frontend.function_behavior(source_function)
    }

    /// Returns one source function's original spelling.
    ///
    /// The spelling is sliced from the retained immutable source context rather
    /// than copied into typed HIR. `None` indicates an identity outside this
    /// compilation result or an internal provenance mismatch.
    #[must_use]
    pub fn source_function_name(&self, source_function: SourceFunctionId) -> Option<&str> {
        let origin = self
            .checked_frontend
            .function_name_origin(source_function)?;
        let span = self.sources.resolve_origin_span(origin)?;
        self.sources.files().slice(span).ok()
    }

    /// Consumes the output into the independently owned producer products.
    #[allow(
        clippy::type_complexity,
        reason = "the tuple mirrors the seven explicit producer boundaries"
    )]
    pub fn into_parts(
        self,
    ) -> (
        SourceContext,
        CheckedFrontendOutput,
        SourceToCoreMap,
        CoreOptimizationOutput,
        LoweringOutput,
        Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure>,
        EmissionOutput,
    ) {
        (
            self.sources,
            self.checked_frontend,
            self.source_to_core,
            self.core_optimization,
            self.lowering,
            self.target_analysis,
            self.emission,
        )
    }
}

/// Stage-aware owned failure from source ingestion through datapack emission.
#[derive(Debug)]
#[non_exhaustive]
pub enum CompilationFailure {
    /// The driver-owned rooted module graph is malformed.
    PackageInput(PackageInputError),
    /// Owned source text could not be admitted to the `u32`-offset source map.
    SourceInput(SourceError),
    /// A frontend producer encountered a non-diagnostic infrastructure failure.
    FrontendInfrastructure {
        /// Source text and any provenance constructed before failure.
        sources: Box<SourceContext>,
        /// Concrete stage-aware infrastructure failure.
        failure: FrontendInfrastructureFailure,
    },
    /// Lexing or parsing produced ordinary user-facing diagnostics.
    Syntax {
        /// Source text and provenance needed to render diagnostics.
        sources: Box<SourceContext>,
        /// Ordered lexer or parser diagnostics.
        diagnostics: Diagnostics,
    },
    /// Name resolution, type checking, or flow checking produced diagnostics.
    Semantic {
        /// Source text and provenance needed to render diagnostics.
        sources: Box<SourceContext>,
        /// Ordered semantic diagnostics.
        diagnostics: Diagnostics,
    },
    /// Typed HIR could not be converted into a complete verified Core program.
    CoreGeneration {
        /// Retained source text and provenance.
        sources: Box<SourceContext>,
        /// Complete checked frontend product consumed by Core generation only by borrow.
        checked_frontend: Box<CheckedFrontendOutput>,
        /// Concrete Core-generation failure.
        failure: Box<CoreGenerationFailure>,
    },
    /// The consuming Core optimization boundary rejected generated Core.
    CoreOptimization {
        /// Retained source text and provenance.
        sources: Box<SourceContext>,
        /// Complete checked frontend product.
        checked_frontend: Box<CheckedFrontendOutput>,
        /// Correlation separated before the Core program was consumed.
        source_to_core: SourceToCoreMap,
        /// Concrete phase-aware optimizer failure.
        failure: Box<CoreOptimizationFailure>,
    },
    /// The verified optimized Core program could not be lowered to Minecraft.
    MinecraftLowering {
        /// Retained source text and provenance.
        sources: Box<SourceContext>,
        /// Complete checked frontend product.
        checked_frontend: Box<CheckedFrontendOutput>,
        /// Retained source/Core correlation.
        source_to_core: SourceToCoreMap,
        /// Verified optimized Core and its report.
        core_optimization: Box<CoreOptimizationOutput>,
        /// Concrete phase-aware Minecraft lowering failure.
        failure: Box<LoweringFailure>,
    },
    /// The verified target program could not be emitted as a complete datapack.
    DatapackEmission {
        /// Retained source text and provenance.
        sources: Box<SourceContext>,
        /// Complete checked frontend product.
        checked_frontend: Box<CheckedFrontendOutput>,
        /// Retained source/Core correlation.
        source_to_core: SourceToCoreMap,
        /// Verified optimized Core and its report.
        core_optimization: Box<CoreOptimizationOutput>,
        /// Complete verified Minecraft lowering product.
        lowering: Box<LoweringOutput>,
        /// Concrete emission diagnostics; no partial artifact is retained.
        diagnostics: Diagnostics,
    },
}

impl CompilationFailure {
    /// Returns the driver-owned package graph error, when validation failed.
    #[must_use]
    pub const fn package_input_error(&self) -> Option<&PackageInputError> {
        match self {
            Self::PackageInput(error) => Some(error),
            Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. }
            | Self::CoreOptimization { .. }
            | Self::MinecraftLowering { .. }
            | Self::DatapackEmission { .. } => None,
        }
    }

    /// Returns the source-ingestion failure, when ingestion was the failed stage.
    #[must_use]
    pub const fn source_input_error(&self) -> Option<&SourceError> {
        match self {
            Self::SourceInput(error) => Some(error),
            Self::PackageInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. }
            | Self::CoreOptimization { .. }
            | Self::MinecraftLowering { .. }
            | Self::DatapackEmission { .. } => None,
        }
    }

    /// Returns the concrete frontend infrastructure failure, when present.
    #[must_use]
    pub const fn frontend_infrastructure_failure(&self) -> Option<&FrontendInfrastructureFailure> {
        match self {
            Self::FrontendInfrastructure { failure, .. } => Some(failure),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. }
            | Self::CoreOptimization { .. }
            | Self::MinecraftLowering { .. }
            | Self::DatapackEmission { .. } => None,
        }
    }

    /// Returns the concrete checked-HIR-to-Core failure, when present.
    #[must_use]
    pub fn core_generation_failure(&self) -> Option<&CoreGenerationFailure> {
        match self {
            Self::CoreGeneration { failure, .. } => Some(failure),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreOptimization { .. }
            | Self::MinecraftLowering { .. }
            | Self::DatapackEmission { .. } => None,
        }
    }

    /// Returns the concrete Core optimization failure, when present.
    #[must_use]
    pub fn core_optimization_failure(&self) -> Option<&CoreOptimizationFailure> {
        match self {
            Self::CoreOptimization { failure, .. } => Some(failure),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. }
            | Self::MinecraftLowering { .. }
            | Self::DatapackEmission { .. } => None,
        }
    }

    /// Returns the concrete Minecraft lowering failure, when present.
    #[must_use]
    pub fn minecraft_lowering_failure(&self) -> Option<&LoweringFailure> {
        match self {
            Self::MinecraftLowering { failure, .. } => Some(failure),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. }
            | Self::CoreOptimization { .. }
            | Self::DatapackEmission { .. } => None,
        }
    }

    /// Returns retained source context, when source ingestion completed.
    #[must_use]
    pub fn sources(&self) -> Option<&SourceContext> {
        match self {
            Self::PackageInput(_) | Self::SourceInput(_) => None,
            Self::FrontendInfrastructure { sources, .. }
            | Self::Syntax { sources, .. }
            | Self::Semantic { sources, .. }
            | Self::CoreGeneration { sources, .. }
            | Self::CoreOptimization { sources, .. }
            | Self::MinecraftLowering { sources, .. }
            | Self::DatapackEmission { sources, .. } => Some(sources.as_ref()),
        }
    }

    /// Returns the checked frontend product retained by a downstream failure.
    #[must_use]
    pub const fn checked_frontend(&self) -> Option<&CheckedFrontendOutput> {
        match self {
            Self::CoreGeneration {
                checked_frontend, ..
            }
            | Self::CoreOptimization {
                checked_frontend, ..
            }
            | Self::MinecraftLowering {
                checked_frontend, ..
            }
            | Self::DatapackEmission {
                checked_frontend, ..
            } => Some(checked_frontend),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. } => None,
        }
    }

    /// Returns the source/Core correlation retained after Core generation.
    #[must_use]
    pub const fn source_to_core(&self) -> Option<&SourceToCoreMap> {
        match self {
            Self::CoreOptimization { source_to_core, .. }
            | Self::MinecraftLowering { source_to_core, .. }
            | Self::DatapackEmission { source_to_core, .. } => Some(source_to_core),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. } => None,
        }
    }

    /// Returns optimized Core retained by a downstream failure.
    #[must_use]
    pub fn core_optimization(&self) -> Option<&CoreOptimizationOutput> {
        match self {
            Self::MinecraftLowering {
                core_optimization, ..
            }
            | Self::DatapackEmission {
                core_optimization, ..
            } => Some(core_optimization),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. }
            | Self::CoreOptimization { .. } => None,
        }
    }

    /// Returns verified lowering retained when only datapack emission failed.
    #[must_use]
    pub fn lowering(&self) -> Option<&LoweringOutput> {
        match self {
            Self::DatapackEmission { lowering, .. } => Some(lowering),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::Syntax { .. }
            | Self::Semantic { .. }
            | Self::CoreGeneration { .. }
            | Self::CoreOptimization { .. }
            | Self::MinecraftLowering { .. } => None,
        }
    }

    /// Returns ordinary user-facing diagnostics carried directly by this failure.
    #[must_use]
    pub const fn diagnostics(&self) -> Option<&Diagnostics> {
        match self {
            Self::Syntax { diagnostics, .. }
            | Self::Semantic { diagnostics, .. }
            | Self::DatapackEmission { diagnostics, .. } => Some(diagnostics),
            Self::PackageInput(_)
            | Self::SourceInput(_)
            | Self::FrontendInfrastructure { .. }
            | Self::CoreGeneration { .. }
            | Self::CoreOptimization { .. }
            | Self::MinecraftLowering { .. } => None,
        }
    }
}

impl fmt::Display for CompilationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PackageInput(error) => write!(formatter, "invalid package input: {error}"),
            Self::SourceInput(error) => write!(formatter, "source ingestion failed: {error}"),
            Self::FrontendInfrastructure { failure, .. } => write!(formatter, "{failure}"),
            Self::Syntax { diagnostics, .. } => {
                write!(formatter, "source syntax is invalid: {diagnostics}")
            }
            Self::Semantic { diagnostics, .. } => {
                write!(formatter, "source semantics are invalid: {diagnostics}")
            }
            Self::CoreGeneration { failure, .. } => {
                write!(formatter, "Core generation failed: {failure}")
            }
            Self::CoreOptimization { failure, .. } => write!(formatter, "{failure}"),
            Self::MinecraftLowering { failure, .. } => write!(formatter, "{failure}"),
            Self::DatapackEmission { diagnostics, .. } => {
                write!(formatter, "datapack emission failed: {diagnostics}")
            }
        }
    }
}

impl Error for CompilationFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PackageInput(error) => Some(error),
            Self::SourceInput(error) => Some(error),
            Self::FrontendInfrastructure { failure, .. } => Some(failure),
            Self::Syntax { diagnostics, .. }
            | Self::Semantic { diagnostics, .. }
            | Self::DatapackEmission { diagnostics, .. } => Some(diagnostics),
            Self::CoreGeneration { failure, .. } => Some(failure.as_ref()),
            Self::CoreOptimization { failure, .. } => Some(failure.as_ref()),
            Self::MinecraftLowering { failure, .. } => Some(failure.as_ref()),
        }
    }
}

/// Compiles one owned in-memory source file into a complete in-memory datapack.
///
/// Each producer is executed exactly once. Target-cost analysis is explicitly
/// nonfatal and remains an owned `Result` in [`CompilationOutput`].
///
/// # Errors
///
/// Returns the first failed compilation stage together with every complete earlier
/// product required by the ownership contract. Invalid frontend syntax and semantics
/// retain their [`SourceContext`] so their provenance can be rendered.
pub fn compile_source(
    input: SourceInput,
    options: &CompilationOptions,
) -> Result<CompilationOutput, CompilationFailure> {
    let root = ModuleKey::single_source_root();
    compile_package(
        PackageInput::new(
            root.clone(),
            vec![ModuleInput::new(root, input, Vec::new())],
        ),
        options,
    )
}

/// Compiles one complete driver-owned rooted module graph into an in-memory datapack.
///
/// Package validation and canonicalization happen before source ingestion. The
/// compiler never opens an import path; every dependency edge and source buffer is
/// owned by `input`.
///
/// # Errors
///
/// Returns a structured graph error or the first failed compilation stage together
/// with every complete earlier product required by the ownership contract.
pub fn compile_package(
    input: PackageInput,
    options: &CompilationOptions,
) -> Result<CompilationOutput, CompilationFailure> {
    let (sources, checked_frontend) = compile_package_frontend(input, options.frontend())?;

    let core_generation =
        match lower_hir(&checked_frontend, &sources, options.frontend().max_tokens()) {
            Ok(output) => output,
            Err(failure) => {
                return Err(CompilationFailure::CoreGeneration {
                    sources: Box::new(sources),
                    checked_frontend: Box::new(checked_frontend),
                    failure: Box::new(failure),
                });
            }
        };
    let (core_program, source_to_core) = core_generation.into_parts();

    let core_optimization = match optimize_core(core_program, &sources, options.core_optimization())
    {
        Ok(output) => output,
        Err(failure) => {
            return Err(CompilationFailure::CoreOptimization {
                sources: Box::new(sources),
                checked_frontend: Box::new(checked_frontend),
                source_to_core,
                failure: Box::new(failure),
            });
        }
    };

    let lowering = match lower_to_minecraft(
        core_optimization.program(),
        &sources,
        options.minecraft_lowering(),
    ) {
        Ok(output) => output,
        Err(failure) => {
            return Err(CompilationFailure::MinecraftLowering {
                sources: Box::new(sources),
                checked_frontend: Box::new(checked_frontend),
                source_to_core,
                core_optimization: Box::new(core_optimization),
                failure: Box::new(failure),
            });
        }
    };

    let target_analysis = lowering.analyze_target_execution(options.target_analysis());
    let emission = match emit_datapack(lowering.program(), &sources, options.emission()) {
        Ok(output) => output,
        Err(diagnostics) => {
            return Err(CompilationFailure::DatapackEmission {
                sources: Box::new(sources),
                checked_frontend: Box::new(checked_frontend),
                source_to_core,
                core_optimization: Box::new(core_optimization),
                lowering: Box::new(lowering),
                diagnostics,
            });
        }
    };

    Ok(CompilationOutput {
        sources,
        checked_frontend,
        source_to_core,
        core_optimization,
        lowering,
        target_analysis,
        emission,
    })
}

const PACKAGE_SYNTAX_TRUNCATED: &str = "frontend.package.syntax-truncated";
const PACKAGE_TOKEN_LIMIT: &str = "frontend.package.token-limit";

struct ParsedPackageModule {
    id: SourceModuleId,
    key: ModuleKey,
    dependencies: Box<[ModuleDependency]>,
    ast: AstModule,
}

enum ParsedPackageModuleOutput {
    Parsed(ParsedPackageModule, usize),
    Diagnostics(Diagnostics, usize),
}

enum PackageModuleFailure {
    SourceInput(SourceError),
    Infrastructure(FrontendInfrastructureFailure),
}

fn compile_package_frontend(
    input: PackageInput,
    limits: FrontendLimits,
) -> Result<(SourceContext, CheckedFrontendOutput), CompilationFailure> {
    let input = validate_package_input(input, limits)?;
    let (root_key, modules) = input.into_parts();
    let mut sources = SourceContext::new();
    let mut parsed_modules = Vec::with_capacity(modules.len());
    let mut syntax_diagnostics = PackageDiagnosticSink::new(limits.max_diagnostics());
    let mut remaining_package_tokens = limits.max_package_tokens();
    let mut root = None;

    for (index, module) in Vec::from(modules).into_iter().enumerate() {
        let Some(id) = SourceModuleId::from_index(index) else {
            return Err(CompilationFailure::FrontendInfrastructure {
                sources: Box::new(sources),
                failure: FrontendInfrastructureFailure::IdentitySpaceExhausted {
                    entity: FrontendEntityKind::Module,
                },
            });
        };
        if module.key() == &root_key {
            root = Some(id);
        }
        let package_budget_exhausted = remaining_package_tokens == 0;
        let module_token_budget = remaining_package_tokens.max(1).min(limits.max_tokens());
        let module_limits = limits.with_module_token_budget(module_token_budget);
        match parse_package_module(
            &mut sources,
            id,
            module,
            module_limits,
            package_budget_exhausted,
        ) {
            Ok(ParsedPackageModuleOutput::Parsed(module, token_count)) => {
                remaining_package_tokens = remaining_package_tokens.saturating_sub(token_count);
                parsed_modules.push(module);
            }
            Ok(ParsedPackageModuleOutput::Diagnostics(diagnostics, token_count)) => {
                remaining_package_tokens = remaining_package_tokens.saturating_sub(token_count);
                syntax_diagnostics.extend(diagnostics);
            }
            Err(PackageModuleFailure::SourceInput(error)) => {
                return Err(CompilationFailure::SourceInput(error));
            }
            Err(PackageModuleFailure::Infrastructure(failure)) => {
                return Err(CompilationFailure::FrontendInfrastructure {
                    sources: Box::new(sources),
                    failure,
                });
            }
        }
    }

    if let Some(diagnostics) = syntax_diagnostics.finish() {
        return Err(CompilationFailure::Syntax {
            sources: Box::new(sources),
            diagnostics,
        });
    }
    let Some(root) = root else {
        return Err(CompilationFailure::PackageInput(
            PackageInputError::MissingRoot { root: root_key },
        ));
    };
    let modules = parsed_modules
        .iter()
        .map(|module| PackageAstModule {
            id: module.id,
            key: &module.key,
            dependencies: &module.dependencies,
            ast: &module.ast,
        })
        .collect::<Vec<_>>();
    let checked = match check_package(&mut sources, &modules, root, limits) {
        Ok(checked) => checked,
        Err(error) => {
            return Err(CompilationFailure::FrontendInfrastructure {
                sources: Box::new(sources),
                failure: error.into(),
            });
        }
    };
    let (checked_frontend, diagnostics) = checked.into_parts();
    let checked_frontend = match (checked_frontend, diagnostics) {
        (Some(checked), None) => checked,
        (None, Some(diagnostics)) => {
            return Err(CompilationFailure::Semantic {
                sources: Box::new(sources),
                diagnostics,
            });
        }
        (None, None) | (Some(_), Some(_)) => {
            return Err(CompilationFailure::FrontendInfrastructure {
                sources: Box::new(sources),
                failure: FrontendInfrastructureFailure::InvalidSemanticOutput,
            });
        }
    };
    Ok((sources, checked_frontend))
}

fn validate_package_input(
    input: PackageInput,
    limits: FrontendLimits,
) -> Result<PackageInput, CompilationFailure> {
    let input = input.validate().map_err(CompilationFailure::PackageInput)?;
    if input.modules().len() > limits.max_modules() {
        return Err(CompilationFailure::PackageInput(
            PackageInputError::ModuleLimitExceeded {
                actual: input.modules().len(),
                limit: limits.max_modules(),
            },
        ));
    }
    Ok(input)
}

fn parse_package_module(
    sources: &mut SourceContext,
    id: SourceModuleId,
    module: ModuleInput,
    limits: FrontendLimits,
    package_budget_exhausted: bool,
) -> Result<ParsedPackageModuleOutput, PackageModuleFailure> {
    let (key, source, dependencies) = module.into_parts();
    let (name, text) = source.into_parts();
    let file = sources
        .add_file(name, text)
        .map_err(PackageModuleFailure::SourceInput)?;
    if package_budget_exhausted {
        let span = sources
            .span(file, 0, 0)
            .map_err(PackageModuleFailure::SourceInput)?;
        let origin = sources.add_origin(Origin::Source(span)).map_err(|error| {
            PackageModuleFailure::Infrastructure(FrontendInfrastructureFailure::Origin {
                phase: FrontendInfrastructurePhase::Lexing,
                error,
            })
        })?;
        let diagnostics = Diagnostics::from_findings(vec![Diagnostic::new(
            PACKAGE_TOKEN_LIMIT,
            "package token limit was exhausted before this module",
            origin,
        )])
        .expect("one package token-limit finding is nonempty");
        return Ok(ParsedPackageModuleOutput::Diagnostics(diagnostics, 0));
    }
    let lexed = lex(sources, file, limits)
        .map_err(|error| PackageModuleFailure::Infrastructure(error.into()))?;
    let (tokens, diagnostics) = lexed.into_parts();
    let token_count = tokens.len();
    if let Some(diagnostics) = diagnostics {
        return Ok(ParsedPackageModuleOutput::Diagnostics(
            diagnostics,
            token_count,
        ));
    }

    let parsed = parse(sources, file, &tokens, limits)
        .map_err(|error| PackageModuleFailure::Infrastructure(error.into()))?;
    let (ast, diagnostics) = parsed.into_parts();
    drop(tokens);
    if let Some(diagnostics) = diagnostics {
        return Ok(ParsedPackageModuleOutput::Diagnostics(
            diagnostics,
            token_count,
        ));
    }
    Ok(ParsedPackageModuleOutput::Parsed(
        ParsedPackageModule {
            id,
            key,
            dependencies,
            ast,
        },
        token_count,
    ))
}

struct PackageDiagnosticSink {
    max_diagnostics: usize,
    findings: Vec<Diagnostic>,
    truncated: bool,
}

impl PackageDiagnosticSink {
    const fn new(max_diagnostics: usize) -> Self {
        Self {
            max_diagnostics,
            findings: vec![],
            truncated: false,
        }
    }

    fn extend(&mut self, diagnostics: Diagnostics) {
        for diagnostic in diagnostics.into_findings() {
            if self.findings.len() < self.max_diagnostics {
                self.findings.push(diagnostic);
            } else if !self.truncated {
                self.truncated = true;
                self.findings.push(Diagnostic::new(
                    PACKAGE_SYNTAX_TRUNCATED,
                    "additional package syntax diagnostics were omitted",
                    diagnostic.origin(),
                ));
            }
        }
    }

    fn finish(self) -> Option<Diagnostics> {
        Diagnostics::from_findings(self.findings)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{
        CompilationFailure, CompilationOptions, FrontendEntityKind, FrontendInfrastructureFailure,
        FrontendInfrastructurePhase, PACKAGE_SYNTAX_TRUNCATED, PACKAGE_TOKEN_LIMIT, SourceInput,
        compile_package, compile_source,
    };
    use crate::analysis::minecraft::{
        AnalysisArithmeticCaps, CommandLimitAssumptions, TargetExecutionAnalysisLimits,
        TargetExecutionAnalysisPhase,
    };
    use crate::datapack::EmissionOptions;
    use crate::entity::EntityId;
    use crate::frontend::check::{CheckError, CheckedEntityKind};
    use crate::frontend::lexer::LexerError;
    use crate::frontend::parser::ParserError;
    use crate::frontend::{
        CoreGenerationFailure, CoreGenerationInvariant, CoreGenerationResource, FrontendLimits,
        FunctionVisibility, ImportName, ModuleDependency, ModuleInput, ModuleKey, PackageInput,
        PackageInputError,
    };
    use crate::ir::core::{
        CanonicalPrinter, CoreFunctionLinkage, CoreOp, CoreProgram, ExternalSemanticBinding,
    };
    use crate::ir::minecraft::{CommandKind, ObjectiveName, PackNamespace};
    use crate::ir::semantic::{ContextRequirement, EntityKind, ObservableEffect};
    use crate::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
    use crate::opt::core::{CoreOptimizationLevel, CoreOptimizationOptions, optimize_core};
    use crate::source::{OriginId, SourceContext, SourceError};
    use crate::target::JavaEditionTarget;

    fn options() -> CompilationOptions {
        options_with_frontend(FrontendLimits::DEFAULT)
    }

    fn options_with_frontend(frontend: FrontendLimits) -> CompilationOptions {
        let lowering = LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl_stage6").unwrap(),
            ObjectiveName::new("mdl.stage6").unwrap(),
        )
        .unwrap()
        .with_optimization_level(MinecraftOptimizationLevel::None);
        let assumptions = lowering.command_limit_assumptions();
        CompilationOptions::new(
            frontend,
            CoreOptimizationOptions::new(CoreOptimizationLevel::None),
            lowering,
            TargetExecutionAnalysisLimits::new(
                AnalysisArithmeticCaps::minimum_for(assumptions),
                100_000,
                100_000,
            ),
            EmissionOptions::new("Stage 6 facade test"),
        )
    }

    fn options_with_core(level: CoreOptimizationLevel) -> CompilationOptions {
        let (frontend, _, lowering, analysis, emission) = options().into_parts();
        CompilationOptions::new(
            frontend,
            CoreOptimizationOptions::new(level),
            lowering,
            analysis,
            emission,
        )
    }

    fn options_with_levels(
        core_level: CoreOptimizationLevel,
        minecraft_level: MinecraftOptimizationLevel,
    ) -> CompilationOptions {
        let (frontend, _, lowering, analysis, emission) = options().into_parts();
        CompilationOptions::new(
            frontend,
            CoreOptimizationOptions::new(core_level),
            lowering.with_optimization_level(minecraft_level),
            analysis,
            emission,
        )
    }

    fn module_key(value: &str) -> ModuleKey {
        ModuleKey::new(value).unwrap()
    }

    fn import_name(value: &str) -> ImportName {
        ImportName::new(value).unwrap()
    }

    fn module(
        key: &str,
        text: &str,
        dependencies: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> ModuleInput {
        ModuleInput::new(
            module_key(key),
            SourceInput::new(format!("{key}.mdl"), text),
            dependencies
                .into_iter()
                .map(|(name, target)| ModuleDependency::new(import_name(name), module_key(target)))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn source_input_and_options_retain_exact_owned_values() {
        let input = SourceInput::new("owned.mdl", "fn main() {}".to_owned());
        assert_eq!(input.name(), "owned.mdl");
        assert_eq!(input.text(), "fn main() {}");
        assert_eq!(
            input.into_parts(),
            (
                Box::<str>::from("owned.mdl"),
                Box::<str>::from("fn main() {}")
            )
        );

        let options = options();
        assert_eq!(options.frontend(), FrontendLimits::DEFAULT);
        assert_eq!(
            options.core_optimization().level(),
            CoreOptimizationLevel::None
        );
        assert_eq!(
            options.minecraft_lowering().optimization_level(),
            MinecraftOptimizationLevel::None
        );
        assert_eq!(options.emission().description(), "Stage 6 facade test");
        let expected = options.clone();
        let (frontend, core, lowering, analysis, emission) = options.into_parts();
        assert_eq!(frontend, expected.frontend());
        assert_eq!(core, *expected.core_optimization());
        assert_eq!(lowering, *expected.minecraft_lowering());
        assert_eq!(analysis, expected.target_analysis());
        assert_eq!(emission, *expected.emission());
    }

    #[test]
    fn compile_source_retains_every_product_and_composes_the_abi_maps() {
        let output = compile_source(
            SourceInput::new(
                "identity.mdl",
                "fn identity(value: Int32) -> Int32 { return value; }",
            ),
            &options(),
        )
        .unwrap();

        assert_eq!(output.sources().files().len(), 1);
        assert_eq!(output.checked_frontend().function_count(), 1);
        assert_eq!(output.source_to_core().len(), 1);
        assert_eq!(
            output.core_optimization().report().level(),
            CoreOptimizationLevel::None
        );
        assert_eq!(output.lowering().map().len(), 1);
        assert_eq!(output.lowering().map().export_count(), 0);
        assert!(output.target_analysis().is_ok());
        assert!(output.emission().pack().file("pack.mcmeta").is_some());

        let source_function = output.checked_frontend().function_ids().next().unwrap();
        let core_function = output.source_to_core().function(source_function).unwrap();
        let direct = output.lowering().map().function(core_function).unwrap();
        let composed = output.source_function_abi(source_function).unwrap();
        assert_eq!(composed, direct);
        assert_eq!(
            output.source_function_name(source_function),
            Some("identity")
        );
        assert_eq!(composed.parameter_homes().len(), 1);
        assert_eq!(composed.result_homes().len(), 1);
        assert_eq!(composed.linkage(), CoreFunctionLinkage::Internal);

        let (sources, checked, map, core, lowering, analysis, emission) = output.into_parts();
        assert_eq!(sources.files().len(), 1);
        assert_eq!(checked.function_count(), 1);
        assert_eq!(map.len(), 1);
        assert_eq!(core.report().level(), CoreOptimizationLevel::None);
        assert_eq!(lowering.map().len(), 1);
        assert!(analysis.is_ok());
        assert!(!emission.pack().files().is_empty());
    }

    #[test]
    fn literal_unsafe_command_crosses_every_verified_boundary_without_becoming_safe() {
        let source = SourceInput::new(
            "unsafe.mdl",
            r#"export fn announce() {
    unsafe minecraft("say stage7 unsafe");
}"#,
        );
        for core_level in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
            for minecraft_level in [
                MinecraftOptimizationLevel::None,
                MinecraftOptimizationLevel::Baseline,
            ] {
                let output = compile_source(
                    source.clone(),
                    &options_with_levels(core_level, minecraft_level),
                )
                .unwrap();
                assert_eq!(output.checked_frontend().external_operation_count(), 1);
                let source_external = output
                    .checked_frontend()
                    .external_operation_ids()
                    .next()
                    .unwrap();
                let core_external = output
                    .source_to_core()
                    .external_operation(source_external)
                    .expect("unsafe source external still has a general Core declaration");
                assert!(matches!(
                    output
                        .core_optimization()
                        .program()
                        .external_op(core_external)
                        .unwrap()
                        .binding(),
                    ExternalSemanticBinding::UnsafeTargetFragment(_)
                ));
                assert_eq!(
                    output.source_to_core().semantic_operation(source_external),
                    None
                );
                assert_eq!(output.source_to_core().semantic_operations().count(), 0);
                assert_eq!(output.source_semantic_command(source_external), None);
                let exported = output.checked_frontend().function_ids().next().unwrap();
                let behavior = output.source_function_behavior(exported).unwrap();
                assert!(behavior.required_ambient_context().executor().is_unknown());
                assert!(behavior.world_effect().is_unknown());
                assert!(behavior.fork_bound().is_unknown());
                assert!(behavior.transitive_work().is_unknown());
                assert!(behavior.contains_unsafe_unknown());
                assert_eq!(output.core_optimization().program().external_ops().len(), 1);
                assert_eq!(
                    output
                        .core_optimization()
                        .program()
                        .target_fragments()
                        .len(),
                    1
                );

                let raw_files = output
                    .emission()
                    .pack()
                    .files()
                    .iter()
                    .filter(|file| {
                        file.path().as_str().ends_with(".mcfunction")
                            && file
                                .bytes()
                                .windows(b"say stage7 unsafe\n".len())
                                .any(|window| window == b"say stage7 unsafe\n")
                    })
                    .count();
                assert_eq!(raw_files, 1, "{core_level:?}/{minecraft_level:?}");
                let analysis = output.target_analysis().as_ref().unwrap();
                assert_eq!(analysis.census().raw_commands(), 1);
            }
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the vertical identity proof intentionally keeps every source-to-target layer in one four-policy test"
    )]
    fn typed_say_retains_direct_command_correlation_and_generated_context() {
        let source = SourceInput::new(
            "typed-say.mdl",
            r#"export fn announce() {
    run.as(mc.entities(ArmorStand).with_tag("stage7").limit(1)) |speaker| {
        speaker.say("hello from typed say");
    }
}"#,
        );
        let mut expected_correlation = None;
        for core_level in [CoreOptimizationLevel::None, CoreOptimizationLevel::Baseline] {
            for minecraft_level in [
                MinecraftOptimizationLevel::None,
                MinecraftOptimizationLevel::Baseline,
            ] {
                let output = compile_source(
                    source.clone(),
                    &options_with_levels(core_level, minecraft_level),
                )
                .unwrap();
                let commands = output
                    .lowering()
                    .map()
                    .semantic_commands()
                    .collect::<Vec<_>>();
                let [(function, instruction, location)] = commands.as_slice() else {
                    panic!("typed Say must correlate to exactly one generated command")
                };
                let source_external = output
                    .checked_frontend()
                    .external_operation_ids()
                    .next()
                    .expect("typed Say has one source external identity");
                let occurrence = output
                    .source_to_core()
                    .semantic_operation(source_external)
                    .expect("typed Say has one exact generated Core occurrence");
                assert_eq!(
                    output.source_to_core().external_operation(source_external),
                    Some(occurrence.external())
                );
                assert_eq!(
                    output
                        .source_to_core()
                        .semantic_operations()
                        .collect::<Vec<_>>(),
                    [(source_external, occurrence)]
                );
                assert_eq!(occurrence.function(), *function);
                assert_eq!(occurrence.instruction(), *instruction);
                assert_eq!(
                    output.source_semantic_command(source_external),
                    Some(*location)
                );
                let correlation = (source_external, occurrence, *location);
                if let Some(expected) = expected_correlation {
                    assert_eq!(correlation, expected, "{core_level:?}/{minecraft_level:?}");
                } else {
                    expected_correlation = Some(correlation);
                }
                assert_eq!(
                    output
                        .lowering()
                        .map()
                        .semantic_command(*function, *instruction),
                    Some(*location)
                );
                let core = output.core_optimization().program();
                let core_instruction = core
                    .function(occurrence.function())
                    .and_then(|function| function.body())
                    .and_then(|body| body.instruction(occurrence.instruction()))
                    .expect("correlated typed Say Core instruction exists");
                assert_eq!(
                    core_instruction.op(),
                    &CoreOp::External(occurrence.external())
                );
                let ExternalSemanticBinding::MinecraftOperation(operation) = core
                    .external_op(occurrence.external())
                    .expect("correlated typed Say external declaration exists")
                    .binding()
                else {
                    panic!("typed source correlation must name a Minecraft operation")
                };
                let operation = core.minecraft_operation(operation).unwrap();
                let source_spelling = |origin| {
                    output
                        .sources()
                        .resolve_origin_span(origin)
                        .and_then(|span| output.sources().files().slice(span).ok())
                        .unwrap()
                };
                assert_eq!(
                    source_spelling(operation.origins().call()),
                    r#"speaker.say("hello from typed say")"#
                );
                assert_eq!(source_spelling(operation.origins().member()), "say");
                assert_eq!(source_spelling(operation.origins().receiver()), "speaker");
                assert_eq!(
                    source_spelling(operation.attributes().message_origin()),
                    r#""hello from typed say""#
                );
                let command = output
                    .lowering()
                    .program()
                    .function(location.function())
                    .and_then(|function| function.body().command(location.command()))
                    .expect("correlated typed Say command exists");
                let CommandKind::Say(say) = command.kind() else {
                    panic!("typed Say correlation must never point at raw target text")
                };
                assert_eq!(say.message().as_str(), "hello from typed say");
                assert_eq!(
                    output
                        .lowering()
                        .map()
                        .function(*function)
                        .unwrap()
                        .generated_entry_requirement()
                        .executor(),
                    ContextRequirement::Required(EntityKind::ArmorStand)
                );
                let helper_fragment = format!("/f{}/x0", function.index());
                assert!(output.lowering().program().functions().all(|(_, target)| {
                    !target.resource().to_string().contains(&helper_fragment)
                }));
                assert_eq!(
                    output
                        .source_function_behavior(
                            output.checked_frontend().function_ids().next().unwrap()
                        )
                        .unwrap()
                        .observable_effect(),
                    ObservableEffect::Observable
                );
                let dump = output.lowering().dump_lowering();
                assert!(dump.contains("semantic=Say recipe=Java26_2Say placement=direct"));
                assert!(dump.contains("constructed-semantic-command"));
            }
        }
    }

    #[test]
    fn unsafe_command_target_length_is_rejected_only_at_target_legality() {
        let max = usize::try_from(
            JavaEditionTarget::V26_2
                .spec()
                .max_logical_command_utf16_units(),
        )
        .unwrap();
        let command = "x".repeat(max + 1);
        let failure = compile_source(
            SourceInput::new(
                "long_unsafe.mdl",
                format!("export fn run() {{ unsafe minecraft(\"{command}\"); }}"),
            ),
            &options(),
        )
        .unwrap_err();
        assert!(failure.checked_frontend().is_some());
        assert!(failure.core_optimization().is_some());
        assert!(failure.lowering().is_none());
        let lowering = failure.minecraft_lowering_failure().unwrap();
        assert_eq!(
            lowering.phase(),
            crate::lower::minecraft::LoweringPhase::Legality
        );
        assert!(
            lowering
                .diagnostics()
                .contains_code("lower.invalid-target-fragment")
        );
    }

    #[test]
    fn source_run_scope_obeys_fork_limits_and_admits_bounded_serial_multiplicity() {
        let source = SourceInput::new(
            "strict-forks.mdl",
            "export fn scoped() { run.as(mc.entities(ArmorStand).limit(1)) {} }",
        );
        let options_for_forks = |max_command_forks| {
            let (frontend, core, lowering, _analysis, emission) = options().into_parts();
            let assumptions = CommandLimitAssumptions::new(100, max_command_forks).unwrap();
            CompilationOptions::new(
                frontend,
                core,
                lowering.with_command_limit_assumptions(assumptions),
                TargetExecutionAnalysisLimits::new(
                    AnalysisArithmeticCaps::minimum_for(assumptions),
                    100_000,
                    100_000,
                ),
                emission,
            )
        };

        let failure = compile_source(source.clone(), &options_for_forks(1)).unwrap_err();
        let lowering = failure.minecraft_lowering_failure().unwrap();
        assert_eq!(
            lowering.phase(),
            crate::lower::minecraft::LoweringPhase::Legality
        );
        assert!(
            lowering
                .diagnostics()
                .contains_code("lower.unsupported-command-fork-assumption")
        );

        let output = compile_source(source, &options_for_forks(2)).unwrap();
        assert_eq!(output.lowering().map().export_count(), 1);
        let map_contract = output
            .lowering()
            .map()
            .execution_contract()
            .command_limits();
        assert_eq!(
            map_contract.configured_assumptions(),
            CommandLimitAssumptions::new(100, 2).unwrap()
        );
        assert_eq!(
            map_contract.target_defaults(),
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2)
        );
        assert_eq!(map_contract.minimum_max_command_forks(), 2);
        assert_eq!(
            output
                .lowering()
                .report()
                .execution_contract()
                .command_limits(),
            map_contract
        );

        let many = SourceInput::new(
            "serial-forks.mdl",
            "export fn scoped() { run.as(mc.entities(ArmorStand).limit(3)) {} }",
        );
        let failure = compile_source(many.clone(), &options_for_forks(3)).unwrap_err();
        assert!(
            failure
                .minecraft_lowering_failure()
                .unwrap()
                .diagnostics()
                .contains_code("lower.unsupported-command-fork-assumption")
        );
        let output = compile_source(many, &options_for_forks(4)).unwrap();
        assert_eq!(
            output.lowering().map().execution_contract().activation(),
            crate::lower::minecraft::ActivationContract::SynchronousSerial
        );
        assert_eq!(
            output
                .lowering()
                .map()
                .execution_contract()
                .command_limits()
                .minimum_max_command_forks(),
            4
        );
    }

    #[test]
    fn compile_package_resolves_public_members_in_canonical_module_order() {
        let package = PackageInput::new(
            module_key("root"),
            vec![
                module(
                    "root",
                    r#"const api = import("api");
export fn run(value: Int32) -> Int32 { return api.identity(value); }"#,
                    [("api", "api")],
                ),
                module(
                    "api",
                    "pub fn identity(value: Int32) -> Int32 { return value; }",
                    [],
                ),
            ],
        );

        let output = compile_package(package.clone(), &options()).unwrap();
        let checked = output.checked_frontend();
        assert_eq!(checked.module_count(), 2);
        let modules = checked.module_ids().collect::<Vec<_>>();
        assert_eq!(
            modules
                .iter()
                .map(|module| checked.module_key(*module).unwrap().as_str())
                .collect::<Vec<_>>(),
            ["api", "root"]
        );
        assert_eq!(checked.root_module(), modules[1]);
        assert_eq!(
            checked.module_key(checked.root_module()).unwrap().as_str(),
            "root"
        );
        let functions = checked.function_ids().collect::<Vec<_>>();
        assert_eq!(functions.len(), 2);
        assert_eq!(checked.function_module(functions[0]).unwrap().index(), 0);
        assert_eq!(
            checked.function_visibility(functions[0]),
            Some(FunctionVisibility::Public)
        );
        assert_eq!(checked.function_module(functions[1]).unwrap().index(), 1);
        assert_eq!(
            checked.function_visibility(functions[1]),
            Some(FunctionVisibility::DatapackExport)
        );
        assert!(
            checked
                .dump(output.sources())
                .contains("call @0(%0:Int32):Int32")
        );
        assert_eq!(output.source_to_core().len(), 2);
        assert_eq!(output.lowering().map().len(), 2);
        assert_eq!(output.lowering().map().export_count(), 1);
        assert_eq!(
            output
                .lowering()
                .map()
                .exported_functions()
                .map(|(_, function)| function.linkage())
                .collect::<Vec<_>>(),
            [CoreFunctionLinkage::DatapackExport]
        );
        assert_eq!(
            output
                .core_optimization()
                .program()
                .functions()
                .map(|(_, function)| function.linkage())
                .collect::<Vec<_>>(),
            [
                CoreFunctionLinkage::Internal,
                CoreFunctionLinkage::DatapackExport,
            ]
        );
        assert!(!output.emission().pack().files().is_empty());

        let optimized =
            compile_package(package, &options_with_core(CoreOptimizationLevel::Baseline)).unwrap();
        assert_eq!(optimized.lowering().map().export_count(), 1);
        assert_eq!(
            optimized
                .core_optimization()
                .program()
                .exported_functions()
                .count(),
            1
        );
    }

    #[test]
    fn compile_package_rejects_private_cross_module_members_with_source_context() {
        let package = PackageInput::new(
            module_key("root"),
            vec![
                module("api", "fn hidden() {}", []),
                module(
                    "root",
                    r#"const api = import("api");
fn run() { api.hidden(); }"#,
                    [("api", "api")],
                ),
            ],
        );

        let failure = compile_package(package, &options()).unwrap_err();
        assert!(failure.sources().is_some());
        let diagnostics = failure.diagnostics().unwrap();
        assert!(diagnostics.contains_code("frontend.check.private-member"));
        assert_eq!(diagnostics.findings()[0].supporting_labels().len(), 1);
        assert!(matches!(failure, CompilationFailure::Semantic { .. }));
    }

    #[test]
    fn package_input_failure_is_typed_and_precedes_source_ingestion() {
        let package = PackageInput::new(
            module_key("root"),
            vec![
                module("root", "fn root() {}", []),
                module("unused", "fn bad(", []),
            ],
        );
        let failure = compile_package(package, &options()).unwrap_err();
        assert_eq!(
            failure.package_input_error(),
            Some(&PackageInputError::UnreachableModule {
                key: module_key("unused")
            })
        );
        assert!(failure.sources().is_none());
        assert!(failure.diagnostics().is_none());
    }

    #[test]
    fn package_input_permutation_preserves_semantics_and_emitted_bytes() {
        let build = |reverse: bool| {
            let mut modules = vec![
                module(
                    "root",
                    r#"const api = import("api");
const util = import("util");
export fn run(value: Int32) -> Int32 { return util.twice(api.identity(value)); }"#,
                    if reverse {
                        vec![("util", "util"), ("api", "api")]
                    } else {
                        vec![("api", "api"), ("util", "util")]
                    },
                ),
                module(
                    "api",
                    "pub fn identity(value: Int32) -> Int32 { return value; }",
                    [],
                ),
                module(
                    "util",
                    "pub fn twice(value: Int32) -> Int32 { return value; }",
                    [],
                ),
            ];
            if reverse {
                modules.reverse();
            }
            compile_package(PackageInput::new(module_key("root"), modules), &options()).unwrap()
        };

        let forward = build(false);
        let reverse = build(true);
        assert_eq!(
            forward.checked_frontend().dump(forward.sources()),
            reverse.checked_frontend().dump(reverse.sources())
        );
        assert_eq!(
            CanonicalPrinter::new(forward.core_optimization().program(), forward.sources())
                .unwrap()
                .render(),
            CanonicalPrinter::new(reverse.core_optimization().program(), reverse.sources())
                .unwrap()
                .render()
        );
        assert_eq!(
            forward.lowering().dump_lowering(),
            reverse.lowering().dump_lowering()
        );
        assert_eq!(forward.emission().pack(), reverse.emission().pack());
        assert_eq!(
            forward.emission().trace().records().collect::<Vec<_>>(),
            reverse.emission().trace().records().collect::<Vec<_>>()
        );
    }

    #[test]
    fn package_limits_bound_modules_tokens_and_diagnostics_globally() {
        let package = || {
            PackageInput::new(
                module_key("root"),
                vec![module("api", "", []), module("root", "", [("api", "api")])],
            )
        };
        let module_limits = FrontendLimits::new(100, 32, 10)
            .unwrap()
            .with_package_limits(1, 100)
            .unwrap();
        let module_failure =
            compile_package(package(), &options_with_frontend(module_limits)).unwrap_err();
        assert_eq!(
            module_failure.package_input_error(),
            Some(&PackageInputError::ModuleLimitExceeded {
                actual: 2,
                limit: 1,
            })
        );

        let token_limits = FrontendLimits::new(100, 32, 10)
            .unwrap()
            .with_package_limits(2, 1)
            .unwrap();
        let token_failure =
            compile_package(package(), &options_with_frontend(token_limits)).unwrap_err();
        assert!(
            token_failure
                .diagnostics()
                .unwrap()
                .contains_code(PACKAGE_TOKEN_LIMIT)
        );

        let diagnostics_package = PackageInput::new(
            module_key("root"),
            vec![
                module("api", "?", []),
                module("root", "?", [("api", "api")]),
            ],
        );
        let diagnostic_limits = FrontendLimits::new(100, 32, 1)
            .unwrap()
            .with_package_limits(2, 100)
            .unwrap();
        let diagnostics_failure = compile_package(
            diagnostics_package,
            &options_with_frontend(diagnostic_limits),
        )
        .unwrap_err();
        let codes = diagnostics_failure
            .diagnostics()
            .unwrap()
            .findings()
            .iter()
            .map(crate::diagnostic::Diagnostic::code)
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            ["frontend.lex.unknown-character", PACKAGE_SYNTAX_TRUNCATED]
        );
    }

    #[test]
    fn dependency_and_call_cycles_lower_through_recursive_activation_frames() {
        let package = PackageInput::new(
            module_key("a"),
            vec![
                module(
                    "a",
                    r#"const b = import("b");
export fn from_a(flag: Bool) -> Bool {
    if (flag) { return b.from_b(false); }
    return flag;
}"#,
                    [("b", "b")],
                ),
                module(
                    "b",
                    r#"const a = import("a");
pub fn from_b(flag: Bool) -> Bool {
    if (flag) { return a.from_a(false); }
    return flag;
}"#,
                    [("a", "a")],
                ),
            ],
        );

        let output = compile_package(package, &options()).unwrap();
        assert_eq!(output.checked_frontend().module_count(), 2);
        assert_eq!(output.checked_frontend().function_count(), 2);
        assert_eq!(
            output.lowering().map().execution_contract().activation(),
            crate::lower::minecraft::ActivationContract::SynchronousRecursiveStack
        );
        let functions = output
            .emission()
            .pack()
            .files()
            .iter()
            .filter(|file| file.path().as_str().ends_with(".mcfunction"))
            .filter_map(|file| std::str::from_utf8(file.bytes()).ok())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(functions.contains("\"frames\" append value {}"));
    }

    #[test]
    fn package_permutation_preserves_syntax_diagnostics_and_provenance() {
        let build = |reverse: bool| {
            let mut modules = vec![
                module("api", "?", []),
                module("root", "fn broken(", [("api", "api")]),
            ];
            if reverse {
                modules.reverse();
            }
            let failure =
                compile_package(PackageInput::new(module_key("root"), modules), &options())
                    .unwrap_err();
            let CompilationFailure::Syntax {
                sources,
                diagnostics,
            } = failure
            else {
                panic!("malformed package must fail during syntax production")
            };
            (sources, diagnostics)
        };

        let (forward_sources, forward) = build(false);
        let (reverse_sources, reverse) = build(true);
        assert_eq!(forward, reverse);
        assert_eq!(
            crate::diagnostic::render_diagnostics(&forward, &forward_sources),
            crate::diagnostic::render_diagnostics(&reverse, &reverse_sources)
        );
    }

    #[test]
    fn invalid_namespace_bindings_are_poisoned_without_member_cascades() {
        let unknown_import = PackageInput::new(
            module_key("root"),
            vec![module(
                "root",
                r#"const api = import("missing");
fn api() {}
fn run() { api.nope(); }"#,
                [],
            )],
        );
        let failure = compile_package(unknown_import, &options()).unwrap_err();
        let codes = failure
            .diagnostics()
            .unwrap()
            .findings()
            .iter()
            .map(crate::diagnostic::Diagnostic::code)
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                "frontend.check.unknown-import",
                "frontend.check.top-level-name-conflict",
            ]
        );

        let unknown_member = PackageInput::new(
            module_key("root"),
            vec![
                module("api", "pub fn present() {}", []),
                module(
                    "root",
                    r#"const api = import("api");
fn run() { api.absent(); }"#,
                    [("api", "api")],
                ),
            ],
        );
        let failure = compile_package(unknown_member, &options()).unwrap_err();
        let diagnostics = failure.diagnostics().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics.contains_code("frontend.check.unknown-member"));
    }

    #[test]
    fn target_analysis_failure_is_nonfatal_and_retains_emission() {
        let (_, core, lowering, _, emission) = options().into_parts();
        let lowering =
            lowering.with_command_limit_assumptions(CommandLimitAssumptions::new(10, 10).unwrap());
        let smaller_assumptions = CommandLimitAssumptions::new(0, 0).unwrap();
        let mismatched = CompilationOptions::new(
            FrontendLimits::DEFAULT,
            core,
            lowering,
            TargetExecutionAnalysisLimits::new(
                AnalysisArithmeticCaps::minimum_for(smaller_assumptions),
                100_000,
                100_000,
            ),
            emission,
        );

        let output =
            compile_source(SourceInput::new("valid.mdl", "fn valid() {}"), &mismatched).unwrap();
        let analysis_failure = output.target_analysis().as_ref().unwrap_err();
        assert_eq!(
            analysis_failure.phase(),
            TargetExecutionAnalysisPhase::Configuration
        );
        assert_eq!(output.lowering().map().len(), 1);
        assert!(!output.emission().pack().files().is_empty());
    }

    #[test]
    fn syntax_and_semantic_failures_retain_renderable_sources() {
        let syntax =
            compile_source(SourceInput::new("bad.mdl", "fn broken("), &options()).unwrap_err();
        assert!(syntax.sources().is_some());
        assert!(syntax.checked_frontend().is_none());
        assert!(syntax.diagnostics().is_some());
        assert!(matches!(syntax, CompilationFailure::Syntax { .. }));

        let semantic = compile_source(
            SourceInput::new("bad.mdl", "fn missing() -> Int32 {}"),
            &options(),
        )
        .unwrap_err();
        assert!(semantic.sources().is_some());
        assert!(semantic.checked_frontend().is_none());
        assert!(
            semantic
                .diagnostics()
                .unwrap()
                .contains_code("frontend.check.missing-return")
        );
        let CompilationFailure::Semantic {
            sources,
            diagnostics,
        } = semantic
        else {
            panic!("missing return must be a semantic failure")
        };
        assert_eq!(sources.files().len(), 1);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn source_derived_join_expansion_limit_fails_at_core_generation() {
        const ARM_COUNT: usize = 128;
        const TOKEN_AND_JOIN_OPERAND_LIMIT: usize = 4_096;

        let mut source = String::from("fn wide(flag: Bool) -> Int32 {\n");
        for local in 0..ARM_COUNT {
            writeln!(source, "var local_{local}: Int32 = 0;").unwrap();
        }
        for arm in 0..ARM_COUNT {
            let prefix = if arm == 0 { "if" } else { "else if" };
            writeln!(source, "{prefix} (flag) {{ local_{arm} = {arm}; }}").unwrap();
        }
        source.push_str("else {}\nreturn local_0;\n}");

        let limits = FrontendLimits::new(TOKEN_AND_JOIN_OPERAND_LIMIT, 256, 100).unwrap();
        let failure = compile_source(
            SourceInput::new("wide.mdl", source),
            &options_with_frontend(limits),
        )
        .unwrap_err();
        assert!(matches!(
            failure.core_generation_failure(),
            Some(CoreGenerationFailure::ResourceLimit {
                source_function,
                resource: CoreGenerationResource::JoinEdgeOperands,
                limit: TOKEN_AND_JOIN_OPERAND_LIMIT,
            }) if source_function.index() == 0
        ));
        assert!(matches!(failure, CompilationFailure::CoreGeneration { .. }));
    }

    #[test]
    fn private_producer_errors_convert_without_flattening_structured_failures() {
        assert_eq!(
            FrontendInfrastructureFailure::from(LexerError::Source(SourceError::EntityLimit)),
            FrontendInfrastructureFailure::Source {
                phase: FrontendInfrastructurePhase::Lexing,
                error: SourceError::EntityLimit,
            }
        );
        assert_eq!(
            FrontendInfrastructureFailure::from(ParserError::Source(SourceError::EntityLimit)),
            FrontendInfrastructureFailure::Source {
                phase: FrontendInfrastructurePhase::Parsing,
                error: SourceError::EntityLimit,
            }
        );
        assert_eq!(
            FrontendInfrastructureFailure::from(CheckError::IdentitySpaceExhausted(
                CheckedEntityKind::Local,
            )),
            FrontendInfrastructureFailure::IdentitySpaceExhausted {
                entity: FrontendEntityKind::Local,
            }
        );
    }

    #[test]
    fn concrete_frontend_and_core_generation_failure_accessors_are_typed() {
        let source = CompilationFailure::SourceInput(SourceError::EntityLimit);
        assert_eq!(source.source_input_error(), Some(&SourceError::EntityLimit));
        assert!(source.frontend_infrastructure_failure().is_none());

        let infrastructure = FrontendInfrastructureFailure::Source {
            phase: FrontendInfrastructurePhase::Lexing,
            error: SourceError::EntityLimit,
        };
        let failure = CompilationFailure::FrontendInfrastructure {
            sources: Box::new(SourceContext::new()),
            failure: infrastructure.clone(),
        };
        assert_eq!(
            failure.frontend_infrastructure_failure(),
            Some(&infrastructure)
        );
        assert!(failure.source_input_error().is_none());

        let output =
            compile_source(SourceInput::new("valid.mdl", "fn valid() {}"), &options()).unwrap();
        let source_function = output.checked_frontend().function_ids().next().unwrap();
        let (sources, checked_frontend, _, _, _, _, _) = output.into_parts();
        let core_failure =
            CoreGenerationFailure::Invariant(CoreGenerationInvariant::EmptyConditional {
                source_function,
            });
        let failure = CompilationFailure::CoreGeneration {
            sources: Box::new(sources),
            checked_frontend: Box::new(checked_frontend),
            failure: Box::new(core_failure.clone()),
        };
        assert_eq!(failure.core_generation_failure(), Some(&core_failure));
        assert!(failure.checked_frontend().is_some());
        assert!(failure.source_to_core().is_none());
    }

    #[test]
    fn optimizer_and_lowering_failure_accessors_preserve_prior_products() {
        let output =
            compile_source(SourceInput::new("valid.mdl", "fn valid() {}"), &options()).unwrap();
        let (sources, checked_frontend, source_to_core, _, _, _, _) = output.into_parts();
        let mut invalid_core = CoreProgram::new();
        invalid_core
            .declare_function(Some("undefined"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let optimizer_failure = optimize_core(
            invalid_core,
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        )
        .unwrap_err();
        let failure = CompilationFailure::CoreOptimization {
            sources: Box::new(sources),
            checked_frontend: Box::new(checked_frontend),
            source_to_core,
            failure: Box::new(optimizer_failure),
        };
        assert!(failure.core_optimization_failure().is_some());
        assert!(failure.source_to_core().is_some());
        assert!(failure.core_optimization().is_none());

        let lowering_failure = compile_source(
            SourceInput::new(
                "unbounded.mdl",
                "export fn run() { run.as(mc.entities(ArmorStand)) {} }",
            ),
            &options(),
        )
        .unwrap_err();
        assert!(lowering_failure.minecraft_lowering_failure().is_some());
        assert!(lowering_failure.checked_frontend().is_some());
        assert!(lowering_failure.source_to_core().is_some());
        assert!(lowering_failure.core_optimization().is_some());
        assert!(lowering_failure.lowering().is_none());
    }

    #[test]
    fn direct_scalar_recursion_uses_activation_frames() {
        let output = compile_source(
            SourceInput::new(
                "recursive.mdl",
                "export fn again(flag: Bool) -> Bool { if (flag) { return again(false); } return flag; }",
            ),
            &options(),
        )
        .unwrap();

        assert_eq!(
            output.lowering().map().execution_contract().activation(),
            crate::lower::minecraft::ActivationContract::SynchronousRecursiveStack
        );
        let data_commands = output
            .lowering()
            .program()
            .functions()
            .flat_map(|(_, function)| function.body().commands())
            .filter(|(_, command)| matches!(command.kind(), CommandKind::Data(_)))
            .count();
        assert!(
            data_commands >= 3,
            "push, pop, and load reset must be explicit"
        );
    }
}
