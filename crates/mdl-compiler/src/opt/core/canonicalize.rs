//! Closed, bounded canonicalization for verified Core function bodies.

use std::error::Error;
use std::fmt;

use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, BlockTarget, ControlFlowGraph, CoreOp, CoreProgram, FunctionBody, FunctionEditor,
    FunctionId, I32Predicate, InstId, OperandSymmetry, PlacementIndex, Terminator, TerminatorKind,
    TypedCoreConstant, ValueDef, ValueId, ValueReplacement,
};
use crate::source::SourceContext;

/// Whether one pass-local limit is derived or explicitly supplied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CanonicalizationLimit<T> {
    Derived,
    Explicit(T),
}

/// Maximum combined allocated block, instruction, and value table entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanonicalizationTableLimit(usize);

impl CanonicalizationTableLimit {
    #[must_use]
    pub(super) const fn new(entries: usize) -> Self {
        Self(entries)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Maximum complete instruction/terminator roots visited by one sweep.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanonicalizationRootLimit(usize);

impl CanonicalizationRootLimit {
    #[must_use]
    pub(super) const fn new(roots: usize) -> Self {
        Self(roots)
    }

    const fn get(self) -> usize {
        self.0
    }
}

/// Pass-local typed limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanonicalizationLimits {
    table_entries: CanonicalizationLimit<CanonicalizationTableLimit>,
    root_visits: CanonicalizationLimit<CanonicalizationRootLimit>,
}

impl CanonicalizationLimits {
    pub(super) const fn derived() -> Self {
        Self {
            table_entries: CanonicalizationLimit::Derived,
            root_visits: CanonicalizationLimit::Derived,
        }
    }

    pub(super) const fn with_table_limit(mut self, limit: CanonicalizationTableLimit) -> Self {
        self.table_entries = CanonicalizationLimit::Explicit(limit);
        self
    }

    pub(super) const fn with_root_limit(mut self, limit: CanonicalizationRootLimit) -> Self {
        self.root_visits = CanonicalizationLimit::Explicit(limit);
        self
    }
}

/// The typed reason a canonicalization sweep stopped conservatively.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CanonicalizationLimitReason {
    DenseTables,
    RootVisits,
}

/// Whether the frozen sweep visited every reachable root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CanonicalizationCompletion {
    Complete,
    StoppedAtLimit(CanonicalizationLimitReason),
}

/// Deterministic counters produced without retaining per-site records.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct CanonicalizationStatistics {
    pub(super) roots_visited: usize,
    pub(super) value_replacements: usize,
    pub(super) constants_materialized: usize,
    pub(super) operands_reordered: usize,
    pub(super) branches_folded: usize,
    pub(super) instructions_erased: usize,
}

/// Result of one bounded canonicalization invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CanonicalizationOutcome {
    pub(super) changed: bool,
    pub(super) completion: CanonicalizationCompletion,
    pub(super) statistics: CanonicalizationStatistics,
}

/// Failure of a mandatory invariant or checked editor application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CanonicalizationError {
    SizeOverflow,
    InconsistentVerifiedBody(&'static str),
    ReplacementCycle { value: ValueId },
    Edit(crate::ir::core::EditError),
}

impl fmt::Display for CanonicalizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for CanonicalizationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Edit(error) => Some(error),
            Self::SizeOverflow
            | Self::InconsistentVerifiedBody(_)
            | Self::ReplacementCycle { .. } => None,
        }
    }
}

impl From<crate::ir::core::EditError> for CanonicalizationError {
    fn from(error: crate::ir::core::EditError) -> Self {
        Self::Edit(error)
    }
}

