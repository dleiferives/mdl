//! Bounded typed source-language frontend and source-to-datapack facade.

use std::error::Error;
use std::fmt;

mod ast;
mod behavior;
mod check;
mod compile;
mod context;
mod entity_schema;
mod hir;
mod input;
mod lexer;
mod lower;
mod parser;
mod target_contract;
mod token;

pub use crate::ir::semantic::{
    AmbientContextRequirements, CommandOutcomeType, ContextRequirement, EntityCapabilities,
    EntityCapability, EntityKind, EntityQueryType, EntityRefType, EntityTag, EntityTagError,
    ExecutorType, ForkBound, FunctionBehavior, MessageLiteral, MessageLiteralError,
    ObservableEffect, QueryCardinality, QueryCardinalityError, SemanticType, StaticEntityQuery,
    TransitiveWork, WorldEffect,
};
pub use compile::{
    CompilationFailure, CompilationOptions, CompilationOutput, FrontendEntityKind,
    FrontendInfrastructureFailure, FrontendInfrastructurePhase, compile_package, compile_source,
};
pub use hir::{
    CheckedFrontendOutput, FunctionResult, FunctionVisibility, SourceExternalOpId,
    SourceFunctionId, SourceModuleId, SourceRunId, ValueType,
};
pub use input::{
    ImportName, ImportNameError, ModuleDependency, ModuleInput, ModuleKey, ModuleKeyError,
    PackageInput, PackageInputError, SourceInput,
};
pub use lower::{
    CoreGenerationFailure, CoreGenerationInvariant, CoreGenerationResource,
    SourceSemanticCoreOccurrence, SourceToCoreMap,
};

/// Source-derived resource limits shared by lexing, parsing, checking, and Core generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrontendLimits {
    tokens: usize,
    package_tokens: usize,
    modules: usize,
    syntax_depth: usize,
    diagnostics: usize,
}

impl FrontendLimits {
    /// Hard cap protecting the recursive Phase 1 parser from host-stack exhaustion.
    pub const MAX_SYNTAX_DEPTH: usize = 256;

    /// Production limits for one source file.
    pub const DEFAULT: Self = Self {
        tokens: 1_000_000,
        package_tokens: 1_000_000,
        modules: 4_096,
        syntax_depth: 256,
        diagnostics: 100,
    };

    /// Creates validated limits for one source file.
    ///
    /// A zero diagnostic limit is valid and retains only the final truncation
    /// finding if an ordinary diagnostic would be produced.
    ///
    /// # Errors
    ///
    /// Returns [`FrontendLimitsError::TokenLimitExcludesEndOfFile`] when the
    /// token limit is zero, [`FrontendLimitsError::SyntaxDepthMustBePositive`] when
    /// syntax depth is zero, or
    /// [`FrontendLimitsError::SyntaxDepthExceedsHardMaximum`] above the reviewed
    /// recursive-parser cap.
    pub const fn new(
        max_tokens: usize,
        max_syntax_depth: usize,
        max_diagnostics: usize,
    ) -> Result<Self, FrontendLimitsError> {
        if max_tokens == 0 {
            return Err(FrontendLimitsError::TokenLimitExcludesEndOfFile);
        }
        if max_syntax_depth == 0 {
            return Err(FrontendLimitsError::SyntaxDepthMustBePositive);
        }
        if max_syntax_depth > Self::MAX_SYNTAX_DEPTH {
            return Err(FrontendLimitsError::SyntaxDepthExceedsHardMaximum {
                requested: max_syntax_depth,
                maximum: Self::MAX_SYNTAX_DEPTH,
            });
        }
        Ok(Self {
            tokens: max_tokens,
            package_tokens: max_tokens,
            modules: Self::DEFAULT.modules,
            syntax_depth: max_syntax_depth,
            diagnostics: max_diagnostics,
        })
    }

    /// Returns the maximum token count, including EOF.
    ///
    /// Core generation also uses this value as its conservative whole-compilation
    /// cap on SSA join-edge value operands.
    #[must_use]
    pub const fn max_tokens(self) -> usize {
        self.tokens
    }

    /// Returns the maximum total token count across the canonical package,
    /// including one EOF token for every module admitted to lexing.
    #[must_use]
    pub const fn max_package_tokens(self) -> usize {
        self.package_tokens
    }

    /// Returns the maximum number of modules in one package compilation.
    #[must_use]
    pub const fn max_modules(self) -> usize {
        self.modules
    }

