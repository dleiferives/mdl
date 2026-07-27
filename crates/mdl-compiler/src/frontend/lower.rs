//! Construction of verified Core SSA from checked frontend HIR.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

#[cfg(test)]
use std::cell::Cell;

use super::hir::{
    CheckedFrontendOutput, FunctionResult, FunctionVisibility, HirBlock, HirCall, HirComparisonOp,
    HirCriterion, HirDestructureTargetRole, HirEntityNbtReceiver, HirEntityPathSegment,
    HirEntityQuery, HirEntityQueryStep, HirExpression, HirExpressionKind, HirExternalOp,
    HirExternalSemantic, HirFunction, HirIf, HirListI32Op, HirMinecraftOperationAttributes, HirRun,
    HirRunModifier, HirStatement, HirStatementKind, HirStringOp, HirSwitchExpression,
    HirSwitchLabel, HirSwitchPatternKind, HirSwitchStatement, HirWhile, HirWrappingArithmeticOp,
    LocalId, SourceExternalOpId, SourceFunctionId, SourceRunId, SourceStructId, ValueType,
};
use crate::diagnostic::Diagnostics;
use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, BlockTarget, BuildError, CoreAmbientAnalysis, CoreAmbientAnalysisError,
    CoreFunctionLinkage, CoreOp, CoreProgram, CoreType, Criterion, EntityNbtPathSegment,
    EntityNbtReceiver, EntityNbtWriteDecl, EntityQueryDecl, EntityQueryStep, ExternalOpId,
    ExternalSemanticBinding, FunctionBody, FunctionBuilder, FunctionId, I32ClosedRange,
    I32Predicate, InstId, ItemMatch, MinecraftOperationAttributes, MinecraftOperationOrigins,
    Operand, ProgramError, RunModifierInstance, TargetFragment, Terminator, TerminatorKind,
    ValueId, verify_program,
};
use crate::source::{OriginId, SourceContext};

/// Deterministic correlation between source functions and generated Core functions.
///
/// Function identities are deliberately retained as a typed map rather than relying
/// on the current implementation detail that both identity spaces are dense and
/// allocated in the same order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceToCoreMap {
    functions: Box<[(SourceFunctionId, FunctionId)]>,
    external_operations: Box<[(SourceExternalOpId, ExternalOpId)]>,
    semantic_operations: Box<[Option<SourceSemanticCoreOccurrence>]>,
    run_scopes: Box<[Option<crate::ir::core::RunScopeId>]>,
}

/// Exact generated Core identity of one typed source-semantic operation occurrence.
///
/// Unsafe source externals and structured run-scope invocations deliberately do not
/// inhabit this map. `external` identifies the normalized Core declaration, while
/// `function` and `instruction` identify its one attached invocation occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceSemanticCoreOccurrence {
    external: ExternalOpId,
    function: FunctionId,
    instruction: InstId,
}

impl SourceSemanticCoreOccurrence {
    const fn new(external: ExternalOpId, function: FunctionId, instruction: InstId) -> Self {
        Self {
            external,
            function,
            instruction,
        }
    }

    /// Returns the normalized Core external declaration used by this occurrence.
    #[must_use]
    pub const fn external(self) -> ExternalOpId {
        self.external
    }

    /// Returns the Core function containing this occurrence.
    #[must_use]
    pub const fn function(self) -> FunctionId {
        self.function
    }

    /// Returns the function-local Core instruction identity of this occurrence.
    #[must_use]
    pub const fn instruction(self) -> InstId {
        self.instruction
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExternalToCoreMap {
    operations: Box<[(SourceExternalOpId, ExternalOpId)]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RunCoreLink {
    function: FunctionId,
    operation: Option<ExternalOpId>,
    scope: Option<crate::ir::core::RunScopeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RunToCoreMap {
    links: Box<[RunCoreLink]>,
}

impl RunToCoreMap {
    fn link(&self, source: SourceRunId) -> Option<RunCoreLink> {
        self.links.get(source.as_usize()?).copied()
    }
}

impl ExternalToCoreMap {
    fn new(operations: Vec<(SourceExternalOpId, ExternalOpId)>) -> Self {
        Self {
            operations: operations.into_boxed_slice(),
        }
    }

    fn operation(&self, source: SourceExternalOpId) -> Option<ExternalOpId> {
        let (mapped_source, operation) = self.operations.get(source.as_usize()?).copied()?;
        (mapped_source == source).then_some(operation)
    }
}

impl SourceToCoreMap {
    fn new(functions: Vec<(SourceFunctionId, FunctionId)>) -> Self {
        Self {
            functions: functions.into_boxed_slice(),
            external_operations: Box::new([]),
            semantic_operations: Box::new([]),
            run_scopes: Box::new([]),
        }
    }

    fn install_external_correlations(
        &mut self,
        externals: Vec<(SourceExternalOpId, ExternalOpId)>,
        operations: Vec<Option<SourceSemanticCoreOccurrence>>,
    ) {
        self.external_operations = externals.into_boxed_slice();
        self.semantic_operations = operations.into_boxed_slice();
    }

    fn install_run_correlations(&mut self, runs: &RunToCoreMap) {
        self.run_scopes = runs.links.iter().map(|link| link.scope).collect();
    }

    /// Returns the generated Core run-scope identity for a source run with modifiers.
    #[must_use]
    pub fn run_scope(&self, source: SourceRunId) -> Option<crate::ir::core::RunScopeId> {
        self.run_scopes.get(source.as_usize()?).copied().flatten()
    }

    /// Returns the generated Core identity for one source function.
    #[must_use]
    pub fn function(&self, source: SourceFunctionId) -> Option<FunctionId> {
        let index = source.as_usize()?;
        let (mapped_source, function) = self.functions.get(index).copied()?;
        (mapped_source == source).then_some(function)
    }

    /// Iterates all correlations in deterministic source declaration order.
    #[must_use]
    pub fn functions(&self) -> impl ExactSizeIterator<Item = (SourceFunctionId, FunctionId)> + '_ {
        self.functions.iter().copied()
    }

    /// Returns the generated Core declaration for one general source external.
    ///
    /// This includes unsafe source commands. Structured run scopes remain in their
    /// distinct [`SourceRunId`] identity domain and therefore never appear here.
    #[must_use]
    pub fn external_operation(&self, source: SourceExternalOpId) -> Option<ExternalOpId> {
        let (mapped_source, external) =
            self.external_operations.get(source.as_usize()?).copied()?;
        (mapped_source == source).then_some(external)
    }

    /// Iterates general source-external declaration correlations in canonical source order.
    #[must_use]
    pub fn external_operations(
        &self,
    ) -> impl ExactSizeIterator<Item = (SourceExternalOpId, ExternalOpId)> + '_ {
        self.external_operations.iter().copied()
    }

    /// Returns the exact generated Core occurrence for one typed source-semantic
    /// external. Unsafe externals, unreachable source operations, and identities
    /// outside this compilation return `None`.
    #[must_use]
    pub fn semantic_operation(
        &self,
        source: SourceExternalOpId,
    ) -> Option<SourceSemanticCoreOccurrence> {
        self.semantic_operations
            .get(source.as_usize()?)
            .copied()
            .flatten()
    }

    /// Iterates all generated typed source-semantic occurrences in canonical source
    /// external-operation order.
    pub fn semantic_operations(
        &self,
    ) -> impl Iterator<Item = (SourceExternalOpId, SourceSemanticCoreOccurrence)> + '_ {
        self.semantic_operations
            .iter()
            .enumerate()
            .filter_map(|(index, occurrence)| {
                Some((SourceExternalOpId::from_index(index)?, (*occurrence)?))
            })
    }

    /// Returns the number of correlated source functions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Returns whether the map contains no functions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}

/// Temporary owned product of checked-HIR-to-Core generation.
///
/// The compilation facade separates these parts before consuming the program in
/// the Core optimization pipeline, retaining only the correlation map beside the
/// optimized program.
#[derive(Debug)]
pub(super) struct CoreGenerationOutput {
    program: CoreProgram,
    source_to_core: SourceToCoreMap,
}

impl CoreGenerationOutput {
    /// Returns the complete verified Core program.
    #[cfg(test)]
    #[must_use]
    pub const fn program(&self) -> &CoreProgram {
        &self.program
    }

    /// Returns the source-to-Core correlation map.
    #[cfg(test)]
    #[must_use]
    pub const fn source_to_core(&self) -> &SourceToCoreMap {
        &self.source_to_core
    }

    /// Consumes this temporary product into its independently owned parts.
    #[must_use]
    pub(super) fn into_parts(self) -> (CoreProgram, SourceToCoreMap) {
        (self.program, self.source_to_core)
    }
}

/// An impossible condition detected while translating already-verified HIR.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CoreGenerationInvariant {
    /// A source function was absent from the predeclared correlation map.
    MissingFunctionMapping {
        /// Missing source function.
        source_function: SourceFunctionId,
    },
    /// A source external operation was absent from the predeclared Core map.
    MissingExternalMapping {
        /// Missing source external operation.
        source_external: SourceExternalOpId,
    },
    /// A typed source-semantic operation did not correlate to exactly its generated
    /// attached Core invocation.
    InvalidSemanticOperationCorrelation {
        /// Source external operation with the invalid correlation.
        source_external: SourceExternalOpId,
    },
    /// The typed semantic-correlation inventory was not dense over source externals.
    SemanticOperationCorrelationCount {
        /// Number of source external-operation slots.
        expected: usize,
        /// Number of retained correlation slots.
        actual: usize,
    },
    /// A structured run scope was absent from the predeclared Core map.
    MissingRunMapping {
        /// Missing source run scope.
        source_run: SourceRunId,
    },
    /// A lowered run invocation exposed a non-`Void` result unexpectedly.
    InvalidRunScope {
        /// Invalid source run scope.
        source_run: SourceRunId,
    },
    /// HIR run-scope count disagreed with its independently traversed inventory.
    RunInventoryCount {
        /// Count recorded by checked HIR.
        expected: usize,
        /// Count found by deterministic traversal.
        actual: usize,
    },
    /// Verified HIR retained an invalid unsafe command fragment.
    InvalidUnsafeCommand {
        /// Invalid source external operation.
        source_external: SourceExternalOpId,
    },
    /// A checked function name could not be recovered from its provenance.
    MissingFunctionName {
        /// Source function whose name was unavailable.
        source_function: SourceFunctionId,
    },
    /// A checked parameter had no corresponding Core entry parameter.
    MissingEntryParameter {
        /// Owning source function.
        source_function: SourceFunctionId,
        /// Zero-based source parameter index.
        parameter: usize,
    },
    /// HIR referred to a binding outside its owning function's inventory.
    MissingBinding {
        /// Owning source function.
        source_function: SourceFunctionId,
        /// Owner-local HIR binding index.
        local: u32,
    },
    /// HIR read a binding without a value on the current path.
    UnassignedBinding {
        /// Owning source function.
        source_function: SourceFunctionId,
        /// Owner-local HIR binding index.
        local: u32,
    },
    /// A checked conditional unexpectedly contained no condition arm.
    EmptyConditional {
        /// Owning source function.
        source_function: SourceFunctionId,
    },
    /// An SSA continuation merge was requested without a predecessor.
    EmptyContinuationMerge {
        /// Owning source function.
        source_function: SourceFunctionId,
    },
    /// Loop control reached lowering without an enclosing structured loop.
    LoopControlOutsideLoop {
        /// Owning source function.
        source_function: SourceFunctionId,
    },
    /// A nominal aggregate could not be expanded into a finite scalar leaf sequence.
    InvalidAggregateType {
        /// Invalid aggregate identity.
        struct_: crate::frontend::hir::SourceStructId,
    },
    /// Checked HIR projected a field from a non-aggregate value.
    InvalidAggregateProjection {
        /// Owning source function.
        source_function: SourceFunctionId,
    },
    /// Checked HIR applied an ordered comparison to Boolean operands.
    InvalidBooleanComparison {
        /// Owning source function.
        source_function: SourceFunctionId,
    },
    /// A generated call exposed a result count inconsistent with checked HIR.
    CallResultCount {
        /// Owning source function containing the call.
        source_function: SourceFunctionId,
        /// Called source function.
        callee: SourceFunctionId,
        /// Result count required by checked HIR.
        expected: usize,
        /// Result count returned by the Core builder.
        actual: usize,
    },
    /// A generated SSA value's type disagreed with its checked expression type.
    ExpressionType {
        /// Owning source function.
        source_function: SourceFunctionId,
        /// Type required by the checked expression.
        expected: CoreType,
        /// Type recorded on the generated Core value.
        actual: CoreType,
    },
    /// A flattened HIR value exposed an unexpected number of Core leaves.
    ExpressionArity {
        /// Owning source function.
        source_function: SourceFunctionId,
        /// Required leaf count.
        expected: usize,
        /// Actual leaf count.
        actual: usize,
    },
    /// A value-returning function unexpectedly retained a fallthrough path.
    ValueFunctionFallthrough {
        /// Source function with the invalid path.
        source_function: SourceFunctionId,
    },
    /// Target-independent Core ambient analysis rejected generated Core.
    AmbientAnalysis {
        /// Concrete analysis invariant failure.
        error: CoreAmbientAnalysisError,
    },
    /// Independent HIR and unoptimized Core ambient requirements disagreed.
    AmbientRequirementMismatch {
        /// Source function whose direct Core mapping disagreed.
        source_function: SourceFunctionId,
        /// Generated Core function.
        core_function: FunctionId,
        /// Requirement inferred over checked HIR.
        expected: crate::ir::semantic::AmbientContextRequirements,
        /// Requirement independently inferred over Core.
        actual: crate::ir::semantic::AmbientContextRequirements,
    },
}

impl fmt::Display for CoreGenerationInvariant {
    #[allow(
        clippy::too_many_lines,
        reason = "every generation invariant has one centralized user-facing rendering"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFunctionMapping { source_function } => {
                write!(formatter, "missing Core mapping for {source_function:?}")
            }
            Self::MissingExternalMapping { source_external } => {
                write!(formatter, "missing Core mapping for {source_external:?}")
            }
            Self::InvalidSemanticOperationCorrelation { source_external } => write!(
                formatter,
                "invalid typed semantic Core correlation for {source_external:?}"
            ),
            Self::SemanticOperationCorrelationCount { expected, actual } => write!(
                formatter,
                "source inventories {expected} external operations but the typed semantic Core correlation has {actual} slots"
            ),
            Self::MissingRunMapping { source_run } => {
                write!(formatter, "missing Core mapping for {source_run:?}")
            }
            Self::InvalidRunScope { source_run } => {
                write!(formatter, "invalid modifier plan for {source_run:?}")
            }
            Self::RunInventoryCount { expected, actual } => write!(
                formatter,
                "HIR inventories {expected} run scopes but traversal found {actual}"
            ),
            Self::InvalidUnsafeCommand { source_external } => write!(
                formatter,
                "invalid unsafe command reached Core lowering for {source_external:?}"
            ),
            Self::MissingFunctionName { source_function } => {
                write!(formatter, "missing source name for {source_function:?}")
            }
            Self::MissingEntryParameter {
                source_function,
                parameter,
            } => write!(
                formatter,
                "missing entry parameter {parameter} for {source_function:?}"
            ),
            Self::MissingBinding {
                source_function,
                local,
            } => write!(formatter, "missing binding %{local} in {source_function:?}"),
            Self::UnassignedBinding {
                source_function,
                local,
            } => write!(
                formatter,
                "binding %{local} has no current value in {source_function:?}"
            ),
            Self::EmptyConditional { source_function } => {
                write!(formatter, "empty conditional in {source_function:?}")
            }
            Self::EmptyContinuationMerge { source_function } => {
                write!(formatter, "empty continuation merge in {source_function:?}")
            }
            Self::LoopControlOutsideLoop { source_function } => write!(
                formatter,
                "loop control appears outside a loop in {source_function:?}"
            ),
            Self::InvalidAggregateType { struct_ } => write!(
                formatter,
                "aggregate type {struct_:?} is missing or recursively unbounded"
            ),
            Self::InvalidAggregateProjection { source_function } => write!(
                formatter,
                "non-aggregate field projection reached Core lowering in {source_function:?}"
            ),
            Self::InvalidBooleanComparison { source_function } => write!(
                formatter,
                "ordered Boolean comparison reached Core lowering in {source_function:?}"
            ),
            Self::CallResultCount {
                source_function,
                callee,
                expected,
                actual,
            } => write!(
                formatter,
                "call from {source_function:?} to {callee:?} produced {actual} results, expected {expected}"
            ),
            Self::ExpressionType {
                source_function,
                expected,
                actual,
            } => write!(
                formatter,
                "expression in {source_function:?} generated {actual}, expected {expected}"
            ),
            Self::ExpressionArity {
                source_function,
                expected,
                actual,
            } => write!(
                formatter,
                "expression in {source_function:?} has {actual} flattened values, expected {expected}"
            ),
            Self::ValueFunctionFallthrough { source_function } => write!(
                formatter,
                "value-returning function {source_function:?} retained a fallthrough path"
            ),
            Self::AmbientAnalysis { error } => {
                write!(
                    formatter,
                    "cannot infer generated Core ambient requirements: {error}"
                )
            }
            Self::AmbientRequirementMismatch {
                source_function,
                core_function,
                expected,
                actual,
            } => write!(
                formatter,
                "ambient requirements disagree for {source_function:?}/{core_function:?}: HIR {expected:?}, Core {actual:?}"
            ),
        }
    }
}