/// Runs one frozen cheap-canonicalization sweep on a verified body.
pub(super) fn run_canonicalization(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    limits: CanonicalizationLimits,
) -> Result<CanonicalizationOutcome, CanonicalizationError> {
    let block_count = body.blocks.len();
    let instruction_count = body.instructions.len();
    let value_count = body.values.len();
    let dense_entries = block_count
        .checked_add(instruction_count)
        .and_then(|count| count.checked_add(value_count))
        .ok_or(CanonicalizationError::SizeOverflow)?;
    if matches!(
        limits.table_entries,
        CanonicalizationLimit::Explicit(limit) if dense_entries > limit.get()
    ) {
        return Ok(CanonicalizationOutcome {
            changed: false,
            completion: CanonicalizationCompletion::StoppedAtLimit(
                CanonicalizationLimitReason::DenseTables,
            ),
            statistics: CanonicalizationStatistics::default(),
        });
    }

    let prepared = plan_canonicalization(
        body,
        limits.root_visits,
        block_count,
        instruction_count,
        value_count,
    )?;
    apply_canonicalization(program, sources, function, body, prepared)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VirtualValue {
    Existing(ValueId),
    Constant(TypedCoreConstant),
}

struct VirtualReplacements {
    mappings: Vec<Option<VirtualValue>>,
    seen_generation: Vec<u32>,
    generation: u32,
}

impl VirtualReplacements {
    fn new(value_count: usize) -> Self {
        Self {
            mappings: vec![None; value_count],
            seen_generation: vec![0; value_count],
            generation: 0,
        }
    }

    fn insert(
        &mut self,
        source: ValueId,
        replacement: VirtualValue,
    ) -> Result<(), CanonicalizationError> {
        let index = value_index(source)
            .and_then(|index| self.mappings.get_mut(index))
            .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                "canonicalization replacement source is invalid",
            ))?;
        if index.is_some() {
            return Err(CanonicalizationError::InconsistentVerifiedBody(
                "canonicalization replaced one SSA result twice",
            ));
        }
        *index = Some(replacement);
        Ok(())
    }

    fn contains_source(&self, value: ValueId) -> bool {
        value_index(value)
            .and_then(|index| self.mappings.get(index))
            .is_some_and(Option::is_some)
    }

    fn resolve(&mut self, value: ValueId) -> Result<VirtualValue, CanonicalizationError> {
        self.begin_generation();
        let generation = self.generation;
        let mut cursor = value;
        let endpoint = loop {
            let index = value_index(cursor)
                .filter(|index| *index < self.mappings.len())
                .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                    "canonicalization encountered an invalid SSA value",
                ))?;
            let Some(replacement) = self.mappings[index] else {
                break VirtualValue::Existing(cursor);
            };
            if self.seen_generation[index] == generation {
                return Err(CanonicalizationError::ReplacementCycle { value: cursor });
            }
            self.seen_generation[index] = generation;
            match replacement {
                VirtualValue::Existing(next) => cursor = next,
                VirtualValue::Constant(constant) => {
                    break VirtualValue::Constant(constant);
                }
            }
        };

        // Walk the marked path a second time and compress it in place. This keeps
        // resolution allocation-free even for very deep replacement chains.
        cursor = value;
        loop {
            let index = value_index(cursor)
                .filter(|index| *index < self.mappings.len())
                .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                    "canonicalization replacement path changed during resolution",
                ))?;
            if self.seen_generation[index] != generation {
                break;
            }
            let replacement =
                self.mappings[index].ok_or(CanonicalizationError::InconsistentVerifiedBody(
                    "canonicalization replacement path lost a mapping",
                ))?;
            self.mappings[index] = Some(endpoint);
            match replacement {
                VirtualValue::Existing(next) => cursor = next,
                VirtualValue::Constant(_) => break,
            }
        }
        Ok(endpoint)
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

struct PreparedCanonicalization {
    replacements: VirtualReplacements,
    operand_overrides: Vec<Option<Vec<ValueId>>>,
    terminator_overrides: Vec<Option<Terminator>>,
    erased: Vec<bool>,
    erased_instructions: Vec<InstId>,
    completion: CanonicalizationCompletion,
    statistics: CanonicalizationStatistics,
}

