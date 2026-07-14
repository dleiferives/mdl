//! Construction of verified Core SSA from checked frontend HIR.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

#[cfg(test)]
use std::cell::Cell;

use super::hir::{
    CheckedFrontendOutput, FunctionResult, HirBlock, HirCall, HirComparisonOp, HirExpression,
    HirExpressionKind, HirFunction, HirIf, HirStatement, HirStatementKind, LocalId,
    SourceFunctionId, ValueType,
};
use crate::diagnostic::Diagnostics;
use crate::ir::core::{
    BlockId, BlockTarget, BuildError, CoreProgram, CoreType, FunctionBody, FunctionBuilder,
    FunctionId, I32Predicate, ProgramError, Terminator, TerminatorKind, ValueId, verify_program,
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
}

impl SourceToCoreMap {
    fn new(functions: Vec<(SourceFunctionId, FunctionId)>) -> Self {
        Self {
            functions: functions.into_boxed_slice(),
        }
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
    /// A value-returning function unexpectedly retained a fallthrough path.
    ValueFunctionFallthrough {
        /// Source function with the invalid path.
        source_function: SourceFunctionId,
    },
}

impl fmt::Display for CoreGenerationInvariant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFunctionMapping { source_function } => {
                write!(formatter, "missing Core mapping for {source_function:?}")
            }
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
            Self::ValueFunctionFallthrough { source_function } => write!(
                formatter,
                "value-returning function {source_function:?} retained a fallthrough path"
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
            Self::Declaration { error, .. } | Self::Definition { error, .. } => Some(error),
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
    let mut correlations = Vec::with_capacity(checked.function_count());
    let mut budget = CoreGenerationBudget::new(max_join_edge_operands);

    // Signatures are deliberately predeclared before any body is constructed, so
    // forward calls, direct recursion, and mutual recursion all have ordinary Core
    // call targets.
    for function in checked.functions() {
        let name = function_name(sources, function)?;
        let parameters = function
            .bindings
            .iter()
            .take(function.parameter_count)
            .map(|binding| core_type(binding.ty))
            .collect();
        let results = match function.result {
            FunctionResult::Void => vec![],
            FunctionResult::Value(ty) => vec![core_type(ty)],
        };
        let core_function = program
            .declare_function(Some(name), parameters, results, function.origin)
            .map_err(|error| CoreGenerationFailure::Declaration {
                source_function: function.id,
                error,
            })?;
        correlations.push((function.id, core_function));
    }
    let source_to_core = SourceToCoreMap::new(correlations);

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
            &program,
            sources,
            checked,
            &source_to_core,
            function,
            core_function,
            &mut budget,
        )?
        .lower()?;
        program
            .define_function(core_function, body)
            .map_err(|error| CoreGenerationFailure::Definition {
                source_function: function.id,
                error,
            })?;
    }

    verify_program(&program, sources)
        .map_err(|diagnostics| CoreGenerationFailure::ProgramVerification { diagnostics })?;
    Ok(CoreGenerationOutput {
        program,
        source_to_core,
    })
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