impl Error for CoreGenerationInvariant {}

/// Source-derived Core expansion protected by a compiler resource limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CoreGenerationResource {
    /// Value operands carried by predecessor edges into source-level SSA joins.
    JoinEdgeOperands,
}

impl fmt::Display for CoreGenerationResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JoinEdgeOperands => formatter.write_str("Core join-edge operands"),
        }
    }
}

/// Typed failure from checked-HIR-to-Core generation.
///
/// No variant exposes a partially constructed or invalid Core program.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CoreGenerationFailure {
    /// A source signature could not be predeclared in Core.
    Declaration {
        /// Source function being declared.
        source_function: SourceFunctionId,
        /// Concrete Core declaration failure.
        error: ProgramError,
    },
    /// A source external operation could not be linked into Core inventories.
    ExternalDeclaration {
        /// Source external operation being linked.
        source_external: SourceExternalOpId,
        /// Concrete Core inventory failure.
        error: ProgramError,
    },
    /// A structured run scope could not be linked into Core inventories.
    RunDeclaration {
        /// Source run scope being linked.
        source_run: SourceRunId,
        /// Concrete Core inventory failure.
        error: ProgramError,
    },
    /// An event handler's advancement could not be linked into Core inventories.
    AdvancementDeclaration {
        /// Source reward function being linked.
        source_function: SourceFunctionId,
        /// Concrete Core inventory failure.
        error: ProgramError,
    },
    /// The Core body builder rejected a construction request.
    Construction {
        /// Source function being lowered.
        source_function: SourceFunctionId,
        /// Concrete builder failure.
        error: BuildError,
    },
    /// Per-function Core verification rejected the completed candidate body.
    BodyVerification {
        /// Source function being lowered.
        source_function: SourceFunctionId,
        /// Concrete verifier diagnostics.
        diagnostics: Diagnostics,
    },
    /// A verified body could not be installed in its declaration.
    Definition {
        /// Source function being defined.
        source_function: SourceFunctionId,
        /// Concrete Core definition failure.
        error: ProgramError,
    },
    /// Whole-program Core verification rejected the complete generated program.
    ProgramVerification {
        /// Concrete verifier diagnostics.
        diagnostics: Diagnostics,
    },
    /// Valid source would exceed a bounded source-derived Core expansion.
    ResourceLimit {
        /// Source function whose next expansion exhausted the shared budget.
        source_function: SourceFunctionId,
        /// Bounded generated resource.
        resource: CoreGenerationResource,
        /// Whole-compilation maximum for this resource.
        limit: usize,
    },
    /// Verified HIR and Core construction contracts disagreed.
    Invariant(CoreGenerationInvariant),
}

impl fmt::Display for CoreGenerationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Declaration {
                source_function,
                error,
            } => write!(
                formatter,
                "cannot declare Core function for {source_function:?}: {error}"
            ),
            Self::ExternalDeclaration {
                source_external,
                error,
            } => write!(
                formatter,
                "cannot declare Core external operation for {source_external:?}: {error}"
            ),
            Self::RunDeclaration { source_run, error } => write!(
                formatter,
                "cannot declare Core run scope for {source_run:?}: {error}"
            ),
            Self::AdvancementDeclaration {
                source_function,
                error,
            } => write!(
                formatter,
                "cannot declare Core advancement for reward {source_function:?}: {error}"
            ),
            Self::Construction {
                source_function,
                error,
            } => write!(
                formatter,
                "cannot construct Core body for {source_function:?}: {error}"
            ),
            Self::BodyVerification {
                source_function,
                diagnostics,
            } => write!(
                formatter,
                "generated Core body for {source_function:?} is invalid: {diagnostics}"
            ),
            Self::Definition {
                source_function,
                error,
            } => write!(
                formatter,
                "cannot define Core function for {source_function:?}: {error}"
            ),
            Self::ProgramVerification { diagnostics } => {
                write!(
                    formatter,
                    "generated Core program is invalid: {diagnostics}"
                )
            }
            Self::ResourceLimit {
                source_function,
                resource,
                limit,
            } => write!(
                formatter,
                "generating {source_function:?} would exceed the whole-compilation {resource} limit of {limit}"
            ),
            Self::Invariant(invariant) => write!(
                formatter,
                "checked HIR to Core invariant failure: {invariant}"
            ),
        }
    }
}

impl Error for CoreGenerationFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Declaration { error, .. }
            | Self::ExternalDeclaration { error, .. }
            | Self::RunDeclaration { error, .. }
            | Self::AdvancementDeclaration { error, .. }
            | Self::Definition { error, .. } => Some(error),
            Self::Construction { error, .. } => Some(error),
            Self::BodyVerification { diagnostics, .. }
            | Self::ProgramVerification { diagnostics } => Some(diagnostics),
            Self::ResourceLimit { .. } => None,
            Self::Invariant(invariant) => Some(invariant),
        }
    }
}

struct CoreGenerationBudget {
    remaining_join_edge_operands: usize,
    join_edge_operand_limit: usize,
}

impl CoreGenerationBudget {
    const fn new(join_edge_operand_limit: usize) -> Self {
        Self {
            remaining_join_edge_operands: join_edge_operand_limit,
            join_edge_operand_limit,
        }
    }

    fn charge_join_edge_operands(
        &mut self,
        source_function: SourceFunctionId,
        phi_count: usize,
        predecessor_count: usize,
    ) -> Result<(), CoreGenerationFailure> {
        let Some(required) = phi_count.checked_mul(predecessor_count) else {
            return Err(self.exhausted(source_function));
        };
        let Some(remaining) = self.remaining_join_edge_operands.checked_sub(required) else {
            return Err(self.exhausted(source_function));
        };
        self.remaining_join_edge_operands = remaining;
        Ok(())
    }

    const fn exhausted(&self, source_function: SourceFunctionId) -> CoreGenerationFailure {
        CoreGenerationFailure::ResourceLimit {
            source_function,
            resource: CoreGenerationResource::JoinEdgeOperands,
            limit: self.join_edge_operand_limit,
        }
    }
}

/// Lowers one fully checked frontend product to complete verified Core.
pub(super) fn lower_hir(
    checked: &CheckedFrontendOutput,
    sources: &SourceContext,
    max_join_edge_operands: usize,
) -> Result<CoreGenerationOutput, CoreGenerationFailure> {
    let mut program = CoreProgram::new();
    let mut budget = CoreGenerationBudget::new(max_join_edge_operands);
    let mut source_to_core = declare_functions(&mut program, checked, sources)?;
    let run_to_core = declare_run_scopes(&mut program, checked)?;
    let external_to_core = declare_external_operations(&mut program, checked)?;
    let mut semantic_operations = vec![None; checked.external_operation_count()];
    let links = CoreLinks {
        functions: &source_to_core,
        externals: &external_to_core,
        runs: &run_to_core,
    };

    for function in checked.functions() {
        let core_function =
            source_to_core
                .function(function.id)
                .ok_or(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::MissingFunctionMapping {
                        source_function: function.id,
                    },
                ))?;
        let body = BodyLowerer::new(
            BodyLoweringContext::new(&program, sources, checked, links),
            function,
            core_function,
            &mut budget,
            &mut semantic_operations,
        )?
        .lower()?;
        program
            .define_function(core_function, body)
            .map_err(|error| CoreGenerationFailure::Definition {
                source_function: function.id,
                error,
            })?;
    }

    for (owner, run) in collect_run_scopes(checked) {
        let link = run_to_core
            .link(run.id)
            .ok_or(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::MissingRunMapping { source_run: run.id },
            ))?;
        let body = BodyLowerer::new(
            BodyLoweringContext::new(&program, sources, checked, links),
            owner,
            link.function,
            &mut budget,
            &mut semantic_operations,
        )?
        .lower_run_body(&run.body)?;
        program
            .define_function(link.function, body)
            .map_err(|error| CoreGenerationFailure::RunDeclaration {
                source_run: run.id,
                error,
            })?;
    }

    declare_advancements(&mut program, checked, &source_to_core)?;

    verify_program(&program, sources)
        .map_err(|diagnostics| CoreGenerationFailure::ProgramVerification { diagnostics })?;
    verify_source_semantic_correlations(
        checked,
        &program,
        &external_to_core,
        &semantic_operations,
    )?;
    let ambient = CoreAmbientAnalysis::analyze(&program).map_err(|error| {
        CoreGenerationFailure::Invariant(CoreGenerationInvariant::AmbientAnalysis { error })
    })?;
    for (source_function, core_function) in source_to_core.functions() {
        let expected = checked
            .function_behavior(source_function)
            .expect("verified checked HIR has one dense behavior summary")
            .required_ambient_context();
        let actual = ambient
            .requirement(core_function)
            .expect("Core ambient analysis is dense over generated functions");
        if expected != actual {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::AmbientRequirementMismatch {
                    source_function,
                    core_function,
                    expected,
                    actual,
                },
            ));
        }
    }
    source_to_core
        .install_external_correlations(external_to_core.operations.to_vec(), semantic_operations);
    source_to_core.install_run_correlations(&run_to_core);
    Ok(CoreGenerationOutput {
        program,
        source_to_core,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "declaration exhaustively translates the closed ordered modifier vocabulary"
)]
fn declare_run_scopes(
    program: &mut CoreProgram,
    checked: &CheckedFrontendOutput,
) -> Result<RunToCoreMap, CoreGenerationFailure> {
    let scopes = collect_run_scopes(checked);
    let mut links = Vec::with_capacity(scopes.len());
    for (_owner, run) in scopes {
        if run.id.as_usize() != Some(links.len()) {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::MissingRunMapping { source_run: run.id },
            ));
        }
        let function = program
            .declare_function(
                Some(format!("run{}", run.id.index())),
                vec![],
                vec![],
                run.origin,
            )
            .map_err(|error| CoreGenerationFailure::RunDeclaration {
                source_run: run.id,
                error,
            })?;
        let (operation, scope) = if run.modifiers.is_empty() {
            (None, None)
        } else {
            let mut modifiers = Vec::with_capacity(run.modifiers.len());
            for modifier in &run.modifiers {
                let lowered = match modifier {
                    HirRunModifier::As { query, origin } | HirRunModifier::At { query, origin } => {
                        let query_id = program
                            .declare_entity_query(lower_entity_query_decl(query))
                            .map_err(|error| CoreGenerationFailure::RunDeclaration {
                                source_run: run.id,
                                error,
                            })?;
                        if matches!(modifier, HirRunModifier::As { .. }) {
                            RunModifierInstance::AsEntityQuery {
                                query: query_id,
                                origin: *origin,
                            }
                        } else {
                            RunModifierInstance::AtEntityQuery {
                                query: query_id,
                                origin: *origin,
                            }
                        }
                    }
                    HirRunModifier::AtExecutor { kind, origin, .. } => {
                        RunModifierInstance::AtExecutor {
                            kind: *kind,
                            origin: *origin,
                        }
                    }
                    HirRunModifier::Positioned { position, origin } => {
                        RunModifierInstance::Positioned {
                            position: position.clone(),
                            origin: *origin,
                        }
                    }
                    HirRunModifier::Rotated { rotation, origin } => RunModifierInstance::Rotated {
                        rotation: rotation.clone(),
                        origin: *origin,
                    },
                    HirRunModifier::In { dimension, origin } => RunModifierInstance::In {
                        dimension: *dimension,
                        origin: *origin,
                    },
                    HirRunModifier::Anchored { anchor, origin } => RunModifierInstance::Anchored {
                        anchor: *anchor,
                        origin: *origin,
                    },
                    HirRunModifier::Align { axes, origin } => RunModifierInstance::Align {
                        axes: *axes,
                        origin: *origin,
                    },
                };
                modifiers.push(lowered);
            }
            let scope = program
                .declare_run_scope(modifiers, function, run.origin)
                .map_err(|error| CoreGenerationFailure::RunDeclaration {
                    source_run: run.id,
                    error,
                })?;
            let operation = Some(
                program
                    .declare_external_op(
                        ExternalSemanticBinding::MinecraftRunScope(scope),
                        vec![],
                        vec![],
                        run.origin,
                    )
                    .map_err(|error| CoreGenerationFailure::RunDeclaration {
                        source_run: run.id,
                        error,
                    })?,
            );
            (operation, Some(scope))
        };
        links.push(RunCoreLink {
            function,
            operation,
            scope,
        });
    }
    if links.len() != checked.run_scope_count() {
        return Err(CoreGenerationFailure::Invariant(
            CoreGenerationInvariant::RunInventoryCount {
                expected: checked.run_scope_count(),
                actual: links.len(),
            },
        ));
    }
    Ok(RunToCoreMap {
        links: links.into_boxed_slice(),
    })
}

fn lower_entity_query_decl(query: &HirEntityQuery) -> EntityQueryDecl {
    let steps = query
        .steps
        .iter()
        .map(|step| match step {
            HirEntityQueryStep::Entities {
                kind,
                origin,
                kind_origin,
            } => EntityQueryStep::Entities {
                kind: *kind,
                origin: *origin,
                kind_origin: *kind_origin,
            },
            HirEntityQueryStep::WithTag {
                tag,
                origin,
                value_origin,
            } => EntityQueryStep::WithTag {
                tag: tag.clone(),
                origin: *origin,
                value_origin: *value_origin,
            },
            HirEntityQueryStep::Limit {
                maximum,
                origin,
                value_origin,
            } => EntityQueryStep::Limit {
                maximum: *maximum,
                origin: *origin,
                value_origin: *value_origin,
            },
        })
        .collect();
    EntityQueryDecl::new(query.semantic.clone(), steps)
}

fn declare_advancements(
    program: &mut CoreProgram,
    checked: &CheckedFrontendOutput,
    source_to_core: &SourceToCoreMap,
) -> Result<(), CoreGenerationFailure> {
    for handler in checked.event_handlers() {
        let core_function =
            source_to_core
                .function(handler.reward)
                .ok_or(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::MissingFunctionMapping {
                        source_function: handler.reward,
                    },
                ))?;
        program
            .declare_advancement(
                core_function,
                lower_criterion(&handler.criterion),
                handler.origin,
            )
            .map_err(|error| CoreGenerationFailure::AdvancementDeclaration {
                source_function: handler.reward,
                error,
            })?;
    }
    Ok(())
}

fn lower_criterion(criterion: &HirCriterion) -> Criterion {
    match criterion {
        HirCriterion::InventoryChanged { items, .. } => Criterion::InventoryChanged {
            items: items
                .iter()
                .map(|item| ItemMatch::new(item.clone()))
                .collect(),
        },
    }
}

fn collect_run_scopes(output: &CheckedFrontendOutput) -> Vec<(&HirFunction, &HirRun)> {
    let mut scopes = Vec::with_capacity(output.run_scope_count());
    for function in output.functions() {
        collect_run_scopes_in_block(function, &function.body, &mut scopes);
    }
    scopes
}

fn collect_run_scopes_in_block<'a>(
    owner: &'a HirFunction,
    block: &'a HirBlock,
    scopes: &mut Vec<(&'a HirFunction, &'a HirRun)>,
) {
    for statement in &block.statements {
        match &statement.kind {
            HirStatementKind::Run(run) => {
                scopes.push((owner, run));
                collect_run_scopes_in_block(owner, &run.body, scopes);
            }
            HirStatementKind::If(conditional) => {
                for arm in &conditional.arms {
                    collect_run_scopes_in_block(owner, &arm.body, scopes);
                }
                if let Some(body) = &conditional.else_body {
                    collect_run_scopes_in_block(owner, body, scopes);
                }
            }
            HirStatementKind::While(statement) => {
                collect_run_scopes_in_block(owner, &statement.body, scopes);
            }
            HirStatementKind::Switch(switch) => {
                for arm in &switch.arms {
                    collect_run_scopes_in_block(owner, &arm.body, scopes);
                }
            }
            HirStatementKind::Declaration { .. }
            | HirStatementKind::Assignment { .. }
            | HirStatementKind::Call(_)
            | HirStatementKind::External(_)
            | HirStatementKind::Return(_)
            | HirStatementKind::Destructure { .. }
            | HirStatementKind::Schedule(_)
            | HirStatementKind::ScheduleClear(_)
            | HirStatementKind::Break
            | HirStatementKind::Continue => {}
        }
    }
}