fn plan_canonicalization(
    body: &FunctionBody,
    configured_root_limit: CanonicalizationLimit<CanonicalizationRootLimit>,
    block_count: usize,
    instruction_count: usize,
    value_count: usize,
) -> Result<PreparedCanonicalization, CanonicalizationError> {
    let placement = PlacementIndex::new(body);
    let cfg = ControlFlowGraph::from_placement(&placement);
    let reverse_postorder = cfg.reverse_postorder();
    let reachable_instructions = reverse_postorder.iter().try_fold(0_usize, |count, block| {
        let instructions = body
            .block(*block)
            .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                "reachable block is absent",
            ))?
            .instructions()
            .len();
        count
            .checked_add(instructions)
            .ok_or(CanonicalizationError::SizeOverflow)
    })?;
    let total_roots = reachable_instructions
        .checked_add(reverse_postorder.len())
        .ok_or(CanonicalizationError::SizeOverflow)?;
    let root_limit = match configured_root_limit {
        CanonicalizationLimit::Derived => total_roots,
        CanonicalizationLimit::Explicit(limit) => limit.get(),
    };

    let mut prepared = PreparedCanonicalization {
        replacements: VirtualReplacements::new(value_count),
        operand_overrides: vec![None; instruction_count],
        terminator_overrides: vec![None; block_count],
        erased: vec![false; instruction_count],
        erased_instructions: vec![],
        completion: CanonicalizationCompletion::Complete,
        statistics: CanonicalizationStatistics::default(),
    };

    'instructions: for block in &reverse_postorder {
        let block_data =
            body.block(*block)
                .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                    "reachable block is absent",
                ))?;
        for instruction in block_data.instructions() {
            if prepared.statistics.roots_visited == root_limit {
                break 'instructions;
            }
            prepared.statistics.roots_visited += 1;
            match analyze_instruction(body, *instruction, &mut prepared.replacements)? {
                InstructionPlan::None => {}
                InstructionPlan::Replace(replacements) => {
                    let data = body.instruction(*instruction).ok_or(
                        CanonicalizationError::InconsistentVerifiedBody(
                            "reachable instruction is absent",
                        ),
                    )?;
                    if !data.op().is_trivially_discardable() {
                        return Err(CanonicalizationError::InconsistentVerifiedBody(
                            "canonicalization tried to erase a non-discardable operation",
                        ));
                    }
                    for (source, replacement) in replacements {
                        prepared.replacements.insert(source, replacement)?;
                    }
                    let index = instruction_index(*instruction).ok_or(
                        CanonicalizationError::InconsistentVerifiedBody(
                            "reachable instruction has an invalid ID",
                        ),
                    )?;
                    prepared.erased[index] = true;
                    prepared.erased_instructions.push(*instruction);
                }
                InstructionPlan::RewriteOperands(operands) => {
                    let index = instruction_index(*instruction).ok_or(
                        CanonicalizationError::InconsistentVerifiedBody(
                            "reachable instruction has an invalid ID",
                        ),
                    )?;
                    prepared.operand_overrides[index] = Some(operands);
                    prepared.statistics.operands_reordered += 1;
                }
            }
        }
    }

    if prepared.statistics.roots_visited == reachable_instructions {
        for block in &reverse_postorder {
            if prepared.statistics.roots_visited == root_limit {
                break;
            }
            prepared.statistics.roots_visited += 1;
            if let Some(replacement) = analyze_terminator(body, *block, &mut prepared.replacements)?
            {
                let index =
                    block_index(*block).ok_or(CanonicalizationError::InconsistentVerifiedBody(
                        "reachable block has an invalid ID",
                    ))?;
                prepared.terminator_overrides[index] = Some(replacement);
                prepared.statistics.branches_folded += 1;
            }
        }
    }

    if prepared.statistics.roots_visited < total_roots {
        prepared.completion =
            CanonicalizationCompletion::StoppedAtLimit(CanonicalizationLimitReason::RootVisits);
    }
    Ok(prepared)
}

enum InstructionPlan {
    None,
    Replace(Vec<(ValueId, VirtualValue)>),
    RewriteOperands(Vec<ValueId>),
}

fn analyze_instruction(
    body: &FunctionBody,
    instruction: InstId,
    replacements: &mut VirtualReplacements,
) -> Result<InstructionPlan, CanonicalizationError> {
    let data =
        body.instruction(instruction)
            .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                "reachable instruction is absent",
            ))?;
    let replacement = match data.op() {
        CoreOp::BoolNot => {
            let operand = only_operand(data.operands())?;
            let effective = replacements.resolve(operand)?;
            if let Some(TypedCoreConstant::Bool(value)) = known_constant(body, effective) {
                Some(vec![(
                    only_result(data.results())?,
                    VirtualValue::Constant(TypedCoreConstant::Bool(!value)),
                )])
            } else if let Some(inner_operand) = raw_bool_not_operand(body, operand) {
                Some(vec![(
                    only_result(data.results())?,
                    replacements.resolve(inner_operand)?,
                )])
            } else {
                None
            }
        }
        CoreOp::I32AddWrapping => {
            let [left, right] = two_operands(data.operands())?;
            let effective_left = replacements.resolve(left)?;
            let effective_right = replacements.resolve(right)?;
            if known_constant(body, effective_left) == Some(TypedCoreConstant::I32(0)) {
                Some(vec![(only_result(data.results())?, effective_right)])
            } else if known_constant(body, effective_right) == Some(TypedCoreConstant::I32(0)) {
                Some(vec![(only_result(data.results())?, effective_left)])
            } else {
                None
            }
        }
        CoreOp::I32AddOverflowing => {
            let [left, right] = two_operands(data.operands())?;
            let effective_left = replacements.resolve(left)?;
            let effective_right = replacements.resolve(right)?;
            let other = if known_constant(body, effective_left) == Some(TypedCoreConstant::I32(0)) {
                Some(effective_right)
            } else if known_constant(body, effective_right) == Some(TypedCoreConstant::I32(0)) {
                Some(effective_left)
            } else {
                None
            };
            if let Some(sum) = other {
                let [sum_result, overflow_result] = two_results(data.results())?;
                Some(vec![
                    (sum_result, sum),
                    (
                        overflow_result,
                        VirtualValue::Constant(TypedCoreConstant::Bool(false)),
                    ),
                ])
            } else {
                None
            }
        }
        CoreOp::I32Compare(predicate) => {
            let [left, right] = two_operands(data.operands())?;
            let effective_left = replacements.resolve(left)?;
            let effective_right = replacements.resolve(right)?;
            if effective_left == effective_right {
                Some(vec![(
                    only_result(data.results())?,
                    VirtualValue::Constant(TypedCoreConstant::Bool(reflexive_result(*predicate))),
                )])
            } else {
                None
            }
        }
        CoreOp::BoolConstant(_) | CoreOp::I32Constant(_) | CoreOp::Call(_) => None,
    };
    if let Some(replacement) = replacement {
        return Ok(InstructionPlan::Replace(replacement));
    }

    match data.op().operand_symmetry() {
        OperandSymmetry::Ordered => Ok(InstructionPlan::None),
        OperandSymmetry::CommutativePair => {
            let [left, right] = two_operands(data.operands())?;
            let left_key = order_key(body, replacements.resolve(left)?, left)?;
            let right_key = order_key(body, replacements.resolve(right)?, right)?;
            if left_key > right_key {
                Ok(InstructionPlan::RewriteOperands(vec![right, left]))
            } else {
                Ok(InstructionPlan::None)
            }
        }
    }
}