const fn core_type(ty: ValueType) -> CoreType {
    match ty {
        ValueType::Bool => CoreType::Bool,
        ValueType::Int32 => CoreType::I32,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BindingState {
    value: Option<ValueId>,
    active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct JournalEntry {
    index: usize,
    previous: BindingState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EnvironmentCheckpoint(usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
        value: Option<ValueId>,
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
        value: ValueId,
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
    ) -> Result<ValueId, CoreGenerationFailure> {
        let Some(index) = Self::index(local) else {
            return Err(missing_binding(owner, local));
        };
        let Some(state) = self.states.get(index).copied() else {
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
            .map(|(order, entry)| (entry.index, order, entry.previous))
            .collect::<Vec<_>>();
        touched.sort_unstable_by_key(|(index, order, _)| (*index, *order));

        let mut overrides = Vec::with_capacity(touched.len());
        let mut offset = 0;
        while let Some((index, _, incoming)) = touched.get(offset).copied() {
            let state = self.states[index];
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
        self.states.get(index).copied()
    }

    fn write(&mut self, index: usize, state: BindingState) {
        let previous = self.states[index];
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
            .map_or(incoming, |offset| self.overrides[offset].state)
    }
}

struct MergeCandidate {
    index: usize,
    merged: BindingState,
    needs_parameter: bool,
}

#[derive(Clone, Copy)]
struct MergeAggregate {
    override_count: usize,
    first_value: Option<ValueId>,
    all_active: bool,
    all_assigned: bool,
    all_values_equal: bool,
}

impl MergeAggregate {
    fn new(state: BindingState) -> Self {
        Self {
            override_count: 1,
            first_value: state.value,
            all_active: state.active,
            all_assigned: state.value.is_some(),
            all_values_equal: true,
        }
    }

    fn include_override(&mut self, state: BindingState) {
        self.override_count += 1;
        self.include_state(state);
    }

    fn include_missing_incoming(&mut self, state: BindingState) {
        self.include_state(state);
    }

    fn include_state(&mut self, state: BindingState) {
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
    source_to_core: &'program SourceToCoreMap,
    function: &'program HirFunction,
    budget: &'budget mut CoreGenerationBudget,
}

impl<'program, 'budget> BodyLowerer<'program, 'budget> {
    fn new(
        program: &'program CoreProgram,
        sources: &'program SourceContext,
        checked: &'program CheckedFrontendOutput,
        source_to_core: &'program SourceToCoreMap,
        function: &'program HirFunction,
        core_function: FunctionId,
        budget: &'budget mut CoreGenerationBudget,
    ) -> Result<Self, CoreGenerationFailure> {
        let builder = FunctionBuilder::new(program, sources, core_function).map_err(|error| {
            CoreGenerationFailure::Construction {
                source_function: function.id,
                error,
            }
        })?;
        Ok(Self {
            builder,
            checked,
            source_to_core,
            function,
            budget,
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
        for index in 0..self.function.parameter_count {
            let local = self.function.bindings[index].id;
            let value = parameters
                .get(index)
                .copied()
                .ok_or(CoreGenerationFailure::Invariant(
                    CoreGenerationInvariant::MissingEntryParameter {
                        source_function: self.function.id,
                        parameter: index,
                    },
                ))?;
            environment.activate(self.function.id, local, Some(value))?;
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
            HirStatementKind::If(conditional) => self.lower_if(block, environment, conditional),
            HirStatementKind::Return(value) => {
                let values = value
                    .as_ref()
                    .map(|value| {
                        self.lower_expression(&mut block, environment, value)
                            .map(|value| vec![value])
                    })
                    .transpose()?
                    .unwrap_or_default();
                self.switch_to(block)?;
                self.terminate(TerminatorKind::Return(values), statement.origin)?;
                Ok(None)
            }
        }
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
            let condition =
                self.lower_expression(&mut condition_block, environment, &arm.condition)?;
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
                    .and_modify(|aggregate| aggregate.include_override(entry.state))
                    .or_insert_with(|| MergeAggregate::new(entry.state));
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
                aggregate.include_missing_incoming(incoming);
            }
            candidates.push(aggregate.candidate(index));
        }

        // This is the single point where the complete phi count and predecessor
        // count are both known. A whole-compilation join-edge budget can be checked
        // here before either parameter or edge-argument storage is allocated.
        let phi_count = candidates
            .iter()
            .filter(|candidate| candidate.needs_parameter)
            .count();
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
                let parameter = self.append_block_parameter(join, core_type(binding.ty), origin)?;
                candidate.merged.value = Some(parameter);
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
                arguments.push(value);
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

    fn lower_expression(
        &mut self,
        block: &mut BlockId,
        environment: &mut Environment,
        expression: &HirExpression,
    ) -> Result<ValueId, CoreGenerationFailure> {
        self.switch_to(*block)?;
        let value = match &expression.kind {
            HirExpressionKind::Bool(value) => self
                .builder
                .bool_constant(*value, expression.origin)
                .map_err(|error| self.construction(error))?,
            HirExpressionKind::Int32(value) => self
                .builder
                .i32_constant(*value, expression.origin)
                .map_err(|error| self.construction(error))?,
            HirExpressionKind::Local(local) => environment.read(self.function.id, *local)?,
            HirExpressionKind::Call(call) => {
                let values = self.lower_call(block, environment, call)?;
                values
                    .into_iter()
                    .next()
                    .ok_or(CoreGenerationFailure::Invariant(
                        CoreGenerationInvariant::CallResultCount {
                            source_function: self.function.id,
                            callee: call.callee,
                            expected: 1,
                            actual: 0,
                        },
                    ))?
            }
            HirExpressionKind::Not(operand) => {
                let operand = self.lower_expression(block, environment, operand)?;
                self.builder
                    .bool_not(operand, expression.origin)
                    .map_err(|error| self.construction(error))?
            }
            HirExpressionKind::Compare { op, left, right } => {
                let left_value = self.lower_expression(block, environment, left)?;
                let right_value = self.lower_expression(block, environment, right)?;
                if left.ty == ValueType::Bool {
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
                }
            }
        };
        *block = self.builder.insertion_block();
        let actual = self
            .builder
            .body()
            .value(value)
            .map(crate::ir::core::ValueData::ty)
            .ok_or_else(|| self.construction(BuildError::InvalidValue { value }))?;
        let expected = core_type(expression.ty);
        if actual != expected {
            return Err(CoreGenerationFailure::Invariant(
                CoreGenerationInvariant::ExpressionType {
                    source_function: self.function.id,
                    expected,
                    actual,
                },
            ));
        }
        Ok(value)
    }

    fn lower_call(
        &mut self,
        block: &mut BlockId,
        environment: &mut Environment,
        call: &HirCall,
    ) -> Result<Vec<ValueId>, CoreGenerationFailure> {
        let mut arguments = Vec::with_capacity(call.arguments.len());
        for argument in &call.arguments {
            arguments.push(self.lower_expression(block, environment, argument)?);
        }
        self.switch_to(*block)?;
        let core_callee =
            self.source_to_core
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
            .map_or(0, |callee| {
                usize::from(!matches!(callee.result, FunctionResult::Void))
            });
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
        CoreGenerationFailure, CoreGenerationOutput, CoreGenerationResource, SourceToCoreMap,
        environment_work_counters, lower_hir, reset_environment_work_counters,
    };
    use crate::entity::EntityId;
    use crate::frontend::FrontendLimits;
    use crate::frontend::check::check;
    use crate::frontend::hir::CheckedFrontendOutput;
    use crate::frontend::lexer::lex;
    use crate::frontend::parser::parse;
    use crate::ir::core::{
        CanonicalPrinter, CoreOp, TerminatorKind, reset_verifier_counters, verifier_counters,
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
}