fn declare_functions(
    program: &mut CoreProgram,
    checked: &CheckedFrontendOutput,
    sources: &SourceContext,
) -> Result<SourceToCoreMap, CoreGenerationFailure> {
    // Signatures are deliberately predeclared before any body is constructed, so
    // forward calls, direct recursion, and mutual recursion all have ordinary Core
    // call targets.
    let schedule_targets = collect_schedule_targets(checked);
    let mut correlations = Vec::with_capacity(checked.function_count());
    for function in checked.functions() {
        let name = function_name(sources, function)?;
        let mut parameters = vec![];
        for binding in function.bindings.iter().take(function.parameter_count) {
            parameters.extend(flattened_core_types(checked, binding.ty)?);
        }
        let results = match function.result {
            FunctionResult::Void => vec![],
            FunctionResult::Value(ty) => flattened_core_types(checked, ty)?,
        };
        let linkage = match function.visibility {
            FunctionVisibility::Private | FunctionVisibility::Public => {
                CoreFunctionLinkage::Internal
            }
            FunctionVisibility::DatapackExport => CoreFunctionLinkage::DatapackExport,
        };
        let core_function = program
            .declare_function_with_linkage(
                Some(name),
                linkage,
                function.one_tick,
                function.tick,
                schedule_targets.contains(&function.id),
                parameters,
                results,
                function.origin,
            )
            .map_err(|error| CoreGenerationFailure::Declaration {
                source_function: function.id,
                error,
            })?;
        correlations.push((function.id, core_function));
    }
    Ok(SourceToCoreMap::new(correlations))
}

/// Collects every function named by a `schedule` (arm) statement anywhere in
/// the whole package (Stage 9B). Deliberately excludes `schedule clear`
/// targets — a function referenced only by `schedule clear` needs no root
/// registration or contract enforcement (see the 9B dossier).
fn collect_schedule_targets(checked: &CheckedFrontendOutput) -> std::collections::BTreeSet<SourceFunctionId> {
    let mut targets = std::collections::BTreeSet::new();
    for function in checked.functions() {
        collect_schedule_targets_in_block(&function.body, &mut targets);
    }
    targets
}

fn collect_schedule_targets_in_block(
    block: &HirBlock,
    targets: &mut std::collections::BTreeSet<SourceFunctionId>,
) {
    for statement in &block.statements {
        match &statement.kind {
            HirStatementKind::Schedule(schedule) => {
                targets.insert(schedule.callee);
            }
            HirStatementKind::Run(run) => {
                collect_schedule_targets_in_block(&run.body, targets);
            }
            HirStatementKind::If(conditional) => {
                for arm in &conditional.arms {
                    collect_schedule_targets_in_block(&arm.body, targets);
                }
                if let Some(body) = &conditional.else_body {
                    collect_schedule_targets_in_block(body, targets);
                }
            }
            HirStatementKind::While(statement) => {
                collect_schedule_targets_in_block(&statement.body, targets);
            }
            HirStatementKind::Switch(switch) => {
                for arm in &switch.arms {
                    collect_schedule_targets_in_block(&arm.body, targets);
                }
            }
            HirStatementKind::Declaration { .. }
            | HirStatementKind::Assignment { .. }
            | HirStatementKind::Call(_)
            | HirStatementKind::External(_)
            | HirStatementKind::Return(_)
            | HirStatementKind::Destructure { .. }
            | HirStatementKind::ScheduleClear(_)
            | HirStatementKind::Break
            | HirStatementKind::Continue => {}
        }
    }
}

#[allow(clippy::too_many_lines)]
fn declare_external_operations(
    program: &mut CoreProgram,
    checked: &CheckedFrontendOutput,
) -> Result<ExternalToCoreMap, CoreGenerationFailure> {
    let mut external_correlations = Vec::with_capacity(checked.external_operation_count());
    for external in checked.external_ops() {
        let binding = match &external.semantic {
            HirExternalSemantic::EntityNbtRead {
                receiver,
                segments,
                result_ty,
                receiver_origin,
                ..
            } => {
                let read = program
                    .declare_entity_nbt_read(
                        lower_entity_nbt_receiver(receiver),
                        lower_entity_nbt_segments(segments),
                        core_type(*result_ty),
                        *receiver_origin,
                    )
                    .map_err(|error| CoreGenerationFailure::ExternalDeclaration {
                        source_external: external.id,
                        error,
                    })?;
                ExternalSemanticBinding::EntityNbtRead(read)
            }
            HirExternalSemantic::EntityNbtWrite {
                receiver,
                segments,
                item_id,
                count,
                receiver_origin,
            } => {
                let write = program
                    .declare_entity_nbt_write(
                        lower_entity_nbt_receiver(receiver),
                        lower_entity_nbt_segments(segments),
                        lower_entity_nbt_item_id(item_id),
                        lower_entity_nbt_count(count),
                        *receiver_origin,
                    )
                    .map_err(|error| CoreGenerationFailure::ExternalDeclaration {
                        source_external: external.id,
                        error,
                    })?;
                ExternalSemanticBinding::EntityNbtWrite(write)
            }
            HirExternalSemantic::UnsafeMinecraftCommand { command, .. } => {
                let fragment = TargetFragment::unsafe_minecraft_command(command).map_err(|_| {
                    CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidUnsafeCommand {
                            source_external: external.id,
                        },
                    )
                })?;
                let fragment = program.declare_target_fragment(fragment).map_err(|error| {
                    CoreGenerationFailure::ExternalDeclaration {
                        source_external: external.id,
                        error,
                    }
                })?;
                ExternalSemanticBinding::UnsafeTargetFragment(fragment)
            }
            HirExternalSemantic::MinecraftOperation {
                key,
                receiver_kind,
                attributes,
                call_origin,
                member_origin,
                receiver_origin,
                ..
            } => {
                let attributes = match attributes {
                    HirMinecraftOperationAttributes::Say {
                        message,
                        message_origin,
                    } => MinecraftOperationAttributes::Say {
                        message: message.clone(),
                        message_origin: *message_origin,
                    },
                    HirMinecraftOperationAttributes::Teleport {
                        position,
                        component_origins,
                    } => MinecraftOperationAttributes::Teleport {
                        position: position.clone(),
                        component_origins: *component_origins,
                    },
                    HirMinecraftOperationAttributes::MoveBy {
                        offset,
                        component_origins,
                    } => MinecraftOperationAttributes::MoveBy {
                        offset: offset.clone(),
                        component_origins: *component_origins,
                    },
                };
                let operation = program
                    .declare_minecraft_operation(
                        *key,
                        *receiver_kind,
                        attributes,
                        MinecraftOperationOrigins::new(
                            *call_origin,
                            *member_origin,
                            *receiver_origin,
                        ),
                    )
                    .map_err(|error| CoreGenerationFailure::ExternalDeclaration {
                        source_external: external.id,
                        error,
                    })?;
                ExternalSemanticBinding::MinecraftOperation(operation)
            }
        };
        let results = match &external.semantic {
            HirExternalSemantic::UnsafeMinecraftCommand { .. }
            | HirExternalSemantic::MinecraftOperation { .. }
            | HirExternalSemantic::EntityNbtWrite { .. } => vec![],
            HirExternalSemantic::EntityNbtRead { result_ty, .. } => vec![core_type(*result_ty)],
        };
        let parameters = match (&external.semantic, &binding) {
            (HirExternalSemantic::EntityNbtRead { segments, .. }, _) => {
                vec![CoreType::I32; count_runtime_entity_nbt_segments(segments)]
            }
            (
                HirExternalSemantic::EntityNbtWrite { .. },
                ExternalSemanticBinding::EntityNbtWrite(write),
            ) => program
                .entity_nbt_write(*write)
                .map(EntityNbtWriteDecl::runtime_operand_types)
                .unwrap_or_default(),
            _ => vec![],
        };
        let operation = program
            .declare_external_op(binding, parameters, results, external.origin)
            .map_err(|error| CoreGenerationFailure::ExternalDeclaration {
                source_external: external.id,
                error,
            })?;
        external_correlations.push((external.id, operation));
    }
    Ok(ExternalToCoreMap::new(external_correlations))
}

/// Maps a checked HIR entity-NBT receiver to its Core-level counterpart.
/// Shared by `EntityNbtRead`/`EntityNbtWrite` declaration (PS-16).
fn lower_entity_nbt_receiver(receiver: &HirEntityNbtReceiver) -> EntityNbtReceiver {
    match receiver {
        HirEntityNbtReceiver::Entity { kind, .. } => EntityNbtReceiver::Entity(*kind),
        HirEntityNbtReceiver::Block { kind, position } => {
            EntityNbtReceiver::Block(*kind, *position)
        }
    }
}

/// Maps checked HIR entity-NBT path segments to their Core-level
/// counterparts, const-folding a compile-time-literal `Int32` index/match
/// value and marking any other value a runtime placeholder (resolved to its
/// real per-occurrence `ValueId` later, in `lower_external`). Shared by
/// `EntityNbtRead`/`EntityNbtWrite` declaration (PS-16).
fn lower_entity_nbt_segments(segments: &[HirEntityPathSegment]) -> Vec<EntityNbtPathSegment> {
    segments
        .iter()
        .map(|segment| match segment {
            HirEntityPathSegment::Key(key) => EntityNbtPathSegment::Key(key.clone()),
            HirEntityPathSegment::Index(index) => EntityNbtPathSegment::Index(match index.kind {
                HirExpressionKind::Int32(value) => Operand::Const(value),
                _ => Operand::Runtime(ValueId::from_index(0)),
            }),
            HirEntityPathSegment::Match { match_key, value } => EntityNbtPathSegment::Match {
                match_key: match_key.clone(),
                value: match value.kind {
                    HirExpressionKind::Int32(value) => Operand::Const(value),
                    _ => Operand::Runtime(ValueId::from_index(0)),
                },
            },
        })
        .collect()
}

/// Const-folds a whole-slot write's item id expression, mirroring
/// `lower_entity_nbt_segments`'s own `Index`/`Match` const-folding.
fn lower_entity_nbt_item_id(item_id: &HirExpression) -> Operand<Box<str>> {
    match &item_id.kind {
        HirExpressionKind::String {
            op: HirStringOp::Constant(text),
            ..
        } => Operand::Const(text.clone()),
        _ => Operand::Runtime(ValueId::from_index(0)),
    }
}

/// Const-folds a whole-slot write's count expression, mirroring
/// `lower_entity_nbt_segments`'s own `Index`/`Match` const-folding.
fn lower_entity_nbt_count(count: &HirExpression) -> Operand<i32> {
    match count.kind {
        HirExpressionKind::Int32(value) => Operand::Const(value),
        _ => Operand::Runtime(ValueId::from_index(0)),
    }
}

/// Counts `Index`/`Match` segments whose value is not a compile-time `Int32`
/// literal — exactly the number of runtime operands the declared external
/// operation expects. Shared by `EntityNbtRead`/`EntityNbtWrite` (PS-16).
fn count_runtime_entity_nbt_segments(segments: &[HirEntityPathSegment]) -> usize {
    segments
        .iter()
        .filter(|segment| {
            matches!(
                segment,
                HirEntityPathSegment::Index(index)
                    | HirEntityPathSegment::Match { value: index, .. }
                    if !matches!(index.kind, HirExpressionKind::Int32(_))
            )
        })
        .count()
}

fn verify_source_semantic_correlations(
    checked: &CheckedFrontendOutput,
    program: &CoreProgram,
    externals: &ExternalToCoreMap,
    semantic_operations: &[Option<SourceSemanticCoreOccurrence>],
) -> Result<(), CoreGenerationFailure> {
    let expected = checked.external_operation_count();
    if semantic_operations.len() != expected || externals.operations.len() != expected {
        return Err(CoreGenerationFailure::Invariant(
            CoreGenerationInvariant::SemanticOperationCorrelationCount {
                expected,
                actual: semantic_operations.len(),
            },
        ));
    }

    let attached = collect_attached_external_occurrences(program);
    for external in checked.external_ops() {
        verify_source_semantic_correlation(
            external,
            program,
            externals,
            semantic_operations,
            &attached,
        )?;
    }
    Ok(())
}

fn collect_attached_external_occurrences(
    program: &CoreProgram,
) -> BTreeMap<ExternalOpId, Vec<(FunctionId, InstId, OriginId)>> {
    let mut attached = BTreeMap::<ExternalOpId, Vec<(FunctionId, InstId, OriginId)>>::new();
    for (function, declaration) in program.functions() {
        let Some(body) = declaration.body() else {
            continue;
        };
        for block in body.block_order() {
            let block = body
                .block(*block)
                .expect("verified generated Core layout names attached blocks");
            for instruction in block.instructions() {
                let instruction_data = body
                    .instruction(*instruction)
                    .expect("verified generated Core blocks name attached instructions");
                if let CoreOp::External(external) = instruction_data.op() {
                    attached.entry(*external).or_default().push((
                        function,
                        *instruction,
                        instruction_data.origin(),
                    ));
                }
            }
        }
    }
    attached
}

#[allow(
    clippy::too_many_lines,
    reason = "one audit keeps all source/external/Core occurrence invariants together"
)]
fn verify_source_semantic_correlation(
    external: &HirExternalOp,
    program: &CoreProgram,
    externals: &ExternalToCoreMap,
    semantic_operations: &[Option<SourceSemanticCoreOccurrence>],
    attached: &BTreeMap<ExternalOpId, Vec<(FunctionId, InstId, OriginId)>>,
) -> Result<(), CoreGenerationFailure> {
    let invalid = || {
        CoreGenerationFailure::Invariant(
            CoreGenerationInvariant::InvalidSemanticOperationCorrelation {
                source_external: external.id,
            },
        )
    };
    let core_external = externals.operation(external.id).ok_or_else(invalid)?;
    let declaration = program.external_op(core_external).ok_or_else(invalid)?;
    if declaration.origin() != external.origin {
        return Err(invalid());
    }
    let slot = semantic_operations
        .get(external.id.as_usize().ok_or_else(invalid)?)
        .copied()
        .flatten();

    match &external.semantic {
        HirExternalSemantic::EntityNbtRead {
            receiver,
            segments,
            result_ty,
            receiver_origin,
            ..
        } => {
            let ExternalSemanticBinding::EntityNbtRead(read) = declaration.binding() else {
                return Err(invalid());
            };
            let read = program.entity_nbt_read(read).ok_or_else(invalid)?;
            if !entity_nbt_receiver_matches(receiver, read.receiver())
                || read.result_ty() != core_type(*result_ty)
                || read.receiver_origin() != *receiver_origin
                || !entity_nbt_segments_match(segments, read.segments())
            {
                return Err(invalid());
            }
            verify_single_or_absent_occurrence(
                core_external,
                attached,
                slot,
                external.origin,
                &invalid,
            )?;
        }
        HirExternalSemantic::EntityNbtWrite {
            receiver,
            segments,
            item_id,
            count,
            receiver_origin,
        } => {
            let ExternalSemanticBinding::EntityNbtWrite(write) = declaration.binding() else {
                return Err(invalid());
            };
            let write = program.entity_nbt_write(write).ok_or_else(invalid)?;
            if !entity_nbt_receiver_matches(receiver, write.receiver())
                || !entity_nbt_item_id_matches(item_id, write.item_id())
                || !entity_nbt_count_matches(count, write.count())
                || write.receiver_origin() != *receiver_origin
                || !entity_nbt_segments_match(segments, write.segments())
            {
                return Err(invalid());
            }
            verify_single_or_absent_occurrence(
                core_external,
                attached,
                slot,
                external.origin,
                &invalid,
            )?;
        }
        HirExternalSemantic::UnsafeMinecraftCommand { .. } => {
            if !matches!(
                declaration.binding(),
                ExternalSemanticBinding::UnsafeTargetFragment(_)
            ) || slot.is_some()
            {
                return Err(invalid());
            }
        }
        HirExternalSemantic::MinecraftOperation {
            key,
            receiver_kind,
            attributes,
            call_origin,
            member_origin,
            receiver_origin,
            ..
        } => {
            let ExternalSemanticBinding::MinecraftOperation(operation) = declaration.binding()
            else {
                return Err(invalid());
            };
            let operation = program.minecraft_operation(operation).ok_or_else(invalid)?;
            let attributes_match = match (attributes, operation.attributes()) {
                (
                    HirMinecraftOperationAttributes::Say {
                        message,
                        message_origin,
                    },
                    MinecraftOperationAttributes::Say {
                        message: core_message,
                        message_origin: core_message_origin,
                    },
                ) => message == core_message && message_origin == core_message_origin,
                (
                    HirMinecraftOperationAttributes::Teleport {
                        position,
                        component_origins,
                    },
                    MinecraftOperationAttributes::Teleport {
                        position: core_position,
                        component_origins: core_origins,
                    },
                ) => position == core_position && component_origins == core_origins,
                (
                    HirMinecraftOperationAttributes::MoveBy {
                        offset,
                        component_origins,
                    },
                    MinecraftOperationAttributes::MoveBy {
                        offset: core_offset,
                        component_origins: core_origins,
                    },
                ) => offset == core_offset && component_origins == core_origins,
                _ => false,
            };
            if operation.key() != *key
                || operation.receiver_kind() != *receiver_kind
                || !attributes_match
                || operation.origins()
                    != MinecraftOperationOrigins::new(
                        *call_origin,
                        *member_origin,
                        *receiver_origin,
                    )
            {
                return Err(invalid());
            }

            verify_single_or_absent_occurrence(
                core_external,
                attached,
                slot,
                external.origin,
                &invalid,
            )?;
        }
    }
    Ok(())
}