fn analyze_terminator(
    body: &FunctionBody,
    block: BlockId,
    replacements: &mut VirtualReplacements,
) -> Result<Option<Terminator>, CanonicalizationError> {
    let data = body
        .block(block)
        .ok_or(CanonicalizationError::InconsistentVerifiedBody(
            "reachable block is absent",
        ))?;
    let terminator = data
        .terminator()
        .ok_or(CanonicalizationError::InconsistentVerifiedBody(
            "reachable block has no terminator",
        ))?;
    let TerminatorKind::Branch {
        then_target,
        else_target,
        ..
    } = terminator.kind()
    else {
        return Ok(None);
    };
    if then_target.block() != else_target.block()
        || then_target.arguments().len() != else_target.arguments().len()
    {
        return Ok(None);
    }
    for (then_value, else_value) in then_target
        .arguments()
        .iter()
        .copied()
        .zip(else_target.arguments().iter().copied())
    {
        if replacements.resolve(then_value)? != replacements.resolve(else_value)? {
            return Ok(None);
        }
    }
    Ok(Some(Terminator::new(
        TerminatorKind::Jump(BlockTarget::new(
            then_target.block(),
            then_target.arguments().to_vec(),
        )),
        terminator.origin(),
    )))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EffectiveOrder {
    Existing(ValueId),
    Bool(bool),
    I32(i32),
}

fn order_key(
    body: &FunctionBody,
    effective: VirtualValue,
    original: ValueId,
) -> Result<(EffectiveOrder, ValueId), CanonicalizationError> {
    let effective = match known_constant(body, effective) {
        Some(TypedCoreConstant::Bool(value)) => EffectiveOrder::Bool(value),
        Some(TypedCoreConstant::I32(value)) => EffectiveOrder::I32(value),
        None => match effective {
            VirtualValue::Existing(value) => EffectiveOrder::Existing(value),
            VirtualValue::Constant(_) => {
                return Err(CanonicalizationError::InconsistentVerifiedBody(
                    "typed virtual constant could not be classified",
                ));
            }
        },
    };
    Ok((effective, original))
}

fn known_constant(body: &FunctionBody, value: VirtualValue) -> Option<TypedCoreConstant> {
    match value {
        VirtualValue::Constant(constant) => Some(constant),
        VirtualValue::Existing(value) => constant_definition(body, value),
    }
}

fn constant_definition(body: &FunctionBody, value: ValueId) -> Option<TypedCoreConstant> {
    let ValueDef::InstResult {
        instruction,
        result_index: 0,
    } = body.value(value)?.definition()
    else {
        return None;
    };
    let data = body.instruction(instruction)?;
    if data.results().first().copied() != Some(value) {
        return None;
    }
    match data.op() {
        CoreOp::BoolConstant(value) => Some(TypedCoreConstant::Bool(*value)),
        CoreOp::I32Constant(value) => Some(TypedCoreConstant::I32(*value)),
        CoreOp::I32AddWrapping
        | CoreOp::I32AddOverflowing
        | CoreOp::I32Compare(_)
        | CoreOp::BoolNot
        | CoreOp::Call(_) => None,
    }
}

fn raw_bool_not_operand(body: &FunctionBody, value: ValueId) -> Option<ValueId> {
    let ValueDef::InstResult {
        instruction,
        result_index: 0,
    } = body.value(value)?.definition()
    else {
        return None;
    };
    let data = body.instruction(instruction)?;
    if !matches!(data.op(), CoreOp::BoolNot) || data.results().first().copied() != Some(value) {
        return None;
    }
    data.operands().first().copied()
}

const fn reflexive_result(predicate: I32Predicate) -> bool {
    match predicate {
        I32Predicate::Eq | I32Predicate::SignedLe | I32Predicate::SignedGe => true,
        I32Predicate::Ne | I32Predicate::SignedLt | I32Predicate::SignedGt => false,
    }
}

fn apply_canonicalization(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &mut FunctionBody,
    mut prepared: PreparedCanonicalization,
) -> Result<CanonicalizationOutcome, CanonicalizationError> {
    let live_sources = find_live_replacement_sources(body, &prepared)?;
    let mut value_edits = vec![];
    let mut constants_materialized = 0_usize;
    for (source, _) in body.values() {
        let Some(index) = value_index(source) else {
            continue;
        };
        if !live_sources.get(index).copied().unwrap_or(false) {
            continue;
        }
        let replacement = match prepared.replacements.resolve(source)? {
            VirtualValue::Existing(value) => ValueReplacement::Existing(value),
            VirtualValue::Constant(constant) => {
                constants_materialized += 1;
                ValueReplacement::Constant(constant)
            }
        };
        value_edits.push((source, replacement));
    }

    let terminator_edits = prepared
        .terminator_overrides
        .into_iter()
        .enumerate()
        .filter_map(|(index, terminator)| terminator.map(|terminator| (index, terminator)))
        .map(|(index, terminator)| {
            let index = u32::try_from(index).map_err(|_| CanonicalizationError::SizeOverflow)?;
            Ok((BlockId::from_index(index), terminator))
        })
        .collect::<Result<Vec<_>, CanonicalizationError>>()?;
    let operand_edits = prepared
        .operand_overrides
        .into_iter()
        .enumerate()
        .filter_map(|(index, operands)| operands.map(|operands| (index, operands)))
        .map(|(index, operands)| {
            let index = u32::try_from(index).map_err(|_| CanonicalizationError::SizeOverflow)?;
            Ok((InstId::from_index(index), operands))
        })
        .collect::<Result<Vec<_>, CanonicalizationError>>()?;

    prepared.statistics.value_replacements = value_edits.len();
    prepared.statistics.constants_materialized = constants_materialized;
    prepared.statistics.instructions_erased = prepared.erased_instructions.len();
    let changed = !terminator_edits.is_empty()
        || !operand_edits.is_empty()
        || !value_edits.is_empty()
        || !prepared.erased_instructions.is_empty();

    let mut editor = FunctionEditor::from_trusted_body(program, sources, function, body);
    if !terminator_edits.is_empty() {
        editor.set_terminators_batch(terminator_edits)?;
    }
    if !operand_edits.is_empty() {
        editor.rewrite_inst_operands_batch(operand_edits)?;
    }
    if !value_edits.is_empty() {
        editor.replace_values_batch(value_edits)?;
    }
    if !prepared.erased_instructions.is_empty() {
        editor.erase_discardable_inst_set(prepared.erased_instructions)?;
    }

    Ok(CanonicalizationOutcome {
        changed,
        completion: prepared.completion,
        statistics: prepared.statistics,
    })
}

fn find_live_replacement_sources(
    body: &FunctionBody,
    prepared: &PreparedCanonicalization,
) -> Result<Vec<bool>, CanonicalizationError> {
    let mut live = vec![false; prepared.replacements.mappings.len()];
    let mark = |value: ValueId, live: &mut [bool]| {
        if prepared.replacements.contains_source(value) {
            if let Some(slot) = value_index(value).and_then(|index| live.get_mut(index)) {
                *slot = true;
            }
        }
    };
    for block in body.block_order() {
        let data = body
            .block(*block)
            .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                "attached block is absent",
            ))?;
        for instruction in data.instructions() {
            let instruction_index = instruction_index(*instruction).ok_or(
                CanonicalizationError::InconsistentVerifiedBody(
                    "attached instruction has an invalid ID",
                ),
            )?;
            if prepared.erased.get(instruction_index).copied() == Some(true) {
                continue;
            }
            let operands = prepared.operand_overrides[instruction_index]
                .as_deref()
                .or_else(|| {
                    body.instruction(*instruction)
                        .map(crate::ir::core::InstData::operands)
                })
                .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                    "attached instruction is absent",
                ))?;
            for operand in operands {
                mark(*operand, &mut live);
            }
        }
        let block_index = block_index(*block).ok_or(
            CanonicalizationError::InconsistentVerifiedBody("attached block has an invalid ID"),
        )?;
        let terminator = prepared.terminator_overrides[block_index]
            .as_ref()
            .or_else(|| data.terminator())
            .ok_or(CanonicalizationError::InconsistentVerifiedBody(
                "attached block has no terminator",
            ))?;
        for_each_terminator_value(terminator.kind(), |value| mark(value, &mut live));
    }
    Ok(live)
}

