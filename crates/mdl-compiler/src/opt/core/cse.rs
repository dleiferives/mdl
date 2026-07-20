//! Bounded, deterministic dominance-scoped common-subexpression elimination.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::hash::{BuildHasher, Hash, RandomState};

use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, ControlFlowGraph, CoreOp, CoreProgram, CoreType, DominatorTree, EffectClass,
    FunctionBody, FunctionEditor, FunctionId, I32ClosedRange, I32Predicate, InstData, InstId,
    OperandSymmetry, PlacementIndex, ResultEquivalence, Speculation, UseIndex, ValueId,
    ValueReplacement,
};
use crate::source::{OriginId, SourceContext};

/// Whether one CSE limit is derived from immutable input metadata or explicit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CseLimit<T> {
    Derived,
    Explicit(T),
}

/// Maximum combined allocated block, instruction, and value analysis slots.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct CseAnalysisSlotLimit(usize);

impl CseAnalysisSlotLimit {
    #[must_use]
    pub(super) const fn new(slots: usize) -> Self {
        Self(slots)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Maximum expressions simultaneously retained on one dominator path.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct CseActiveEntryLimit(usize);

impl CseActiveEntryLimit {
    #[must_use]
    pub(super) const fn new(entries: usize) -> Self {
        Self(entries)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Maximum complete reachable instruction roots inspected by one invocation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct CseRootLimit(usize);

impl CseRootLimit {
    #[must_use]
    pub(super) const fn new(roots: usize) -> Self {
        Self(roots)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Typed resource policy for one frozen CSE snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CseLimits {
    analysis_slots: CseLimit<CseAnalysisSlotLimit>,
    active_entries: CseLimit<CseActiveEntryLimit>,
    root_visits: CseLimit<CseRootLimit>,
}

impl CseLimits {
    #[must_use]
    pub(super) const fn derived() -> Self {
        Self {
            analysis_slots: CseLimit::Derived,
            active_entries: CseLimit::Derived,
            root_visits: CseLimit::Derived,
        }
    }

    #[must_use]
    pub(super) const fn with_analysis_slot_limit(mut self, limit: CseAnalysisSlotLimit) -> Self {
        self.analysis_slots = CseLimit::Explicit(limit);
        self
    }

    #[must_use]
    pub(super) const fn with_active_entry_limit(mut self, limit: CseActiveEntryLimit) -> Self {
        self.active_entries = CseLimit::Explicit(limit);
        self
    }

    #[must_use]
    pub(super) const fn with_root_limit(mut self, limit: CseRootLimit) -> Self {
        self.root_visits = CseLimit::Explicit(limit);
        self
    }
}

impl Default for CseLimits {
    fn default() -> Self {
        Self::derived()
    }
}

/// Stable reason CSE stopped conservatively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CseLimitReason {
    AnalysisSize,
    ActiveEntries,
    RootVisits,
}

/// Whether the complete reachable frozen snapshot was scanned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CseCompletion {
    Complete,
    StoppedAtLimit(CseLimitReason),
}

/// Deterministic counters for fact construction, planning, and application.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CseStatistics {
    pub(super) planning_fact_snapshots_built: usize,
    pub(super) planning_dominator_builds: usize,
    pub(super) replacement_validation_dominator_builds: usize,
    pub(super) post_replacement_use_audits: usize,
    pub(super) roots_visited: usize,
    pub(super) keys_built: usize,
    pub(super) table_insertions: usize,
    pub(super) table_hits: usize,
    pub(super) table_removals: usize,
    pub(super) maximum_active_entries: usize,
    pub(super) value_replacements: usize,
    pub(super) instructions_erased: usize,
    pub(super) replacement_batches: usize,
    pub(super) erasure_batches: usize,
}

/// Result of one bounded CSE invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CseOutcome {
    pub(super) changed: bool,
    pub(super) completion: CseCompletion,
    pub(super) statistics: CseStatistics,
}

/// One successfully applied duplicate-to-representative correspondence.
///
/// The borrowed result slices preserve the complete operation result contract in
/// contract order. The pass submits these records only after both edit batches
/// succeed; retaining or bounding them remains the runner-owned sink's decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CseAppliedRewrite<'a> {
    pub(super) duplicate: InstId,
    pub(super) representative: InstId,
    pub(super) duplicate_results: &'a [ValueId],
    pub(super) representative_results: &'a [ValueId],
    pub(super) duplicate_origin: OriginId,
    pub(super) representative_origin: OriginId,
}

/// Failure of verified-body invariants, checked arithmetic, or atomic editing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CseError {
    SizeOverflow,
    InconsistentVerifiedBody(&'static str),
    ReplacementCycle { value: ValueId },
    Edit(crate::ir::core::EditError),
}

impl fmt::Display for CseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for CseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Edit(error) => Some(error),
            Self::SizeOverflow
            | Self::InconsistentVerifiedBody(_)
            | Self::ReplacementCycle { .. } => None,
        }
    }
}

impl From<crate::ir::core::EditError> for CseError {
    fn from(error: crate::ir::core::EditError) -> Self {
        Self::Edit(error)
    }
}