/// Checks that an external declaration's Core-side occurrence bookkeeping
/// agrees with its retained semantic-operation slot — either both report no
/// occurrence, or both agree on the exact same one. Shared by every
/// semantic-operation kind's cross-check above (`EntityNbtRead`,
/// `EntityNbtWrite`, `MinecraftOperation`).
fn verify_single_or_absent_occurrence(
    core_external: ExternalOpId,
    attached: &BTreeMap<ExternalOpId, Vec<(FunctionId, InstId, OriginId)>>,
    slot: Option<SourceSemanticCoreOccurrence>,
    external_origin: OriginId,
    invalid: &impl Fn() -> CoreGenerationFailure,
) -> Result<(), CoreGenerationFailure> {
    match attached.get(&core_external).map_or(&[][..], Vec::as_slice) {
        [] if slot.is_none() => Ok(()),
        [(function, instruction, instruction_origin)]
            if *instruction_origin == external_origin
                && slot
                    == Some(SourceSemanticCoreOccurrence::new(
                        core_external,
                        *function,
                        *instruction,
                    )) =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}

/// Checks an entity-NBT path's segments correspond one-to-one between HIR and
/// Core (`Key` names match; `Index`/`Match` const values match, runtime
/// values are only checked for `Operand::Runtime`-ness). Shared by
/// `EntityNbtRead`/`EntityNbtWrite`'s cross-checks above.
fn entity_nbt_segments_match(hir: &[HirEntityPathSegment], core: &[EntityNbtPathSegment]) -> bool {
    hir.len() == core.len()
        && hir.iter().zip(core).all(|(hir, core)| match (hir, core) {
            (HirEntityPathSegment::Key(key), EntityNbtPathSegment::Key(core_key)) => {
                key.as_ref() == core_key.as_ref()
            }
            (HirEntityPathSegment::Index(index), EntityNbtPathSegment::Index(core_index)) => {
                match (&index.kind, core_index) {
                    (HirExpressionKind::Int32(n), Operand::Const(core_n)) => n == core_n,
                    (_, Operand::Runtime(_)) => true,
                    _ => false,
                }
            }
            (
                HirEntityPathSegment::Match { match_key, value },
                EntityNbtPathSegment::Match {
                    match_key: core_match_key,
                    value: core_value,
                },
            ) => {
                match_key.as_ref() == core_match_key.as_ref()
                    && match (&value.kind, core_value) {
                        (HirExpressionKind::Int32(n), Operand::Const(core_n)) => n == core_n,
                        (_, Operand::Runtime(_)) => true,
                        _ => false,
                    }
            }
            _ => false,
        })
}

/// Checks an entity-NBT path's root receiver corresponds between HIR and
/// Core. Shared by `EntityNbtRead`/`EntityNbtWrite`'s cross-checks above.
fn entity_nbt_receiver_matches(hir: &HirEntityNbtReceiver, core: EntityNbtReceiver) -> bool {
    match (hir, core) {
        (HirEntityNbtReceiver::Entity { kind, .. }, EntityNbtReceiver::Entity(core_kind)) => {
            *kind == core_kind
        }
        (
            HirEntityNbtReceiver::Block { kind, position },
            EntityNbtReceiver::Block(core_kind, core_position),
        ) => *kind == core_kind && *position == core_position,
        _ => false,
    }
}

/// Checks a whole-slot write's item id corresponds between HIR and Core: a
/// `Const` value must match textually; a `Runtime` value only needs the HIR
/// side to not itself be the constant form (real `ValueId` agreement is out
/// of scope for this identity-only cross-check, matching
/// `entity_nbt_segments_match`'s own `Operand::Runtime` leniency).
fn entity_nbt_item_id_matches(hir: &HirExpression, core: &Operand<Box<str>>) -> bool {
    match (&hir.kind, core) {
        (
            HirExpressionKind::String {
                op: HirStringOp::Constant(text),
                ..
            },
            Operand::Const(core_text),
        ) => text == core_text,
        (_, Operand::Runtime(_)) => true,
        _ => false,
    }
}

/// Checks a whole-slot write's count corresponds between HIR and Core —
/// same rationale as `entity_nbt_item_id_matches`.
fn entity_nbt_count_matches(hir: &HirExpression, core: Operand<i32>) -> bool {
    match (&hir.kind, core) {
        (HirExpressionKind::Int32(n), Operand::Const(core_n)) => *n == core_n,
        (_, Operand::Runtime(_)) => true,
        _ => false,
    }
}

fn function_name(
    sources: &SourceContext,
    function: &HirFunction,
) -> Result<Box<str>, CoreGenerationFailure> {
    sources
        .resolve_origin_span(function.name_origin)
        .and_then(|span| sources.files().slice(span).ok())
        .map(Into::into)
        .ok_or(CoreGenerationFailure::Invariant(
            CoreGenerationInvariant::MissingFunctionName {
                source_function: function.id,
            },
        ))
}

fn core_type(ty: ValueType) -> CoreType {
    match ty {
        ValueType::Bool => CoreType::Bool,
        ValueType::Int32 | ValueType::Enum(_) => CoreType::I32,
        ValueType::ListI32 => CoreType::ListI32,
        ValueType::String => CoreType::String,
        ValueType::Struct(_) | ValueType::AnonymousStruct(_) => {
            unreachable!("aggregate types are expanded before Core")
        }
    }
}

fn flattened_core_types(
    checked: &CheckedFrontendOutput,
    ty: ValueType,
) -> Result<Vec<CoreType>, CoreGenerationFailure> {
    fn visit(
        checked: &CheckedFrontendOutput,
        ty: ValueType,
        active: &mut Vec<crate::frontend::hir::SourceStructId>,
        output: &mut Vec<CoreType>,
    ) -> Result<(), CoreGenerationFailure> {
        match ty {
            ValueType::Bool
            | ValueType::Int32
            | ValueType::ListI32
            | ValueType::String
            | ValueType::Enum(_) => {
                output.push(core_type(ty));
            }
            ValueType::Struct(struct_) => {
                if active.contains(&struct_) {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType { struct_ },
                    ));
                }
                let declaration =
                    checked
                        .struct_(struct_)
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType { struct_ },
                        ))?;
                active.push(struct_);
                for field in &declaration.fields {
                    visit(checked, field.ty, active, output)?;
                }
                active.pop();
            }
            ValueType::AnonymousStruct(struct_) => {
                let declaration =
                    checked
                        .anonymous_struct(struct_)
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateProjection {
                                source_function: SourceFunctionId::from_index(0)
                                    .expect("zero fits"),
                            },
                        ))?;
                match &declaration.kind {
                    crate::frontend::hir::HirAnonymousStructKind::Named(fields) => {
                        for field in fields {
                            visit(checked, field.ty, active, output)?;
                        }
                    }
                    crate::frontend::hir::HirAnonymousStructKind::Positional(fields) => {
                        for field in fields {
                            visit(checked, *field, active, output)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    let mut output = vec![];
    visit(checked, ty, &mut vec![], &mut output)?;
    Ok(output)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValueBundle(Box<[ValueId]>);

impl ValueBundle {
    fn scalar(value: ValueId) -> Self {
        Self(Box::new([value]))
    }

    fn into_values(self) -> Vec<ValueId> {
        self.0.into_vec()
    }

    fn scalar_value(&self, owner: SourceFunctionId) -> Result<ValueId, CoreGenerationFailure> {
        let [value] = self.0.as_ref() else {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::ExpressionArity {
                    source_function: owner,
                    expected: 1,
                    actual: self.0.len(),
                },
            ));
        };
        Ok(*value)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct BindingState {
    value: Option<ValueBundle>,
    active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct JournalEntry {
    index: usize,
    previous: BindingState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EnvironmentCheckpoint(usize);

#[derive(Clone, Debug, Eq, PartialEq)]
struct EnvironmentOverride {
    index: usize,
    state: BindingState,
}

struct Environment {
    states: Vec<BindingState>,
    journal: Vec<JournalEntry>,
}

impl Environment {
    fn new(binding_count: usize) -> Self {
        Self {
            states: vec![BindingState::default(); binding_count],
            journal: vec![],
        }
    }

    fn index(local: LocalId) -> Option<usize> {
        usize::try_from(local.index()).ok()
    }

    fn activate(
        &mut self,
        owner: SourceFunctionId,
        local: LocalId,
        value: Option<ValueBundle>,
    ) -> Result<(), CoreGenerationFailure> {
        let index = Self::index(local).filter(|index| *index < self.states.len());
        let Some(index) = index else {
            return Err(missing_binding(owner, local));
        };
        self.write(
            index,
            BindingState {
                value,
                active: true,
            },
        );
        Ok(())
    }

    fn assign(
        &mut self,
        owner: SourceFunctionId,
        local: LocalId,
        value: ValueBundle,
    ) -> Result<(), CoreGenerationFailure> {
        let index = Self::index(local)
            .filter(|index| self.states.get(*index).is_some_and(|state| state.active));
        let Some(index) = index else {
            return Err(missing_binding(owner, local));
        };
        self.write(
            index,
            BindingState {
                value: Some(value),
                active: true,
            },
        );
        Ok(())
    }

    fn read(
        &self,
        owner: SourceFunctionId,
        local: LocalId,
    ) -> Result<ValueBundle, CoreGenerationFailure> {
        let Some(index) = Self::index(local) else {
            return Err(missing_binding(owner, local));
        };
        let Some(state) = self.states.get(index).cloned() else {
            return Err(missing_binding(owner, local));
        };
        if !state.active {
            return Err(missing_binding(owner, local));
        }
        state.value.ok_or(CoreGenerationFailure::Invariant(
            CoreGenerationInvariant::UnassignedBinding {
                source_function: owner,
                local: local.index(),
            },
        ))
    }

    fn deactivate(&mut self, local: LocalId) {
        if let Some(index) = Self::index(local) {
            if index < self.states.len() {
                self.write(index, BindingState::default());
            }
        }
    }

    fn checkpoint(&self) -> EnvironmentCheckpoint {
        EnvironmentCheckpoint(self.journal.len())
    }

    fn rollback(&mut self, checkpoint: EnvironmentCheckpoint) {
        debug_assert!(checkpoint.0 <= self.journal.len());
        while self.journal.len() > checkpoint.0 {
            let Some(entry) = self.journal.pop() else {
                break;
            };
            self.states[entry.index] = entry.previous;
        }
    }

    fn overrides_since(&self, checkpoint: EnvironmentCheckpoint) -> Vec<EnvironmentOverride> {
        debug_assert!(checkpoint.0 <= self.journal.len());
        let entries = self.journal.get(checkpoint.0..).unwrap_or_default();
        note_override_entry_visits(entries.len());

        // Sorting by both binding and journal order lets the first entry in each
        // binding group recover that binding's exact checkpoint state. A binding
        // activated and then deactivated inside a branch therefore produces no
        // net override and cannot leak out of its lexical scope.
        let mut touched = entries
            .iter()
            .enumerate()
            .map(|(order, entry)| (entry.index, order, entry.previous.clone()))
            .collect::<Vec<_>>();
        touched.sort_unstable_by_key(|(index, order, _)| (*index, *order));

        let mut overrides = Vec::with_capacity(touched.len());
        let mut offset = 0;
        while let Some((index, _, incoming)) = touched.get(offset).cloned() {
            let state = self.states[index].clone();
            if state != incoming {
                overrides.push(EnvironmentOverride { index, state });
            }
            offset += 1;
            while touched.get(offset).is_some_and(|entry| entry.0 == index) {
                offset += 1;
            }
        }
        overrides
    }

    fn state(&self, index: usize) -> Option<BindingState> {
        self.states.get(index).cloned()
    }

    fn active_assigned_indices(&self) -> Vec<usize> {
        self.states
            .iter()
            .enumerate()
            .filter_map(|(index, state)| (state.active && state.value.is_some()).then_some(index))
            .collect()
    }

    fn write(&mut self, index: usize, state: BindingState) {
        let previous = self.states[index].clone();
        if previous == state {
            return;
        }
        self.journal.push(JournalEntry { index, previous });
        self.states[index] = state;
    }
}

#[cfg(test)]
thread_local! {
    static OVERRIDE_ENTRY_VISITS: Cell<usize> = const { Cell::new(0) };
    static MERGE_CANDIDATE_VISITS: Cell<usize> = const { Cell::new(0) };
    static CANDIDATE_STATE_VISITS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn note_override_entry_visits(count: usize) {
    OVERRIDE_ENTRY_VISITS.set(OVERRIDE_ENTRY_VISITS.get().saturating_add(count));
}

#[cfg(not(test))]
const fn note_override_entry_visits(_: usize) {}

#[cfg(test)]
fn note_merge_candidate_visits(count: usize) {
    MERGE_CANDIDATE_VISITS.set(MERGE_CANDIDATE_VISITS.get().saturating_add(count));
}

#[cfg(not(test))]
const fn note_merge_candidate_visits(_: usize) {}

#[cfg(test)]
fn note_candidate_state_visits(count: usize) {
    CANDIDATE_STATE_VISITS.set(CANDIDATE_STATE_VISITS.get().saturating_add(count));
}

#[cfg(not(test))]
const fn note_candidate_state_visits(_: usize) {}

#[cfg(test)]
fn reset_environment_work_counters() {
    OVERRIDE_ENTRY_VISITS.set(0);
    MERGE_CANDIDATE_VISITS.set(0);
    CANDIDATE_STATE_VISITS.set(0);
}

#[cfg(test)]
fn environment_work_counters() -> (usize, usize, usize) {
    (
        OVERRIDE_ENTRY_VISITS.get(),
        MERGE_CANDIDATE_VISITS.get(),
        CANDIDATE_STATE_VISITS.get(),
    )
}

fn missing_binding(owner: SourceFunctionId, local: LocalId) -> CoreGenerationFailure {
    CoreGenerationFailure::Invariant(CoreGenerationInvariant::MissingBinding {
        source_function: owner,
        local: local.index(),
    })
}

struct BranchContinuation {
    block: BlockId,
    overrides: Vec<EnvironmentOverride>,
}

impl BranchContinuation {
    fn state(&self, index: usize, incoming: BindingState) -> BindingState {
        self.overrides
            .binary_search_by_key(&index, |entry| entry.index)
            .map_or(incoming, |offset| self.overrides[offset].state.clone())
    }
}

struct MergeCandidate {
    index: usize,
    merged: BindingState,
    needs_parameter: bool,
}

#[derive(Clone)]
struct MergeAggregate {
    override_count: usize,
    first_value: Option<ValueBundle>,
    all_active: bool,
    all_assigned: bool,
    all_values_equal: bool,
}

impl MergeAggregate {
    fn new(state: &BindingState) -> Self {
        Self {
            override_count: 1,
            first_value: state.value.clone(),
            all_active: state.active,
            all_assigned: state.value.is_some(),
            all_values_equal: true,
        }
    }

    fn include_override(&mut self, state: &BindingState) {
        self.override_count += 1;
        self.include_state(state);
    }

    fn include_missing_incoming(&mut self, state: &BindingState) {
        self.include_state(state);
    }

    fn include_state(&mut self, state: &BindingState) {
        self.all_active &= state.active;
        self.all_assigned &= state.value.is_some();
        self.all_values_equal &= state.value == self.first_value;
    }

    fn candidate(self, index: usize) -> MergeCandidate {
        MergeCandidate {
            index,
            merged: if self.all_active {
                BindingState {
                    value: self.all_assigned.then_some(self.first_value).flatten(),
                    active: true,
                }
            } else {
                BindingState::default()
            },
            needs_parameter: self.all_active && self.all_assigned && !self.all_values_equal,
        }
    }
}

struct BodyLowerer<'program, 'budget> {
    builder: FunctionBuilder<'program>,
    checked: &'program CheckedFrontendOutput,
    links: CoreLinks<'program>,
    function: &'program HirFunction,
    core_function: FunctionId,
    budget: &'budget mut CoreGenerationBudget,
    semantic_operations: &'budget mut [Option<SourceSemanticCoreOccurrence>],
    loops: Vec<LoopLoweringContext>,
}

#[derive(Clone)]
struct LoopLoweringContext {
    header: BlockId,
    exit: BlockId,
    carried: Vec<usize>,
}

#[derive(Clone, Copy)]
struct BodyLoweringContext<'program> {
    program: &'program CoreProgram,
    sources: &'program SourceContext,
    checked: &'program CheckedFrontendOutput,
    links: CoreLinks<'program>,
}

impl<'program> BodyLoweringContext<'program> {
    const fn new(
        program: &'program CoreProgram,
        sources: &'program SourceContext,
        checked: &'program CheckedFrontendOutput,
        links: CoreLinks<'program>,
    ) -> Self {
        Self {
            program,
            sources,
            checked,
            links,
        }
    }
}

#[derive(Clone, Copy)]
struct CoreLinks<'program> {
    functions: &'program SourceToCoreMap,
    externals: &'program ExternalToCoreMap,
    runs: &'program RunToCoreMap,
}

impl<'program, 'budget> BodyLowerer<'program, 'budget> {
    fn new(
        context: BodyLoweringContext<'program>,
        function: &'program HirFunction,
        core_function: FunctionId,
        budget: &'budget mut CoreGenerationBudget,
        semantic_operations: &'budget mut [Option<SourceSemanticCoreOccurrence>],
    ) -> Result<Self, CoreGenerationFailure> {
        let builder = FunctionBuilder::new(context.program, context.sources, core_function)
            .map_err(|error| CoreGenerationFailure::Construction {
                source_function: function.id,
                error,
            })?;
        Ok(Self {
            builder,
            checked: context.checked,
            links: context.links,
            function,
            core_function,
            budget,
            semantic_operations,
            loops: vec![],
        })
    }

    fn lower(mut self) -> Result<FunctionBody, CoreGenerationFailure> {
        let entry = self.builder.entry_block();
        let parameters = self
            .builder
            .body()
            .block(entry)
            .map(|block| {
                block
                    .parameters()
                    .iter()
                    .map(crate::ir::core::BlockParam::value)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut environment = Environment::new(self.function.bindings.len());
        let mut parameter_offset = 0usize;
        for index in 0..self.function.parameter_count {
            let local = self.function.bindings[index].id;
            let leaf_count =
                flattened_core_types(self.checked, self.function.bindings[index].ty)?.len();
            let end = parameter_offset.checked_add(leaf_count).ok_or(
                CoreGenerationFailure::Invariant(CoreGenerationInvariant::MissingEntryParameter {
                    source_function: self.function.id,
                    parameter: index,
                }),
            )?;
            let values =
                parameters
                    .get(parameter_offset..end)
                    .ok_or(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::MissingEntryParameter {
                            source_function: self.function.id,
                            parameter: index,
                        },
                    ))?;
            environment.activate(
                self.function.id,
                local,
                Some(ValueBundle(values.to_vec().into_boxed_slice())),
            )?;
            parameter_offset = end;
        }

        let continuation = self.lower_block(entry, &mut environment, &self.function.body)?;
        if let Some(block) = continuation {
            if !matches!(self.function.result, FunctionResult::Void) {
                return Err(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::ValueFunctionFallthrough {
                        source_function: self.function.id,
                    },
                ));
            }
            self.switch_to(block)?;
            self.terminate(
                TerminatorKind::Return(vec![]),
                self.function.body.closing_brace_origin,
            )?;
        }

        self.builder
            .finish()
            .map_err(|diagnostics| CoreGenerationFailure::BodyVerification {
                source_function: self.function.id,
                diagnostics,
            })
    }

    fn lower_run_body(mut self, body: &HirBlock) -> Result<FunctionBody, CoreGenerationFailure> {
        let entry = self.builder.entry_block();
        let mut environment = Environment::new(self.function.bindings.len());
        let continuation = self.lower_block(entry, &mut environment, body)?;
        if let Some(block) = continuation {
            self.switch_to(block)?;
            self.terminate(TerminatorKind::Return(vec![]), body.closing_brace_origin)?;
        }
        self.builder
            .finish()
            .map_err(|diagnostics| CoreGenerationFailure::BodyVerification {
                source_function: self.function.id,
                diagnostics,
            })
    }

    fn lower_block(
        &mut self,
        block_id: BlockId,
        environment: &mut Environment,
        block: &HirBlock,
    ) -> Result<Option<BlockId>, CoreGenerationFailure> {
        let mut continuation = Some(block_id);
        let mut scope_locals = vec![];
        for statement in &block.statements {
            let Some(block_id) = continuation else {
                // Checked unreachable source remains useful for diagnostics but is
                // deliberately absent from generated Core after a terminator.
                break;
            };
            let declared = match &statement.kind {
                HirStatementKind::Declaration { local, .. } => Some(*local),
                _ => None,
            };
            continuation = self.lower_statement(block_id, environment, statement)?;
            if let Some(local) = declared {
                scope_locals.push(local);
            }
        }
        if continuation.is_some() {
            for local in scope_locals {
                environment.deactivate(local);
            }
        }
        Ok(continuation)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "statement lowering keeps one arm per HIR statement kind in a single dispatch"
    )]
    fn lower_statement(
        &mut self,
        mut block: BlockId,
        environment: &mut Environment,
        statement: &HirStatement,
    ) -> Result<Option<BlockId>, CoreGenerationFailure> {
        self.switch_to(block)?;
        match &statement.kind {
            HirStatementKind::Declaration { local, initializer } => {
                let value = initializer
                    .as_ref()
                    .map(|initializer| self.lower_expression(&mut block, environment, initializer))
                    .transpose()?;
                environment.activate(self.function.id, *local, value)?;
                Ok(Some(block))
            }
            HirStatementKind::Assignment { target, value } => {
                let value = self.lower_expression(&mut block, environment, value)?;
                environment.assign(self.function.id, *target, value)?;
                Ok(Some(block))
            }
            HirStatementKind::Call(call) => {
                let _ = self.lower_call(&mut block, environment, call)?;
                Ok(Some(block))
            }
            HirStatementKind::External(external) => {
                let _ =
                    self.lower_external(*external, &mut block, environment, statement.origin)?;
                Ok(Some(block))
            }
            HirStatementKind::If(conditional) => self.lower_if(block, environment, conditional),
            HirStatementKind::Switch(switch) => {
                self.lower_switch_statement(block, environment, switch)
            }
            HirStatementKind::While(statement) => self.lower_while(block, environment, statement),
            HirStatementKind::Break => {
                self.lower_loop_control(block, environment, true, statement.origin)
            }
            HirStatementKind::Continue => {
                self.lower_loop_control(block, environment, false, statement.origin)
            }
            HirStatementKind::Run(run) => {
                self.lower_run(run, statement.origin)?;
                Ok(Some(block))
            }
            HirStatementKind::Schedule(schedule) => {
                self.switch_to(block)?;
                let core_callee = self.links.functions.function(schedule.callee).ok_or(
                    CoreGenerationFailure::Invariant(CoreGenerationInvariant::MissingFunctionMapping {
                        source_function: schedule.callee,
                    }),
                )?;
                self.builder
                    .schedule(core_callee, schedule.delay_ticks, schedule.mode, schedule.origin)
                    .map_err(|error| self.construction(error))?;
                Ok(Some(block))
            }
            HirStatementKind::ScheduleClear(clear) => {
                self.switch_to(block)?;
                let core_callee = self.links.functions.function(clear.callee).ok_or(
                    CoreGenerationFailure::Invariant(CoreGenerationInvariant::MissingFunctionMapping {
                        source_function: clear.callee,
                    }),
                )?;
                self.builder
                    .schedule_clear(core_callee, clear.origin)
                    .map_err(|error| self.construction(error))?;
                Ok(Some(block))
            }
            HirStatementKind::Return(value) => {
                let values = value
                    .as_ref()
                    .map(|value| self.lower_expression(&mut block, environment, value))
                    .transpose()?
                    .map_or_else(Vec::new, ValueBundle::into_values);
                self.switch_to(block)?;
                self.terminate(TerminatorKind::Return(values), statement.origin)?;
                Ok(None)
            }
            HirStatementKind::Destructure { operand, targets } => {
                let value = self.lower_expression(&mut block, environment, operand)?;
                let ValueType::AnonymousStruct(struct_) = operand.ty else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateProjection {
                            source_function: self.function.id,
                        },
                    ));
                };
                let declaration = self.checked.anonymous_struct(struct_).ok_or(
                    CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType {
                            struct_: SourceStructId::from_index(0).expect("zero fits"),
                        },
                    ),
                )?;
                let components: &[ValueType] = match &declaration.kind {
                    crate::frontend::hir::HirAnonymousStructKind::Named(_) => {
                        return Err(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateProjection {
                                source_function: self.function.id,
                            },
                        ));
                    }
                    crate::frontend::hir::HirAnonymousStructKind::Positional(fields) => fields,
                };
                for target in targets {
                    let component_index = usize::try_from(target.component).unwrap_or(usize::MAX);
                    let Some(component_ty) = components.get(component_index) else {
                        return Err(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType {
                                struct_: SourceStructId::from_index(0).expect("zero fits"),
                            },
                        ));
                    };
                    let start =
                        components[..component_index]
                            .iter()
                            .try_fold(0usize, |offset, ty| {
                                Ok::<_, CoreGenerationFailure>(
                                    offset.saturating_add(
                                        flattened_core_types(self.checked, *ty)?.len(),
                                    ),
                                )
                            })?;
                    let len = flattened_core_types(self.checked, *component_ty)?.len();
                    let end = start.saturating_add(len);
                    let component_values =
                        value
                            .0
                            .get(start..end)
                            .ok_or(CoreGenerationFailure::Invariant(
                                CoreGenerationInvariant::InvalidAggregateType {
                                    struct_: SourceStructId::from_index(0).expect("zero fits"),
                                },
                            ))?;
                    let bundle = ValueBundle(component_values.to_vec().into_boxed_slice());
                    if let Some(local) = target.local {
                        match target.role {
                            HirDestructureTargetRole::Assign => {
                                environment.assign(self.function.id, local, bundle)?;
                            }
                            HirDestructureTargetRole::Const | HirDestructureTargetRole::Var => {
                                environment.activate(self.function.id, local, Some(bundle))?;
                            }
                            HirDestructureTargetRole::Discard => {}
                        }
                    }
                }
                Ok(Some(block))
            }
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "loop CFG construction is kept atomic so parameter inventories are frozen before edges"
    )]
    fn lower_while(
        &mut self,
        incoming_block: BlockId,
        environment: &mut Environment,
        statement: &HirWhile,
    ) -> Result<Option<BlockId>, CoreGenerationFailure> {
        let carried = environment.active_assigned_indices();
        let header = self.create_block(statement.origin)?;
        let exit = self.create_block(statement.origin)?;
        let mut incoming_arguments = vec![];
        let mut header_parameters = Vec::with_capacity(carried.len());
        let mut exit_parameters = Vec::with_capacity(carried.len());

        for &index in &carried {
            let binding =
                self.function
                    .bindings
                    .get(index)
                    .ok_or(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::MissingBinding {
                            source_function: self.function.id,
                            local: u32::try_from(index).unwrap_or(u32::MAX),
                        },
                    ))?;
            let state = environment
                .state(index)
                .ok_or(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::MissingBinding {
                        source_function: self.function.id,
                        local: binding.id.index(),
                    },
                ))?;
            incoming_arguments.extend(
                state
                    .value
                    .ok_or(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::UnassignedBinding {
                            source_function: self.function.id,
                            local: binding.id.index(),
                        },
                    ))?
                    .into_values(),
            );
            let leaf_types = flattened_core_types(self.checked, binding.ty)?;
            let mut header_values = Vec::with_capacity(leaf_types.len());
            let mut exit_values = Vec::with_capacity(leaf_types.len());
            for ty in leaf_types {
                header_values.push(self.append_block_parameter(header, ty, statement.origin)?);
                exit_values.push(self.append_block_parameter(exit, ty, statement.origin)?);
            }
            header_parameters.push(ValueBundle(header_values.into_boxed_slice()));
            exit_parameters.push(ValueBundle(exit_values.into_boxed_slice()));
        }

        self.switch_to(incoming_block)?;
        self.terminate(
            TerminatorKind::Jump(BlockTarget::new(header, incoming_arguments)),
            statement.origin,
        )?;
        for ((&index, parameter), _) in carried.iter().zip(&header_parameters).zip(&exit_parameters)
        {
            environment.write(
                index,
                BindingState {
                    value: Some(parameter.clone()),
                    active: true,
                },
            );
        }
        let header_environment = environment.checkpoint();

        let mut condition_block = header;
        let condition = self
            .lower_expression(&mut condition_block, environment, &statement.condition)?
            .scalar_value(self.function.id)?;
        let body = self.create_block(statement.origin)?;
        let false_arguments = self.loop_arguments(environment, &carried)?;
        self.switch_to(condition_block)?;
        self.terminate(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(body, vec![]),
                else_target: BlockTarget::new(exit, false_arguments),
            },
            statement.origin,
        )?;

        self.loops.push(LoopLoweringContext {
            header,
            exit,
            carried: carried.clone(),
        });
        let body_continuation = self.lower_block(body, environment, &statement.body);
        let loop_context = self.loops.pop().expect("loop context was pushed");
        let body_continuation = body_continuation?;
        if let Some(block) = body_continuation {
            let arguments = self.loop_arguments(environment, &loop_context.carried)?;
            self.switch_to(block)?;
            self.terminate(
                TerminatorKind::Jump(BlockTarget::new(loop_context.header, arguments)),
                statement.origin,
            )?;
        }

        environment.rollback(header_environment);
        for ((&index, parameter), _) in carried.iter().zip(&exit_parameters).zip(&header_parameters)
        {
            environment.write(
                index,
                BindingState {
                    value: Some(parameter.clone()),
                    active: true,
                },
            );
        }
        self.switch_to(exit)?;
        Ok(Some(exit))
    }

    fn lower_loop_control(
        &mut self,
        block: BlockId,
        environment: &Environment,
        is_break: bool,
        origin: OriginId,
    ) -> Result<Option<BlockId>, CoreGenerationFailure> {
        let context = self
            .loops
            .last()
            .cloned()
            .ok_or(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::LoopControlOutsideLoop {
                    source_function: self.function.id,
                },
            ))?;
        let arguments = self.loop_arguments(environment, &context.carried)?;
        let target = if is_break {
            context.exit
        } else {
            context.header
        };
        self.switch_to(block)?;
        self.terminate(
            TerminatorKind::Jump(BlockTarget::new(target, arguments)),
            origin,
        )?;
        Ok(None)
    }

    fn loop_arguments(
        &self,
        environment: &Environment,
        carried: &[usize],
    ) -> Result<Vec<ValueId>, CoreGenerationFailure> {
        let mut arguments = vec![];
        for &index in carried {
            let local = u32::try_from(index).unwrap_or(u32::MAX);
            let value = environment
                .state(index)
                .and_then(|state| state.value)
                .ok_or(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::UnassignedBinding {
                        source_function: self.function.id,
                        local,
                    },
                ))?;
            arguments.extend(value.into_values());
        }
        Ok(arguments)
    }

    fn lower_external(
        &mut self,
        external: SourceExternalOpId,
        block: &mut BlockId,
        environment: &mut Environment,
        origin: OriginId,
    ) -> Result<Vec<ValueId>, CoreGenerationFailure> {
        let operation =
            self.links
                .externals
                .operation(external)
                .ok_or(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::MissingExternalMapping {
                        source_external: external,
                    },
                ))?;
        let sem = self.checked.external_op(external).map(|op| &op.semantic);
        let operands = match sem {
            Some(HirExternalSemantic::EntityNbtRead { segments, .. }) => {
                self.lower_runtime_entity_nbt_segments(block, environment, segments)?
            }
            Some(HirExternalSemantic::EntityNbtWrite {
                segments,
                item_id,
                count,
                ..
            }) => {
                let mut operands =
                    self.lower_runtime_entity_nbt_segments(block, environment, segments)?;
                if !matches!(
                    item_id.kind,
                    HirExpressionKind::String {
                        op: HirStringOp::Constant(_),
                        ..
                    }
                ) {
                    let bundle = self.lower_expression(block, environment, item_id)?;
                    operands.extend(bundle.into_values());
                }
                if !matches!(count.kind, HirExpressionKind::Int32(_)) {
                    let bundle = self.lower_expression(block, environment, count)?;
                    operands.extend(bundle.into_values());
                }
                operands
            }
            _ => vec![],
        };
        let (instruction, results) = self
            .builder
            .external_with_identity(operation, operands, origin)
            .map_err(|error| self.construction(error))?;
        if matches!(
            sem,
            Some(
                HirExternalSemantic::MinecraftOperation { .. }
                    | HirExternalSemantic::EntityNbtRead { .. }
                    | HirExternalSemantic::EntityNbtWrite { .. }
            )
        ) {
            self.record_semantic_operation(external, operation, instruction)?;
        }
        Ok(results)
    }

    /// Lowers every runtime `Index`/`Match` segment value into real SSA
    /// operands, in path order — a compile-time-literal segment contributes
    /// no operand. Shared by `EntityNbtRead`/`EntityNbtWrite` (PS-16).
    fn lower_runtime_entity_nbt_segments(
        &mut self,
        block: &mut BlockId,
        environment: &mut Environment,
        segments: &[HirEntityPathSegment],
    ) -> Result<Vec<ValueId>, CoreGenerationFailure> {
        let mut operands = Vec::new();
        for segment in segments {
            let (HirEntityPathSegment::Index(index)
            | HirEntityPathSegment::Match { value: index, .. }) = segment
            else {
                continue;
            };
            if matches!(index.kind, HirExpressionKind::Int32(_)) {
                continue;
            }
            let bundle = self.lower_expression(block, environment, index)?;
            operands.extend(bundle.into_values());
        }
        Ok(operands)
    }

    fn record_semantic_operation(
        &mut self,
        external: SourceExternalOpId,
        operation: ExternalOpId,
        instruction: InstId,
    ) -> Result<(), CoreGenerationFailure> {
        let invalid = || {
            CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::InvalidSemanticOperationCorrelation {
                    source_external: external,
                },
            )
        };
        let slot = self
            .semantic_operations
            .get_mut(external.as_usize().ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        if slot
            .replace(SourceSemanticCoreOccurrence::new(
                operation,
                self.core_function,
                instruction,
            ))
            .is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn lower_run(&mut self, run: &HirRun, origin: OriginId) -> Result<(), CoreGenerationFailure> {
        let link = self
            .links
            .runs
            .link(run.id)
            .ok_or(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::MissingRunMapping { source_run: run.id },
            ))?;
        let results = if let Some(operation) = link.operation {
            self.builder
                .external(operation, vec![], origin)
                .map_err(|error| self.construction(error))?
        } else {
            self.builder
                .call(link.function, vec![], origin)
                .map_err(|error| self.construction(error))?
        };
        if !results.is_empty() {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::InvalidRunScope { source_run: run.id },
            ));
        }
        Ok(())
    }

    fn lower_if(
        &mut self,
        block: BlockId,
        environment: &mut Environment,
        conditional: &HirIf,
    ) -> Result<Option<BlockId>, CoreGenerationFailure> {
        if conditional.arms.is_empty() {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::EmptyConditional {
                    source_function: self.function.id,
                },
            ));
        }

        let incoming = environment.checkpoint();
        let mut condition_block = block;
        let mut continuing = vec![];
        for arm in &conditional.arms {
            self.switch_to(condition_block)?;
            let condition = self
                .lower_expression(&mut condition_block, environment, &arm.condition)?
                .scalar_value(self.function.id)?;
            let body_block = self.create_block(conditional.origin)?;
            let next_condition = self.create_block(conditional.origin)?;
            self.switch_to(condition_block)?;
            self.terminate(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(body_block, vec![]),
                    else_target: BlockTarget::new(next_condition, vec![]),
                },
                conditional.origin,
            )?;

            if let Some(block) = self.lower_block(body_block, environment, &arm.body)? {
                continuing.push(BranchContinuation {
                    block,
                    overrides: environment.overrides_since(incoming),
                });
            }
            environment.rollback(incoming);
            condition_block = next_condition;
        }

        if let Some(else_body) = &conditional.else_body {
            if let Some(block) = self.lower_block(condition_block, environment, else_body)? {
                continuing.push(BranchContinuation {
                    block,
                    overrides: environment.overrides_since(incoming),
                });
            }
            environment.rollback(incoming);
        } else {
            continuing.push(BranchContinuation {
                block: condition_block,
                overrides: vec![],
            });
        }

        if continuing.is_empty() {
            return Ok(None);
        }
        self.merge_paths(continuing, environment, conditional.origin)
            .map(Some)
    }

    fn lower_switch_statement(
        &mut self,
        block: BlockId,
        environment: &mut Environment,
        switch: &HirSwitchStatement,
    ) -> Result<Option<BlockId>, CoreGenerationFailure> {
        if switch.arms.is_empty() {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::EmptyConditional {
                    source_function: self.function.id,
                },
            ));
        }
        let incoming = environment.checkpoint();
        let mut dispatch = block;
        let scrutinee = self
            .lower_expression(&mut dispatch, environment, &switch.scrutinee)?
            .scalar_value(self.function.id)?;
        let mut continuing = vec![];
        let final_index = switch.arms.len() - 1;
        for (index, arm) in switch.arms.iter().enumerate() {
            let body_block = if index == final_index {
                dispatch
            } else {
                let body = self.create_block(arm.origin)?;
                self.lower_switch_tests(&mut dispatch, scrutinee, &arm.label, body, arm.origin)?;
                body
            };
            if let Some(block) = self.lower_block(body_block, environment, &arm.body)? {
                continuing.push(BranchContinuation {
                    block,
                    overrides: environment.overrides_since(incoming),
                });
            }
            environment.rollback(incoming);
        }
        if continuing.is_empty() {
            return Ok(None);
        }
        self.merge_paths(continuing, environment, switch.origin)
            .map(Some)
    }

    fn lower_switch_tests(
        &mut self,
        dispatch: &mut BlockId,
        scrutinee: ValueId,
        label: &HirSwitchLabel,
        body: BlockId,
        origin: OriginId,
    ) -> Result<(), CoreGenerationFailure> {
        let HirSwitchLabel::Patterns(patterns) = label else {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::EmptyConditional {
                    source_function: self.function.id,
                },
            ));
        };
        for pattern in patterns {
            self.switch_to(*dispatch)?;
            let (min, max) = match pattern.kind {
                HirSwitchPatternKind::IntRange { min, max } => (min, max),
                HirSwitchPatternKind::EnumVariant { variant, .. } => {
                    let tag = i32::try_from(variant.index()).map_err(|_| {
                        CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::EmptyConditional {
                                source_function: self.function.id,
                            },
                        )
                    })?;
                    (tag, tag)
                }
            };
            let range = I32ClosedRange::new(min, max).ok_or(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::EmptyConditional {
                    source_function: self.function.id,
                },
            ))?;
            let condition = self
                .builder
                .i32_in_closed_range(scrutinee, range, pattern.origin)
                .map_err(|error| self.construction(error))?;
            let next = self.create_block(origin)?;
            self.terminate(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(body, vec![]),
                    else_target: BlockTarget::new(next, vec![]),
                },
                pattern.origin,
            )?;
            *dispatch = next;
        }
        Ok(())
    }

    fn lower_switch_expression(
        &mut self,
        block: &mut BlockId,
        environment: &mut Environment,
        switch: &HirSwitchExpression,
    ) -> Result<ValueBundle, CoreGenerationFailure> {
        if switch.arms.is_empty() {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::EmptyConditional {
                    source_function: self.function.id,
                },
            ));
        }
        let scrutinee = self
            .lower_expression(block, environment, &switch.scrutinee)?
            .scalar_value(self.function.id)?;
        let mut dispatch = *block;
        let final_index = switch.arms.len() - 1;
        let mut paths = vec![];
        for (index, arm) in switch.arms.iter().enumerate() {
            let body_block = if index == final_index {
                dispatch
            } else {
                let body = self.create_block(arm.origin)?;
                self.lower_switch_tests(&mut dispatch, scrutinee, &arm.label, body, arm.origin)?;
                body
            };
            let mut body_end = body_block;
            let value = self.lower_expression(&mut body_end, environment, &arm.body)?;
            paths.push((body_end, value));
        }
        let types = flattened_core_types(self.checked, switch.arms[0].body.ty)?;
        self.budget
            .charge_join_edge_operands(self.function.id, types.len(), paths.len())?;
        let join = self.create_block(switch.origin)?;
        let mut parameters = vec![];
        for ty in types {
            parameters.push(self.append_block_parameter(join, ty, switch.origin)?);
        }
        for (path, values) in paths {
            self.switch_to(path)?;
            self.terminate(
                TerminatorKind::Jump(BlockTarget::new(join, values.into_values())),
                switch.origin,
            )?;
        }
        self.switch_to(join)?;
        *block = join;
        Ok(ValueBundle(parameters.into_boxed_slice()))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "sparse SSA merge planning freezes aggregate leaf parameters before installing edges"
    )]
    fn merge_paths(
        &mut self,
        paths: Vec<BranchContinuation>,
        environment: &mut Environment,
        origin: OriginId,
    ) -> Result<BlockId, CoreGenerationFailure> {
        if paths.is_empty() {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::EmptyContinuationMerge {
                    source_function: self.function.id,
                },
            ));
        }
        let join = self.create_block(origin)?;
        let predecessor_count = paths.len();
        let mut aggregates = BTreeMap::<usize, MergeAggregate>::new();
        for path in &paths {
            for entry in &path.overrides {
                note_candidate_state_visits(1);
                aggregates
                    .entry(entry.index)
                    .and_modify(|aggregate| aggregate.include_override(&entry.state))
                    .or_insert_with(|| MergeAggregate::new(&entry.state));
            }
        }
        note_merge_candidate_visits(aggregates.len());

        // Determine the complete sparse merge shape before mutating the builder
        // or the incoming environment. Only bindings changed by at least one
        // predecessor can differ at the join.
        let mut candidates = Vec::with_capacity(aggregates.len());
        for (index, mut aggregate) in aggregates {
            let incoming = environment.state(index).ok_or_else(|| {
                CoreGenerationFailure::Invariant(CoreGenerationInvariant::MissingBinding {
                    source_function: self.function.id,
                    local: u32::try_from(index).unwrap_or(u32::MAX),
                })
            })?;
            debug_assert!(aggregate.override_count <= predecessor_count);
            if aggregate.override_count < predecessor_count {
                note_candidate_state_visits(1);
                aggregate.include_missing_incoming(&incoming);
            }
            candidates.push(aggregate.candidate(index));
        }

        // This is the single point where the complete phi count and predecessor
        // count are both known. A whole-compilation join-edge budget can be checked
        // here before either parameter or edge-argument storage is allocated.
        let phi_count = candidates
            .iter()
            .filter(|candidate| candidate.needs_parameter)
            .try_fold(0usize, |count, candidate| {
                let binding = self.function.bindings.get(candidate.index).ok_or(
                    CoreGenerationFailure::Invariant(CoreGenerationInvariant::MissingBinding {
                        source_function: self.function.id,
                        local: u32::try_from(candidate.index).unwrap_or(u32::MAX),
                    }),
                )?;
                Ok::<_, CoreGenerationFailure>(
                    count.saturating_add(flattened_core_types(self.checked, binding.ty)?.len()),
                )
            })?;
        self.budget
            .charge_join_edge_operands(self.function.id, phi_count, predecessor_count)?;
        let mut carried = Vec::with_capacity(phi_count);

        // Freeze the complete join-parameter inventory first. Only then are any
        // predecessor terminators installed, so edge argument arity cannot observe
        // a partially constructed destination signature.
        for candidate in &mut candidates {
            if candidate.needs_parameter {
                let Some(binding) = self.function.bindings.get(candidate.index) else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::MissingBinding {
                            source_function: self.function.id,
                            local: u32::try_from(candidate.index).unwrap_or(u32::MAX),
                        },
                    ));
                };
                let types = flattened_core_types(self.checked, binding.ty)?;
                let mut parameters = Vec::with_capacity(types.len());
                for ty in types {
                    parameters.push(self.append_block_parameter(join, ty, origin)?);
                }
                candidate.merged.value = Some(ValueBundle(parameters.into_boxed_slice()));
                carried.push(candidate.index);
            }
        }
        debug_assert_eq!(paths.len(), predecessor_count);
        for path in paths {
            let mut arguments = Vec::with_capacity(phi_count);
            for index in &carried {
                let incoming = environment.state(*index).ok_or_else(|| {
                    CoreGenerationFailure::Invariant(CoreGenerationInvariant::MissingBinding {
                        source_function: self.function.id,
                        local: u32::try_from(*index).unwrap_or(u32::MAX),
                    })
                })?;
                let value =
                    path.state(*index, incoming)
                        .value
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::UnassignedBinding {
                                source_function: self.function.id,
                                local: u32::try_from(*index).unwrap_or(u32::MAX),
                            },
                        ))?;
                arguments.extend(value.into_values());
            }
            self.switch_to(path.block)?;
            self.terminate(
                TerminatorKind::Jump(BlockTarget::new(join, arguments)),
                origin,
            )?;
        }
        for candidate in candidates {
            environment.write(candidate.index, candidate.merged);
        }
        self.switch_to(join)?;
        Ok(join)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "1:N expression conversion validates the complete closed HIR expression vocabulary"
    )]
    fn lower_expression(
        &mut self,
        block: &mut BlockId,
        environment: &mut Environment,
        expression: &HirExpression,
    ) -> Result<ValueBundle, CoreGenerationFailure> {
        self.switch_to(*block)?;
        let value = match &expression.kind {
            HirExpressionKind::Bool(value) => ValueBundle::scalar(
                self.builder
                    .bool_constant(*value, expression.origin)
                    .map_err(|error| self.construction(error))?,
            ),
            HirExpressionKind::Int32(value) => ValueBundle::scalar(
                self.builder
                    .i32_constant(*value, expression.origin)
                    .map_err(|error| self.construction(error))?,
            ),
            HirExpressionKind::EnumVariant { variant, .. } => ValueBundle::scalar(
                self.builder
                    .i32_constant(
                        i32::try_from(variant.index()).map_err(|_| {
                            CoreGenerationFailure::Invariant(
                                CoreGenerationInvariant::ExpressionArity {
                                    source_function: self.function.id,
                                    expected: 1,
                                    actual: 0,
                                },
                            )
                        })?,
                        expression.origin,
                    )
                    .map_err(|error| self.construction(error))?,
            ),
            HirExpressionKind::Local(local) => environment.read(self.function.id, *local)?,
            HirExpressionKind::Call(call) => ValueBundle(
                self.lower_call(block, environment, call)?
                    .into_boxed_slice(),
            ),
            HirExpressionKind::External(external) => ValueBundle(
                self.lower_external(*external, block, environment, expression.origin)?
                    .into_boxed_slice(),
            ),
            HirExpressionKind::StructConstruct { struct_, fields } => {
                let declaration =
                    self.checked
                        .struct_(*struct_)
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType { struct_: *struct_ },
                        ))?;
                let mut lowered = vec![None; declaration.fields.len()];
                for field in fields {
                    let index = usize::try_from(field.field).unwrap_or(usize::MAX);
                    let Some(slot) = lowered.get_mut(index) else {
                        return Err(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType { struct_: *struct_ },
                        ));
                    };
                    *slot = Some(self.lower_expression(block, environment, &field.value)?);
                }
                let mut values = vec![];
                for field in lowered {
                    values.extend(
                        field
                            .ok_or(CoreGenerationFailure::Invariant(
                                CoreGenerationInvariant::InvalidAggregateType { struct_: *struct_ },
                            ))?
                            .into_values(),
                    );
                }
                ValueBundle(values.into_boxed_slice())
            }
            HirExpressionKind::StructProject { aggregate, field } => {
                let ValueType::Struct(struct_) = aggregate.ty else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateProjection {
                            source_function: self.function.id,
                        },
                    ));
                };
                let declaration =
                    self.checked
                        .struct_(struct_)
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType { struct_ },
                        ))?;
                let field_index = usize::try_from(*field).unwrap_or(usize::MAX);
                let Some(field_declaration) = declaration.fields.get(field_index) else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType { struct_ },
                    ));
                };
                let start =
                    declaration.fields[..field_index].iter().try_fold(
                        0usize,
                        |offset, field| {
                            Ok::<_, CoreGenerationFailure>(offset.saturating_add(
                                flattened_core_types(self.checked, field.ty)?.len(),
                            ))
                        },
                    )?;
                let end = start.saturating_add(
                    flattened_core_types(self.checked, field_declaration.ty)?.len(),
                );
                let aggregate = self.lower_expression(block, environment, aggregate)?;
                let values =
                    aggregate
                        .0
                        .get(start..end)
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType { struct_ },
                        ))?;
                ValueBundle(values.to_vec().into_boxed_slice())
            }
            HirExpressionKind::ListI32 { op, operands } => {
                let mut lowered = Vec::with_capacity(operands.len());
                for operand in operands {
                    lowered.push(
                        self.lower_expression(block, environment, operand)?
                            .scalar_value(self.function.id)?,
                    );
                }
                let result = match (op, lowered.as_slice()) {
                    (HirListI32Op::Empty, []) => self.builder.list_i32_empty(expression.origin),
                    (HirListI32Op::Length, [list]) => {
                        self.builder.list_i32_length(*list, expression.origin)
                    }
                    (HirListI32Op::Push, [list, element]) => {
                        self.builder
                            .list_i32_push(*list, *element, expression.origin)
                    }
                    (HirListI32Op::LastOrZero, [list]) => {
                        self.builder.list_i32_last_or_zero(*list, expression.origin)
                    }
                    (HirListI32Op::WithoutLast, [list]) => {
                        self.builder.list_i32_without_last(*list, expression.origin)
                    }
                    _ => Err(BuildError::InvalidOperationContract),
                }
                .map_err(|error| self.construction(error))?;
                ValueBundle::scalar(result)
            }
            HirExpressionKind::String { op, operands } => {
                let mut lowered = Vec::with_capacity(operands.len());
                for operand in operands {
                    lowered.push(
                        self.lower_expression(block, environment, operand)?
                            .scalar_value(self.function.id)?,
                    );
                }
                let result = match (op, lowered.as_slice()) {
                    (HirStringOp::Constant(value), []) => self
                        .builder
                        .string_constant(value.clone(), expression.origin),
                    (HirStringOp::Length, [value]) => {
                        self.builder.string_length(*value, expression.origin)
                    }
                    (HirStringOp::EndsWithAscii(ascii), [value]) => self
                        .builder
                        .string_ends_with_ascii(*value, *ascii, expression.origin),
                    (HirStringOp::WithoutLastUnit, [value]) => self
                        .builder
                        .string_without_last_unit(*value, expression.origin),
                    _ => Err(BuildError::InvalidOperationContract),
                }
                .map_err(|error| self.construction(error))?;
                ValueBundle::scalar(result)
            }
            HirExpressionKind::Not(operand) => {
                let operand = self
                    .lower_expression(block, environment, operand)?
                    .scalar_value(self.function.id)?;
                ValueBundle::scalar(
                    self.builder
                        .bool_not(operand, expression.origin)
                        .map_err(|error| self.construction(error))?,
                )
            }
            HirExpressionKind::WrappingArithmetic { op, left, right } => {
                let left = self
                    .lower_expression(block, environment, left)?
                    .scalar_value(self.function.id)?;
                let right = self
                    .lower_expression(block, environment, right)?
                    .scalar_value(self.function.id)?;
                let result = match op {
                    HirWrappingArithmeticOp::Add => {
                        self.builder
                            .i32_add_wrapping(left, right, expression.origin)
                    }
                    HirWrappingArithmeticOp::Subtract => {
                        self.builder
                            .i32_sub_wrapping(left, right, expression.origin)
                    }
                }
                .map_err(|error| self.construction(error))?;
                ValueBundle::scalar(result)
            }
            HirExpressionKind::Compare { op, left, right } => {
                let left_value = self
                    .lower_expression(block, environment, left)?
                    .scalar_value(self.function.id)?;
                let right_value = self
                    .lower_expression(block, environment, right)?
                    .scalar_value(self.function.id)?;
                let result = if left.ty == ValueType::Bool {
                    self.lower_bool_comparison(
                        block,
                        *op,
                        left_value,
                        right_value,
                        expression.origin,
                    )?
                } else {
                    self.builder
                        .i32_compare(
                            comparison_predicate(*op),
                            left_value,
                            right_value,
                            expression.origin,
                        )
                        .map_err(|error| self.construction(error))?
                };
                ValueBundle::scalar(result)
            }
            HirExpressionKind::AnonymousStructConstruct { struct_, fields } => {
                let declaration = self.checked.anonymous_struct(*struct_).ok_or(
                    CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType {
                            struct_: SourceStructId::from_index(0).expect("zero fits"),
                        },
                    ),
                )?;
                let field_count = match &declaration.kind {
                    crate::frontend::hir::HirAnonymousStructKind::Named(fields) => fields.len(),
                    crate::frontend::hir::HirAnonymousStructKind::Positional(fields) => {
                        fields.len()
                    }
                };
                let mut lowered = vec![None; field_count];
                for field in fields {
                    let index = usize::try_from(field.field).unwrap_or(usize::MAX);
                    let Some(slot) = lowered.get_mut(index) else {
                        return Err(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType {
                                struct_: SourceStructId::from_index(0).expect("zero fits"),
                            },
                        ));
                    };
                    *slot = Some(self.lower_expression(block, environment, &field.value)?);
                }
                let mut values = vec![];
                for field in lowered {
                    values.extend(
                        field
                            .ok_or(CoreGenerationFailure::Invariant(
                                CoreGenerationInvariant::InvalidAggregateType {
                                    struct_: SourceStructId::from_index(0).expect("zero fits"),
                                },
                            ))?
                            .into_values(),
                    );
                }
                ValueBundle(values.into_boxed_slice())
            }
            HirExpressionKind::AnonymousStructProject { aggregate, field } => {
                let ValueType::AnonymousStruct(struct_) = aggregate.ty else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateProjection {
                            source_function: self.function.id,
                        },
                    ));
                };
                let declaration = self.checked.anonymous_struct(struct_).ok_or(
                    CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType {
                            struct_: SourceStructId::from_index(0).expect("zero fits"),
                        },
                    ),
                )?;
                let field_index = usize::try_from(*field).unwrap_or(usize::MAX);
                let components: &[ValueType] = match &declaration.kind {
                    crate::frontend::hir::HirAnonymousStructKind::Named(fields) => {
                        let types: Vec<ValueType> = fields.iter().map(|f| f.ty).collect();
                        types.leak()
                    }
                    crate::frontend::hir::HirAnonymousStructKind::Positional(fields) => fields,
                };
                let Some(component_ty) = components.get(field_index) else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType {
                            struct_: SourceStructId::from_index(0).expect("zero fits"),
                        },
                    ));
                };
                let start = components[..field_index]
                    .iter()
                    .try_fold(0usize, |offset, ty| {
                        Ok::<_, CoreGenerationFailure>(
                            offset.saturating_add(flattened_core_types(self.checked, *ty)?.len()),
                        )
                    })?;
                let end =
                    start.saturating_add(flattened_core_types(self.checked, *component_ty)?.len());
                let aggregate = self.lower_expression(block, environment, aggregate)?;
                let values =
                    aggregate
                        .0
                        .get(start..end)
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType {
                                struct_: SourceStructId::from_index(0).expect("zero fits"),
                            },
                        ))?;
                ValueBundle(values.to_vec().into_boxed_slice())
            }
            HirExpressionKind::Index {
                aggregate,
                component,
            } => {
                let ValueType::AnonymousStruct(struct_) = aggregate.ty else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateProjection {
                            source_function: self.function.id,
                        },
                    ));
                };
                let declaration = self.checked.anonymous_struct(struct_).ok_or(
                    CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType {
                            struct_: SourceStructId::from_index(0).expect("zero fits"),
                        },
                    ),
                )?;
                let component_index = usize::try_from(*component).unwrap_or(usize::MAX);
                let components: &[ValueType] = match &declaration.kind {
                    crate::frontend::hir::HirAnonymousStructKind::Named(_) => {
                        return Err(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateProjection {
                                source_function: self.function.id,
                            },
                        ));
                    }
                    crate::frontend::hir::HirAnonymousStructKind::Positional(fields) => fields,
                };
                let Some(component_ty) = components.get(component_index) else {
                    return Err(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::InvalidAggregateType {
                            struct_: SourceStructId::from_index(0).expect("zero fits"),
                        },
                    ));
                };
                let start =
                    components[..component_index]
                        .iter()
                        .try_fold(0usize, |offset, ty| {
                            Ok::<_, CoreGenerationFailure>(
                                offset
                                    .saturating_add(flattened_core_types(self.checked, *ty)?.len()),
                            )
                        })?;
                let end =
                    start.saturating_add(flattened_core_types(self.checked, *component_ty)?.len());
                let aggregate = self.lower_expression(block, environment, aggregate)?;
                let values =
                    aggregate
                        .0
                        .get(start..end)
                        .ok_or(CoreGenerationFailure::Invariant(
                            CoreGenerationInvariant::InvalidAggregateType {
                                struct_: SourceStructId::from_index(0).expect("zero fits"),
                            },
                        ))?;
                ValueBundle(values.to_vec().into_boxed_slice())
            }
            HirExpressionKind::Switch(switch) => {
                self.lower_switch_expression(block, environment, switch)?
            }
        };
        *block = self.builder.insertion_block();
        let expected = flattened_core_types(self.checked, expression.ty)?;
        if value.0.len() != expected.len() {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::ExpressionArity {
                    source_function: self.function.id,
                    expected: expected.len(),
                    actual: value.0.len(),
                },
            ));
        }
        for (&value, expected) in value.0.iter().zip(expected) {
            let actual = self
                .builder
                .body()
                .value(value)
                .map(crate::ir::core::ValueData::ty)
                .ok_or_else(|| self.construction(BuildError::InvalidValue { value }))?;
            if actual != expected {
                return Err(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::ExpressionType {
                        source_function: self.function.id,
                        expected,
                        actual,
                    },
                ));
            }
        }
        Ok(value)
    }

    fn lower_call(
        &mut self,
        block: &mut BlockId,
        environment: &mut Environment,
        call: &HirCall,
    ) -> Result<Vec<ValueId>, CoreGenerationFailure> {
        let mut arguments = vec![];
        for argument in &call.arguments {
            arguments.extend(
                self.lower_expression(block, environment, argument)?
                    .into_values(),
            );
        }
        self.switch_to(*block)?;
        let core_callee =
            self.links
                .functions
                .function(call.callee)
                .ok_or(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::MissingFunctionMapping {
                        source_function: call.callee,
                    },
                ))?;
        let values = self
            .builder
            .call(core_callee, arguments, call.origin)
            .map_err(|error| self.construction(error))?;
        *block = self.builder.insertion_block();
        let expected = self
            .checked
            .functions()
            .get(call.callee.as_usize().unwrap_or(usize::MAX))
            .map(|callee| match callee.result {
                FunctionResult::Void => Ok(0),
                FunctionResult::Value(ty) => {
                    flattened_core_types(self.checked, ty).map(|types| types.len())
                }
            })
            .transpose()?
            .unwrap_or(0);
        if values.len() != expected {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::CallResultCount {
                    source_function: self.function.id,
                    callee: call.callee,
                    expected,
                    actual: values.len(),
                },
            ));
        }
        Ok(values)
    }

    fn lower_bool_comparison(
        &mut self,
        block: &mut BlockId,
        op: HirComparisonOp,
        left: ValueId,
        right: ValueId,
        origin: OriginId,
    ) -> Result<ValueId, CoreGenerationFailure> {
        if !matches!(op, HirComparisonOp::Equal | HirComparisonOp::NotEqual) {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::InvalidBooleanComparison {
                    source_function: self.function.id,
                },
            ));
        }
        let then_block = self.create_block(origin)?;
        let else_block = self.create_block(origin)?;
        let join = self.create_block(origin)?;
        let result = self.append_block_parameter(join, CoreType::Bool, origin)?;

        self.switch_to(*block)?;
        self.terminate(
            TerminatorKind::Branch {
                condition: left,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            origin,
        )?;

        let equal = matches!(op, HirComparisonOp::Equal);
        self.switch_to(then_block)?;
        let then_value = if equal {
            right
        } else {
            self.builder
                .bool_not(right, origin)
                .map_err(|error| self.construction(error))?
        };
        self.terminate(
            TerminatorKind::Jump(BlockTarget::new(join, vec![then_value])),
            origin,
        )?;

        self.switch_to(else_block)?;
        let else_value = if equal {
            self.builder
                .bool_not(right, origin)
                .map_err(|error| self.construction(error))?
        } else {
            right
        };
        self.terminate(
            TerminatorKind::Jump(BlockTarget::new(join, vec![else_value])),
            origin,
        )?;
        self.switch_to(join)?;
        *block = join;
        Ok(result)
    }

    fn construction(&self, error: BuildError) -> CoreGenerationFailure {
        CoreGenerationFailure::Construction {
            source_function: self.function.id,
            error,
        }
    }

    fn create_block(&mut self, origin: OriginId) -> Result<BlockId, CoreGenerationFailure> {
        let source_function = self.function.id;
        self.builder
            .create_block(origin)
            .map_err(|error| CoreGenerationFailure::Construction {
                source_function,
                error,
            })
    }

    fn append_block_parameter(
        &mut self,
        block: BlockId,
        ty: CoreType,
        origin: OriginId,
    ) -> Result<ValueId, CoreGenerationFailure> {
        let source_function = self.function.id;
        self.builder
            .append_block_parameter(block, ty, origin)
            .map_err(|error| CoreGenerationFailure::Construction {
                source_function,
                error,
            })
    }

    fn switch_to(&mut self, block: BlockId) -> Result<(), CoreGenerationFailure> {
        let source_function = self.function.id;
        self.builder
            .switch_to_block(block)
            .map_err(|error| CoreGenerationFailure::Construction {
                source_function,
                error,
            })
    }

    fn terminate(
        &mut self,
        kind: TerminatorKind,
        origin: OriginId,
    ) -> Result<(), CoreGenerationFailure> {
        let source_function = self.function.id;
        self.builder
            .terminate(Terminator::new(kind, origin))
            .map_err(|error| CoreGenerationFailure::Construction {
                source_function,
                error,
            })
    }
}

