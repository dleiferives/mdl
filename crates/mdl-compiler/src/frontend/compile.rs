//! Owned source-to-datapack compilation facade.

use std::error::Error;
use std::fmt;

use super::FrontendLimits;
use super::check::{CheckError, CheckedEntityKind, check};
use super::hir::{CheckedFrontendOutput, SourceFunctionId};
use super::lexer::{LexerError, lex};
use super::lower::{CoreGenerationFailure, SourceToCoreMap, lower_hir};
use super::parser::{ParserError, parse};
use crate::analysis::minecraft::{
    TargetExecutionAnalysisFailure, TargetExecutionAnalysisLimits, TargetExecutionCostReport,
};
use crate::datapack::{EmissionOptions, EmissionOutput, emit_datapack};
use crate::diagnostic::Diagnostics;
use crate::lower::minecraft::{
    LoweredFunction, LoweringFailure, LoweringOptions, LoweringOutput, lower_to_minecraft,
};
use crate::opt::core::{
    CoreOptimizationFailure, CoreOptimizationOptions, CoreOptimizationOutput, optimize_core,
};
use crate::source::{OriginError, SourceContext, SourceError};

/// One owned in-memory source compilation unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceInput {
    name: Box<str>,
    text: Box<str>,
}

impl SourceInput {
    /// Creates one source input with a diagnostic name and UTF-8 text.
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, text: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }

    /// Returns the diagnostic source name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the complete source text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Consumes the input into its diagnostic name and source text.
    #[must_use]
    pub fn into_parts(self) -> (Box<str>, Box<str>) {
        (self.name, self.text)
    }
}

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
    /// Source functions in declaration order.
    Function,
    /// Function-owned parameters and local bindings.
    Local,
}

impl fmt::Display for FrontendEntityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Function => formatter.write_str("function"),
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
            CheckError::IdentitySpaceExhausted(CheckedEntityKind::Function) => {
                Self::IdentitySpaceExhausted {
                    entity: FrontendEntityKind::Function,
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
        checked_frontend: CheckedFrontendOutput,
        /// Concrete Core-generation failure.
        failure: Box<CoreGenerationFailure>,
    },
    /// The consuming Core optimization boundary rejected generated Core.
    CoreOptimization {
        /// Retained source text and provenance.
        sources: Box<SourceContext>,
        /// Complete checked frontend product.
        checked_frontend: CheckedFrontendOutput,
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
        checked_frontend: CheckedFrontendOutput,
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
        checked_frontend: CheckedFrontendOutput,
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
    /// Returns the source-ingestion failure, when ingestion was the failed stage.
    #[must_use]
    pub const fn source_input_error(&self) -> Option<&SourceError> {
        match self {
            Self::SourceInput(error) => Some(error),
            Self::FrontendInfrastructure { .. }
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
            Self::SourceInput(_)
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
            Self::SourceInput(_)
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
            Self::SourceInput(_)
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
            Self::SourceInput(_)
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
            Self::SourceInput(_) => None,
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
            Self::SourceInput(_)
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
            Self::SourceInput(_)
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
            Self::SourceInput(_)
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
            Self::SourceInput(_)
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
            Self::SourceInput(_)
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
    let (sources, checked_frontend) = compile_frontend(input, options.frontend())?;

    let core_generation =
        match lower_hir(&checked_frontend, &sources, options.frontend().max_tokens()) {
            Ok(output) => output,
            Err(failure) => {
                return Err(CompilationFailure::CoreGeneration {
                    sources: Box::new(sources),
                    checked_frontend,
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
                checked_frontend,
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
                checked_frontend,
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
                checked_frontend,
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

fn compile_frontend(
    input: SourceInput,
    limits: FrontendLimits,
) -> Result<(SourceContext, CheckedFrontendOutput), CompilationFailure> {
    let (name, text) = input.into_parts();
    let mut sources = SourceContext::new();
    let file = sources
        .add_file(name, text)
        .map_err(CompilationFailure::SourceInput)?;

    let lexed = match lex(&mut sources, file, limits) {
        Ok(output) => output,
        Err(error) => {
            return Err(CompilationFailure::FrontendInfrastructure {
                sources: Box::new(sources),
                failure: error.into(),
            });
        }
    };
    let (tokens, diagnostics) = lexed.into_parts();
    if let Some(diagnostics) = diagnostics {
        return Err(CompilationFailure::Syntax {
            sources: Box::new(sources),
            diagnostics,
        });
    }

    let parsed = match parse(&mut sources, file, &tokens, limits) {
        Ok(output) => output,
        Err(error) => {
            return Err(CompilationFailure::FrontendInfrastructure {
                sources: Box::new(sources),
                failure: error.into(),
            });
        }
    };
    let (module, diagnostics) = parsed.into_parts();
    drop(tokens);
    if let Some(diagnostics) = diagnostics {
        return Err(CompilationFailure::Syntax {
            sources: Box::new(sources),
            diagnostics,
        });
    }

    let checked = match check(&mut sources, &module, limits) {
        Ok(output) => output,
        Err(error) => {
            return Err(CompilationFailure::FrontendInfrastructure {
                sources: Box::new(sources),
                failure: error.into(),
            });
        }
    };
    drop(module);
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

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{
        CompilationFailure, CompilationOptions, FrontendEntityKind, FrontendInfrastructureFailure,
        FrontendInfrastructurePhase, SourceInput, compile_source,
    };
    use crate::analysis::minecraft::{
        AnalysisArithmeticCaps, CommandLimitAssumptions, TargetExecutionAnalysisLimits,
        TargetExecutionAnalysisPhase,
    };
    use crate::datapack::EmissionOptions;
    use crate::frontend::check::{CheckError, CheckedEntityKind};
    use crate::frontend::lexer::LexerError;
    use crate::frontend::parser::ParserError;
    use crate::frontend::{
        CoreGenerationFailure, CoreGenerationInvariant, CoreGenerationResource, FrontendLimits,
    };
    use crate::ir::core::CoreProgram;
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
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
            checked_frontend,
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
            checked_frontend,
            source_to_core,
            failure: Box::new(optimizer_failure),
        };
        assert!(failure.core_optimization_failure().is_some());
        assert!(failure.source_to_core().is_some());
        assert!(failure.core_optimization().is_none());

        let recursive = compile_source(
            SourceInput::new(
                "recursive.mdl",
                "fn again(flag: Bool) -> Bool { if (flag) { return again(false); } return flag; }",
            ),
            &options(),
        )
        .unwrap_err();
        assert!(recursive.minecraft_lowering_failure().is_some());
        assert!(recursive.checked_frontend().is_some());
        assert!(recursive.source_to_core().is_some());
        assert!(recursive.core_optimization().is_some());
        assert!(recursive.lowering().is_none());
    }
}