    /// Selects whole-package module and token budgets.
    ///
    /// # Errors
    ///
    /// Returns an error when either limit is zero.
    pub const fn with_package_limits(
        mut self,
        max_modules: usize,
        max_package_tokens: usize,
    ) -> Result<Self, FrontendLimitsError> {
        if max_modules == 0 {
            return Err(FrontendLimitsError::ModuleLimitMustBePositive);
        }
        if max_package_tokens == 0 {
            return Err(FrontendLimitsError::PackageTokenLimitExcludesEndOfFile);
        }
        self.modules = max_modules;
        self.package_tokens = max_package_tokens;
        Ok(self)
    }

    pub(super) const fn with_module_token_budget(mut self, max_tokens: usize) -> Self {
        self.tokens = max_tokens;
        self
    }

    /// Returns the maximum number of nested syntax constructs.
    #[must_use]
    pub const fn max_syntax_depth(self) -> usize {
        self.syntax_depth
    }

    /// Returns the maximum number of ordinary frontend diagnostics.
    ///
    /// A final truncation finding may be emitted in addition to this count.
    #[must_use]
    pub const fn max_diagnostics(self) -> usize {
        self.diagnostics
    }
}

impl Default for FrontendLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Invalid frontend resource-limit configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrontendLimitsError {
    /// Token capacity must always include the required EOF token.
    TokenLimitExcludesEndOfFile,
    /// A package must permit at least one module.
    ModuleLimitMustBePositive,
    /// A package token budget must reserve at least one EOF token.
    PackageTokenLimitExcludesEndOfFile,
    /// Syntax parsing must permit at least one construct level.
    SyntaxDepthMustBePositive,
    /// Recursive syntax depth cannot exceed the reviewed host-stack cap.
    SyntaxDepthExceedsHardMaximum {
        /// Requested nesting limit.
        requested: usize,
        /// Hard reviewed maximum.
        maximum: usize,
    },
}

impl fmt::Display for FrontendLimitsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenLimitExcludesEndOfFile => {
                formatter.write_str("frontend token limit must reserve one token for EOF")
            }
            Self::ModuleLimitMustBePositive => {
                formatter.write_str("frontend package module limit must be positive")
            }
            Self::PackageTokenLimitExcludesEndOfFile => {
                formatter.write_str("frontend package token limit must reserve one token for EOF")
            }
            Self::SyntaxDepthMustBePositive => {
                formatter.write_str("frontend syntax-depth limit must be positive")
            }
            Self::SyntaxDepthExceedsHardMaximum { requested, maximum } => write!(
                formatter,
                "frontend syntax-depth limit {requested} exceeds hard maximum {maximum}"
            ),
        }
    }
}

impl Error for FrontendLimitsError {}

#[cfg(test)]
mod tests {
    use super::{FrontendLimits, FrontendLimitsError};

    #[test]
    fn defaults_match_the_frozen_frontend_contract() {
        let limits = FrontendLimits::DEFAULT;
        assert_eq!(limits.max_tokens(), 1_000_000);
        assert_eq!(limits.max_package_tokens(), 1_000_000);
        assert_eq!(limits.max_modules(), 4_096);
        assert_eq!(limits.max_syntax_depth(), 256);
        assert_eq!(limits.max_diagnostics(), 100);
        assert_eq!(FrontendLimits::default(), limits);
    }

    #[test]
    fn custom_limits_validate_required_nonzero_capacities() {
        assert_eq!(
            FrontendLimits::new(0, 1, 0),
            Err(FrontendLimitsError::TokenLimitExcludesEndOfFile)
        );
        assert_eq!(
            FrontendLimits::new(1, 0, 0),
            Err(FrontendLimitsError::SyntaxDepthMustBePositive)
        );
        assert_eq!(
            FrontendLimits::new(1, FrontendLimits::MAX_SYNTAX_DEPTH + 1, 0),
            Err(FrontendLimitsError::SyntaxDepthExceedsHardMaximum {
                requested: 257,
                maximum: 256,
            })
        );

        let limits = FrontendLimits::new(1, 1, 0).unwrap();
        assert_eq!(limits.max_tokens(), 1);
        assert_eq!(limits.max_syntax_depth(), 1);
        assert_eq!(limits.max_diagnostics(), 0);
        assert_eq!(limits.max_package_tokens(), 1);
        assert_eq!(limits.max_modules(), 4_096);

        assert_eq!(
            limits.with_package_limits(0, 1),
            Err(FrontendLimitsError::ModuleLimitMustBePositive)
        );
        assert_eq!(
            limits.with_package_limits(1, 0),
            Err(FrontendLimitsError::PackageTokenLimitExcludesEndOfFile)
        );
        let package = limits.with_package_limits(2, 3).unwrap();
        assert_eq!(package.max_modules(), 2);
        assert_eq!(package.max_package_tokens(), 3);
    }
}