/// Eliminates structurally equal scalar instructions on one dominator path.
pub(super) fn run_cse(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    limits: CseLimits,
) -> Result<CseOutcome, CseError> {
    run_cse_with_hasher_and_optional_rewrite_sink(
        program,
        sources,
        function,
        body,
        limits,
        RandomState::new(),
        None::<fn(CseAppliedRewrite<'_>)>,
    )
}

/// Runs CSE and submits applied correspondences without retaining pass-owned detail.
pub(super) fn run_cse_with_rewrite_sink<F>(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    limits: CseLimits,
    on_applied_rewrite: F,
) -> Result<CseOutcome, CseError>
where
    F: FnMut(CseAppliedRewrite<'_>),
{
    run_cse_with_hasher_and_optional_rewrite_sink(
        program,
        sources,
        function,
        body,
        limits,
        RandomState::new(),
        Some(on_applied_rewrite),
    )
}

#[cfg(test)]
fn run_cse_with_hasher<S: BuildHasher>(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    limits: CseLimits,
    hash_builder: S,
) -> Result<CseOutcome, CseError> {
    run_cse_with_hasher_and_optional_rewrite_sink(
        program,
        sources,
        function,
        body,
        limits,
        hash_builder,
        None::<fn(CseAppliedRewrite<'_>)>,
    )
}

fn run_cse_with_hasher_and_optional_rewrite_sink<S, F>(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    limits: CseLimits,
    hash_builder: S,
    on_applied_rewrite: Option<F>,
) -> Result<CseOutcome, CseError>
where
    S: BuildHasher,
    F: FnMut(CseAppliedRewrite<'_>),
{
    let Some(analysis_slots) = body
        .blocks
        .len()
        .checked_add(body.instructions.len())
        .and_then(|slots| slots.checked_add(body.values.len()))
    else {
        return Ok(stopped_before_analysis());
    };
    if matches!(
        limits.analysis_slots,
        CseLimit::Explicit(limit) if analysis_slots > limit.get()
    ) {
        return Ok(stopped_before_analysis());
    }

    let plan = plan_cse(body, limits, hash_builder)?;
    apply_cse(program, sources, function, body, plan, on_applied_rewrite)
}

const fn stopped_before_analysis() -> CseOutcome {
    CseOutcome {
        changed: false,
        completion: CseCompletion::StoppedAtLimit(CseLimitReason::AnalysisSize),
        statistics: CseStatistics {
            planning_fact_snapshots_built: 0,
            planning_dominator_builds: 0,
            replacement_validation_dominator_builds: 0,
            post_replacement_use_audits: 0,
            roots_visited: 0,
            keys_built: 0,
            table_insertions: 0,
            table_hits: 0,
            table_removals: 0,
            maximum_active_entries: 0,
            value_replacements: 0,
            instructions_erased: 0,
            replacement_batches: 0,
            erasure_batches: 0,
        },
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum SmallKeySeq<T> {
    Zero,
    One(T),
    Two(T, T),
    Many(Box<[T]>),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum CseOpKey {
    BoolConstant(bool),
    I32Constant(i32),
    I32AddWrapping,
    I32SubWrapping,
    I32AddOverflowing,
    I32Compare(I32Predicate),
    I32InClosedRange(I32ClosedRange),
    BoolNot,
    ListI32Empty,
    ListI32Length,
    ListI32Push,
    ListI32LastOrZero,
    ListI32WithoutLast,
    StringConstant(Box<str>),
    StringLength,
    StringEndsWithAscii(u8),
    StringWithoutLastUnit,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CoreExpressionKey {
    op: CseOpKey,
    operands: SmallKeySeq<ValueId>,
    result_types: SmallKeySeq<CoreType>,
}

struct PlannedSubstitutions {
    mappings: Vec<Option<ValueId>>,
    seen_generation: Vec<u32>,
    generation: u32,
}

impl PlannedSubstitutions {
    fn new(value_count: usize) -> Self {
        Self {
            mappings: vec![None; value_count],
            seen_generation: vec![0; value_count],
            generation: 0,
        }
    }

    fn resolve(&mut self, value: ValueId) -> Result<ValueId, CseError> {
        self.begin_generation();
        let generation = self.generation;
        let mut cursor = value;
        let endpoint = loop {
            let index = value_index(cursor)
                .filter(|index| *index < self.mappings.len())
                .ok_or(CseError::InconsistentVerifiedBody(
                    "CSE encountered an invalid operand value",
                ))?;
            let Some(next) = self.mappings[index] else {
                break cursor;
            };
            if self.seen_generation[index] == generation {
                return Err(CseError::ReplacementCycle { value: cursor });
            }
            self.seen_generation[index] = generation;
            cursor = next;
        };

        cursor = value;
        loop {
            let index = value_index(cursor)
                .filter(|index| *index < self.mappings.len())
                .ok_or(CseError::InconsistentVerifiedBody(
                    "CSE replacement path became invalid",
                ))?;
            if self.seen_generation[index] != generation {
                break;
            }
            let next = self.mappings[index].ok_or(CseError::InconsistentVerifiedBody(
                "CSE replacement path lost a mapping",
            ))?;
            self.mappings[index] = Some(endpoint);
            cursor = next;
        }
        Ok(endpoint)
    }

    fn insert_instruction_results(
        &mut self,
        body: &FunctionBody,
        duplicate: InstId,
        representative: InstId,
    ) -> Result<usize, CseError> {
        let duplicate_data =
            body.instruction(duplicate)
                .ok_or(CseError::InconsistentVerifiedBody(
                    "CSE duplicate instruction is absent",
                ))?;
        let representative_data =
            body.instruction(representative)
                .ok_or(CseError::InconsistentVerifiedBody(
                    "CSE representative instruction is absent",
                ))?;
        validate_complete_result_contract(body, duplicate_data, representative_data)?;

        for (source, target) in duplicate_data
            .results()
            .iter()
            .copied()
            .zip(representative_data.results().iter().copied())
        {
            if source == target {
                return Err(CseError::InconsistentVerifiedBody(
                    "CSE duplicate and representative share a result identity",
                ));
            }
            let source_slot = value_index(source)
                .and_then(|index| self.mappings.get(index))
                .ok_or(CseError::InconsistentVerifiedBody(
                    "CSE duplicate result is outside the substitution table",
                ))?;
            if source_slot.is_some() {
                return Err(CseError::InconsistentVerifiedBody(
                    "CSE instruction was planned as a duplicate more than once",
                ));
            }
            if value_index(target)
                .and_then(|index| self.mappings.get(index))
                .is_some_and(Option::is_some)
            {
                return Err(CseError::InconsistentVerifiedBody(
                    "CSE representative is itself a planned duplicate",
                ));
            }
        }

        for (source, target) in duplicate_data
            .results()
            .iter()
            .copied()
            .zip(representative_data.results().iter().copied())
        {
            let slot = value_index(source)
                .and_then(|index| self.mappings.get_mut(index))
                .ok_or(CseError::InconsistentVerifiedBody(
                    "CSE duplicate result is outside the substitution table",
                ))?;
            *slot = Some(target);
        }
        Ok(duplicate_data.results().len())
    }

    fn begin_generation(&mut self) {
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
        } else {
            self.seen_generation.fill(0);
            self.generation = 1;
        }
    }
}

struct PreparedCse {
    representatives: Vec<Option<InstId>>,
    completion: CseCompletion,
    statistics: CseStatistics,
}

struct CsePlanner<'body, S> {
    body: &'body FunctionBody,
    children: Vec<Vec<BlockId>>,
    table: HashMap<CoreExpressionKey, InstId, S>,
    undo: Vec<CoreExpressionKey>,
    substitutions: PlannedSubstitutions,
    representatives: Vec<Option<InstId>>,
    root_limit: usize,
    active_entry_limit: usize,
    completion: CseCompletion,
    statistics: CseStatistics,
}

struct TraversalFrame {
    block: BlockId,
    next_child: usize,
    undo_checkpoint: usize,
    processed: bool,
}

impl TraversalFrame {
    const fn new(block: BlockId, undo_checkpoint: usize) -> Self {
        Self {
            block,
            next_child: 0,
            undo_checkpoint,
            processed: false,
        }
    }
}

enum ProcessControl {
    Continue,
    Stop(CseLimitReason),
}

fn plan_cse<S: BuildHasher>(
    body: &FunctionBody,
    limits: CseLimits,
    hash_builder: S,
) -> Result<PreparedCse, CseError> {
    let placement = PlacementIndex::new(body);
    let cfg = ControlFlowGraph::from_placement(&placement);
    let dominators = DominatorTree::new(&cfg);
    let (children, reachable_instruction_count) = build_dominator_children(body, &dominators)?;
    let root_limit = match limits.root_visits {
        CseLimit::Derived => reachable_instruction_count,
        CseLimit::Explicit(limit) => limit.get(),
    };
    let active_entry_limit = match limits.active_entries {
        CseLimit::Derived => reachable_instruction_count,
        CseLimit::Explicit(limit) => limit.get(),
    };
    let mut planner = CsePlanner {
        body,
        children,
        table: HashMap::with_hasher(hash_builder),
        undo: vec![],
        substitutions: PlannedSubstitutions::new(body.values.len()),
        representatives: vec![None; body.instructions.len()],
        root_limit,
        active_entry_limit,
        completion: CseCompletion::Complete,
        statistics: CseStatistics {
            planning_fact_snapshots_built: 1,
            planning_dominator_builds: 1,
            ..CseStatistics::default()
        },
    };
    planner.walk_dominator_tree()?;
    Ok(PreparedCse {
        representatives: planner.representatives,
        completion: planner.completion,
        statistics: planner.statistics,
    })
}

fn build_dominator_children(
    body: &FunctionBody,
    dominators: &DominatorTree<'_>,
) -> Result<(Vec<Vec<BlockId>>, usize), CseError> {
    let mut children = vec![vec![]; body.blocks.len()];
    let mut reachable_instruction_count = 0_usize;
    for block in body.blocks.keys() {
        if !dominators.reachability().contains(block) {
            continue;
        }
        let data = body.block(block).ok_or(CseError::InconsistentVerifiedBody(
            "reachable CSE block is absent",
        ))?;
        reachable_instruction_count = reachable_instruction_count
            .checked_add(data.instructions().len())
            .ok_or(CseError::SizeOverflow)?;
        let immediate =
            dominators
                .immediate_dominator(block)
                .ok_or(CseError::InconsistentVerifiedBody(
                    "reachable CSE block has no immediate dominator",
                ))?;
        if block == body.entry() {
            if immediate != block {
                return Err(CseError::InconsistentVerifiedBody(
                    "CSE entry does not dominate itself",
                ));
            }
            continue;
        }
        if immediate == block {
            return Err(CseError::InconsistentVerifiedBody(
                "non-entry CSE block immediately dominates itself",
            ));
        }
        let child_list = block_index(immediate)
            .and_then(|index| children.get_mut(index))
            .ok_or(CseError::InconsistentVerifiedBody(
                "CSE immediate dominator is outside the block table",
            ))?;
        child_list.push(block);
    }
    if !dominators.reachability().contains(body.entry()) {
        return Err(CseError::InconsistentVerifiedBody(
            "CSE entry is not reachable in the verified body",
        ));
    }
    Ok((children, reachable_instruction_count))
}

impl<S: BuildHasher> CsePlanner<'_, S> {
    fn walk_dominator_tree(&mut self) -> Result<(), CseError> {
        let mut stack = vec![TraversalFrame::new(self.body.entry(), 0)];
        while !stack.is_empty() {
            let frame_index = stack.len() - 1;
            if !stack[frame_index].processed {
                stack[frame_index].processed = true;
                let block = stack[frame_index].block;
                if let ProcessControl::Stop(reason) = self.process_block(block)? {
                    self.completion = CseCompletion::StoppedAtLimit(reason);
                    self.unwind_to(0)?;
                    return Ok(());
                }
                continue;
            }

            let block = stack[frame_index].block;
            let children = self.children_for(block)?;
            if stack[frame_index].next_child < children.len() {
                let child = children[stack[frame_index].next_child];
                stack[frame_index].next_child += 1;
                stack.push(TraversalFrame::new(child, self.undo.len()));
                continue;
            }

            let checkpoint = stack[frame_index].undo_checkpoint;
            stack.pop();
            self.unwind_to(checkpoint)?;
        }
        if !self.table.is_empty() || !self.undo.is_empty() {
            return Err(CseError::InconsistentVerifiedBody(
                "CSE scoped table was not empty after traversal",
            ));
        }
        Ok(())
    }

    fn process_block(&mut self, block: BlockId) -> Result<ProcessControl, CseError> {
        let data = self
            .body
            .block(block)
            .ok_or(CseError::InconsistentVerifiedBody(
                "CSE dominator traversal block is absent",
            ))?;
        for instruction in data.instructions() {
            match self.process_instruction(*instruction)? {
                ProcessControl::Continue => {}
                stop @ ProcessControl::Stop(_) => return Ok(stop),
            }
        }
        Ok(ProcessControl::Continue)
    }

    fn process_instruction(&mut self, instruction: InstId) -> Result<ProcessControl, CseError> {
        if self.statistics.roots_visited == self.root_limit {
            return Ok(ProcessControl::Stop(CseLimitReason::RootVisits));
        }
        increment(&mut self.statistics.roots_visited)?;
        let data = self
            .body
            .instruction(instruction)
            .ok_or(CseError::InconsistentVerifiedBody(
                "reachable CSE instruction is absent",
            ))?;
        let Some(key) = build_expression_key(self.body, data, &mut self.substitutions)? else {
            return Ok(ProcessControl::Continue);
        };
        increment(&mut self.statistics.keys_built)?;

        if let Some(representative) = self.table.get(&key).copied() {
            self.plan_duplicate(instruction, representative)?;
            increment(&mut self.statistics.table_hits)?;
            return Ok(ProcessControl::Continue);
        }
        if self.table.len() == self.active_entry_limit {
            return Ok(ProcessControl::Stop(CseLimitReason::ActiveEntries));
        }
        if self.table.insert(key.clone(), instruction).is_some() {
            return Err(CseError::InconsistentVerifiedBody(
                "CSE miss replaced an active representative",
            ));
        }
        self.undo.push(key);
        increment(&mut self.statistics.table_insertions)?;
        self.statistics.maximum_active_entries =
            self.statistics.maximum_active_entries.max(self.table.len());
        Ok(ProcessControl::Continue)
    }

    fn plan_duplicate(
        &mut self,
        duplicate: InstId,
        representative: InstId,
    ) -> Result<(), CseError> {
        let duplicate_index = instruction_index(duplicate).ok_or(
            CseError::InconsistentVerifiedBody("CSE duplicate has an invalid instruction ID"),
        )?;
        let slot =
            self.representatives
                .get(duplicate_index)
                .ok_or(CseError::InconsistentVerifiedBody(
                    "CSE duplicate is outside the dense plan",
                ))?;
        if slot.is_some() {
            return Err(CseError::InconsistentVerifiedBody(
                "CSE duplicate was planned twice",
            ));
        }
        self.substitutions
            .insert_instruction_results(self.body, duplicate, representative)?;
        self.representatives[duplicate_index] = Some(representative);
        Ok(())
    }

    fn children_for(&self, block: BlockId) -> Result<&[BlockId], CseError> {
        block_index(block)
            .and_then(|index| self.children.get(index))
            .map(Vec::as_slice)
            .ok_or(CseError::InconsistentVerifiedBody(
                "CSE traversal block is outside child storage",
            ))
    }

    fn unwind_to(&mut self, checkpoint: usize) -> Result<(), CseError> {
        if checkpoint > self.undo.len() {
            return Err(CseError::InconsistentVerifiedBody(
                "CSE undo checkpoint is outside the active log",
            ));
        }
        while self.undo.len() > checkpoint {
            let key = self.undo.pop().expect("length checked");
            if self.table.remove(&key).is_none() {
                return Err(CseError::InconsistentVerifiedBody(
                    "CSE undo key is absent from the scoped table",
                ));
            }
            increment(&mut self.statistics.table_removals)?;
        }
        Ok(())
    }
}

fn build_expression_key(
    body: &FunctionBody,
    data: &InstData,
    substitutions: &mut PlannedSubstitutions,
) -> Result<Option<CoreExpressionKey>, CseError> {
    if data.op().effects() != EffectClass::Pure
        || data.op().speculation() != Speculation::Always
        || data.op().result_equivalence() != ResultEquivalence::Structural
    {
        return Ok(None);
    }
    let op = match data.op() {
        CoreOp::BoolConstant(value) => CseOpKey::BoolConstant(*value),
        CoreOp::I32Constant(value) => CseOpKey::I32Constant(*value),
        CoreOp::I32AddWrapping => CseOpKey::I32AddWrapping,
        CoreOp::I32SubWrapping => CseOpKey::I32SubWrapping,
        CoreOp::I32AddOverflowing => CseOpKey::I32AddOverflowing,
        CoreOp::I32Compare(predicate) => CseOpKey::I32Compare(*predicate),
        CoreOp::I32InClosedRange(range) => CseOpKey::I32InClosedRange(*range),
        CoreOp::BoolNot => CseOpKey::BoolNot,
        CoreOp::ListI32Empty => CseOpKey::ListI32Empty,
        CoreOp::ListI32Length => CseOpKey::ListI32Length,
        CoreOp::ListI32Push => CseOpKey::ListI32Push,
        CoreOp::ListI32LastOrZero => CseOpKey::ListI32LastOrZero,
        CoreOp::ListI32WithoutLast => CseOpKey::ListI32WithoutLast,
        CoreOp::StringConstant(value) => CseOpKey::StringConstant(value.clone()),
        CoreOp::StringLength => CseOpKey::StringLength,
        CoreOp::StringEndsWithAscii(value) => CseOpKey::StringEndsWithAscii(*value),
        CoreOp::StringWithoutLastUnit => CseOpKey::StringWithoutLastUnit,
        CoreOp::Call(_) | CoreOp::External(_) => {
            return Err(CseError::InconsistentVerifiedBody(
                "structurally opaque operation passed the CSE eligibility gate",
            ));
        }
    };
    Ok(Some(CoreExpressionKey {
        op,
        operands: build_operand_key(data, substitutions)?,
        result_types: build_result_type_key(body, data)?,
    }))
}

fn build_operand_key(
    data: &InstData,
    substitutions: &mut PlannedSubstitutions,
) -> Result<SmallKeySeq<ValueId>, CseError> {
    match data.operands() {
        [] => Ok(SmallKeySeq::Zero),
        [operand] => Ok(SmallKeySeq::One(substitutions.resolve(*operand)?)),
        [left, right] => {
            let mut left = substitutions.resolve(*left)?;
            let mut right = substitutions.resolve(*right)?;
            if data.op().operand_symmetry() == OperandSymmetry::CommutativePair && right < left {
                std::mem::swap(&mut left, &mut right);
            }
            Ok(SmallKeySeq::Two(left, right))
        }
        operands => {
            if data.op().operand_symmetry() != OperandSymmetry::Ordered {
                return Err(CseError::InconsistentVerifiedBody(
                    "commutative-pair operation does not have exactly two operands",
                ));
            }
            let mut resolved = Vec::with_capacity(operands.len());
            for operand in operands {
                resolved.push(substitutions.resolve(*operand)?);
            }
            Ok(SmallKeySeq::Many(resolved.into_boxed_slice()))
        }
    }
}

fn build_result_type_key(
    body: &FunctionBody,
    data: &InstData,
) -> Result<SmallKeySeq<CoreType>, CseError> {
    match data.results() {
        [] => Ok(SmallKeySeq::Zero),
        [result] => Ok(SmallKeySeq::One(value_type(body, *result)?)),
        [first, second] => Ok(SmallKeySeq::Two(
            value_type(body, *first)?,
            value_type(body, *second)?,
        )),
        results => {
            let mut types = Vec::with_capacity(results.len());
            for result in results {
                types.push(value_type(body, *result)?);
            }
            Ok(SmallKeySeq::Many(types.into_boxed_slice()))
        }
    }
}

fn validate_complete_result_contract(
    body: &FunctionBody,
    duplicate: &InstData,
    representative: &InstData,
) -> Result<(), CseError> {
    if duplicate.results().len() != representative.results().len() {
        return Err(CseError::InconsistentVerifiedBody(
            "CSE hit has unequal complete result arity",
        ));
    }
    for (duplicate_result, representative_result) in duplicate
        .results()
        .iter()
        .copied()
        .zip(representative.results().iter().copied())
    {
        if value_type(body, duplicate_result)? != value_type(body, representative_result)? {
            return Err(CseError::InconsistentVerifiedBody(
                "CSE hit has unequal complete result types",
            ));
        }
    }
    Ok(())
}

fn audit_duplicate_results_unused(
    body: &FunctionBody,
    duplicates: &[InstId],
) -> Result<(), CseError> {
    let uses = UseIndex::new(body);
    for duplicate in duplicates {
        let data = body
            .instruction(*duplicate)
            .ok_or(CseError::InconsistentVerifiedBody(
                "replaced CSE duplicate is absent",
            ))?;
        for result in data.results() {
            if !uses.uses(*result).is_empty() {
                return Err(CseError::InconsistentVerifiedBody(
                    "replaced CSE duplicate retains an attached result use",
                ));
            }
        }
    }
    Ok(())
}

fn apply_cse<F>(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    mut plan: PreparedCse,
    mut on_applied_rewrite: Option<F>,
) -> Result<CseOutcome, CseError>
where
    F: FnMut(CseAppliedRewrite<'_>),
{
    let mut value_edits = vec![];
    let mut duplicates = vec![];
    for block in body.block_order() {
        let data = body
            .block(*block)
            .ok_or(CseError::InconsistentVerifiedBody(
                "attached CSE application block is absent",
            ))?;
        for duplicate in data.instructions() {
            let duplicate_index = instruction_index(*duplicate).ok_or(
                CseError::InconsistentVerifiedBody("attached CSE instruction has an invalid ID"),
            )?;
            let Some(representative) = plan.representatives.get(duplicate_index).copied().flatten()
            else {
                continue;
            };
            let duplicate_data =
                body.instruction(*duplicate)
                    .ok_or(CseError::InconsistentVerifiedBody(
                        "planned CSE duplicate is absent",
                    ))?;
            let representative_data =
                body.instruction(representative)
                    .ok_or(CseError::InconsistentVerifiedBody(
                        "planned CSE representative is absent",
                    ))?;
            validate_complete_result_contract(body, duplicate_data, representative_data)?;
            for (source, target) in duplicate_data
                .results()
                .iter()
                .copied()
                .zip(representative_data.results().iter().copied())
            {
                value_edits.push((source, ValueReplacement::Existing(target)));
            }
            duplicates.push(*duplicate);
        }
    }
    if duplicates.len() != plan.statistics.table_hits {
        return Err(CseError::InconsistentVerifiedBody(
            "CSE planned-hit count differs from its attached application set",
        ));
    }

    plan.statistics.value_replacements = value_edits.len();
    plan.statistics.instructions_erased = duplicates.len();
    let changed = !duplicates.is_empty();
    if changed {
        let replacement_validation_needed = !value_edits.is_empty();
        let mut editor = FunctionEditor::from_trusted_body(program, sources, function, body);
        editor.replace_values_batch(value_edits)?;
        plan.statistics.replacement_batches = 1;
        plan.statistics.replacement_validation_dominator_builds =
            usize::from(replacement_validation_needed);
        audit_duplicate_results_unused(editor.body(), &duplicates)?;
        plan.statistics.post_replacement_use_audits = 1;
        editor.erase_discardable_inst_set(duplicates)?;
        plan.statistics.erasure_batches = 1;
        if let Some(on_applied_rewrite) = &mut on_applied_rewrite {
            submit_applied_rewrites(editor.body(), &plan.representatives, on_applied_rewrite)?;
        }
    }
    Ok(CseOutcome {
        changed,
        completion: plan.completion,
        statistics: plan.statistics,
    })
}

/// Streams successful correspondences in allocated `InstId` order.
///
/// The dense representative table is already retained by the plan and erased raw
/// instruction records remain available. Reusing both avoids a second correspondence
/// vector whose size would otherwise ignore the runner's remark-record cap.
fn submit_applied_rewrites<F>(
    body: &FunctionBody,
    representatives: &[Option<InstId>],
    on_applied_rewrite: &mut F,
) -> Result<(), CseError>
where
    F: FnMut(CseAppliedRewrite<'_>),
{
    for (duplicate_index, representative) in representatives.iter().copied().enumerate() {
        let Some(representative) = representative else {
            continue;
        };
        let duplicate_index = u32::try_from(duplicate_index).map_err(|_| CseError::SizeOverflow)?;
        let duplicate = InstId::from_index(duplicate_index);
        let duplicate_data =
            body.instruction(duplicate)
                .ok_or(CseError::InconsistentVerifiedBody(
                    "applied CSE duplicate is absent during reporting",
                ))?;
        let representative_data =
            body.instruction(representative)
                .ok_or(CseError::InconsistentVerifiedBody(
                    "applied CSE representative is absent during reporting",
                ))?;
        on_applied_rewrite(CseAppliedRewrite {
            duplicate,
            representative,
            duplicate_results: duplicate_data.results(),
            representative_results: representative_data.results(),
            duplicate_origin: duplicate_data.origin(),
            representative_origin: representative_data.origin(),
        });
    }
    Ok(())
}

fn value_type(body: &FunctionBody, value: ValueId) -> Result<CoreType, CseError> {
    body.value(value)
        .map(crate::ir::core::ValueData::ty)
        .ok_or(CseError::InconsistentVerifiedBody(
            "CSE result references an invalid value",
        ))
}

fn increment(counter: &mut usize) -> Result<(), CseError> {
    *counter = counter.checked_add(1).ok_or(CseError::SizeOverflow)?;
    Ok(())
}

fn block_index(block: BlockId) -> Option<usize> {
    usize::try_from(block.index()).ok()
}

fn instruction_index(instruction: InstId) -> Option<usize> {
    usize::try_from(instruction.index()).ok()
}

fn value_index(value: ValueId) -> Option<usize> {
    usize::try_from(value.index()).ok()
}

#[cfg(test)]
mod tests {
    use std::hash::{BuildHasherDefault, DefaultHasher, Hasher};

    use super::*;
    use crate::entity::EntityVec;
    use crate::ir::core::{
        BlockData, BlockParam, BlockTarget, FunctionBuilder, PlacementIndex, Terminator,
        TerminatorKind, ValueData, ValueDef, verify_function,
    };
    use crate::source::{Origin, OriginId};

    #[derive(Default)]
    struct CollisionHasher;

    impl Hasher for CollisionHasher {
        fn finish(&self) -> u64 {
            0
        }

        fn write(&mut self, _bytes: &[u8]) {}
    }

    type CollisionBuildHasher = BuildHasherDefault<CollisionHasher>;

    fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
        builder.body().block(block).unwrap().parameters()[index].value()
    }

    fn defining_instruction(body: &FunctionBody, value: ValueId) -> InstId {
        match body.value(value).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => panic!("expected instruction result"),
        }
    }

    fn returned_values(body: &FunctionBody, block: BlockId) -> &[ValueId] {
        match body.block(block).unwrap().terminator().unwrap().kind() {
            TerminatorKind::Return(values) => values,
            _ => panic!("expected return"),
        }
    }

    fn key_for(body: &FunctionBody, value: ValueId) -> Option<CoreExpressionKey> {
        let instruction = defining_instruction(body, value);
        let mut substitutions = PlannedSubstitutions::new(body.values.len());
        build_expression_key(
            body,
            body.instruction(instruction).unwrap(),
            &mut substitutions,
        )
        .unwrap()
    }

    fn hash(key: &CoreExpressionKey) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    fn raw_block(terminator: Option<Terminator>) -> BlockData {
        BlockData {
            origin: OriginId::UNKNOWN,
            parameters: vec![],
            instructions: vec![],
            terminator,
        }
    }

    type RewriteSnapshot = (
        InstId,
        InstId,
        Vec<ValueId>,
        Vec<ValueId>,
        OriginId,
        OriginId,
    );

    fn snapshot_rewrite(rewrite: CseAppliedRewrite<'_>) -> RewriteSnapshot {
        (
            rewrite.duplicate,
            rewrite.representative,
            rewrite.duplicate_results.to_vec(),
            rewrite.representative_results.to_vec(),
            rewrite.duplicate_origin,
            rewrite.representative_origin,
        )
    }

    fn assert_cse_idempotent(
        program: &CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
        body: &mut FunctionBody,
    ) {
        let before = format!("{body:#?}");
        let second = run_cse(program, sources, function, body, CseLimits::derived()).unwrap();
        assert!(!second.changed);
        assert_eq!(second.completion, CseCompletion::Complete);
        assert_eq!(format!("{body:#?}"), before);
    }

    struct AddFixture {
        sources: SourceContext,
        program: CoreProgram,
        function: FunctionId,
        body: FunctionBody,
        entry: BlockId,
        representative: ValueId,
        duplicate: ValueId,
    }

    fn repeated_add_fixture() -> AddFixture {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("repeated-add"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let left = parameter(&builder, entry, 0);
        let right = parameter(&builder, entry, 1);
        let representative = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(right, left, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        AddFixture {
            sources,
            program,
            function,
            body,
            entry,
            representative,
            duplicate,
        }
    }

    fn raw_deep_chain(
        program: &CoreProgram,
        function: FunctionId,
        block_count: usize,
    ) -> (FunctionBody, ValueId) {
        assert!(block_count > 0);
        let mut blocks = EntityVec::new();
        let mut block_order = Vec::with_capacity(block_count);
        for _ in 0..block_count {
            let block = blocks
                .push(BlockData {
                    origin: OriginId::UNKNOWN,
                    parameters: vec![],
                    instructions: vec![],
                    terminator: None,
                })
                .unwrap();
            block_order.push(block);
        }
        let entry = block_order[0];
        let mut values = EntityVec::new();
        let parameter = values
            .push(ValueData {
                ty: CoreType::I32,
                definition: ValueDef::BlockParam {
                    block: entry,
                    parameter_index: 0,
                },
            })
            .unwrap();
        blocks.get_mut(entry).unwrap().parameters.push(BlockParam {
            value: parameter,
            origin: OriginId::UNKNOWN,
        });
        let mut instructions = EntityVec::new();
        let mut last_result = parameter;
        for (index, block) in block_order.iter().copied().enumerate() {
            let instruction = instructions
                .push(InstData {
                    op: CoreOp::I32AddWrapping,
                    operands: vec![parameter, parameter],
                    results: vec![],
                    origin: OriginId::UNKNOWN,
                })
                .unwrap();
            let result = values
                .push(ValueData {
                    ty: CoreType::I32,
                    definition: ValueDef::InstResult {
                        instruction,
                        result_index: 0,
                    },
                })
                .unwrap();
            instructions
                .get_mut(instruction)
                .unwrap()
                .results
                .push(result);
            let data = blocks.get_mut(block).unwrap();
            data.instructions.push(instruction);
            data.terminator = Some(Terminator::new(
                if let Some(next) = block_order.get(index + 1) {
                    TerminatorKind::Jump(BlockTarget::new(*next, vec![]))
                } else {
                    TerminatorKind::Return(vec![result])
                },
                OriginId::UNKNOWN,
            ));
            last_result = result;
        }
        let body = FunctionBody {
            blocks,
            instructions,
            values,
            block_order,
            entry,
        };
        assert!(program.function(function).is_some());
        (body, last_result)
    }

    fn raw_wide_dominator_tree(join_count: usize) -> FunctionBody {
        assert!(join_count > 0);
        let mut blocks = EntityVec::new();
        let entry = blocks.push(raw_block(None)).unwrap();
        let mut left = Vec::with_capacity(join_count);
        let mut right = Vec::with_capacity(join_count);
        let mut joins = Vec::with_capacity(join_count);
        for storage in [&mut left, &mut right, &mut joins] {
            for _ in 0..join_count {
                storage.push(blocks.push(raw_block(None)).unwrap());
            }
        }
        let left_exit = blocks
            .push(raw_block(Some(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))))
            .unwrap();
        let right_exit = blocks
            .push(raw_block(Some(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))))
            .unwrap();
        let mut values = EntityVec::new();
        let condition = values
            .push(ValueData {
                ty: CoreType::Bool,
                definition: ValueDef::BlockParam {
                    block: entry,
                    parameter_index: 0,
                },
            })
            .unwrap();
        blocks.get_mut(entry).unwrap().parameters.push(BlockParam {
            value: condition,
            origin: OriginId::UNKNOWN,
        });
        blocks.get_mut(entry).unwrap().terminator = Some(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(left[0], vec![]),
                else_target: BlockTarget::new(right[0], vec![]),
            },
            OriginId::UNKNOWN,
        ));
        for index in 0..join_count {
            let left_next = left.get(index + 1).copied().unwrap_or(left_exit);
            let right_next = right.get(index + 1).copied().unwrap_or(right_exit);
            blocks.get_mut(left[index]).unwrap().terminator = Some(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(joins[index], vec![]),
                    else_target: BlockTarget::new(left_next, vec![]),
                },
                OriginId::UNKNOWN,
            ));
            blocks.get_mut(right[index]).unwrap().terminator = Some(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(joins[index], vec![]),
                    else_target: BlockTarget::new(right_next, vec![]),
                },
                OriginId::UNKNOWN,
            ));
            blocks.get_mut(joins[index]).unwrap().terminator = Some(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ));
        }
        let block_order = blocks.keys().collect();
        FunctionBody {
            blocks,
            instructions: EntityVec::new(),
            values,
            block_order,
            entry,
        }
    }

    fn raw_return_blocks(block_count: usize) -> FunctionBody {
        assert!(block_count > 0);
        let mut blocks = EntityVec::new();
        for _ in 0..block_count {
            blocks
                .push(BlockData {
                    origin: OriginId::UNKNOWN,
                    parameters: vec![],
                    instructions: vec![],
                    terminator: Some(Terminator::new(
                        TerminatorKind::Return(vec![]),
                        OriginId::UNKNOWN,
                    )),
                })
                .unwrap();
        }
        let entry = blocks.keys().next().unwrap();
        let block_order = blocks.keys().collect();
        FunctionBody {
            blocks,
            instructions: EntityVec::new(),
            values: EntityVec::new(),
            block_order,
            entry,
        }
    }

    #[test]
    fn eligibility_and_payload_keying_cover_every_current_operation() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let callee = program
            .declare_function(
                Some("callee"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = program
            .declare_function(
                Some("keys"),
                vec![CoreType::I32, CoreType::I32, CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let left = parameter(&builder, entry, 0);
        let right = parameter(&builder, entry, 1);
        let boolean = parameter(&builder, entry, 2);
        let bool_constant = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let i32_constant = builder.i32_constant(-7, OriginId::UNKNOWN).unwrap();
        let wrapping = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        let overflowing = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap()
            .0;
        let comparisons = [
            I32Predicate::Eq,
            I32Predicate::Ne,
            I32Predicate::SignedLt,
            I32Predicate::SignedLe,
            I32Predicate::SignedGt,
            I32Predicate::SignedGe,
        ]
        .map(|predicate| {
            builder
                .i32_compare(predicate, left, right, OriginId::UNKNOWN)
                .unwrap()
        });
        let negation = builder.bool_not(boolean, OriginId::UNKNOWN).unwrap();
        let call = builder.call(callee, vec![left], OriginId::UNKNOWN).unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();

        for value in [bool_constant, i32_constant, wrapping, overflowing, negation]
            .into_iter()
            .chain(comparisons)
        {
            let data = body
                .instruction(defining_instruction(&body, value))
                .unwrap();
            assert_eq!(
                data.op().result_equivalence(),
                ResultEquivalence::Structural
            );
            assert!(key_for(&body, value).is_some());
        }
        let call_data = body.instruction(defining_instruction(&body, call)).unwrap();
        assert_eq!(
            call_data.op().result_equivalence(),
            ResultEquivalence::Opaque
        );
        assert_eq!(key_for(&body, call), None);
    }

    #[test]
    fn structurally_identical_calls_remain_opaque_in_the_complete_pass() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let callee = program
            .declare_function(
                Some("opaque-callee"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = program
            .declare_function(
                Some("opaque-calls"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let argument = parameter(&builder, entry, 0);
        let first = builder
            .call(callee, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
        let second = builder
            .call(callee, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
        let first_instruction = defining_instruction(builder.body(), first);
        let second_instruction = defining_instruction(builder.body(), second);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![second]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let before = format!("{body:#?}");

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(outcome.statistics.roots_visited, 2);
        assert_eq!(outcome.statistics.keys_built, 0);
        assert_eq!(outcome.statistics.table_hits, 0);
        assert_eq!(format!("{body:#?}"), before);
        let placement = PlacementIndex::new(&body);
        assert!(placement.is_instruction_attached(first_instruction));
        assert!(placement.is_instruction_attached(second_instruction));
    }

    #[test]
    fn keys_include_payload_symmetry_operand_order_and_complete_result_contract() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("key-contract"),
                vec![CoreType::I32, CoreType::I32],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let left = parameter(&builder, entry, 0);
        let right = parameter(&builder, entry, 1);
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let two = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let add_lr = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        let add_rl = builder
            .i32_add_wrapping(right, left, OriginId::UNKNOWN)
            .unwrap();
        let overflow_lr = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap()
            .0;
        let overflow_rl = builder
            .i32_add_overflowing(right, left, OriginId::UNKNOWN)
            .unwrap()
            .0;
        let eq_lr = builder
            .i32_compare(I32Predicate::Eq, left, right, OriginId::UNKNOWN)
            .unwrap();
        let eq_rl = builder
            .i32_compare(I32Predicate::Eq, right, left, OriginId::UNKNOWN)
            .unwrap();
        let ne_lr = builder
            .i32_compare(I32Predicate::Ne, left, right, OriginId::UNKNOWN)
            .unwrap();
        let ne_rl = builder
            .i32_compare(I32Predicate::Ne, right, left, OriginId::UNKNOWN)
            .unwrap();
        let ordered = [
            I32Predicate::SignedLt,
            I32Predicate::SignedLe,
            I32Predicate::SignedGt,
            I32Predicate::SignedGe,
        ]
        .map(|predicate| {
            let forward = builder
                .i32_compare(predicate, left, right, OriginId::UNKNOWN)
                .unwrap();
            let reverse = builder
                .i32_compare(predicate, right, left, OriginId::UNKNOWN)
                .unwrap();
            (forward, reverse)
        });
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();

        assert_ne!(key_for(&body, one), key_for(&body, two));
        for (first, second) in [
            (add_lr, add_rl),
            (overflow_lr, overflow_rl),
            (eq_lr, eq_rl),
            (ne_lr, ne_rl),
        ] {
            let first = key_for(&body, first).unwrap();
            let second = key_for(&body, second).unwrap();
            assert_eq!(first, second);
            assert_eq!(hash(&first), hash(&second));
        }
        for (forward, reverse) in ordered {
            assert_ne!(key_for(&body, forward), key_for(&body, reverse));
        }
        assert_ne!(key_for(&body, eq_lr), key_for(&body, ne_lr));

        let base = key_for(&body, add_lr).unwrap();
        let different_type = CoreExpressionKey {
            result_types: SmallKeySeq::One(CoreType::Bool),
            ..base.clone()
        };
        let different_arity = CoreExpressionKey {
            result_types: SmallKeySeq::Two(CoreType::I32, CoreType::Bool),
            ..base.clone()
        };
        assert_ne!(base, different_type);
        assert_ne!(base, different_arity);
    }

    #[test]
    fn defensive_result_contract_and_post_replacement_use_audits_are_explicit() {
        let fixture = repeated_add_fixture();
        let representative = defining_instruction(&fixture.body, fixture.representative);
        let duplicate = defining_instruction(&fixture.body, fixture.duplicate);
        validate_complete_result_contract(
            &fixture.body,
            fixture.body.instruction(duplicate).unwrap(),
            fixture.body.instruction(representative).unwrap(),
        )
        .unwrap();
        assert_eq!(
            audit_duplicate_results_unused(&fixture.body, &[duplicate]),
            Err(CseError::InconsistentVerifiedBody(
                "replaced CSE duplicate retains an attached result use"
            ))
        );

        let mut wrong_arity = fixture.body.clone();
        wrong_arity
            .instruction_mut(duplicate)
            .unwrap()
            .results
            .clear();
        assert_eq!(
            validate_complete_result_contract(
                &wrong_arity,
                wrong_arity.instruction(duplicate).unwrap(),
                wrong_arity.instruction(representative).unwrap(),
            ),
            Err(CseError::InconsistentVerifiedBody(
                "CSE hit has unequal complete result arity"
            ))
        );

        let mut wrong_type = fixture.body.clone();
        wrong_type.values.get_mut(fixture.duplicate).unwrap().ty = CoreType::Bool;
        assert_eq!(
            validate_complete_result_contract(
                &wrong_type,
                wrong_type.instruction(duplicate).unwrap(),
                wrong_type.instruction(representative).unwrap(),
            ),
            Err(CseError::InconsistentVerifiedBody(
                "CSE hit has unequal complete result types"
            ))
        );

        let mut applied = fixture.body;
        run_cse(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut applied,
            CseLimits::derived(),
        )
        .unwrap();
        audit_duplicate_results_unused(&applied, &[duplicate]).unwrap();
    }

    #[test]
    fn same_block_transitive_substitution_is_atomic_and_idempotent() {
        let mut sources = SourceContext::new();
        let representative_origin = sources
            .add_origin(Origin::Fused {
                inputs: vec![],
                reason: Some("representative".into()),
            })
            .unwrap();
        let duplicate_origin = sources
            .add_origin(Origin::Fused {
                inputs: vec![],
                reason: Some("duplicate".into()),
            })
            .unwrap();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("transitive"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let left = parameter(&builder, entry, 0);
        let right = parameter(&builder, entry, 1);
        let first = builder
            .i32_add_wrapping(left, right, representative_origin)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(right, left, duplicate_origin)
            .unwrap();
        let dependent = builder
            .i32_add_wrapping(first, left, representative_origin)
            .unwrap();
        let dependent_duplicate = builder
            .i32_add_wrapping(duplicate, left, duplicate_origin)
            .unwrap();
        let first_instruction = defining_instruction(builder.body(), first);
        let duplicate_instruction = defining_instruction(builder.body(), duplicate);
        let dependent_instruction = defining_instruction(builder.body(), dependent);
        let dependent_duplicate_instruction =
            defining_instruction(builder.body(), dependent_duplicate);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![dependent_duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let mut applied_rewrites = vec![];
        let outcome = run_cse_with_rewrite_sink(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
            |rewrite| applied_rewrites.push(snapshot_rewrite(rewrite)),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.completion, CseCompletion::Complete);
        assert_eq!(outcome.statistics.table_hits, 2);
        assert_eq!(outcome.statistics.value_replacements, 2);
        assert_eq!(outcome.statistics.replacement_batches, 1);
        assert_eq!(outcome.statistics.erasure_batches, 1);
        let placement = PlacementIndex::new(&body);
        assert!(placement.is_instruction_attached(first_instruction));
        assert!(placement.is_instruction_attached(dependent_instruction));
        assert!(!placement.is_instruction_attached(duplicate_instruction));
        assert!(!placement.is_instruction_attached(dependent_duplicate_instruction));
        assert_eq!(returned_values(&body, entry), &[dependent]);
        assert_eq!(
            body.instruction(first_instruction).unwrap().origin(),
            representative_origin
        );
        assert_eq!(
            applied_rewrites,
            vec![
                (
                    duplicate_instruction,
                    first_instruction,
                    vec![duplicate],
                    vec![first],
                    duplicate_origin,
                    representative_origin,
                ),
                (
                    dependent_duplicate_instruction,
                    dependent_instruction,
                    vec![dependent_duplicate],
                    vec![dependent],
                    duplicate_origin,
                    representative_origin,
                ),
            ]
        );
        verify_function(&program, &sources, function, &body).unwrap();
        assert_cse_idempotent(&program, &sources, function, &mut body);
    }

    #[test]
    fn entry_representative_dominates_diamond_siblings_and_join() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("dominating-diamond"),
                vec![CoreType::Bool],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let left = builder.create_block(OriginId::UNKNOWN).unwrap();
        let right = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let representative = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(left, vec![]),
                    else_target: BlockTarget::new(right, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut duplicates = vec![];
        for (block, target) in [(left, join), (right, join)] {
            builder.switch_to_block(block).unwrap();
            let duplicate = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
            duplicates.push(defining_instruction(builder.body(), duplicate));
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(target, vec![])),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        builder.switch_to_block(join).unwrap();
        let join_duplicate = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        duplicates.push(defining_instruction(builder.body(), join_duplicate));
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![join_duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.statistics.table_hits, 3);
        assert_eq!(returned_values(&body, join), &[representative]);
        let placement = PlacementIndex::new(&body);
        assert!(
            duplicates
                .into_iter()
                .all(|instruction| !placement.is_instruction_attached(instruction))
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn sibling_expressions_are_popped_before_the_next_sibling_and_join() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("sibling-scope"),
                vec![CoreType::Bool],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let left = builder.create_block(OriginId::UNKNOWN).unwrap();
        let right = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(left, vec![]),
                    else_target: BlockTarget::new(right, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut expressions = vec![];
        for block in [left, right] {
            builder.switch_to_block(block).unwrap();
            let expression = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
            expressions.push(defining_instruction(builder.body(), expression));
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(join, vec![])),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        builder.switch_to_block(join).unwrap();
        let joined = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        expressions.push(defining_instruction(builder.body(), joined));
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![joined]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let before = format!("{body:#?}");

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(outcome.statistics.table_hits, 0);
        assert_eq!(outcome.statistics.table_insertions, 3);
        assert_eq!(outcome.statistics.table_removals, 3);
        assert_eq!(outcome.statistics.maximum_active_entries, 1);
        assert_eq!(format!("{body:#?}"), before);
        let placement = PlacementIndex::new(&body);
        assert!(
            expressions
                .into_iter()
                .all(|instruction| placement.is_instruction_attached(instruction))
        );
    }

    #[test]
    fn loop_backedge_and_self_block_order_preserve_dominating_header_value() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("loop"),
                vec![CoreType::Bool],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let body_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let representative = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let same_block_duplicate = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(body_block, vec![]),
                    else_target: BlockTarget::new(exit, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(body_block).unwrap();
        let body_duplicate = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        let exit_duplicate = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![exit_duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let duplicate_instructions = [same_block_duplicate, body_duplicate, exit_duplicate]
            .map(|value| defining_instruction(builder.body(), value));
        let mut body = builder.finish().unwrap();

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.statistics.table_hits, 3);
        assert_eq!(returned_values(&body, exit), &[representative]);
        let placement = PlacementIndex::new(&body);
        assert!(
            duplicate_instructions
                .into_iter()
                .all(|instruction| !placement.is_instruction_attached(instruction))
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn attached_unreachable_instructions_never_represent_reachable_work() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("unreachable"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let left = parameter(&builder, entry, 0);
        let right = parameter(&builder, entry, 1);
        let earlier_unreachable = builder.create_block(OriginId::UNKNOWN).unwrap();
        let unreachable_user = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder.switch_to_block(earlier_unreachable).unwrap();
        let unreachable_expression = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        let unreachable_instruction = defining_instruction(builder.body(), unreachable_expression);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![unreachable_expression]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(entry).unwrap();
        let representative = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(right, left, OriginId::UNKNOWN)
            .unwrap();
        let representative_instruction = defining_instruction(builder.body(), representative);
        let duplicate_instruction = defining_instruction(builder.body(), duplicate);
        assert!(unreachable_instruction < representative_instruction);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(unreachable_user).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.statistics.roots_visited, 2);
        assert_eq!(outcome.statistics.table_hits, 1);
        let placement = PlacementIndex::new(&body);
        assert!(placement.is_instruction_attached(unreachable_instruction));
        assert!(placement.is_instruction_attached(representative_instruction));
        assert!(!placement.is_instruction_attached(duplicate_instruction));
        assert_eq!(returned_values(&body, entry), &[representative]);
        assert_eq!(returned_values(&body, unreachable_user), &[representative]);
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn overflowing_add_replaces_zero_one_or_both_live_results_as_one_contract() {
        #[derive(Clone, Copy)]
        enum LiveResults {
            None,
            Sum,
            Flag,
            Both,
        }

        for live in [
            LiveResults::None,
            LiveResults::Sum,
            LiveResults::Flag,
            LiveResults::Both,
        ] {
            let sources = SourceContext::new();
            let mut program = CoreProgram::new();
            let result_types = match live {
                LiveResults::None => vec![],
                LiveResults::Sum => vec![CoreType::I32],
                LiveResults::Flag => vec![CoreType::Bool],
                LiveResults::Both => vec![CoreType::I32, CoreType::Bool],
            };
            let function = program
                .declare_function(
                    Some("overflow-results"),
                    vec![CoreType::I32, CoreType::I32],
                    result_types,
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
            let entry = builder.entry_block();
            let left = parameter(&builder, entry, 0);
            let right = parameter(&builder, entry, 1);
            let representative = builder
                .i32_add_overflowing(left, right, OriginId::UNKNOWN)
                .unwrap();
            let duplicate = builder
                .i32_add_overflowing(right, left, OriginId::UNKNOWN)
                .unwrap();
            let duplicate_instruction = defining_instruction(builder.body(), duplicate.0);
            let returned = match live {
                LiveResults::None => vec![],
                LiveResults::Sum => vec![duplicate.0],
                LiveResults::Flag => vec![duplicate.1],
                LiveResults::Both => vec![duplicate.0, duplicate.1],
            };
            let expected = match live {
                LiveResults::None => vec![],
                LiveResults::Sum => vec![representative.0],
                LiveResults::Flag => vec![representative.1],
                LiveResults::Both => vec![representative.0, representative.1],
            };
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(returned),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            let mut body = builder.finish().unwrap();

            let mut rewrite_results = vec![];
            let outcome = run_cse_with_rewrite_sink(
                &program,
                &sources,
                function,
                &mut body,
                CseLimits::derived(),
                |rewrite| {
                    rewrite_results.push((
                        rewrite.duplicate_results.to_vec(),
                        rewrite.representative_results.to_vec(),
                    ));
                },
            )
            .unwrap();
            assert!(outcome.changed);
            assert_eq!(outcome.statistics.table_hits, 1);
            assert_eq!(outcome.statistics.value_replacements, 2);
            assert_eq!(
                rewrite_results,
                vec![(
                    vec![duplicate.0, duplicate.1],
                    vec![representative.0, representative.1],
                )]
            );
            assert_eq!(returned_values(&body, entry), expected);
            assert!(!PlacementIndex::new(&body).is_instruction_attached(duplicate_instruction));
            verify_function(&program, &sources, function, &body).unwrap();
        }
    }

    #[test]
    fn analysis_slot_limit_uses_exact_allocated_identity_cardinality() {
        let fixture = repeated_add_fixture();
        let exact = fixture.body.block_counts().allocated
            + fixture.body.instruction_counts().allocated
            + fixture.body.value_counts().allocated;
        let mut rejected = fixture.body.clone();
        let before = format!("{rejected:#?}");
        let rejected_outcome = run_cse(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut rejected,
            CseLimits::derived().with_analysis_slot_limit(CseAnalysisSlotLimit::new(exact - 1)),
        )
        .unwrap();
        assert!(!rejected_outcome.changed);
        assert_eq!(
            rejected_outcome.completion,
            CseCompletion::StoppedAtLimit(CseLimitReason::AnalysisSize)
        );
        assert_eq!(rejected_outcome.statistics.planning_fact_snapshots_built, 0);
        assert_eq!(rejected_outcome.statistics.planning_dominator_builds, 0);
        assert_eq!(format!("{rejected:#?}"), before);

        let mut admitted = fixture.body;
        let admitted_outcome = run_cse(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut admitted,
            CseLimits::derived().with_analysis_slot_limit(CseAnalysisSlotLimit::new(exact)),
        )
        .unwrap();
        assert!(admitted_outcome.changed);
        assert_eq!(admitted_outcome.completion, CseCompletion::Complete);
        assert_eq!(admitted_outcome.statistics.planning_fact_snapshots_built, 1);
        assert_eq!(admitted_outcome.statistics.planning_dominator_builds, 1);
        assert_eq!(
            admitted_outcome
                .statistics
                .replacement_validation_dominator_builds,
            1
        );
        assert_eq!(admitted_outcome.statistics.post_replacement_use_audits, 1);
        verify_function(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &admitted,
        )
        .unwrap();
    }

    #[test]
    fn root_and_active_entry_limits_apply_only_complete_instruction_prefixes() {
        let fixture = repeated_add_fixture();
        for (limit, expected_roots) in [(0, 0), (1, 1)] {
            let mut body = fixture.body.clone();
            let before = format!("{body:#?}");
            let outcome = run_cse(
                &fixture.program,
                &fixture.sources,
                fixture.function,
                &mut body,
                CseLimits::derived().with_root_limit(CseRootLimit::new(limit)),
            )
            .unwrap();
            assert!(!outcome.changed);
            assert_eq!(outcome.statistics.roots_visited, expected_roots);
            assert_eq!(
                outcome.completion,
                CseCompletion::StoppedAtLimit(CseLimitReason::RootVisits)
            );
            assert_eq!(format!("{body:#?}"), before);
        }
        let mut exact_roots = fixture.body.clone();
        let exact_outcome = run_cse(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut exact_roots,
            CseLimits::derived().with_root_limit(CseRootLimit::new(2)),
        )
        .unwrap();
        assert!(exact_outcome.changed);
        assert_eq!(exact_outcome.completion, CseCompletion::Complete);
        assert_eq!(
            returned_values(&exact_roots, fixture.entry),
            &[fixture.representative]
        );

        let mut no_entries = fixture.body.clone();
        let no_entry_outcome = run_cse(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut no_entries,
            CseLimits::derived().with_active_entry_limit(CseActiveEntryLimit::new(0)),
        )
        .unwrap();
        assert!(!no_entry_outcome.changed);
        assert_eq!(no_entry_outcome.statistics.roots_visited, 1);
        assert_eq!(
            no_entry_outcome.completion,
            CseCompletion::StoppedAtLimit(CseLimitReason::ActiveEntries)
        );

        let mut one_entry = fixture.body;
        let one_entry_outcome = run_cse(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut one_entry,
            CseLimits::derived().with_active_entry_limit(CseActiveEntryLimit::new(1)),
        )
        .unwrap();
        assert!(one_entry_outcome.changed);
        assert_eq!(one_entry_outcome.completion, CseCompletion::Complete);
        assert_eq!(one_entry_outcome.statistics.maximum_active_entries, 1);
        assert!(
            !PlacementIndex::new(&one_entry)
                .is_instruction_attached(defining_instruction(&one_entry, fixture.duplicate))
        );
    }

    #[test]
    fn root_limit_stops_before_the_first_instruction_in_a_descendant_block() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("descendant-root-limit"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let child = builder.create_block(OriginId::UNKNOWN).unwrap();
        let representative = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(child, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(child).unwrap();
        let duplicate = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let duplicate_instruction = defining_instruction(builder.body(), duplicate);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let original = builder.finish().unwrap();

        let mut stopped = original.clone();
        let before = format!("{stopped:#?}");
        let stopped_outcome = run_cse(
            &program,
            &sources,
            function,
            &mut stopped,
            CseLimits::derived().with_root_limit(CseRootLimit::new(1)),
        )
        .unwrap();
        assert!(!stopped_outcome.changed);
        assert_eq!(stopped_outcome.statistics.roots_visited, 1);
        assert_eq!(stopped_outcome.statistics.table_insertions, 1);
        assert_eq!(stopped_outcome.statistics.table_hits, 0);
        assert_eq!(stopped_outcome.statistics.table_removals, 1);
        assert_eq!(
            stopped_outcome.completion,
            CseCompletion::StoppedAtLimit(CseLimitReason::RootVisits)
        );
        assert_eq!(format!("{stopped:#?}"), before);

        let mut admitted = original;
        let admitted_outcome = run_cse(
            &program,
            &sources,
            function,
            &mut admitted,
            CseLimits::derived().with_root_limit(CseRootLimit::new(2)),
        )
        .unwrap();
        assert!(admitted_outcome.changed);
        assert_eq!(admitted_outcome.completion, CseCompletion::Complete);
        assert_eq!(returned_values(&admitted, child), &[representative]);
        assert!(!PlacementIndex::new(&admitted).is_instruction_attached(duplicate_instruction));
        verify_function(&program, &sources, function, &admitted).unwrap();
        assert_eq!(admitted.entry(), entry);
    }

    #[test]
    fn forced_hash_collisions_and_randomized_seeds_produce_identical_output() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("collisions"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let left = parameter(&builder, entry, 0);
        let right = parameter(&builder, entry, 1);
        builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        let duplicate = builder
            .i32_add_wrapping(right, left, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![duplicate]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let original = builder.finish().unwrap();
        let mut randomized = original.clone();
        let mut collided = original;

        let randomized_outcome = run_cse(
            &program,
            &sources,
            function,
            &mut randomized,
            CseLimits::derived(),
        )
        .unwrap();
        let collided_outcome = run_cse_with_hasher(
            &program,
            &sources,
            function,
            &mut collided,
            CseLimits::derived(),
            CollisionBuildHasher::default(),
        )
        .unwrap();
        assert_eq!(randomized_outcome, collided_outcome);
        assert_eq!(format!("{randomized:#?}"), format!("{collided:#?}"));
        assert_eq!(randomized_outcome.statistics.table_insertions, 3);
        assert_eq!(randomized_outcome.statistics.table_hits, 1);
        verify_function(&program, &sources, function, &collided).unwrap();
    }

    #[test]
    fn allocated_detached_history_is_charged_but_never_scanned_as_a_root() {
        let fixture = repeated_add_fixture();
        let mut body = fixture.body;
        for _ in 0..1_000 {
            let block = body
                .blocks
                .push(BlockData {
                    origin: OriginId::UNKNOWN,
                    parameters: vec![],
                    instructions: vec![],
                    terminator: None,
                })
                .unwrap();
            let instruction = body
                .instructions
                .push(InstData {
                    op: CoreOp::I32Constant(17),
                    operands: vec![],
                    results: vec![],
                    origin: OriginId::UNKNOWN,
                })
                .unwrap();
            let value = body
                .values
                .push(ValueData {
                    ty: CoreType::I32,
                    definition: ValueDef::InstResult {
                        instruction,
                        result_index: 0,
                    },
                })
                .unwrap();
            body.instruction_mut(instruction)
                .unwrap()
                .results
                .push(value);
            let detached = body.block_mut(block).unwrap();
            detached.instructions.push(instruction);
            detached.terminator = Some(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ));
        }
        verify_function(&fixture.program, &fixture.sources, fixture.function, &body).unwrap();
        let attached_proxy = body.block_counts().attached
            + body.instruction_counts().attached
            + body.value_counts().attached;
        let before = format!("{body:#?}");
        let outcome = run_cse(
            &fixture.program,
            &fixture.sources,
            fixture.function,
            &mut body,
            CseLimits::derived()
                .with_analysis_slot_limit(CseAnalysisSlotLimit::new(attached_proxy)),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(
            outcome.completion,
            CseCompletion::StoppedAtLimit(CseLimitReason::AnalysisSize)
        );
        assert_eq!(outcome.statistics.planning_fact_snapshots_built, 0);
        assert_eq!(outcome.statistics.planning_dominator_builds, 0);
        assert_eq!(format!("{body:#?}"), before);
    }

    #[test]
    fn twenty_thousand_repeated_expressions_use_one_table_entry_and_two_batches() {
        const EXPRESSION_COUNT: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("repeated-scale"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let left = parameter(&builder, entry, 0);
        let right = parameter(&builder, entry, 1);
        let mut result = left;
        for _ in 0..EXPRESSION_COUNT {
            result = builder
                .i32_add_wrapping(left, right, OriginId::UNKNOWN)
                .unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let mut detailed_body = body.clone();
        let representative = body.block(entry).unwrap().instructions()[0];

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.completion, CseCompletion::Complete);
        assert_eq!(outcome.statistics.roots_visited, EXPRESSION_COUNT);
        assert_eq!(outcome.statistics.table_insertions, 1);
        assert_eq!(outcome.statistics.table_hits, EXPRESSION_COUNT - 1);
        assert_eq!(outcome.statistics.maximum_active_entries, 1);
        assert_eq!(outcome.statistics.replacement_batches, 1);
        assert_eq!(outcome.statistics.erasure_batches, 1);
        assert_eq!(body.instruction_counts().attached, 1);
        assert!(PlacementIndex::new(&body).is_instruction_attached(representative));
        verify_function(&program, &sources, function, &body).unwrap();

        // Model a record cap of one without retaining an all-site container. CSE
        // streams every successful correspondence from its existing dense table;
        // the consumer constructs detail only for the first record and counts the
        // rest as omitted.
        let mut submitted = 0_usize;
        let mut retained = None;
        let detailed_outcome = run_cse_with_rewrite_sink(
            &program,
            &sources,
            function,
            &mut detailed_body,
            CseLimits::derived(),
            |rewrite| {
                submitted += 1;
                if retained.is_none() {
                    retained = Some(snapshot_rewrite(rewrite));
                }
            },
        )
        .unwrap();
        assert_eq!(submitted, EXPRESSION_COUNT - 1);
        assert!(retained.is_some());
        assert_eq!(
            submitted - usize::from(retained.is_some()),
            EXPRESSION_COUNT - 2
        );
        assert_eq!(detailed_outcome, outcome);
        verify_function(&program, &sources, function, &detailed_body).unwrap();
    }

    #[test]
    fn twenty_thousand_block_dominator_chain_uses_no_host_recursion() {
        const BLOCK_COUNT: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("deep-scale"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let (mut body, _) = raw_deep_chain(&program, function, BLOCK_COUNT);
        verify_function(&program, &sources, function, &body).unwrap();

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.completion, CseCompletion::Complete);
        assert_eq!(outcome.statistics.roots_visited, BLOCK_COUNT);
        assert_eq!(outcome.statistics.table_insertions, 1);
        assert_eq!(outcome.statistics.table_hits, BLOCK_COUNT - 1);
        assert_eq!(outcome.statistics.table_removals, 1);
        assert_eq!(body.instruction_counts().attached, 1);
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn actual_wide_dominator_children_are_stable_block_ids() {
        const JOIN_COUNT: usize = 256;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("wide-scale"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = raw_wide_dominator_tree(JOIN_COUNT);
        verify_function(&program, &sources, function, &body).unwrap();
        let placement = PlacementIndex::new(&body);
        let cfg = ControlFlowGraph::from_placement(&placement);
        let dominators = DominatorTree::new(&cfg);
        let (children, instruction_count) = build_dominator_children(&body, &dominators).unwrap();
        assert_eq!(instruction_count, 0);
        let entry_children = &children[block_index(body.entry()).unwrap()];
        assert_eq!(entry_children.len(), JOIN_COUNT + 2);
        assert!(entry_children.windows(2).all(|pair| pair[0] < pair[1]));

        let outcome = run_cse(
            &program,
            &sources,
            function,
            &mut body,
            CseLimits::derived(),
        )
        .unwrap();
        assert!(!outcome.changed);
        assert_eq!(outcome.completion, CseCompletion::Complete);
        assert_eq!(outcome.statistics.planning_fact_snapshots_built, 1);
        assert_eq!(outcome.statistics.planning_dominator_builds, 1);
        assert_eq!(
            outcome.statistics.replacement_validation_dominator_builds,
            0
        );
        assert_eq!(outcome.statistics.post_replacement_use_audits, 0);
        assert_eq!(outcome.statistics.roots_visited, 0);
        assert_eq!(outcome.statistics.table_insertions, 0);
    }

    #[test]
    fn scoped_walk_handles_twenty_thousand_wide_children_without_host_recursion() {
        const CHILD_COUNT: usize = 20_000;

        let body = raw_return_blocks(CHILD_COUNT + 1);
        let mut children = vec![vec![]; body.blocks.len()];
        children[block_index(body.entry()).unwrap()] = body.blocks.keys().skip(1).collect();
        let mut planner = CsePlanner {
            body: &body,
            children,
            table: HashMap::with_hasher(RandomState::new()),
            undo: vec![],
            substitutions: PlannedSubstitutions::new(0),
            representatives: vec![],
            root_limit: 0,
            active_entry_limit: 0,
            completion: CseCompletion::Complete,
            statistics: CseStatistics {
                planning_fact_snapshots_built: 1,
                planning_dominator_builds: 1,
                ..CseStatistics::default()
            },
        };
        planner.walk_dominator_tree().unwrap();
        assert_eq!(planner.completion, CseCompletion::Complete);
        assert_eq!(planner.statistics.roots_visited, 0);
        assert!(planner.table.is_empty());
        assert!(planner.undo.is_empty());
    }
}