const fn comparison_predicate(op: HirComparisonOp) -> I32Predicate {
    match op {
        HirComparisonOp::Equal => I32Predicate::Eq,
        HirComparisonOp::NotEqual => I32Predicate::Ne,
        HirComparisonOp::Less => I32Predicate::SignedLt,
        HirComparisonOp::LessEqual => I32Predicate::SignedLe,
        HirComparisonOp::Greater => I32Predicate::SignedGt,
        HirComparisonOp::GreaterEqual => I32Predicate::SignedGe,
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{
        CoreGenerationFailure, CoreGenerationInvariant, CoreGenerationOutput,
        CoreGenerationResource, ExternalToCoreMap, SourceSemanticCoreOccurrence, SourceToCoreMap,
        environment_work_counters, lower_hir, reset_environment_work_counters,
        verify_source_semantic_correlations,
    };
    use crate::entity::EntityId;
    use crate::frontend::FrontendLimits;
    use crate::frontend::check::check;
    use crate::frontend::hir::CheckedFrontendOutput;
    use crate::frontend::lexer::lex;
    use crate::frontend::parser::parse;
    use crate::ir::core::{
        CanonicalPrinter, CoreOp, Criterion, EntityQueryStep, RunModifierInstance, TerminatorKind,
        reset_verifier_counters, verifier_counters,
    };
    use crate::source::SourceContext;

    fn checked(text: &str) -> (SourceContext, CheckedFrontendOutput) {
        let limits = FrontendLimits::DEFAULT;
        let mut sources = SourceContext::new();
        let file = sources.add_file("test.mdl", text).unwrap();
        let lexed = lex(&mut sources, file, limits).unwrap();
        assert_eq!(lexed.diagnostics(), None);
        let parsed = parse(&mut sources, file, lexed.tokens(), limits).unwrap();
        assert_eq!(parsed.diagnostics(), None);
        let (module, _) = parsed.into_parts();
        let checked = check(&mut sources, &module, limits).unwrap();
        assert_eq!(checked.diagnostics(), None);
        let (checked, diagnostics) = checked.into_parts();
        assert_eq!(diagnostics, None);
        (sources, checked.unwrap())
    }

    fn lower(text: &str) -> (SourceContext, CheckedFrontendOutput, CoreGenerationOutput) {
        let (sources, checked) = checked(text);
        let generated =
            lower_hir(&checked, &sources, FrontendLimits::DEFAULT.max_tokens()).unwrap();
        (sources, checked, generated)
    }

    fn print(output: &CoreGenerationOutput, sources: &SourceContext) -> String {
        CanonicalPrinter::new(output.program(), sources)
            .unwrap()
            .render()
    }

    #[test]
    fn predeclares_forward_calls_and_retains_typed_correlation() {
        let text = r"
fn first(value: Int32) -> Bool { return second(value); }
fn second(value: Int32) -> Bool { return !(value >= 2); }
fn invoke() { first(1); }
";
        let (sources, checked, generated) = lower(text);
        let map = generated.source_to_core();
        assert_eq!(map.len(), 3);
        assert!(!map.is_empty());
        let pairs = map.functions().collect::<Vec<_>>();
        assert_eq!(pairs.len(), checked.function_count());
        for (index, (source, core)) in pairs.iter().copied().enumerate() {
            assert_eq!(source.index(), u32::try_from(index).unwrap());
            assert_eq!(core.index(), u32::try_from(index).unwrap());
            assert_eq!(map.function(source), Some(core));
        }
        assert_eq!(
            print(&generated, &sources),
            concat!(
                "func @fn0(%v0: i32) -> (bool) // first {\n",
                "^bb0:\n",
                "  %v1 = core.call @fn1 %v0\n",
                "  return %v1\n",
                "}\n",
                "\n",
                "func @fn1(%v0: i32) -> (bool) // second {\n",
                "^bb0:\n",
                "  %v1 = core.i32.constant 2\n",
                "  %v2 = core.i32.compare sge %v0, %v1\n",
                "  %v3 = core.bool.not %v2\n",
                "  return %v3\n",
                "}\n",
                "\n",
                "func @fn2() // invoke {\n",
                "^bb0:\n",
                "  %v0 = core.i32.constant 1\n",
                "  %v1 = core.call @fn0 %v0\n",
                "  return\n",
                "}\n",
            )
        );
    }

    #[test]
    fn lowers_repeated_as_modifiers_as_one_ordered_core_scope() {
        let source = r#"fn scoped() {
            run.as(mc.entities(ArmorStand).with_tag("first").limit(1))
                .as(mc.entities(ArmorStand).with_tag("second").limit(1)) {}
        }"#;
        let (sources, _, generated) = lower(source);
        let program = generated.program();
        let queries = program.entity_queries().collect::<Vec<_>>();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0].1.semantic().tags()[0].as_str(), "first");
        assert_eq!(queries[1].1.semantic().tags()[0].as_str(), "second");

        let (_, scope) = program.run_scopes().next().unwrap();
        assert_eq!(scope.modifiers().len(), 2);
        let RunModifierInstance::AsEntityQuery { query: first, .. } = scope.modifiers()[0] else {
            panic!("expected as")
        };
        let RunModifierInstance::AsEntityQuery { query: second, .. } = scope.modifiers()[1] else {
            panic!("expected as")
        };
        assert_eq!(first, queries[0].0);
        assert_eq!(second, queries[1].0);
        assert_eq!(scope.invocation_bounds().lower(), 0);
        assert_eq!(scope.invocation_bounds().upper(), Some(1));

        let [
            EntityQueryStep::Entities { kind_origin, .. },
            EntityQueryStep::WithTag { value_origin, .. },
            EntityQueryStep::Limit {
                value_origin: limit_origin,
                ..
            },
        ] = queries[0].1.steps()
        else {
            panic!("expected the exact root/tag/limit occurrence trace");
        };
        for (origin, expected) in [
            (*kind_origin, "ArmorStand"),
            (*value_origin, "\"first\""),
            (*limit_origin, "1"),
        ] {
            let span = sources.resolve_origin_span(origin).unwrap();
            assert_eq!(sources.files().slice(span).unwrap(), expected);
        }
    }

    #[test]
    fn mutable_if_join_uses_local_order_and_complete_edge_arguments() {
        let text = r"
fn choose(condition: Bool, left: Int32, right: Int32) -> Int32 {
    var first: Int32;
    var second: Int32;
    if (condition) { first = left; second = right; }
    else { first = right; second = left; }
    return first;
}
";
        let (sources, _, generated) = lower(text);
        assert_eq!(
            print(&generated, &sources),
            concat!(
                "func @fn0(%v0: bool, %v1: i32, %v2: i32) -> (i32) // choose {\n",
                "^bb0:\n",
                "  branch %v0, ^bb1, ^bb2\n",
                "^bb1():\n",
                "  jump ^bb3(%v1, %v2)\n",
                "^bb2():\n",
                "  jump ^bb3(%v2, %v1)\n",
                "^bb3(%v3: i32, %v4: i32):\n",
                "  return %v3\n",
                "}\n",
            )
        );
    }

    #[test]
    fn joins_only_continuing_predecessors_and_omits_unreachable_source() {
        let text = r"
fn choose(condition: Bool, left: Int32, right: Int32) -> Int32 {
    var selected: Int32;
    if (condition) { return left; } else { selected = right; }
    return selected;
    selected = left;
}
";
        let (sources, _, generated) = lower(text);
        assert_eq!(
            print(&generated, &sources),
            concat!(
                "func @fn0(%v0: bool, %v1: i32, %v2: i32) -> (i32) // choose {\n",
                "^bb0:\n",
                "  branch %v0, ^bb1, ^bb2\n",
                "^bb1():\n",
                "  return %v1\n",
                "^bb2():\n",
                "  jump ^bb3\n",
                "^bb3():\n",
                "  return %v2\n",
                "}\n",
            )
        );
    }

    #[test]
    fn missing_else_carries_the_incoming_environment() {
        let text = r"
fn choose(condition: Bool, left: Int32, right: Int32) -> Int32 {
    var selected: Int32 = left;
    if (condition) { selected = right; }
    return selected;
}
";
        let (sources, _, generated) = lower(text);
        assert_eq!(
            print(&generated, &sources),
            concat!(
                "func @fn0(%v0: bool, %v1: i32, %v2: i32) -> (i32) // choose {\n",
                "^bb0:\n",
                "  branch %v0, ^bb1, ^bb2\n",
                "^bb1():\n",
                "  jump ^bb3(%v2)\n",
                "^bb2():\n",
                "  jump ^bb3(%v1)\n",
                "^bb3(%v3: i32):\n",
                "  return %v3\n",
                "}\n",
            )
        );
    }

    #[test]
    fn branch_local_bindings_are_removed_before_the_outer_join() {
        let text = r"
fn choose(condition: Bool, input: Int32) -> Int32 {
    var result: Int32 = input;
    if (condition) {
        const scratch: Int32 = 1;
        result = scratch;
    } else {
        const scratch: Int32 = 2;
        result = scratch;
    }
    return result;
}
";
        let (sources, _, generated) = lower(text);
        let rendered = print(&generated, &sources);
        assert!(rendered.contains("^bb3(%v4: i32):"));
        assert!(!rendered.contains("^bb3(%v4: i32, "));
        let body = generated
            .program()
            .functions()
            .next()
            .and_then(|(_, function)| function.body())
            .unwrap();
        let join = *body.block_order().last().unwrap();
        assert_eq!(body.block(join).unwrap().parameters().len(), 1);
    }

    #[test]
    fn later_else_if_conditions_remain_control_dependent() {
        let text = r"
fn probe(value: Bool) -> Bool { return value; }
fn choose(first: Bool, second: Bool) -> Int32 {
    if (probe(first)) { return 1; }
    else if (probe(second)) { return 2; }
    else { return 3; }
}
";
        let (_, _, generated) = lower(text);
        let body = generated
            .program()
            .functions()
            .nth(1)
            .and_then(|(_, function)| function.body())
            .unwrap();
        let entry = body.block(body.entry()).unwrap();
        assert_eq!(entry.instructions().len(), 1);
        assert!(matches!(
            body.instruction(entry.instructions()[0]).map(crate::ir::core::InstData::op),
            Some(CoreOp::Call(callee)) if callee.index() == 0
        ));
        let second_condition = body.block(body.block_order()[2]).unwrap();
        assert_eq!(second_condition.instructions().len(), 1);
        assert!(matches!(
            body.instruction(second_condition.instructions()[0])
                .map(crate::ir::core::InstData::op),
            Some(CoreOp::Call(callee)) if callee.index() == 0
        ));
    }

    #[test]
    fn bool_equality_uses_a_typed_cfg_without_reordering_operand_calls() {
        let text = r"
fn identity(value: Bool) -> Bool { return value; }
fn same(left: Bool, right: Bool) -> Bool {
    return identity(left) == identity(right);
}
";
        let (sources, _, generated) = lower(text);
        assert_eq!(
            print(&generated, &sources),
            concat!(
                "func @fn0(%v0: bool) -> (bool) // identity {\n",
                "^bb0:\n",
                "  return %v0\n",
                "}\n",
                "\n",
                "func @fn1(%v0: bool, %v1: bool) -> (bool) // same {\n",
                "^bb0:\n",
                "  %v2 = core.call @fn0 %v0\n",
                "  %v3 = core.call @fn0 %v1\n",
                "  branch %v2, ^bb1, ^bb2\n",
                "^bb1():\n",
                "  jump ^bb3(%v3)\n",
                "^bb2():\n",
                "  %v5 = core.bool.not %v3\n",
                "  jump ^bb3(%v5)\n",
                "^bb3(%v4: bool):\n",
                "  return %v4\n",
                "}\n",
            )
        );
    }

    #[test]
    fn direct_recursion_is_structurally_valid_core() {
        let text = r"
fn again(flag: Bool) -> Bool {
    if (flag) { return again(false); }
    return flag;
}
";
        let (sources, _, generated) = lower(text);
        let declaration = generated.program().functions().next().unwrap().1;
        let body = declaration.body().unwrap();
        assert!(body.block_order().iter().any(|block| {
            body.block(*block).is_some_and(|block| {
                block.instructions().iter().any(|instruction| {
                    matches!(
                        body.instruction(*instruction)
                            .map(crate::ir::core::InstData::op),
                        Some(CoreOp::Call(callee)) if callee.index() == 0
                    )
                })
            })
        }));
        assert!(CanonicalPrinter::new(generated.program(), &sources).is_ok());
    }

    #[test]
    fn verifies_each_body_and_program_and_preserves_every_entity_origin() {
        reset_verifier_counters();
        let (sources, _, generated) =
            lower("fn one(value: Int32) -> Int32 { return value; } fn two() { one(2); }");
        // FunctionBuilder::finish verifies each body once; whole-program
        // verification then verifies both bodies again.
        assert_eq!(verifier_counters(), (4, 1));

        for (_, declaration) in generated.program().functions() {
            assert!(sources.origin(declaration.origin()).is_some());
            let body = declaration.body().unwrap();
            for block in body.block_order() {
                let block = body.block(*block).unwrap();
                assert!(sources.origin(block.origin()).is_some());
                for parameter in block.parameters() {
                    assert!(sources.origin(parameter.origin()).is_some());
                }
                for instruction in block.instructions() {
                    let instruction = body.instruction(*instruction).unwrap();
                    assert!(sources.origin(instruction.origin()).is_some());
                }
                let terminator = block.terminator().unwrap();
                assert!(sources.origin(terminator.origin()).is_some());
                assert!(!matches!(terminator.kind(), TerminatorKind::Unreachable));
            }
        }
    }

    #[test]
    fn represented_source_constructs_keep_their_exact_spans() {
        let text = "fn choose(flag: Bool) -> Int32 { var value: Int32 = 1; if (flag) { value = 2; } return value; }";
        let (sources, _, generated) = lower(text);
        let body = generated
            .program()
            .functions()
            .next()
            .and_then(|(_, function)| function.body())
            .unwrap();
        let spelling = |origin| {
            sources
                .resolve_origin_span(origin)
                .and_then(|span| sources.files().slice(span).ok())
                .unwrap()
        };

        let entry = body.block(body.entry()).unwrap();
        let first_constant = body.instruction(entry.instructions()[0]).unwrap();
        assert_eq!(spelling(first_constant.origin()), "1");
        assert!(spelling(entry.terminator().unwrap().origin()).starts_with("if (flag)"));

        let then_block = body.block(body.block_order()[1]).unwrap();
        let second_constant = body.instruction(then_block.instructions()[0]).unwrap();
        assert_eq!(spelling(second_constant.origin()), "2");
        let join = body.block(*body.block_order().last().unwrap()).unwrap();
        assert!(spelling(join.origin()).starts_with("if (flag)"));
        assert_eq!(
            spelling(join.terminator().unwrap().origin()),
            "return value;"
        );
    }

    #[test]
    fn repeated_lowering_is_byte_deterministic() {
        let text = r"
fn choose(a: Bool, b: Bool, left: Int32, right: Int32) -> Int32 {
    var selected: Int32 = left;
    if (a) { if (b) { selected = right; } }
    else if (b) { selected = right; }
    return selected;
}
";
        let (first_sources, _, first) = lower(text);
        let (second_sources, _, second) = lower(text);
        assert_eq!(
            print(&first, &first_sources),
            print(&second, &second_sources)
        );
        assert_eq!(
            first.source_to_core().functions().collect::<Vec<_>>(),
            second.source_to_core().functions().collect::<Vec<_>>()
        );
    }

    #[test]
    fn typed_source_external_correlations_are_exact_verified_and_deterministic() {
        let text = r#"export fn mixed() {
            unsafe minecraft("say unsafe");
            run.as(mc.entities(ArmorStand).limit(1)) |speaker| {
                speaker.say("first");
                speaker.say("second");
            }
        }"#;
        let (sources, checked, generated) = lower(text);
        let (repeated_sources, _, repeated) = lower(text);
        assert_eq!(generated.source_to_core(), repeated.source_to_core());

        let source_ids = checked.external_operation_ids().collect::<Vec<_>>();
        let [unsafe_source, first_source, second_source] = source_ids.as_slice() else {
            panic!("fixture must contain one unsafe and two typed source externals")
        };
        let map = generated.source_to_core();
        let unsafe_external = map.external_operation(*unsafe_source).unwrap();
        assert_eq!(map.semantic_operation(*unsafe_source), None);
        let first = map.semantic_operation(*first_source).unwrap();
        let second = map.semantic_operation(*second_source).unwrap();
        assert_ne!(first.external(), second.external());
        assert_eq!(first.function(), second.function());
        assert_ne!(first.instruction(), second.instruction());
        let source_function = map
            .function(checked.function_ids().next().unwrap())
            .unwrap();
        assert_ne!(
            first.function(),
            source_function,
            "typed operations inside run scope belong to the outlined Core body"
        );
        let first_instruction = generated
            .program()
            .function(first.function())
            .and_then(|function| function.body())
            .and_then(|body| body.instruction(first.instruction()))
            .unwrap();
        assert_eq!(
            first_instruction.origin(),
            checked.external_op(*first_source).unwrap().origin
        );

        let external_map = ExternalToCoreMap::new(map.external_operations().collect());
        let valid = map.semantic_operations.to_vec();
        assert_eq!(
            verify_source_semantic_correlations(
                &checked,
                generated.program(),
                &external_map,
                &valid,
            ),
            Ok(())
        );
        let invalid_for = |slots: &[Option<SourceSemanticCoreOccurrence>], source| {
            matches!(
                verify_source_semantic_correlations(
                    &checked,
                    generated.program(),
                    &external_map,
                    slots,
                ),
                Err(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::InvalidSemanticOperationCorrelation {
                        source_external,
                    }
                )) if source_external == source
            )
        };

        let mut missing = valid.clone();
        missing[first_source.as_usize().unwrap()] = None;
        assert!(invalid_for(&missing, *first_source));

        let mut wrong_external = valid.clone();
        wrong_external[first_source.as_usize().unwrap()] = Some(SourceSemanticCoreOccurrence::new(
            unsafe_external,
            first.function(),
            first.instruction(),
        ));
        assert!(invalid_for(&wrong_external, *first_source));

        let mut unsafe_slot = valid.clone();
        unsafe_slot[unsafe_source.as_usize().unwrap()] = Some(first);
        assert!(invalid_for(&unsafe_slot, *unsafe_source));

        let mut duplicate = valid;
        duplicate[second_source.as_usize().unwrap()] = Some(first);
        assert!(invalid_for(&duplicate, *second_source));

        assert_eq!(
            print(&generated, &sources),
            print(&repeated, &repeated_sources)
        );
    }

    #[test]
    fn many_live_locals_and_empty_conditionals_do_no_inventory_merge_work() {
        const LIVE_LOCALS: usize = 192;
        const EMPTY_CONDITIONALS: usize = 256;

        let mut text = String::from("fn sparse(flag: Bool) -> Int32 {\n");
        for local in 0..LIVE_LOCALS {
            writeln!(text, "const local_{local}: Int32 = {local};").unwrap();
        }
        for _ in 0..EMPTY_CONDITIONALS {
            text.push_str("if (flag) {}\n");
        }
        text.push_str("return 7;\n}");

        reset_environment_work_counters();
        let (_, _, generated) = lower(&text);
        let (override_entries, merge_candidates, candidate_states) = environment_work_counters();

        // Empty arms have no journal delta, so their cost is independent of the
        // 192 simultaneously live locals. The old snapshot/full-inventory merge
        // performed LIVE_LOCALS * EMPTY_CONDITIONALS state inspections here.
        assert_eq!(override_entries, 0);
        assert_eq!(merge_candidates, 0);
        assert_eq!(candidate_states, 0);
        let body = generated
            .program()
            .functions()
            .next()
            .and_then(|(_, function)| function.body())
            .unwrap();
        assert_eq!(body.block_order().len(), 1 + 3 * EMPTY_CONDITIONALS);
    }

    #[test]
    fn distinct_unassigned_arm_writes_are_classified_from_sparse_overrides() {
        const ARM_COUNT: usize = 192;

        let mut text = String::from("fn sparse_arms(flag: Bool) -> Int32 {\n");
        for local in 0..ARM_COUNT {
            writeln!(text, "var local_{local}: Int32;").unwrap();
        }
        for arm in 0..ARM_COUNT {
            if arm == 0 {
                writeln!(text, "if (flag) {{ local_{arm} = {arm}; }}").unwrap();
            } else {
                writeln!(text, "else if (flag) {{ local_{arm} = {arm}; }}").unwrap();
            }
        }
        text.push_str("else {}\nreturn 7;\n}");

        reset_environment_work_counters();
        let (_, _, generated) = lower(&text);
        let (override_entries, merge_candidates, candidate_states) = environment_work_counters();

        // Each arm changes one distinct live-but-unassigned local. Every merge
        // candidate sees that one override plus one representative incoming
        // state for all other predecessors: O(arms), not O(arms * candidates).
        assert_eq!(override_entries, ARM_COUNT);
        assert_eq!(merge_candidates, ARM_COUNT);
        assert_eq!(candidate_states, 2 * ARM_COUNT);
        let body = generated
            .program()
            .functions()
            .next()
            .and_then(|(_, function)| function.body())
            .unwrap();
        let join = body.block(*body.block_order().last().unwrap()).unwrap();
        assert!(join.parameters().is_empty());
    }

    #[test]
    fn join_edge_operand_budget_is_exact_and_shared_across_functions() {
        let text = r"
fn first(flag: Bool, left: Int32, right: Int32) -> Int32 {
    var result: Int32 = left;
    if (flag) { result = right; }
    return result;
}
fn second(flag: Bool, left: Int32, right: Int32) -> Int32 {
    var result: Int32 = left;
    if (flag) { result = right; }
    return result;
}
";
        let (sources, checked) = checked(text);

        let error = lower_hir(&checked, &sources, 3).unwrap_err();
        assert!(matches!(
            error,
            CoreGenerationFailure::ResourceLimit {
                source_function,
                resource: CoreGenerationResource::JoinEdgeOperands,
                limit: 3,
            } if source_function.index() == 1
        ));
        assert!(lower_hir(&checked, &sources, 4).is_ok());
    }

    #[test]
    fn output_can_be_consumed_without_cloning_the_unoptimized_program() {
        let (_, _, generated) = lower("fn empty() {}");
        let (program, map): (_, SourceToCoreMap) = generated.into_parts();
        assert_eq!(program.len(), 1);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn event_handler_declares_one_advancement_with_a_zero_arity_reward() {
        let text = r#"on inventory_changed(.items = ["minecraft:diamond"]) |player| {
    player.say("MDL_GOT_DIAMOND");
}"#;
        let (_, checked, generated) = lower(text);
        let program = generated.program();
        assert_eq!(program.len(), 1);
        assert_eq!(checked.event_handlers().len(), 1);
        let advancements = program.advancements().collect::<Vec<_>>();
        assert_eq!(advancements.len(), 1);
        let (_, declaration) = advancements[0];
        let reward = program.function(declaration.reward()).unwrap();
        assert!(reward.parameters().is_empty());
        assert!(reward.results().is_empty());
        let Criterion::InventoryChanged { items } = declaration.criterion();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].as_str(), "minecraft:diamond");
    }
}