fn for_each_terminator_value(kind: &TerminatorKind, mut visit: impl FnMut(ValueId)) {
    match kind {
        TerminatorKind::Jump(target) => {
            for value in target.arguments() {
                visit(*value);
            }
        }
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => {
            visit(*condition);
            for value in then_target.arguments() {
                visit(*value);
            }
            for value in else_target.arguments() {
                visit(*value);
            }
        }
        TerminatorKind::Return(values) => {
            for value in values {
                visit(*value);
            }
        }
        TerminatorKind::Unreachable => {}
    }
}

fn only_operand(operands: &[ValueId]) -> Result<ValueId, CanonicalizationError> {
    match operands {
        [operand] => Ok(*operand),
        _ => Err(CanonicalizationError::InconsistentVerifiedBody(
            "verified unary operation has the wrong operand count",
        )),
    }
}

fn two_operands(operands: &[ValueId]) -> Result<[ValueId; 2], CanonicalizationError> {
    match operands {
        [left, right] => Ok([*left, *right]),
        _ => Err(CanonicalizationError::InconsistentVerifiedBody(
            "verified binary operation has the wrong operand count",
        )),
    }
}

fn only_result(results: &[ValueId]) -> Result<ValueId, CanonicalizationError> {
    match results {
        [result] => Ok(*result),
        _ => Err(CanonicalizationError::InconsistentVerifiedBody(
            "verified scalar operation has the wrong result count",
        )),
    }
}

fn two_results(results: &[ValueId]) -> Result<[ValueId; 2], CanonicalizationError> {
    match results {
        [first, second] => Ok([*first, *second]),
        _ => Err(CanonicalizationError::InconsistentVerifiedBody(
            "verified multi-result operation has the wrong result count",
        )),
    }
}

fn value_index(value: ValueId) -> Option<usize> {
    usize::try_from(value.index()).ok()
}

fn instruction_index(instruction: InstId) -> Option<usize> {
    usize::try_from(instruction.index()).ok()
}

fn block_index(block: BlockId) -> Option<usize> {
    usize::try_from(block.index()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::core::{
        CoreType, FunctionBuilder, PlacementIndex, Terminator, TerminatorKind, verify_function,
    };
    use crate::source::OriginId;

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

    fn i32_identity_fixture() -> (
        SourceContext,
        CoreProgram,
        FunctionId,
        FunctionBody,
        BlockId,
        ValueId,
    ) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("limits"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = parameter(&builder, entry, 0);
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let identity = builder
            .i32_add_wrapping(input, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![identity]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        (sources, program, function, body, entry, input)
    }

    #[test]
    fn folds_triple_and_quadruple_not_without_internal_constant_history() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("not-chain"),
                vec![],
                vec![CoreType::Bool, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let constant = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let first = builder.bool_not(constant, OriginId::UNKNOWN).unwrap();
        let second = builder.bool_not(first, OriginId::UNKNOWN).unwrap();
        let third = builder.bool_not(second, OriginId::UNKNOWN).unwrap();
        let fourth = builder.bool_not(third, OriginId::UNKNOWN).unwrap();
        let not_instructions =
            [first, second, third, fourth].map(|value| defining_instruction(builder.body(), value));
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![third, fourth]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let allocated_before = body.value_counts().allocated;

        let outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.completion, CanonicalizationCompletion::Complete);
        assert_eq!(outcome.statistics.constants_materialized, 2);
        assert_eq!(outcome.statistics.instructions_erased, 4);
        assert_eq!(body.value_counts().allocated, allocated_before + 2);
        let placement = PlacementIndex::new(&body);
        assert!(
            not_instructions
                .into_iter()
                .all(|instruction| !placement.is_instruction_attached(instruction))
        );
        let returned = returned_values(&body, entry);
        assert_eq!(
            constant_definition(&body, returned[0]),
            Some(TypedCoreConstant::Bool(false))
        );
        assert_eq!(
            constant_definition(&body, returned[1]),
            Some(TypedCoreConstant::Bool(true))
        );
        verify_function(&program, &sources, function, &body).unwrap();

        let second = run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        assert!(!second.changed);
        assert_eq!(second.completion, CanonicalizationCompletion::Complete);
    }

    #[test]
    fn zero_identities_compose_and_replace_the_complete_overflow_tuple() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("zero"),
                vec![CoreType::I32],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = parameter(&builder, entry, 0);
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let wrapping = builder
            .i32_add_wrapping(input, zero, OriginId::UNKNOWN)
            .unwrap();
        let (sum, overflow) = builder
            .i32_add_overflowing(zero, wrapping, OriginId::UNKNOWN)
            .unwrap();
        let wrapping_instruction = defining_instruction(builder.body(), wrapping);
        let overflowing_instruction = defining_instruction(builder.body(), sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum, overflow]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        assert!(outcome.changed);
        assert_eq!(outcome.statistics.value_replacements, 2);
        assert_eq!(outcome.statistics.constants_materialized, 1);
        let returned = returned_values(&body, entry);
        assert_eq!(returned[0], input);
        assert_eq!(
            constant_definition(&body, returned[1]),
            Some(TypedCoreConstant::Bool(false))
        );
        let placement = PlacementIndex::new(&body);
        assert!(!placement.is_instruction_attached(wrapping_instruction));
        assert!(!placement.is_instruction_attached(overflowing_instruction));
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn reflexive_comparisons_cover_every_signed_predicate() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("reflexive"),
                vec![CoreType::I32],
                vec![CoreType::Bool; 6],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let value = parameter(&builder, entry, 0);
        let predicates = [
            I32Predicate::Eq,
            I32Predicate::Ne,
            I32Predicate::SignedLt,
            I32Predicate::SignedLe,
            I32Predicate::SignedGt,
            I32Predicate::SignedGe,
        ];
        let results = predicates
            .iter()
            .copied()
            .map(|predicate| {
                builder
                    .i32_compare(predicate, value, value, OriginId::UNKNOWN)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(results),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        let expected = [true, false, false, true, false, true];
        for (value, expected) in returned_values(&body, entry).iter().zip(expected) {
            assert_eq!(
                constant_definition(&body, *value),
                Some(TypedCoreConstant::Bool(expected))
            );
        }
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn commutative_order_puts_nonconstants_first_then_uses_stable_ids() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("order"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let first = parameter(&builder, entry, 0);
        let second = parameter(&builder, entry, 1);
        let constant = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let by_id = builder
            .i32_add_wrapping(second, first, OriginId::UNKNOWN)
            .unwrap();
        let by_kind = builder
            .i32_add_wrapping(constant, first, OriginId::UNKNOWN)
            .unwrap();
        let by_id_instruction = defining_instruction(builder.body(), by_id);
        let by_kind_instruction = defining_instruction(builder.body(), by_kind);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![by_id, by_kind]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.statistics.operands_reordered, 2);
        assert_eq!(
            body.instruction(by_id_instruction).unwrap().operands(),
            &[first, second]
        );
        assert_eq!(
            body.instruction(by_kind_instruction).unwrap().operands(),
            &[first, constant]
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn branch_arm_equality_uses_the_completed_virtual_map() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("branch"),
                vec![CoreType::Bool, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = parameter(&builder, entry, 0);
        let input = parameter(&builder, entry, 1);
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join_value = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let identity = builder
            .i32_add_wrapping(input, zero, OriginId::UNKNOWN)
            .unwrap();
        let identity_instruction = defining_instruction(builder.body(), identity);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(join, vec![identity]),
                    else_target: BlockTarget::new(join, vec![input]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![join_value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.statistics.branches_folded, 1);
        let TerminatorKind::Jump(target) = body.block(entry).unwrap().terminator().unwrap().kind()
        else {
            panic!("expected folded jump");
        };
        assert_eq!(target.block(), join);
        assert_eq!(target.arguments(), &[input]);
        assert!(!PlacementIndex::new(&body).is_instruction_attached(identity_instruction));
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn reverse_postorder_not_layout_orders_planning_and_unreachable_roots_are_skipped() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("rpo"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let input = parameter(&builder, entry, 0);
        // Deliberately allocate the use before its definition in layout order.
        let use_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let definition_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let unreachable_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(definition_block, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(definition_block).unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(use_block, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(use_block).unwrap();
        let reachable_identity = builder
            .i32_add_wrapping(input, zero, OriginId::UNKNOWN)
            .unwrap();
        let reachable_instruction = defining_instruction(builder.body(), reachable_identity);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![reachable_identity]),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(unreachable_block).unwrap();
        let unreachable_zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let unreachable_identity = builder
            .i32_add_wrapping(input, unreachable_zero, OriginId::UNKNOWN)
            .unwrap();
        let unreachable_instruction = defining_instruction(builder.body(), unreachable_identity);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![unreachable_identity]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        assert_eq!(
            body.block_order(),
            &[entry, use_block, definition_block, unreachable_block]
        );

        let outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.statistics.roots_visited, 5);
        assert_eq!(returned_values(&body, use_block), &[input]);
        let placement = PlacementIndex::new(&body);
        assert!(!placement.is_instruction_attached(reachable_instruction));
        assert!(placement.is_instruction_attached(unreachable_instruction));
        assert_eq!(
            body.instruction(unreachable_instruction)
                .unwrap()
                .operands(),
            &[input, unreachable_zero]
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn twenty_thousand_folded_roots_materialize_only_the_live_out_constant() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("canonical-scale"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let mut value = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        for _ in 0..20_000 {
            value = builder.bool_not(value, OriginId::UNKNOWN).unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let allocated_values = body.value_counts().allocated;

        let outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut body,
            CanonicalizationLimits::derived(),
        )
        .unwrap();
        assert_eq!(outcome.completion, CanonicalizationCompletion::Complete);
        assert_eq!(outcome.statistics.instructions_erased, 20_000);
        assert_eq!(outcome.statistics.constants_materialized, 1);
        assert_eq!(body.value_counts().allocated, allocated_values + 1);
        assert_eq!(body.instruction_counts().attached, 2);
        assert_eq!(
            constant_definition(&body, returned_values(&body, entry)[0]),
            Some(TypedCoreConstant::Bool(false))
        );
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn table_and_root_limits_apply_only_verified_unchanged_or_whole_root_prefixes() {
        let (sources, program, function, original, entry, input) = i32_identity_fixture();
        let dense_entries = original.block_counts().allocated
            + original.instruction_counts().allocated
            + original.value_counts().allocated;

        let mut table_limited = original.clone();
        let before = format!("{table_limited:#?}");
        let table_outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut table_limited,
            CanonicalizationLimits::derived().with_table_limit(CanonicalizationTableLimit::new(0)),
        )
        .unwrap();
        assert_eq!(
            table_outcome.completion,
            CanonicalizationCompletion::StoppedAtLimit(CanonicalizationLimitReason::DenseTables)
        );
        assert!(!table_outcome.changed);
        assert_eq!(format!("{table_limited:#?}"), before);

        let mut exact_table = original.clone();
        let exact_table_outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut exact_table,
            CanonicalizationLimits::derived()
                .with_table_limit(CanonicalizationTableLimit::new(dense_entries)),
        )
        .unwrap();
        assert!(exact_table_outcome.changed);
        assert_eq!(
            exact_table_outcome.completion,
            CanonicalizationCompletion::Complete
        );
        assert_eq!(returned_values(&exact_table, entry), &[input]);

        let mut one_root = original.clone();
        let one_outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut one_root,
            CanonicalizationLimits::derived().with_root_limit(CanonicalizationRootLimit::new(1)),
        )
        .unwrap();
        assert!(!one_outcome.changed);
        assert_eq!(
            one_outcome.completion,
            CanonicalizationCompletion::StoppedAtLimit(CanonicalizationLimitReason::RootVisits)
        );
        assert_eq!(one_outcome.statistics.roots_visited, 1);
        assert_eq!(format!("{one_root:#?}"), format!("{original:#?}"));

        let mut two_roots = original.clone();
        let two_outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut two_roots,
            CanonicalizationLimits::derived().with_root_limit(CanonicalizationRootLimit::new(2)),
        )
        .unwrap();
        assert!(two_outcome.changed);
        assert_eq!(
            two_outcome.completion,
            CanonicalizationCompletion::StoppedAtLimit(CanonicalizationLimitReason::RootVisits)
        );
        assert_eq!(two_outcome.statistics.roots_visited, 2);
        assert_eq!(returned_values(&two_roots, entry), &[input]);
        verify_function(&program, &sources, function, &two_roots).unwrap();

        let mut exact_roots = original;
        let exact_root_outcome = run_canonicalization(
            &program,
            &sources,
            function,
            &mut exact_roots,
            CanonicalizationLimits::derived().with_root_limit(CanonicalizationRootLimit::new(3)),
        )
        .unwrap();
        assert!(exact_root_outcome.changed);
        assert_eq!(
            exact_root_outcome.completion,
            CanonicalizationCompletion::Complete
        );
        assert_eq!(exact_root_outcome.statistics.roots_visited, 3);
        assert_eq!(returned_values(&exact_roots, entry), &[input]);
        verify_function(&program, &sources, function, &exact_roots).unwrap();
    }
}
