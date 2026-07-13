//! Atomic in-place edits guarded by type, dominance, placement, and effect checks.

use std::error::Error;
use std::fmt;

use super::analysis::definition_block;
use super::{
    BlockId, BlockTarget, ControlFlowGraph, CoreProgram, CoreType, Diagnostic, Diagnostics,
    Dominance, DominatorTree, EffectClass, FunctionBody, FunctionId, InstData, InstId,
    PlacementIndex, Reachability, Terminator, TerminatorKind, TypedCoreConstant, UseIndex, UseSite,
    ValueData, ValueDef, ValueId, verify_function,
};
use crate::entity::EntityId;
use crate::source::{OriginId, SourceContext};

const ENTITY_CAPACITY: u64 = u32::MAX as u64 + 1;

type OperandEdit = (InstId, Vec<ValueId>);
type BlockInstructionUpdate = (BlockId, Vec<InstId>);
type PreparedBlockLayout = Vec<BlockInstructionUpdate>;

/// Final intent for one source value in an atomic replacement batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueReplacement {
    /// Resolve through another existing value. Chains are flattened before commit.
    Existing(ValueId),
    /// Materialize a fresh constant at the source's definition site.
    Constant(TypedCoreConstant),
}

/// Minimal authoritative description of one maximal straight-line jump region.
///
/// The first block survives. Every later block is consumed in the listed CFG
/// order; instruction lists, terminators, and parameter substitutions are always
/// re-derived from the editor-owned body snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct JumpRegionEdit {
    pub(crate) blocks: Vec<BlockId>,
}

impl JumpRegionEdit {
    pub(crate) const fn new(blocks: Vec<BlockId>) -> Self {
        Self { blocks }
    }
}

/// Exact fixed-table work performed while preparing jump fusion.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct JumpFusionFactStatistics {
    pub(crate) fact_builds: usize,
    pub(crate) required_fact_table_entries: usize,
    pub(crate) attached_edges_counted: usize,
    pub(crate) raw_instruction_memberships: usize,
}

/// Result of the editor's optional dense-fact-table admission check.
#[allow(
    clippy::large_enum_variant,
    reason = "boxing the exclusive prepared facts would add an unbudgeted allocation"
)]
pub(crate) enum JumpFusionPreparation<'editor> {
    Ready(PreparedJumpFusion<'editor>),
    StoppedAtFactTableLimit {
        statistics: JumpFusionFactStatistics,
    },
}

/// One bounded, head-first region discovery result.
pub(crate) struct JumpFusionDiscovery {
    pub(crate) regions: Vec<JumpRegionEdit>,
    pub(crate) block_visits: usize,
    pub(crate) stopped_at_block_visit_limit: bool,
}

/// Exact structural work performed by one atomic fusion batch.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct JumpFusionApplicationStatistics {
    pub(crate) regions_fused: usize,
    pub(crate) blocks_consumed: usize,
    pub(crate) selected_instruction_memberships_preflighted: usize,
    pub(crate) parameter_mapping_slots_used: usize,
    pub(crate) replacement_links_visited: usize,
    pub(crate) attached_values_scanned: usize,
    pub(crate) attached_rewrite_scans: usize,
    pub(crate) head_reservations: usize,
    pub(crate) layout_retain_scans: usize,
}

/// Exact evidence for selected generic-batch scan boundaries.
///
/// This is deliberately not a claim that every preparation loop has been counted.
/// It records the placement construction, the prepared commit, and erasure's two
/// required attached-layout preflights at their authoritative loops. The structure
/// remains crate-private evidence for pass and scale tests rather than a public
/// editing contract.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BatchEditStatistics {
    pub(crate) placement_indices_built: usize,
    pub(crate) erasure_ownership_scans: usize,
    pub(crate) erasure_ownership_memberships_scanned: usize,
    pub(crate) erasure_closure_scans: usize,
    pub(crate) erasure_closure_instruction_memberships_scanned: usize,
    pub(crate) erasure_closure_blocks_scanned: usize,
    pub(crate) application_scans: usize,
    pub(crate) attached_instruction_memberships_scanned: usize,
    pub(crate) attached_blocks_scanned: usize,
}

trait BatchEditInstrumentation {
    fn placement_index_built(&mut self) {}

    fn erasure_ownership_scan_started(&mut self) {}

    fn erasure_ownership_membership_scanned(&mut self) {}

    fn erasure_closure_scan_started(&mut self) {}

    fn erasure_closure_instruction_membership_scanned(&mut self) {}

    fn erasure_closure_block_scanned(&mut self) {}

    fn application_scan_started(&mut self) {}

    fn attached_instruction_membership_scanned(&mut self) {}

    fn attached_block_scanned(&mut self) {}
}

struct NoBatchEditInstrumentation;

impl BatchEditInstrumentation for NoBatchEditInstrumentation {}

#[cfg(test)]
impl BatchEditInstrumentation for BatchEditStatistics {
    fn placement_index_built(&mut self) {
        self.placement_indices_built += 1;
    }

    fn erasure_ownership_scan_started(&mut self) {
        self.erasure_ownership_scans += 1;
    }

    fn erasure_ownership_membership_scanned(&mut self) {
        self.erasure_ownership_memberships_scanned += 1;
    }

    fn erasure_closure_scan_started(&mut self) {
        self.erasure_closure_scans += 1;
    }

    fn erasure_closure_instruction_membership_scanned(&mut self) {
        self.erasure_closure_instruction_memberships_scanned += 1;
    }

    fn erasure_closure_block_scanned(&mut self) {
        self.erasure_closure_blocks_scanned += 1;
    }

    fn application_scan_started(&mut self) {
        self.application_scans += 1;
    }

    fn attached_instruction_membership_scanned(&mut self) {
        self.attached_instruction_memberships_scanned += 1;
    }

    fn attached_block_scanned(&mut self) {
        self.attached_blocks_scanned += 1;
    }
}

/// A body-scoped editing surface.
pub struct FunctionEditor<'a> {
    program: &'a CoreProgram,
    sources: &'a SourceContext,
    function: FunctionId,
    body: &'a mut FunctionBody,
}

impl<'a> FunctionEditor<'a> {
    /// Opens an already-valid body for editing.
    ///
    /// # Errors
    ///
    /// Returns verifier diagnostics if the input body is not a valid pass boundary.
    pub fn new(
        program: &'a CoreProgram,
        sources: &'a SourceContext,
        function: FunctionId,
        body: &'a mut FunctionBody,
    ) -> Result<Self, EditError> {
        verify_function(program, sources, function, body).map_err(EditError::InvalidInput)?;
        Ok(Self::from_trusted_body(program, sources, function, body))
    }

    /// Opens a body already verified by the enclosing pass pipeline.
    ///
    /// This is crate-private so public callers cannot bypass the checked boundary.
    pub(crate) const fn from_trusted_body(
        program: &'a CoreProgram,
        sources: &'a SourceContext,
        function: FunctionId,
        body: &'a mut FunctionBody,
    ) -> Self {
        Self {
            program,
            sources,
            function,
            body,
        }
    }

    /// Returns a read-only view of the body being edited.
    #[must_use]
    pub const fn body(&self) -> &FunctionBody {
        self.body
    }

    /// Replaces every attached use of one value.
    ///
    /// # Errors
    ///
    /// Returns any atomic batch validation error. An explicit self replacement is
    /// a cycle; callers must omit no-op mappings.
    pub fn replace_value(&mut self, old: ValueId, new: ValueId) -> Result<(), EditError> {
        self.replace_values_batch(vec![(old, ValueReplacement::Existing(new))])
    }

    /// Resolves and applies a set of value replacements as one atomic edit.
    ///
    /// Input order never controls allocation or application order. Replacement
    /// chains are resolved iteratively, and every source ending in a constant gets
    /// its own constant at that source's definition site.
    ///
    /// # Errors
    ///
    /// Returns before mutation for duplicate sources, cycles, invalid or detached
    /// definitions, type/dominance failures, or entity-capacity exhaustion.
    pub fn replace_values_batch(
        &mut self,
        edits: Vec<(ValueId, ValueReplacement)>,
    ) -> Result<(), EditError> {
        let mut instrumentation = NoBatchEditInstrumentation;
        let prepared = self.prepare_value_batch(edits, ENTITY_CAPACITY, &mut instrumentation)?;
        prepared.apply(self.body, &mut instrumentation);
        Ok(())
    }

    /// Applies one value batch and returns counters tied to its actual scan loops.
    #[cfg(test)]
    pub(crate) fn replace_values_batch_with_statistics(
        &mut self,
        edits: Vec<(ValueId, ValueReplacement)>,
    ) -> Result<BatchEditStatistics, EditError> {
        let mut statistics = BatchEditStatistics::default();
        let prepared = self.prepare_value_batch(edits, ENTITY_CAPACITY, &mut statistics)?;
        prepared.apply(self.body, &mut statistics);
        Ok(statistics)
    }

    #[cfg(test)]
    fn replace_values_batch_with_entity_limit(
        &mut self,
        edits: Vec<(ValueId, ValueReplacement)>,
        maximum_entities: u64,
    ) -> Result<(), EditError> {
        let mut instrumentation = NoBatchEditInstrumentation;
        let prepared = self.prepare_value_batch(edits, maximum_entities, &mut instrumentation)?;
        prepared.apply(self.body, &mut instrumentation);
        Ok(())
    }

    fn prepare_value_batch<I: BatchEditInstrumentation>(
        &self,
        edits: Vec<(ValueId, ValueReplacement)>,
        maximum_entities: u64,
        instrumentation: &mut I,
    ) -> Result<PreparedValueBatch, EditError> {
        if edits.is_empty() {
            return Ok(PreparedValueBatch::empty());
        }

        instrumentation.placement_index_built();
        let placement = PlacementIndex::new(self.body);
        let (mut sources, resolved) = normalize_value_replacements(self.body, &placement, edits)?;
        let layout_positions = block_layout_positions(self.body);
        sources.sort_by_key(|source| {
            definition_site_key(self.body, &placement, &layout_positions, *source)
        });
        validate_resolved_replacements(self.body, &placement, &sources, &resolved)?;
        let materialized = prepare_constant_replacements(
            self.body,
            &placement,
            &sources,
            &resolved,
            maximum_entities,
        )?;
        let block_instruction_updates =
            prepare_inserted_layout(self.body, &materialized.block_insertions)?;

        Ok(PreparedValueBatch {
            final_values: materialized.final_values,
            new_instructions: materialized.new_instructions,
            new_values: materialized.new_values,
            block_instruction_updates,
            attached_blocks: self.body.block_order.clone(),
        })
    }

    /// Rewrites complete operand lists for attached instructions atomically.
    ///
    /// # Errors
    ///
    /// Returns before mutation for duplicate/detached instructions, invalid
    /// contracts, types, definitions, order, or dominance.
    pub fn rewrite_inst_operands_batch(
        &mut self,
        edits: Vec<(InstId, Vec<ValueId>)>,
    ) -> Result<(), EditError> {
        let prepared = self.prepare_operand_batch(edits)?;
        prepared.apply(self.body);
        Ok(())
    }

    fn prepare_operand_batch(
        &self,
        edits: Vec<(InstId, Vec<ValueId>)>,
    ) -> Result<PreparedOperandBatch, EditError> {
        if edits.is_empty() {
            return Ok(PreparedOperandBatch { edits: vec![] });
        }
        let placement = PlacementIndex::new(self.body);
        let Some(edits) = normalize_operand_edits(self.body, &placement, edits)? else {
            return Ok(PreparedOperandBatch { edits: vec![] });
        };

        let cfg = ControlFlowGraph::from_placement(&placement);
        let dominators = DominatorTree::new(&cfg);
        let mut prepared = Vec::with_capacity(edits.len());
        for (instruction, operands) in edits {
            let (block, position) = placement
                .instruction(instruction)
                .ok_or(EditError::InvalidInstruction { instruction })?;
            let data = self
                .body
                .instruction(instruction)
                .ok_or(EditError::InvalidInstruction { instruction })?;
            let signature = data
                .op
                .signature(self.program)
                .ok_or(EditError::InvalidOperation { instruction })?;
            if operands.len() != signature.operands.len() {
                return Err(EditError::OperandCount {
                    instruction,
                    expected: signature.operands.len(),
                    actual: operands.len(),
                });
            }
            for (operand_index, (operand, expected)) in operands
                .iter()
                .copied()
                .zip(signature.operands.iter().copied())
                .enumerate()
            {
                let actual = self
                    .body
                    .value(operand)
                    .ok_or(EditError::InvalidValue { value: operand })?
                    .ty;
                if actual != expected {
                    return Err(EditError::OperandType {
                        instruction,
                        operand_index,
                        expected,
                        actual,
                    });
                }
                validate_value_at_use(
                    self.body,
                    &placement,
                    &dominators,
                    operand,
                    UseSite::InstructionOperand {
                        block,
                        instruction,
                        operand_index,
                    },
                )?;
            }
            prepared.push((block, position, instruction, operands));
        }
        let layout_positions = block_layout_positions(self.body);
        prepared.sort_by_key(|(block, position, instruction, _)| {
            (
                layout_position(&layout_positions, *block),
                *position,
                instruction.index(),
            )
        });
        Ok(PreparedOperandBatch {
            edits: prepared
                .into_iter()
                .map(|(_, _, instruction, operands)| (instruction, operands))
                .collect(),
        })
    }

    /// Detaches a closed set of unused, trivially discardable instructions.
    ///
    /// Results may be used by another selected instruction, but no selected result
    /// may be used by an unselected instruction or terminator.
    ///
    /// # Errors
    ///
    /// Returns before mutation for duplicates, detached/multiply-attached members,
    /// non-discardable operations, or escaping result uses.
    pub fn erase_discardable_inst_set(
        &mut self,
        instructions: Vec<InstId>,
    ) -> Result<(), EditError> {
        let mut instrumentation = NoBatchEditInstrumentation;
        let prepared = self.prepare_erasure(instructions, &mut instrumentation)?;
        prepared.apply(self.body, &mut instrumentation);
        Ok(())
    }

    /// Applies one erasure batch and returns counters tied to its actual scan loops.
    #[cfg(test)]
    pub(crate) fn erase_discardable_inst_set_with_statistics(
        &mut self,
        instructions: Vec<InstId>,
    ) -> Result<BatchEditStatistics, EditError> {
        let mut statistics = BatchEditStatistics::default();
        let prepared = self.prepare_erasure(instructions, &mut statistics)?;
        prepared.apply(self.body, &mut statistics);
        Ok(statistics)
    }

    fn prepare_erasure<I: BatchEditInstrumentation>(
        &self,
        instructions: Vec<InstId>,
        instrumentation: &mut I,
    ) -> Result<PreparedErasure, EditError> {
        if instructions.is_empty() {
            return Ok(PreparedErasure {
                selected: vec![],
                attached_blocks: vec![],
            });
        }
        instrumentation.placement_index_built();
        let placement = PlacementIndex::new(self.body);
        let mut membership_count = vec![0_u8; self.body.instructions.len()];
        instrumentation.erasure_ownership_scan_started();
        for block in &self.body.block_order {
            let Some(data) = self.body.block(*block) else {
                continue;
            };
            for instruction in &data.instructions {
                instrumentation.erasure_ownership_membership_scanned();
                if let Some(count) = instruction_index(*instruction)
                    .and_then(|index| membership_count.get_mut(index))
                {
                    *count = count.saturating_add(1);
                }
            }
        }
        let mut selected = vec![false; self.body.instructions.len()];
        let mut instructions = instructions;
        instructions.sort_by_key(|instruction| instruction.index());
        for instruction in instructions {
            let index = instruction_index(instruction)
                .ok_or(EditError::InvalidInstruction { instruction })?;
            let slot = selected
                .get_mut(index)
                .ok_or(EditError::InvalidInstruction { instruction })?;
            if *slot {
                return Err(EditError::DuplicateInstructionEdit { instruction });
            }
            *slot = true;
            if membership_count.get(index).copied() != Some(1)
                || !placement.is_instruction_attached(instruction)
            {
                return Err(EditError::InvalidInstruction { instruction });
            }
            let data = self
                .body
                .instruction(instruction)
                .ok_or(EditError::InvalidInstruction { instruction })?;
            if !data.op.is_trivially_discardable() {
                return Err(EditError::NotDiscardable { instruction });
            }
        }

        let selected_results = selected_result_bits(self.body, &selected)?;
        reject_escaping_selected_results(self.body, &selected, &selected_results, instrumentation)?;
        Ok(PreparedErasure {
            selected,
            attached_blocks: self.body.block_order.clone(),
        })
    }

    /// Replaces all results of one pure instruction and detaches the old one.
    ///
    /// Semantic equivalence is the caller's proof obligation.
    ///
    /// # Errors
    ///
    /// Returns before mutation unless both operations are attached and pure and
    /// every result replacement satisfies the atomic value-batch contract.
    pub fn replace_pure_inst(&mut self, old: InstId, replacement: InstId) -> Result<(), EditError> {
        if old == replacement {
            return Ok(());
        }
        let placement = PlacementIndex::new(self.body);
        for instruction in [old, replacement] {
            if !placement.is_instruction_attached(instruction) {
                return Err(EditError::InvalidInstruction { instruction });
            }
        }
        let old_data = self
            .body
            .instruction(old)
            .ok_or(EditError::InvalidInstruction { instruction: old })?;
        let replacement_data =
            self.body
                .instruction(replacement)
                .ok_or(EditError::InvalidInstruction {
                    instruction: replacement,
                })?;
        if old_data.op.effects() != EffectClass::Pure {
            return Err(EditError::NotPure { instruction: old });
        }
        if replacement_data.op.effects() != EffectClass::Pure {
            return Err(EditError::NotPure {
                instruction: replacement,
            });
        }
        if old_data.results.len() != replacement_data.results.len() {
            return Err(EditError::ResultCount {
                old: old_data.results.len(),
                replacement: replacement_data.results.len(),
            });
        }
        let edits = old_data
            .results
            .iter()
            .copied()
            .zip(replacement_data.results.iter().copied())
            .map(|(old, replacement)| (old, ValueReplacement::Existing(replacement)))
            .collect();
        if !old_data.op.is_trivially_discardable() {
            return Err(EditError::NotDiscardable { instruction: old });
        }
        let mut instrumentation = NoBatchEditInstrumentation;
        let prepared_replacements =
            self.prepare_value_batch(edits, ENTITY_CAPACITY, &mut instrumentation)?;
        let mut selected = vec![false; self.body.instructions.len()];
        let selected_slot = instruction_index(old)
            .and_then(|index| selected.get_mut(index))
            .ok_or(EditError::InvalidInstruction { instruction: old })?;
        *selected_slot = true;
        let prepared_erasure = PreparedErasure {
            selected,
            attached_blocks: self.body.block_order.clone(),
        };
        prepared_replacements.apply(self.body, &mut instrumentation);
        prepared_erasure.apply(self.body, &mut instrumentation);
        Ok(())
    }

    /// Detaches one unused pure instruction.
    ///
    /// # Errors
    ///
    /// Returns before mutation when the instruction is detached, impure, not
    /// safely discardable, or has an attached result use.
    pub fn erase_pure_inst(&mut self, instruction: InstId) -> Result<(), EditError> {
        let data = self
            .body
            .instruction(instruction)
            .ok_or(EditError::InvalidInstruction { instruction })?;
        if data.op.effects() != EffectClass::Pure {
            return Err(EditError::NotPure { instruction });
        }
        self.erase_discardable_inst_set(vec![instruction])
    }

    /// Replaces one attached block terminator atomically.
    ///
    /// # Errors
    ///
    /// Returns before mutation when the replacement violates provenance,
    /// structure, typing, placement, reachability, or SSA dominance.
    pub fn set_terminator(
        &mut self,
        block: BlockId,
        replacement: Terminator,
    ) -> Result<(), EditError> {
        self.set_terminators_batch(vec![(block, replacement)])
    }

    /// Replaces attached terminators as one projected-CFG transaction.
    ///
    /// The full projected graph is used for reachability and dominance, so making
    /// formerly unreachable blocks reachable cannot expose invalid unchanged uses.
    ///
    /// # Errors
    ///
    /// Returns before mutation for duplicate/detached blocks or any invalid
    /// projected terminator, CFG, or SSA use.
    pub fn set_terminators_batch(
        &mut self,
        edits: Vec<(BlockId, Terminator)>,
    ) -> Result<(), EditError> {
        let prepared = self.prepare_terminators(edits)?;
        prepared.apply(self.body);
        Ok(())
    }

    fn prepare_terminators(
        &self,
        edits: Vec<(BlockId, Terminator)>,
    ) -> Result<PreparedTerminators, EditError> {
        if edits.is_empty() {
            return Ok(PreparedTerminators { edits: vec![] });
        }
        let declaration =
            self.program
                .function(self.function)
                .ok_or(EditError::InvalidFunction {
                    function: self.function,
                })?;
        let placement = PlacementIndex::new(self.body);
        let mut seen = vec![false; self.body.blocks.len()];
        let mut edits = edits;
        edits.sort_by_key(|(block, _)| block.index());
        for (block, terminator) in &edits {
            let index = block_index(*block).ok_or(EditError::InvalidBlock { block: *block })?;
            let slot = seen
                .get_mut(index)
                .ok_or(EditError::InvalidBlock { block: *block })?;
            if *slot {
                return Err(EditError::DuplicateBlockEdit { block: *block });
            }
            *slot = true;
            if !placement.is_block_attached(*block) {
                return Err(EditError::InvalidBlock { block: *block });
            }
            if self.sources.origin(terminator.origin).is_none() {
                return Err(terminator_error(
                    "core.invalid-origin",
                    format!(
                        "terminator in {block:?} has invalid origin {:?}",
                        terminator.origin
                    ),
                    self.body
                        .block(*block)
                        .map_or(OriginId::UNKNOWN, |data| data.origin),
                ));
            }
            validate_terminator_contract(
                self.body,
                &placement,
                *block,
                terminator,
                &declaration.results,
            )?;
        }
        let layout_positions = block_layout_positions(self.body);
        edits.sort_by_key(|(block, _)| (layout_position(&layout_positions, *block), block.index()));

        let mut overrides = vec![None; self.body.blocks.len()];
        for (block, terminator) in &edits {
            overrides[block_index(*block).expect("validated dense block")] = Some(&terminator.kind);
        }
        let projected_cfg = ControlFlowGraph::with_terminator_overrides(&placement, &overrides);
        let dominators = DominatorTree::new(&projected_cfg);
        for block in &self.body.block_order {
            let data = self
                .body
                .block(*block)
                .ok_or(EditError::InvalidBlock { block: *block })?;
            for instruction in &data.instructions {
                if placement.instruction(*instruction).map(|placed| placed.0) != Some(*block) {
                    continue;
                }
                let instruction_data =
                    self.body
                        .instruction(*instruction)
                        .ok_or(EditError::InvalidInstruction {
                            instruction: *instruction,
                        })?;
                for (operand_index, operand) in
                    instruction_data.operands.iter().copied().enumerate()
                {
                    validate_value_at_use(
                        self.body,
                        &placement,
                        &dominators,
                        operand,
                        UseSite::InstructionOperand {
                            block: *block,
                            instruction: *instruction,
                            operand_index,
                        },
                    )?;
                }
            }
            let kind = overrides[block_index(*block).expect("validated dense block")]
                .or_else(|| data.terminator.as_ref().map(|terminator| &terminator.kind))
                .ok_or_else(|| {
                    terminator_error(
                        "core.missing-terminator",
                        format!("attached block {block:?} has no terminator"),
                        data.origin,
                    )
                })?;
            validate_terminator_ssa(self.body, &placement, &dominators, *block, kind)?;
        }
        drop(projected_cfg);

        Ok(PreparedTerminators { edits })
    }

    /// Freezes the exact CFG and raw-container facts consumed by straight-line
    /// jump fusion under this editor's exclusive mutable borrow.
    ///
    /// `maximum_fact_table_entries` is an optional typed admission limit. Its unit
    /// is one element of a stable-ID-indexed fact table or one flattened eligible
    /// destination-parameter substitution slot. A rejected limit leaves the body
    /// unchanged and does not expose a partially prepared edit.
    pub(crate) fn prepare_jump_fusion(
        &mut self,
        maximum_fact_table_entries: Option<usize>,
    ) -> Result<JumpFusionPreparation<'_>, EditError> {
        match build_jump_fusion_facts(self.body, maximum_fact_table_entries)? {
            BuiltJumpFusionFacts::Ready(facts) => {
                Ok(JumpFusionPreparation::Ready(PreparedJumpFusion {
                    body: self.body,
                    facts,
                }))
            }
            BuiltJumpFusionFacts::StoppedAtFactTableLimit { statistics } => {
                Ok(JumpFusionPreparation::StoppedAtFactTableLimit { statistics })
            }
        }
    }

    /// Detaches every attached block not reachable from entry.
    ///
    /// Allocated IDs and raw data remain for debugging. No verifier is called by
    /// this operation; pass-boundary policy owns whole-function verification.
    ///
    /// # Errors
    ///
    /// Returns before mutation if entry/layout/successor structure is invalid.
    pub fn detach_unreachable_blocks(&mut self) -> Result<usize, EditError> {
        let placement = PlacementIndex::new(self.body);
        if !placement.is_block_attached(self.body.entry) {
            return Err(EditError::InvalidBlock {
                block: self.body.entry,
            });
        }
        let cfg = ControlFlowGraph::from_placement(&placement);
        let reachability = Reachability::new(&cfg);
        let retained = self
            .body
            .block_order
            .iter()
            .copied()
            .filter(|block| reachability.contains(*block))
            .collect::<Vec<_>>();
        for block in &retained {
            let data = self
                .body
                .block(*block)
                .ok_or(EditError::InvalidBlock { block: *block })?;
            let terminator = data.terminator.as_ref().ok_or_else(|| {
                terminator_error(
                    "core.missing-terminator",
                    format!("attached block {block:?} has no terminator"),
                    data.origin,
                )
            })?;
            let mut invalid_target = None;
            terminator.kind.for_each_successor(|target| {
                if !placement.is_block_attached(target.block) {
                    invalid_target = Some(target.block);
                }
            });
            if let Some(target) = invalid_target {
                return Err(terminator_error(
                    "core.detached-target",
                    format!("{block:?} targets detached or invalid {target:?}"),
                    terminator.origin,
                ));
            }
        }
        let removed = self.body.block_order.len() - retained.len();
        drop(reachability);
        drop(cfg);
        drop(placement);
        self.body.block_order = retained;
        Ok(removed)
    }
}

const FUSION_ATTACHED: u8 = 1 << 0;
const FUSION_REACHABLE: u8 = 1 << 1;
const FUSION_CLAIMED: u8 = 1 << 2;
const FUSION_WALKING: u8 = 1 << 3;
const FUSION_SELECTED: u8 = 1 << 4;
const FUSION_CONSUMED: u8 = 1 << 5;

#[derive(Clone, Copy, Debug, Default)]
struct FusionSuccessors {
    first: Option<BlockId>,
    second: Option<BlockId>,
}

impl FusionSuccessors {
    fn push(&mut self, block: BlockId) -> Result<(), EditError> {
        if self.first.is_none() {
            self.first = Some(block);
        } else if self.second.is_none() {
            self.second = Some(block);
        } else {
            return Err(EditError::InconsistentJumpFusion(
                "a Core terminator has more than two successor edges",
            ));
        }
        Ok(())
    }

    fn for_each(self, mut visit: impl FnMut(BlockId)) {
        if let Some(block) = self.first {
            visit(block);
        }
        if let Some(block) = self.second {
            visit(block);
        }
    }
}

struct JumpFusionFacts {
    state: Vec<u8>,
    successors: Vec<FusionSuccessors>,
    incoming_edges: Vec<usize>,
    eligible_successor: Vec<Option<BlockId>>,
    eligible_predecessor: Vec<Option<BlockId>>,
    replacement_offsets: Vec<Option<usize>>,
    replacements: Vec<Option<ValueId>>,
    raw_instruction_owner: Vec<Option<BlockId>>,
    attached_value_occurrences: usize,
    statistics: JumpFusionFactStatistics,
}

enum BuiltJumpFusionFacts {
    Ready(JumpFusionFacts),
    StoppedAtFactTableLimit {
        statistics: JumpFusionFactStatistics,
    },
}

#[allow(
    clippy::too_many_lines,
    reason = "the two-stage frozen-fact admission is intentionally one auditable sequence"
)]
fn build_jump_fusion_facts(
    body: &FunctionBody,
    maximum_fact_table_entries: Option<usize>,
) -> Result<BuiltJumpFusionFacts, EditError> {
    let block_count = body.blocks.len();
    let instruction_count = body.instructions.len();

    // Six block-indexed tables and one instruction-owner table form the base
    // requirement. Exact eligible-destination parameter slots are admitted in a
    // second stage after those base facts classify the frozen CFG.
    let base_fact_table_entries = block_count
        .checked_mul(6)
        .and_then(|entries| entries.checked_add(instruction_count))
        .ok_or(EditError::SizeOverflow)?;
    checked_jump_fusion_fact_bytes(block_count, instruction_count, 0)?;
    if maximum_fact_table_entries.is_some_and(|maximum| base_fact_table_entries > maximum) {
        return Ok(BuiltJumpFusionFacts::StoppedAtFactTableLimit {
            statistics: JumpFusionFactStatistics {
                required_fact_table_entries: base_fact_table_entries,
                ..JumpFusionFactStatistics::default()
            },
        });
    }

    let mut state = vec![0_u8; block_count];
    let mut successors = vec![FusionSuccessors::default(); block_count];
    let mut incoming_edges = vec![0_usize; block_count];
    let mut eligible_successor = vec![None; block_count];
    let mut eligible_predecessor = vec![None; block_count];
    let mut replacement_offsets = vec![None; block_count];
    let mut raw_instruction_owner = vec![None; instruction_count];
    let mut statistics = JumpFusionFactStatistics {
        fact_builds: 1,
        required_fact_table_entries: base_fact_table_entries,
        ..JumpFusionFactStatistics::default()
    };
    let mut attached_value_occurrences = 0_usize;

    for (block, data) in body.blocks.iter() {
        for instruction in &data.instructions {
            let index = instruction_index(*instruction).ok_or(
                EditError::InconsistentJumpFusion("raw block contains an invalid instruction ID"),
            )?;
            let owner =
                raw_instruction_owner
                    .get_mut(index)
                    .ok_or(EditError::InconsistentJumpFusion(
                        "raw block instruction is not allocated",
                    ))?;
            if owner.replace(block).is_some() {
                return Err(EditError::InconsistentJumpFusion(
                    "one instruction ID occurs in more than one raw block-container position",
                ));
            }
            statistics.raw_instruction_memberships = statistics
                .raw_instruction_memberships
                .checked_add(1)
                .ok_or(EditError::SizeOverflow)?;
        }
    }

    for block in &body.block_order {
        let index = block_index(*block).ok_or(EditError::InconsistentJumpFusion(
            "attached fusion block has an invalid ID",
        ))?;
        let block_state = state
            .get_mut(index)
            .ok_or(EditError::InconsistentJumpFusion(
                "attached fusion block is outside dense state",
            ))?;
        if *block_state & FUSION_ATTACHED != 0 {
            return Err(EditError::InconsistentJumpFusion(
                "one block occurs more than once in attached layout",
            ));
        }
        *block_state |= FUSION_ATTACHED;

        let data = body.block(*block).ok_or(EditError::InconsistentJumpFusion(
            "attached fusion block is absent",
        ))?;
        if data.terminator.is_none() {
            return Err(EditError::InconsistentJumpFusion(
                "attached fusion block has no terminator",
            ));
        }
        for instruction in &data.instructions {
            let operand_count = body
                .instruction(*instruction)
                .ok_or(EditError::InconsistentJumpFusion(
                    "attached block contains an absent instruction",
                ))?
                .operands
                .len();
            attached_value_occurrences = attached_value_occurrences
                .checked_add(operand_count)
                .ok_or(EditError::SizeOverflow)?;
        }
        attached_value_occurrences = attached_value_occurrences
            .checked_add(terminator_value_count(
                &data
                    .terminator
                    .as_ref()
                    .expect("attached terminator was checked")
                    .kind,
            )?)
            .ok_or(EditError::SizeOverflow)?;
        for (parameter_index, parameter) in data.parameters.iter().enumerate() {
            let parameter_index_u32 =
                u32::try_from(parameter_index).map_err(|_| EditError::SizeOverflow)?;
            let value = body
                .value(parameter.value)
                .ok_or(EditError::InconsistentJumpFusion(
                    "block parameter value is not allocated",
                ))?;
            if value.definition
                != (ValueDef::BlockParam {
                    block: *block,
                    parameter_index: parameter_index_u32,
                })
            {
                return Err(EditError::InconsistentJumpFusion(
                    "block parameter and value definition disagree",
                ));
            }
        }
        for instruction in &data.instructions {
            let owner = instruction_index(*instruction)
                .and_then(|instruction_index| raw_instruction_owner.get(instruction_index))
                .copied()
                .flatten();
            if owner != Some(*block) {
                return Err(EditError::InconsistentJumpFusion(
                    "attached instruction does not have its attached block as raw owner",
                ));
            }
        }
    }
    for block in &body.block_order {
        let source_index = block_index(*block).ok_or(EditError::InconsistentJumpFusion(
            "attached fusion block has an invalid ID",
        ))?;
        let terminator = body
            .block(*block)
            .and_then(|data| data.terminator.as_ref())
            .ok_or(EditError::InconsistentJumpFusion(
                "attached fusion block has no terminator",
            ))?;
        let mut edge_error = None;
        terminator.kind.for_each_successor(|target| {
            if edge_error.is_some() {
                return;
            }
            let Some(target_index) = block_index(target.block) else {
                edge_error = Some(EditError::InconsistentJumpFusion(
                    "attached edge target has an invalid block ID",
                ));
                return;
            };
            if state
                .get(target_index)
                .is_none_or(|target_state| target_state & FUSION_ATTACHED == 0)
            {
                edge_error = Some(EditError::InconsistentJumpFusion(
                    "attached edge targets a detached or absent block",
                ));
                return;
            }
            if let Err(error) = successors[source_index].push(target.block) {
                edge_error = Some(error);
                return;
            }
            let Some(count) = incoming_edges.get_mut(target_index) else {
                edge_error = Some(EditError::InconsistentJumpFusion(
                    "attached edge target is outside dense incoming-edge state",
                ));
                return;
            };
            let Some(next) = count.checked_add(1) else {
                edge_error = Some(EditError::SizeOverflow);
                return;
            };
            *count = next;
            let Some(next) = statistics.attached_edges_counted.checked_add(1) else {
                edge_error = Some(EditError::SizeOverflow);
                return;
            };
            statistics.attached_edges_counted = next;
        });
        if let Some(error) = edge_error {
            return Err(error);
        }
    }

    let entry_index = block_index(body.entry).ok_or(EditError::InconsistentJumpFusion(
        "fusion entry block has an invalid ID",
    ))?;
    let entry_state = state
        .get_mut(entry_index)
        .ok_or(EditError::InconsistentJumpFusion(
            "fusion entry block is outside dense state",
        ))?;
    if *entry_state & FUSION_ATTACHED == 0 {
        return Err(EditError::InconsistentJumpFusion(
            "fusion entry block is detached",
        ));
    }
    *entry_state |= FUSION_REACHABLE;
    let stack_bytes = body
        .block_order
        .len()
        .checked_mul(std::mem::size_of::<BlockId>())
        .ok_or(EditError::SizeOverflow)?;
    if stack_bytes > isize::MAX as usize {
        return Err(EditError::SizeOverflow);
    }
    let mut stack = Vec::with_capacity(body.block_order.len());
    stack.push(body.entry);
    while let Some(block) = stack.pop() {
        let index = block_index(block).ok_or(EditError::InconsistentJumpFusion(
            "reachable fusion block has an invalid ID",
        ))?;
        let successor_facts =
            successors
                .get(index)
                .copied()
                .ok_or(EditError::InconsistentJumpFusion(
                    "reachable block is outside successor state",
                ))?;
        let mut push_error = None;
        successor_facts.for_each(|successor| {
            if push_error.is_some() {
                return;
            }
            let Some(successor_index) = block_index(successor) else {
                push_error = Some(EditError::InconsistentJumpFusion(
                    "reachable successor has an invalid ID",
                ));
                return;
            };
            let Some(successor_state) = state.get_mut(successor_index) else {
                push_error = Some(EditError::InconsistentJumpFusion(
                    "reachable successor is outside dense state",
                ));
                return;
            };
            if *successor_state & FUSION_REACHABLE == 0 {
                *successor_state |= FUSION_REACHABLE;
                stack.push(successor);
            }
        });
        if let Some(error) = push_error {
            return Err(error);
        }
    }

    for block in &body.block_order {
        let source_index = block_index(*block).ok_or(EditError::InconsistentJumpFusion(
            "attached fusion block has an invalid ID",
        ))?;
        if state[source_index] & FUSION_REACHABLE == 0 {
            continue;
        }
        let Some(TerminatorKind::Jump(target)) = body
            .block(*block)
            .and_then(|data| data.terminator.as_ref())
            .map(|terminator| &terminator.kind)
        else {
            continue;
        };
        let destination_index = block_index(target.block).ok_or(
            EditError::InconsistentJumpFusion("jump target has an invalid block ID"),
        )?;
        if target.block == *block
            || target.block == body.entry
            || state[destination_index] & FUSION_REACHABLE == 0
            || incoming_edges[destination_index] != 1
        {
            continue;
        }
        eligible_successor[source_index] = Some(target.block);
        let predecessor = eligible_predecessor.get_mut(destination_index).ok_or(
            EditError::InconsistentJumpFusion("eligible destination is outside predecessor state"),
        )?;
        if predecessor.replace(*block).is_some() {
            return Err(EditError::InconsistentJumpFusion(
                "eligible destination has more than one eligible predecessor",
            ));
        }
    }

    let mut parameter_slot_count = 0_usize;
    for block in &body.block_order {
        let index = block_index(*block).ok_or(EditError::InconsistentJumpFusion(
            "eligible fusion destination has an invalid block ID",
        ))?;
        if eligible_predecessor[index].is_none() {
            continue;
        }
        replacement_offsets[index] = Some(parameter_slot_count);
        parameter_slot_count = parameter_slot_count
            .checked_add(
                body.block(*block)
                    .ok_or(EditError::InconsistentJumpFusion(
                        "eligible fusion destination is absent",
                    ))?
                    .parameters
                    .len(),
            )
            .ok_or(EditError::SizeOverflow)?;
    }
    let required_fact_table_entries = base_fact_table_entries
        .checked_add(parameter_slot_count)
        .ok_or(EditError::SizeOverflow)?;
    checked_jump_fusion_fact_bytes(block_count, instruction_count, parameter_slot_count)?;
    statistics.required_fact_table_entries = required_fact_table_entries;
    if maximum_fact_table_entries.is_some_and(|maximum| required_fact_table_entries > maximum) {
        return Ok(BuiltJumpFusionFacts::StoppedAtFactTableLimit { statistics });
    }
    let replacements = vec![None; parameter_slot_count];

    Ok(BuiltJumpFusionFacts::Ready(JumpFusionFacts {
        state,
        successors,
        incoming_edges,
        eligible_successor,
        eligible_predecessor,
        replacement_offsets,
        replacements,
        raw_instruction_owner,
        attached_value_occurrences,
        statistics,
    }))
}

fn terminator_value_count(kind: &TerminatorKind) -> Result<usize, EditError> {
    match kind {
        TerminatorKind::Jump(target) => Ok(target.arguments.len()),
        TerminatorKind::Branch {
            then_target,
            else_target,
            ..
        } => then_target
            .arguments
            .len()
            .checked_add(else_target.arguments.len())
            .and_then(|count| count.checked_add(1))
            .ok_or(EditError::SizeOverflow),
        TerminatorKind::Return(values) => Ok(values.len()),
        TerminatorKind::Unreachable => Ok(0),
    }
}

fn checked_jump_fusion_fact_bytes(
    block_count: usize,
    instruction_count: usize,
    parameter_slot_count: usize,
) -> Result<usize, EditError> {
    let mut bytes = 0_usize;
    for entry_size in [
        std::mem::size_of::<u8>(),
        std::mem::size_of::<FusionSuccessors>(),
        std::mem::size_of::<usize>(),
        std::mem::size_of::<Option<BlockId>>(),
        std::mem::size_of::<Option<BlockId>>(),
        std::mem::size_of::<Option<usize>>(),
    ] {
        bytes = bytes
            .checked_add(
                block_count
                    .checked_mul(entry_size)
                    .ok_or(EditError::SizeOverflow)?,
            )
            .ok_or(EditError::SizeOverflow)?;
    }
    bytes = bytes
        .checked_add(
            instruction_count
                .checked_mul(std::mem::size_of::<Option<BlockId>>())
                .ok_or(EditError::SizeOverflow)?,
        )
        .and_then(|bytes| {
            bytes.checked_add(
                parameter_slot_count.checked_mul(std::mem::size_of::<Option<ValueId>>())?,
            )
        })
        .ok_or(EditError::SizeOverflow)?;
    if bytes > isize::MAX as usize {
        return Err(EditError::SizeOverflow);
    }
    Ok(bytes)
}

/// Consumer-specific frozen facts and exclusive body borrow for one fusion pass.
pub(crate) struct PreparedJumpFusion<'editor> {
    body: &'editor mut FunctionBody,
    facts: JumpFusionFacts,
}

impl PreparedJumpFusion<'_> {
    pub(crate) const fn fact_statistics(&self) -> JumpFusionFactStatistics {
        self.facts.statistics
    }

    /// Discovers a complete prefix of maximal regions in authoritative head order.
    pub(crate) fn discover_maximal_regions(
        &mut self,
        maximum_block_visits: usize,
    ) -> Result<JumpFusionDiscovery, EditError> {
        let maximum_block_visits = maximum_block_visits.min(self.body.block_order.len());
        let region_bytes = maximum_block_visits
            .checked_mul(std::mem::size_of::<BlockId>())
            .ok_or(EditError::SizeOverflow)?;
        if region_bytes > isize::MAX as usize {
            return Err(EditError::SizeOverflow);
        }
        let mut regions = Vec::new();
        let mut block_visits = 0_usize;
        let mut stopped_at_block_visit_limit = false;

        for layout_index in 0..self.body.block_order.len() {
            let head = self.body.block_order[layout_index];
            let head_index = block_index(head).ok_or(EditError::InconsistentJumpFusion(
                "fusion region head has an invalid block ID",
            ))?;
            if self.facts.eligible_successor[head_index].is_none()
                || self.facts.eligible_predecessor[head_index].is_some()
            {
                continue;
            }

            let mut blocks = Vec::new();
            let mut current = head;
            loop {
                if block_visits == maximum_block_visits {
                    stopped_at_block_visit_limit = true;
                    for block in &blocks {
                        let index =
                            block_index(*block).ok_or(EditError::InconsistentJumpFusion(
                                "unfinished fusion region contains an invalid block ID",
                            ))?;
                        self.facts.state[index] &= !FUSION_WALKING;
                    }
                    break;
                }
                block_visits = block_visits.checked_add(1).ok_or(EditError::SizeOverflow)?;
                let current_index = block_index(current).ok_or(
                    EditError::InconsistentJumpFusion("fusion region contains an invalid block ID"),
                )?;
                let state = self.facts.state.get_mut(current_index).ok_or(
                    EditError::InconsistentJumpFusion("fusion region block is outside dense state"),
                )?;
                if *state & FUSION_WALKING != 0 {
                    return Err(EditError::InconsistentJumpFusion(
                        "eligible fusion edges contain a cycle",
                    ));
                }
                if *state & FUSION_CLAIMED != 0 {
                    return Err(EditError::InconsistentJumpFusion(
                        "two fusion region heads reach the same block",
                    ));
                }
                *state |= FUSION_WALKING;
                blocks.push(current);
                let Some(successor) = self.facts.eligible_successor[current_index] else {
                    for block in &blocks {
                        let index =
                            block_index(*block).ok_or(EditError::InconsistentJumpFusion(
                                "completed fusion region contains an invalid block ID",
                            ))?;
                        self.facts.state[index] &= !FUSION_WALKING;
                        self.facts.state[index] |= FUSION_CLAIMED;
                    }
                    if blocks.len() < 2 {
                        return Err(EditError::InconsistentJumpFusion(
                            "eligible fusion region contains fewer than two blocks",
                        ));
                    }
                    regions.push(JumpRegionEdit::new(blocks));
                    break;
                };
                current = successor;
            }
            if stopped_at_block_visit_limit {
                break;
            }
        }

        if !stopped_at_block_visit_limit {
            for (index, successor) in self.facts.eligible_successor.iter().enumerate() {
                if successor.is_some() && self.facts.state[index] & FUSION_CLAIMED == 0 {
                    return Err(EditError::InconsistentJumpFusion(
                        "complete fusion discovery left an eligible edge unclaimed",
                    ));
                }
            }
        }

        Ok(JumpFusionDiscovery {
            regions,
            block_visits,
            stopped_at_block_visit_limit,
        })
    }

    /// Applies all supplied maximal regions as one preflighted structural batch.
    pub(crate) fn fuse_jump_regions_batch(
        self,
        edits: &[JumpRegionEdit],
    ) -> Result<JumpFusionApplicationStatistics, EditError> {
        self.fuse_jump_regions_batch_with_instruction_limit(edits, usize::MAX)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "all fallible whole-batch preflight must visibly precede the semantic commit"
    )]
    fn fuse_jump_regions_batch_with_instruction_limit(
        mut self,
        edits: &[JumpRegionEdit],
        maximum_head_instruction_count: usize,
    ) -> Result<JumpFusionApplicationStatistics, EditError> {
        if edits.is_empty() {
            return Ok(JumpFusionApplicationStatistics::default());
        }

        let mut statistics = JumpFusionApplicationStatistics::default();
        let mut maximum_edge_arity = 0_usize;
        let mut head_growth = Vec::with_capacity(edits.len());

        // Establish whole-batch membership before validating final successors.
        for edit in edits {
            if edit.blocks.len() < 2 {
                return Err(EditError::InconsistentJumpFusion(
                    "jump fusion edit contains fewer than two blocks",
                ));
            }
            for (position, block) in edit.blocks.iter().copied().enumerate() {
                let index = block_index(block).ok_or(EditError::InconsistentJumpFusion(
                    "jump fusion edit contains an invalid block ID",
                ))?;
                let state =
                    self.facts
                        .state
                        .get_mut(index)
                        .ok_or(EditError::InconsistentJumpFusion(
                            "jump fusion edit block is outside dense state",
                        ))?;
                if *state & (FUSION_ATTACHED | FUSION_REACHABLE)
                    != FUSION_ATTACHED | FUSION_REACHABLE
                {
                    return Err(EditError::InconsistentJumpFusion(
                        "jump fusion edit contains a detached or unreachable block",
                    ));
                }
                if *state & FUSION_CLAIMED == 0 {
                    return Err(EditError::InconsistentJumpFusion(
                        "jump fusion edit was not produced by this discovery snapshot",
                    ));
                }
                if *state & FUSION_SELECTED != 0 {
                    return Err(EditError::InconsistentJumpFusion(
                        "jump fusion edits overlap or repeat a block",
                    ));
                }
                *state |= FUSION_SELECTED;
                if position != 0 {
                    if block == self.body.entry {
                        return Err(EditError::InconsistentJumpFusion(
                            "jump fusion edit consumes the entry block",
                        ));
                    }
                    *state |= FUSION_CONSUMED;
                }
            }
        }

        for edit in edits {
            let head = edit.blocks[0];
            let tail = *edit.blocks.last().expect("fusion edit length checked");
            let head_index = block_index(head).ok_or(EditError::InconsistentJumpFusion(
                "jump fusion head has an invalid block ID",
            ))?;
            let tail_index = block_index(tail).ok_or(EditError::InconsistentJumpFusion(
                "jump fusion tail has an invalid block ID",
            ))?;
            if self.facts.eligible_predecessor[head_index].is_some()
                || self.facts.eligible_successor[tail_index].is_some()
            {
                return Err(EditError::InconsistentJumpFusion(
                    "jump fusion edit is not a maximal eligible region",
                ));
            }

            let mut total_instruction_count = 0_usize;
            for block in &edit.blocks {
                let data = self
                    .body
                    .block(*block)
                    .ok_or(EditError::InconsistentJumpFusion(
                        "jump fusion edit block is absent",
                    ))?;
                for instruction in &data.instructions {
                    statistics.selected_instruction_memberships_preflighted = statistics
                        .selected_instruction_memberships_preflighted
                        .checked_add(1)
                        .ok_or(EditError::SizeOverflow)?;
                    let owner = instruction_index(*instruction)
                        .and_then(|index| self.facts.raw_instruction_owner.get(index))
                        .copied()
                        .flatten();
                    if owner != Some(*block) {
                        return Err(EditError::InconsistentJumpFusion(
                            "jump fusion instruction raw owner changed after preparation",
                        ));
                    }
                }
                let count = data.instructions.len();
                total_instruction_count = total_instruction_count
                    .checked_add(count)
                    .ok_or(EditError::SizeOverflow)?;
            }
            if total_instruction_count > maximum_head_instruction_count {
                return Err(EditError::SizeOverflow);
            }
            let head_instruction_count = self
                .body
                .block(head)
                .ok_or(EditError::InconsistentJumpFusion(
                    "jump fusion head is absent",
                ))?
                .instructions
                .len();
            let growth = total_instruction_count
                .checked_sub(head_instruction_count)
                .ok_or(EditError::SizeOverflow)?;
            let instruction_bytes = total_instruction_count
                .checked_mul(std::mem::size_of::<InstId>())
                .ok_or(EditError::SizeOverflow)?;
            if instruction_bytes > isize::MAX as usize {
                return Err(EditError::SizeOverflow);
            }
            head_growth.push((head, growth));

            for edge in edit.blocks.windows(2) {
                let source = edge[0];
                let destination = edge[1];
                let source_index = block_index(source).ok_or(EditError::InconsistentJumpFusion(
                    "jump fusion edge source has an invalid ID",
                ))?;
                if self.facts.eligible_successor[source_index] != Some(destination) {
                    return Err(EditError::InconsistentJumpFusion(
                        "jump fusion edit does not follow the frozen eligible edge",
                    ));
                }
                let destination_index =
                    block_index(destination).ok_or(EditError::InconsistentJumpFusion(
                        "jump fusion edge destination has an invalid ID",
                    ))?;
                if self.facts.incoming_edges[destination_index] != 1
                    || self.facts.successors[source_index].first != Some(destination)
                    || self.facts.successors[source_index].second.is_some()
                {
                    return Err(EditError::InconsistentJumpFusion(
                        "jump fusion edit contradicts frozen exact-edge facts",
                    ));
                }
                let target = match self
                    .body
                    .block(source)
                    .and_then(|data| data.terminator.as_ref())
                    .map(|terminator| &terminator.kind)
                {
                    Some(TerminatorKind::Jump(target)) if target.block == destination => target,
                    _ => {
                        return Err(EditError::InconsistentJumpFusion(
                            "jump fusion edge is no longer the exact unconditional jump",
                        ));
                    }
                };
                let destination_data =
                    self.body
                        .block(destination)
                        .ok_or(EditError::InconsistentJumpFusion(
                            "jump fusion destination block is absent",
                        ))?;
                if target.arguments.len() != destination_data.parameters.len() {
                    return Err(EditError::InconsistentJumpFusion(
                        "jump fusion edge argument arity differs from destination parameters",
                    ));
                }
                maximum_edge_arity = maximum_edge_arity.max(target.arguments.len());
                for (argument, parameter) in target
                    .arguments
                    .iter()
                    .copied()
                    .zip(&destination_data.parameters)
                {
                    let argument_type =
                        self.body
                            .value(argument)
                            .ok_or(EditError::InconsistentJumpFusion(
                                "jump fusion edge argument value is absent",
                            ))?;
                    let parameter_type = self.body.value(parameter.value).ok_or(
                        EditError::InconsistentJumpFusion(
                            "jump fusion destination parameter value is absent",
                        ),
                    )?;
                    if argument_type.ty != parameter_type.ty {
                        return Err(EditError::InconsistentJumpFusion(
                            "jump fusion edge argument type differs from destination parameter",
                        ));
                    }
                }
            }
        }

        for edit in edits {
            let head = edit.blocks[0];
            let tail = *edit.blocks.last().expect("fusion edit length checked");
            let tail_terminator = self
                .body
                .block(tail)
                .and_then(|data| data.terminator.as_ref())
                .ok_or(EditError::InconsistentJumpFusion(
                    "jump fusion tail has no terminator",
                ))?;
            let mut invalid_successor = false;
            tail_terminator.kind.for_each_successor(|target| {
                let target_state = block_index(target.block)
                    .and_then(|index| self.facts.state.get(index))
                    .copied();
                let survives = target_state.is_some_and(|state| {
                    state & FUSION_ATTACHED != 0 && state & FUSION_CONSUMED == 0
                });
                if !survives && target.block != head {
                    invalid_successor = true;
                }
            });
            if invalid_successor {
                return Err(EditError::InconsistentJumpFusion(
                    "jump fusion tail targets a block consumed by the batch",
                ));
            }
        }

        let scratch_bytes = maximum_edge_arity
            .checked_mul(std::mem::size_of::<ValueId>())
            .ok_or(EditError::SizeOverflow)?;
        if scratch_bytes > isize::MAX as usize {
            return Err(EditError::SizeOverflow);
        }
        let mut resolved_arguments = Vec::with_capacity(maximum_edge_arity);

        // Resolve a whole edge tuple before publishing any destination mapping.
        for edit in edits {
            for edge in edit.blocks.windows(2) {
                let source = edge[0];
                let destination = edge[1];
                let argument_count = match self
                    .body
                    .block(source)
                    .and_then(|data| data.terminator.as_ref())
                    .map(|terminator| &terminator.kind)
                {
                    Some(TerminatorKind::Jump(target)) => target.arguments.len(),
                    _ => unreachable!("exact jump was preflighted"),
                };
                resolved_arguments.clear();
                for argument_index in 0..argument_count {
                    let argument = match self
                        .body
                        .block(source)
                        .and_then(|data| data.terminator.as_ref())
                        .map(|terminator| &terminator.kind)
                    {
                        Some(TerminatorKind::Jump(target)) => target.arguments[argument_index],
                        _ => unreachable!("exact jump was preflighted"),
                    };
                    resolved_arguments
                        .push(self.resolve_jump_fusion_value(argument, &mut statistics)?);
                }
                for (parameter_index, replacement) in resolved_arguments.iter().copied().enumerate()
                {
                    let parameter = self
                        .body
                        .block(destination)
                        .expect("destination was preflighted")
                        .parameters[parameter_index]
                        .value;
                    let replacement_type = self
                        .body
                        .value(replacement)
                        .expect("resolved replacement remains allocated")
                        .ty;
                    let parameter_type = self
                        .body
                        .value(parameter)
                        .expect("destination parameter was preflighted")
                        .ty;
                    if replacement_type != parameter_type {
                        return Err(EditError::InconsistentJumpFusion(
                            "composed jump-fusion replacement changed type",
                        ));
                    }
                    let slot = self.jump_fusion_replacement_slot(parameter)?.ok_or(
                        EditError::InconsistentJumpFusion(
                            "destination parameter has no flattened replacement slot",
                        ),
                    )?;
                    if self.facts.replacements[slot].replace(replacement).is_some() {
                        return Err(EditError::InconsistentJumpFusion(
                            "jump fusion assigns one destination parameter more than once",
                        ));
                    }
                    statistics.parameter_mapping_slots_used = statistics
                        .parameter_mapping_slots_used
                        .checked_add(1)
                        .ok_or(EditError::SizeOverflow)?;
                }
            }
        }

        for slot in 0..self.facts.replacements.len() {
            if let Some(replacement) = self.facts.replacements[slot] {
                let resolved = self.resolve_jump_fusion_value(replacement, &mut statistics)?;
                self.facts.replacements[slot] = Some(resolved);
            }
        }

        statistics.regions_fused = edits.len();
        statistics.blocks_consumed = edits
            .iter()
            .try_fold(0_usize, |count, edit| {
                count.checked_add(edit.blocks.len() - 1)
            })
            .ok_or(EditError::SizeOverflow)?;
        statistics.attached_values_scanned = self.facts.attached_value_occurrences;
        statistics.attached_rewrite_scans = 1;
        statistics.head_reservations = head_growth.len();

        // Reserve every surviving head before the first semantic rewrite/move.
        for (head, growth) in &head_growth {
            self.body
                .block_mut(*head)
                .expect("fusion head was preflighted")
                .instructions
                .reserve_exact(*growth);
        }

        self.rewrite_attached_jump_fusion_values();

        for edit in edits {
            let head = edit.blocks[0];
            let mut tail_terminator = None;
            self.body
                .block_mut(head)
                .expect("fusion head was preflighted")
                .terminator
                .take()
                .expect("fusion head jump was preflighted");

            for (position, consumed) in edit.blocks.iter().copied().enumerate().skip(1) {
                let is_tail = position + 1 == edit.blocks.len();
                let (mut moved_instructions, terminator) = {
                    let data = self
                        .body
                        .block_mut(consumed)
                        .expect("consumed fusion block was preflighted");
                    (
                        std::mem::take(&mut data.instructions),
                        data.terminator.take(),
                    )
                };
                self.body
                    .block_mut(head)
                    .expect("fusion head remains allocated")
                    .instructions
                    .append(&mut moved_instructions);
                if is_tail {
                    tail_terminator = terminator;
                }
            }
            self.body
                .block_mut(head)
                .expect("fusion head remains allocated")
                .terminator =
                Some(tail_terminator.expect("fusion tail terminator was preflighted"));
        }

        self.body.block_order.retain(|block| {
            block_index(*block)
                .and_then(|index| self.facts.state.get(index))
                .is_none_or(|state| state & FUSION_CONSUMED == 0)
        });
        statistics.layout_retain_scans = 1;
        Ok(statistics)
    }

    fn jump_fusion_replacement_slot(&self, value: ValueId) -> Result<Option<usize>, EditError> {
        let Some(data) = self.body.value(value) else {
            return Err(EditError::InconsistentJumpFusion(
                "jump fusion replacement references an absent value",
            ));
        };
        let ValueDef::BlockParam {
            block,
            parameter_index,
        } = data.definition
        else {
            return Ok(None);
        };
        let block_index = block_index(block).ok_or(EditError::InconsistentJumpFusion(
            "jump fusion parameter definition has an invalid block ID",
        ))?;
        let Some(offset) = self
            .facts
            .replacement_offsets
            .get(block_index)
            .copied()
            .flatten()
        else {
            return Ok(None);
        };
        let parameter_index =
            usize::try_from(parameter_index).map_err(|_| EditError::SizeOverflow)?;
        let slot = offset
            .checked_add(parameter_index)
            .ok_or(EditError::SizeOverflow)?;
        if slot >= self.facts.replacements.len() {
            return Err(EditError::InconsistentJumpFusion(
                "jump fusion parameter replacement slot is outside flattened state",
            ));
        }
        Ok(Some(slot))
    }

    fn resolve_jump_fusion_value(
        &mut self,
        value: ValueId,
        statistics: &mut JumpFusionApplicationStatistics,
    ) -> Result<ValueId, EditError> {
        let mut current = value;
        let mut steps = 0_usize;
        while let Some(slot) = self.jump_fusion_replacement_slot(current)? {
            let Some(next) = self.facts.replacements[slot] else {
                break;
            };
            statistics.replacement_links_visited = statistics
                .replacement_links_visited
                .checked_add(1)
                .ok_or(EditError::SizeOverflow)?;
            steps = steps.checked_add(1).ok_or(EditError::SizeOverflow)?;
            if steps > self.facts.replacements.len() {
                return Err(EditError::InconsistentJumpFusion(
                    "jump fusion parameter replacements contain a cycle",
                ));
            }
            current = next;
        }
        let resolved = current;

        current = value;
        let mut compression_steps = 0_usize;
        while let Some(slot) = self.jump_fusion_replacement_slot(current)? {
            let Some(next) = self.facts.replacements[slot] else {
                break;
            };
            statistics.replacement_links_visited = statistics
                .replacement_links_visited
                .checked_add(1)
                .ok_or(EditError::SizeOverflow)?;
            self.facts.replacements[slot] = Some(resolved);
            compression_steps = compression_steps
                .checked_add(1)
                .ok_or(EditError::SizeOverflow)?;
            if compression_steps > self.facts.replacements.len() {
                return Err(EditError::InconsistentJumpFusion(
                    "jump fusion parameter replacement compression found a cycle",
                ));
            }
            if next == resolved {
                break;
            }
            current = next;
        }
        Ok(resolved)
    }

    fn rewrite_attached_jump_fusion_values(&mut self) {
        for layout_index in 0..self.body.block_order.len() {
            let block = self.body.block_order[layout_index];
            let instruction_count = self
                .body
                .block(block)
                .expect("attached fusion block was preflighted")
                .instructions
                .len();
            for instruction_index_in_block in 0..instruction_count {
                let instruction = self
                    .body
                    .block(block)
                    .expect("attached fusion block remains allocated")
                    .instructions[instruction_index_in_block];
                let operand_count = self
                    .body
                    .instruction(instruction)
                    .expect("attached fusion instruction was preflighted")
                    .operands
                    .len();
                for operand_index in 0..operand_count {
                    let value = self
                        .body
                        .instruction(instruction)
                        .expect("attached fusion instruction remains allocated")
                        .operands[operand_index];
                    let replacement = self
                        .final_jump_fusion_replacement(value)
                        .expect("attached fusion value was verified with frozen facts");
                    self.body
                        .instruction_mut(instruction)
                        .expect("attached fusion instruction remains allocated")
                        .operands[operand_index] = replacement;
                }
            }

            let mut terminator = self
                .body
                .block_mut(block)
                .and_then(|data| data.terminator.take())
                .expect("attached fusion terminator was preflighted");
            rewrite_terminator_values(&mut terminator.kind, |value: &mut ValueId| {
                *value = self
                    .final_jump_fusion_replacement(*value)
                    .expect("attached terminator value was verified with frozen facts");
            });
            self.body
                .block_mut(block)
                .expect("attached fusion block remains allocated")
                .terminator = Some(terminator);
        }
    }

    fn final_jump_fusion_replacement(&self, value: ValueId) -> Result<ValueId, EditError> {
        let Some(slot) = self.jump_fusion_replacement_slot(value)? else {
            return Ok(value);
        };
        Ok(self.facts.replacements[slot].unwrap_or(value))
    }
}

#[derive(Clone, Copy)]
enum ResolvedReplacement {
    Existing(ValueId),
    Constant(TypedCoreConstant),
}

fn normalize_value_replacements(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    mut edits: Vec<(ValueId, ValueReplacement)>,
) -> Result<(Vec<ValueId>, Vec<Option<ResolvedReplacement>>), EditError> {
    edits.sort_by_key(|(source, _)| source.index());
    let mut mappings = vec![None; body.values.len()];
    let mut sources = Vec::with_capacity(edits.len());
    for (source, replacement) in edits {
        let source_data = body
            .value(source)
            .ok_or(EditError::InvalidValue { value: source })?;
        let source_index = value_index(source).ok_or(EditError::InvalidValue { value: source })?;
        let slot = mappings
            .get_mut(source_index)
            .ok_or(EditError::InvalidValue { value: source })?;
        if slot.is_some() {
            return Err(EditError::DuplicateValueReplacement { value: source });
        }
        if definition_block(body, placement, source).is_none() {
            return Err(EditError::DetachedDefinition { value: source });
        }
        match replacement {
            ValueReplacement::Existing(target) => {
                let target_data = body
                    .value(target)
                    .ok_or(EditError::InvalidValue { value: target })?;
                if source_data.ty != target_data.ty {
                    return Err(EditError::TypeMismatch {
                        old: source_data.ty,
                        new: target_data.ty,
                    });
                }
            }
            ValueReplacement::Constant(constant) if source_data.ty != constant.ty() => {
                return Err(EditError::ConstantTypeMismatch {
                    value: source,
                    value_type: source_data.ty,
                    constant_type: constant.ty(),
                });
            }
            ValueReplacement::Constant(_) => {}
        }
        *slot = Some(replacement);
        sources.push(source);
    }
    let resolved = resolve_replacements(&mappings, &sources)?;
    Ok((sources, resolved))
}

fn validate_resolved_replacements(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    sources: &[ValueId],
    resolved: &[Option<ResolvedReplacement>],
) -> Result<(), EditError> {
    let mut existing = vec![];
    for source in sources {
        if let ResolvedReplacement::Existing(target) = resolved_for(resolved, *source)? {
            if definition_block(body, placement, target).is_none() {
                return Err(EditError::DetachedDefinition { value: target });
            }
            existing.push((*source, target));
        }
    }
    if existing.is_empty() {
        return Ok(());
    }
    let uses = UseIndex::from_placement(placement);
    let cfg = ControlFlowGraph::from_placement(placement);
    let dominators = DominatorTree::new(&cfg);
    for (source, target) in existing {
        for use_site in uses.uses(source) {
            validate_value_at_use(body, placement, &dominators, target, *use_site)?;
        }
    }
    Ok(())
}

struct MaterializedConstants {
    final_values: Vec<Option<ValueId>>,
    new_instructions: Vec<(InstId, InstData)>,
    new_values: Vec<(ValueId, ValueData)>,
    block_insertions: Vec<Vec<(Option<InstId>, InstId)>>,
}

fn prepare_constant_replacements(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    sources: &[ValueId],
    resolved: &[Option<ResolvedReplacement>],
    maximum_entities: u64,
) -> Result<MaterializedConstants, EditError> {
    let constant_count = sources
        .iter()
        .filter(|source| {
            matches!(
                resolved_for(resolved, **source),
                Ok(ResolvedReplacement::Constant(_))
            )
        })
        .count();
    checked_entity_growth(body.instructions.len(), constant_count, maximum_entities)?;
    checked_entity_growth(body.values.len(), constant_count, maximum_entities)?;

    let mut materialized = MaterializedConstants {
        final_values: vec![None; body.values.len()],
        new_instructions: Vec::with_capacity(constant_count),
        new_values: Vec::with_capacity(constant_count),
        block_insertions: vec![vec![]; body.blocks.len()],
    };
    for source in sources {
        match resolved_for(resolved, *source)? {
            ResolvedReplacement::Existing(target) => {
                materialized.final_values[value_index(*source).expect("validated dense value")] =
                    Some(target);
            }
            ResolvedReplacement::Constant(constant) => {
                materialize_one_constant(body, placement, *source, constant, &mut materialized)?;
            }
        }
    }
    Ok(materialized)
}

fn materialize_one_constant(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    source: ValueId,
    constant: TypedCoreConstant,
    materialized: &mut MaterializedConstants,
) -> Result<(), EditError> {
    let (block, before, origin) = definition_site(body, placement, source)?;
    let offset = materialized.new_instructions.len();
    let instruction = future_entity_id::<InstId>(body.instructions.len(), offset)?;
    let value = future_entity_id::<ValueId>(body.values.len(), offset)?;
    materialized.new_instructions.push((
        instruction,
        InstData {
            op: constant.op(),
            operands: vec![],
            results: vec![value],
            origin,
        },
    ));
    materialized.new_values.push((
        value,
        ValueData {
            ty: constant.ty(),
            definition: ValueDef::InstResult {
                instruction,
                result_index: 0,
            },
        },
    ));
    materialized.final_values[value_index(source).expect("validated dense value")] = Some(value);
    materialized.block_insertions[block_index(block).expect("validated dense block")]
        .push((before, instruction));
    Ok(())
}

fn prepare_inserted_layout(
    body: &FunctionBody,
    block_insertions: &[Vec<(Option<InstId>, InstId)>],
) -> Result<PreparedBlockLayout, EditError> {
    let mut updates = vec![];
    for block in &body.block_order {
        let data = body
            .block(*block)
            .ok_or(EditError::InvalidBlock { block: *block })?;
        let insertions = &block_insertions[block_index(*block).expect("validated dense block")];
        if insertions.is_empty() {
            continue;
        }
        let rebuilt = insert_instructions(&data.instructions, insertions);
        updates.push((*block, rebuilt));
    }
    Ok(updates)
}

fn insert_instructions(
    original: &[InstId],
    insertions: &[(Option<InstId>, InstId)],
) -> Vec<InstId> {
    let mut rebuilt = Vec::with_capacity(original.len() + insertions.len());
    let mut next = 0_usize;
    while insertions
        .get(next)
        .is_some_and(|(before, _)| before.is_none())
    {
        rebuilt.push(insertions[next].1);
        next += 1;
    }
    for instruction in original {
        while insertions
            .get(next)
            .is_some_and(|(before, _)| *before == Some(*instruction))
        {
            rebuilt.push(insertions[next].1);
            next += 1;
        }
        rebuilt.push(*instruction);
    }
    debug_assert_eq!(next, insertions.len());
    rebuilt
}

struct PreparedValueBatch {
    final_values: Vec<Option<ValueId>>,
    new_instructions: Vec<(InstId, InstData)>,
    new_values: Vec<(ValueId, ValueData)>,
    block_instruction_updates: Vec<(BlockId, Vec<InstId>)>,
    attached_blocks: Vec<BlockId>,
}

impl PreparedValueBatch {
    const fn empty() -> Self {
        Self {
            final_values: vec![],
            new_instructions: vec![],
            new_values: vec![],
            block_instruction_updates: vec![],
            attached_blocks: vec![],
        }
    }

    fn apply<I: BatchEditInstrumentation>(self, body: &mut FunctionBody, instrumentation: &mut I) {
        if self.final_values.is_empty() {
            return;
        }
        for (expected, instruction) in self.new_instructions {
            let actual = body
                .instructions
                .push(instruction)
                .expect("prepared instruction growth was capacity-checked");
            assert_eq!(actual, expected, "prepared instruction ID changed");
        }
        for (expected, value) in self.new_values {
            let actual = body
                .values
                .push(value)
                .expect("prepared value growth was capacity-checked");
            assert_eq!(actual, expected, "prepared value ID changed");
        }
        for (block, instructions) in self.block_instruction_updates {
            body.block_mut(block)
                .expect("prepared block remains allocated")
                .instructions = instructions;
        }
        let rewrite = |value: &mut ValueId| {
            if let Some(replacement) = value_index(*value)
                .and_then(|index| self.final_values.get(index))
                .copied()
                .flatten()
            {
                *value = replacement;
            }
        };
        instrumentation.application_scan_started();
        for block in self.attached_blocks {
            instrumentation.attached_block_scanned();
            let instruction_count = body
                .block(block)
                .expect("prepared attached block remains allocated")
                .instructions
                .len();
            for position in 0..instruction_count {
                instrumentation.attached_instruction_membership_scanned();
                let instruction = body
                    .block(block)
                    .expect("prepared attached block remains allocated")
                    .instructions[position];
                let data = body
                    .instruction_mut(instruction)
                    .expect("prepared attached instruction remains allocated");
                for operand in &mut data.operands {
                    rewrite(operand);
                }
            }
            let terminator = body
                .block_mut(block)
                .and_then(|data| data.terminator.as_mut())
                .expect("prepared attached block remains terminated");
            rewrite_terminator_values(&mut terminator.kind, &rewrite);
        }
    }
}

fn normalize_operand_edits(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    mut edits: Vec<OperandEdit>,
) -> Result<Option<Vec<OperandEdit>>, EditError> {
    edits.sort_by_key(|(instruction, _)| instruction.index());
    let mut seen = vec![false; body.instructions.len()];
    let mut all_identical = true;
    for (instruction, operands) in &edits {
        let index = instruction_index(*instruction).ok_or(EditError::InvalidInstruction {
            instruction: *instruction,
        })?;
        let slot = seen.get_mut(index).ok_or(EditError::InvalidInstruction {
            instruction: *instruction,
        })?;
        if *slot {
            return Err(EditError::DuplicateInstructionEdit {
                instruction: *instruction,
            });
        }
        *slot = true;
        if !placement.is_instruction_attached(*instruction) {
            return Err(EditError::InvalidInstruction {
                instruction: *instruction,
            });
        }
        let data = body
            .instruction(*instruction)
            .ok_or(EditError::InvalidInstruction {
                instruction: *instruction,
            })?;
        all_identical &= data.operands == *operands;
    }
    Ok((!all_identical).then_some(edits))
}

struct PreparedOperandBatch {
    edits: Vec<(InstId, Vec<ValueId>)>,
}

impl PreparedOperandBatch {
    fn apply(self, body: &mut FunctionBody) {
        for (instruction, operands) in self.edits {
            body.instruction_mut(instruction)
                .expect("prepared instruction remains allocated")
                .operands = operands;
        }
    }
}

struct PreparedErasure {
    selected: Vec<bool>,
    attached_blocks: Vec<BlockId>,
}

fn selected_result_bits(body: &FunctionBody, selected: &[bool]) -> Result<Vec<bool>, EditError> {
    let mut results = vec![false; body.values.len()];
    for (index, is_selected) in selected.iter().copied().enumerate() {
        if !is_selected {
            continue;
        }
        let instruction = InstId::from_index(
            u32::try_from(index).expect("instruction storage is u32 constrained"),
        );
        let data = body
            .instruction(instruction)
            .expect("selected instruction was validated");
        for result in &data.results {
            let result_index =
                value_index(*result).ok_or(EditError::InvalidValue { value: *result })?;
            let slot = results
                .get_mut(result_index)
                .ok_or(EditError::InvalidValue { value: *result })?;
            *slot = true;
        }
    }
    Ok(results)
}

fn reject_escaping_selected_results<I: BatchEditInstrumentation>(
    body: &FunctionBody,
    selected_instructions: &[bool],
    selected_values: &[bool],
    instrumentation: &mut I,
) -> Result<(), EditError> {
    instrumentation.erasure_closure_scan_started();
    for block in &body.block_order {
        instrumentation.erasure_closure_block_scanned();
        let data = body
            .block(*block)
            .ok_or(EditError::InvalidBlock { block: *block })?;
        for instruction in &data.instructions {
            instrumentation.erasure_closure_instruction_membership_scanned();
            let consumer_selected = instruction_index(*instruction)
                .and_then(|index| selected_instructions.get(index))
                .copied()
                .unwrap_or(false);
            let consumer = body
                .instruction(*instruction)
                .ok_or(EditError::InvalidInstruction {
                    instruction: *instruction,
                })?;
            if !consumer_selected {
                if let Some(value) = consumer
                    .operands
                    .iter()
                    .copied()
                    .find(|value| is_selected_value(selected_values, *value))
                {
                    return Err(EditError::ValueStillUsed { value });
                }
            }
        }
        if let Some(value) = data.terminator.as_ref().and_then(|terminator| {
            first_selected_terminator_value(&terminator.kind, selected_values)
        }) {
            return Err(EditError::ValueStillUsed { value });
        }
    }
    Ok(())
}

impl PreparedErasure {
    fn apply<I: BatchEditInstrumentation>(self, body: &mut FunctionBody, instrumentation: &mut I) {
        if self.selected.is_empty() {
            return;
        }
        instrumentation.application_scan_started();
        for block in self.attached_blocks {
            instrumentation.attached_block_scanned();
            let selected = &self.selected;
            let data = body
                .block_mut(block)
                .expect("prepared attached block remains allocated");
            data.instructions.retain(|instruction| {
                instrumentation.attached_instruction_membership_scanned();
                !instruction_index(*instruction)
                    .and_then(|index| selected.get(index))
                    .copied()
                    .unwrap_or(false)
            });
        }
    }
}

struct PreparedTerminators {
    edits: Vec<(BlockId, Terminator)>,
}

impl PreparedTerminators {
    fn apply(self, body: &mut FunctionBody) {
        for (block, terminator) in self.edits {
            body.block_mut(block)
                .expect("prepared block remains allocated")
                .terminator = Some(terminator);
        }
    }
}

fn resolve_replacements(
    mappings: &[Option<ValueReplacement>],
    sources: &[ValueId],
) -> Result<Vec<Option<ResolvedReplacement>>, EditError> {
    let mut state = vec![0_u8; mappings.len()];
    let mut resolved = vec![None; mappings.len()];
    for source in sources {
        let source_index =
            value_index(*source).ok_or(EditError::InvalidValue { value: *source })?;
        if state[source_index] == 2 {
            continue;
        }
        let mut path = vec![];
        let mut cursor = *source;
        let endpoint = loop {
            let index = value_index(cursor).ok_or(EditError::InvalidValue { value: cursor })?;
            match state[index] {
                1 => return Err(EditError::ReplacementCycle { value: cursor }),
                2 => break resolved[index].expect("resolved state has endpoint"),
                _ => {}
            }
            state[index] = 1;
            path.push(cursor);
            match mappings[index].expect("path only follows replacement sources") {
                ValueReplacement::Existing(next) => {
                    if value_index(next)
                        .and_then(|next_index| mappings.get(next_index))
                        .is_some_and(Option::is_some)
                    {
                        cursor = next;
                    } else {
                        break ResolvedReplacement::Existing(next);
                    }
                }
                ValueReplacement::Constant(constant) => {
                    break ResolvedReplacement::Constant(constant);
                }
            }
        };
        for value in path.into_iter().rev() {
            let index = value_index(value).expect("validated dense value");
            state[index] = 2;
            resolved[index] = Some(endpoint);
        }
    }
    Ok(resolved)
}

fn resolved_for(
    resolved: &[Option<ResolvedReplacement>],
    value: ValueId,
) -> Result<ResolvedReplacement, EditError> {
    value_index(value)
        .and_then(|index| resolved.get(index))
        .copied()
        .flatten()
        .ok_or(EditError::InvalidValue { value })
}

fn validate_value_at_use(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    dominators: &DominatorTree<'_>,
    value: ValueId,
    use_site: UseSite,
) -> Result<(), EditError> {
    body.value(value).ok_or(EditError::InvalidValue { value })?;
    let definition =
        definition_block(body, placement, value).ok_or(EditError::DetachedDefinition { value })?;
    let use_block = use_site.block();
    if definition == use_block {
        if !definition_precedes(body, placement, value, use_site) {
            return Err(EditError::DoesNotDominate { value, use_block });
        }
    } else if dominators.dominates(definition, use_block) == Dominance::DoesNotDominate {
        return Err(EditError::DoesNotDominate { value, use_block });
    }
    Ok(())
}

fn definition_precedes(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    value: ValueId,
    use_site: UseSite,
) -> bool {
    match body.value(value).map(|data| data.definition) {
        Some(ValueDef::BlockParam { .. }) => true,
        Some(ValueDef::InstResult { instruction, .. }) => {
            let Some((_, definition_position)) = placement.instruction(instruction) else {
                return false;
            };
            match use_site {
                UseSite::InstructionOperand {
                    instruction: user, ..
                } => placement
                    .instruction(user)
                    .is_some_and(|(_, use_position)| definition_position < use_position),
                UseSite::BranchCondition { .. }
                | UseSite::EdgeArgument { .. }
                | UseSite::Return { .. } => true,
            }
        }
        None => false,
    }
}

fn definition_site_key(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    layout_positions: &[usize],
    value: ValueId,
) -> (usize, u8, usize, u32, u32) {
    let layout = definition_block(body, placement, value)
        .map_or(usize::MAX, |block| layout_position(layout_positions, block));
    match body.value(value).map(|data| data.definition) {
        Some(ValueDef::BlockParam {
            parameter_index, ..
        }) => (layout, 0, 0, parameter_index, value.index()),
        Some(ValueDef::InstResult {
            instruction,
            result_index,
        }) => (
            layout,
            1,
            placement
                .instruction(instruction)
                .map_or(usize::MAX, |placed| placed.1),
            result_index,
            value.index(),
        ),
        None => (usize::MAX, u8::MAX, usize::MAX, u32::MAX, value.index()),
    }
}

fn definition_site(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    value: ValueId,
) -> Result<(BlockId, Option<InstId>, OriginId), EditError> {
    match body
        .value(value)
        .ok_or(EditError::InvalidValue { value })?
        .definition
    {
        ValueDef::BlockParam {
            block,
            parameter_index,
        } => {
            if !placement.is_block_attached(block) {
                return Err(EditError::DetachedDefinition { value });
            }
            let origin = usize::try_from(parameter_index)
                .ok()
                .and_then(|index| body.block(block)?.parameters.get(index))
                .map(|parameter| parameter.origin)
                .ok_or(EditError::InvalidValue { value })?;
            Ok((block, None, origin))
        }
        ValueDef::InstResult { instruction, .. } => {
            let (block, _) = placement
                .instruction(instruction)
                .ok_or(EditError::DetachedDefinition { value })?;
            let origin = body
                .instruction(instruction)
                .ok_or(EditError::InvalidInstruction { instruction })?
                .origin;
            Ok((block, Some(instruction), origin))
        }
    }
}

fn validate_terminator_contract(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    block: BlockId,
    terminator: &Terminator,
    function_results: &[CoreType],
) -> Result<(), EditError> {
    match &terminator.kind {
        TerminatorKind::Jump(target) => {
            validate_target(body, placement, block, target, terminator.origin)
        }
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => {
            let condition_type = body
                .value(*condition)
                .ok_or(EditError::InvalidValue { value: *condition })?
                .ty;
            if condition_type != CoreType::Bool {
                return Err(terminator_error(
                    "core.branch-condition-type",
                    format!("branch condition {condition:?} has type {condition_type}"),
                    terminator.origin,
                ));
            }
            validate_target(body, placement, block, then_target, terminator.origin)?;
            validate_target(body, placement, block, else_target, terminator.origin)
        }
        TerminatorKind::Return(values) => {
            if values.len() != function_results.len() {
                return Err(terminator_error(
                    "core.return-count",
                    format!(
                        "return expects {} values but has {}",
                        function_results.len(),
                        values.len()
                    ),
                    terminator.origin,
                ));
            }
            for (index, (value, expected)) in values
                .iter()
                .copied()
                .zip(function_results.iter().copied())
                .enumerate()
            {
                let actual = body
                    .value(value)
                    .ok_or(EditError::InvalidValue { value })?
                    .ty;
                if actual != expected {
                    return Err(terminator_error(
                        "core.return-type",
                        format!("return value {index} expects {expected} but has {actual}"),
                        terminator.origin,
                    ));
                }
            }
            Ok(())
        }
        TerminatorKind::Unreachable => Ok(()),
    }
}

fn validate_target(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    source: BlockId,
    target: &BlockTarget,
    origin: OriginId,
) -> Result<(), EditError> {
    if target.block == body.entry {
        return Err(terminator_error(
            "core.target-entry",
            format!("{source:?} targets the entry block"),
            origin,
        ));
    }
    if !placement.is_block_attached(target.block) {
        return Err(terminator_error(
            "core.detached-target",
            format!("{source:?} targets detached or invalid {:?}", target.block),
            origin,
        ));
    }
    let destination = body.block(target.block).ok_or(EditError::InvalidBlock {
        block: target.block,
    })?;
    if target.arguments.len() != destination.parameters.len() {
        return Err(terminator_error(
            "core.edge-argument-count",
            format!(
                "edge to {:?} expects {} arguments but has {}",
                target.block,
                destination.parameters.len(),
                target.arguments.len()
            ),
            origin,
        ));
    }
    for (index, (argument, parameter)) in target
        .arguments
        .iter()
        .copied()
        .zip(&destination.parameters)
        .enumerate()
    {
        let actual = body
            .value(argument)
            .ok_or(EditError::InvalidValue { value: argument })?
            .ty;
        let expected = body
            .value(parameter.value)
            .ok_or(EditError::InvalidValue {
                value: parameter.value,
            })?
            .ty;
        if actual != expected {
            return Err(terminator_error(
                "core.edge-argument-type",
                format!("edge argument {index} expects {expected} but has {actual}"),
                origin,
            ));
        }
    }
    Ok(())
}

fn validate_terminator_ssa(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    dominators: &DominatorTree<'_>,
    block: BlockId,
    kind: &TerminatorKind,
) -> Result<(), EditError> {
    match kind {
        TerminatorKind::Jump(target) => {
            for (argument_index, value) in target.arguments.iter().copied().enumerate() {
                validate_value_at_use(
                    body,
                    placement,
                    dominators,
                    value,
                    UseSite::EdgeArgument {
                        block,
                        successor_index: 0,
                        argument_index,
                    },
                )?;
            }
        }
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => {
            validate_value_at_use(
                body,
                placement,
                dominators,
                *condition,
                UseSite::BranchCondition { block },
            )?;
            for (successor_index, target) in [then_target, else_target].into_iter().enumerate() {
                for (argument_index, value) in target.arguments.iter().copied().enumerate() {
                    validate_value_at_use(
                        body,
                        placement,
                        dominators,
                        value,
                        UseSite::EdgeArgument {
                            block,
                            successor_index,
                            argument_index,
                        },
                    )?;
                }
            }
        }
        TerminatorKind::Return(values) => {
            for (result_index, value) in values.iter().copied().enumerate() {
                validate_value_at_use(
                    body,
                    placement,
                    dominators,
                    value,
                    UseSite::Return {
                        block,
                        result_index,
                    },
                )?;
            }
        }
        TerminatorKind::Unreachable => {}
    }
    Ok(())
}

fn rewrite_terminator_values(kind: &mut TerminatorKind, mut rewrite: impl FnMut(&mut ValueId)) {
    match kind {
        TerminatorKind::Jump(target) => {
            for value in &mut target.arguments {
                rewrite(value);
            }
        }
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => {
            rewrite(condition);
            for value in &mut then_target.arguments {
                rewrite(value);
            }
            for value in &mut else_target.arguments {
                rewrite(value);
            }
        }
        TerminatorKind::Return(values) => {
            for value in values {
                rewrite(value);
            }
        }
        TerminatorKind::Unreachable => {}
    }
}

fn first_selected_terminator_value(
    kind: &TerminatorKind,
    selected_values: &[bool],
) -> Option<ValueId> {
    match kind {
        TerminatorKind::Jump(target) => target
            .arguments
            .iter()
            .copied()
            .find(|value| is_selected_value(selected_values, *value)),
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => is_selected_value(selected_values, *condition)
            .then_some(*condition)
            .or_else(|| {
                then_target
                    .arguments
                    .iter()
                    .copied()
                    .find(|value| is_selected_value(selected_values, *value))
            })
            .or_else(|| {
                else_target
                    .arguments
                    .iter()
                    .copied()
                    .find(|value| is_selected_value(selected_values, *value))
            }),
        TerminatorKind::Return(values) => values
            .iter()
            .copied()
            .find(|value| is_selected_value(selected_values, *value)),
        TerminatorKind::Unreachable => None,
    }
}

fn is_selected_value(selected_values: &[bool], value: ValueId) -> bool {
    value_index(value)
        .and_then(|index| selected_values.get(index))
        .copied()
        .unwrap_or(false)
}

fn terminator_error(code: &'static str, message: impl Into<String>, origin: OriginId) -> EditError {
    EditError::InvalidTerminator(
        Diagnostics::from_findings(vec![Diagnostic::new(code, message, origin)])
            .expect("terminator edit produces one diagnostic"),
    )
}

fn checked_entity_growth(
    current: usize,
    additional: usize,
    maximum_entities: u64,
) -> Result<(), EditError> {
    let final_count = u64::try_from(current)
        .ok()
        .and_then(|current| {
            u64::try_from(additional)
                .ok()
                .and_then(|additional| current.checked_add(additional))
        })
        .ok_or(EditError::EntityLimit)?;
    if final_count > maximum_entities.min(ENTITY_CAPACITY) {
        return Err(EditError::EntityLimit);
    }
    Ok(())
}

fn future_entity_id<I: EntityId>(current: usize, offset: usize) -> Result<I, EditError> {
    let index = current
        .checked_add(offset)
        .and_then(|index| u32::try_from(index).ok())
        .ok_or(EditError::EntityLimit)?;
    Ok(I::from_index(index))
}

fn block_layout_positions(body: &FunctionBody) -> Vec<usize> {
    let mut positions = vec![usize::MAX; body.blocks.len()];
    for (position, block) in body.block_order.iter().copied().enumerate() {
        if let Some(slot) = block_index(block).and_then(|index| positions.get_mut(index)) {
            *slot = position;
        }
    }
    positions
}

fn layout_position(positions: &[usize], block: BlockId) -> usize {
    block_index(block)
        .and_then(|index| positions.get(index))
        .copied()
        .unwrap_or(usize::MAX)
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

/// Rejected in-place edit. Every variant is reported before mutation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditError {
    /// The checked editor was opened on invalid input IR.
    InvalidInput(Diagnostics),
    /// The function declaration is absent.
    InvalidFunction { function: FunctionId },
    /// A block is absent or detached.
    InvalidBlock { block: BlockId },
    /// An instruction is absent or detached.
    InvalidInstruction { instruction: InstId },
    /// A value is absent.
    InvalidValue { value: ValueId },
    /// More than one edit names the same value source.
    DuplicateValueReplacement { value: ValueId },
    /// More than one edit names the same instruction.
    DuplicateInstructionEdit { instruction: InstId },
    /// More than one edit names the same block.
    DuplicateBlockEdit { block: BlockId },
    /// A replacement graph contains a cycle, including an explicit self map.
    ReplacementCycle { value: ValueId },
    /// Replacement value types differ.
    TypeMismatch { old: CoreType, new: CoreType },
    /// A constant replacement does not match its source type.
    ConstantTypeMismatch {
        value: ValueId,
        value_type: CoreType,
        constant_type: CoreType,
    },
    /// A source or final replacement is defined in detached code.
    DetachedDefinition { value: ValueId },
    /// A replacement or operand does not dominate one reachable use.
    DoesNotDominate { value: ValueId, use_block: BlockId },
    /// An instruction's operation contract cannot be resolved.
    InvalidOperation { instruction: InstId },
    /// A complete operand edit has the wrong arity.
    OperandCount {
        instruction: InstId,
        expected: usize,
        actual: usize,
    },
    /// One replacement operand has the wrong type.
    OperandType {
        instruction: InstId,
        operand_index: usize,
        expected: CoreType,
        actual: CoreType,
    },
    /// Generic legacy replacement requires a pure operation.
    NotPure { instruction: InstId },
    /// Set erasure requires a pure and always-speculatable operation.
    NotDiscardable { instruction: InstId },
    /// A result still has an attached use outside the erased set.
    ValueStillUsed { value: ValueId },
    /// Instruction result arities differ.
    ResultCount { old: usize, replacement: usize },
    /// A candidate terminator is structurally or SSA-invalid.
    InvalidTerminator(Diagnostics),
    /// The instruction or value ID space cannot hold the prepared edit.
    EntityLimit,
    /// Checked scratch, instruction-count, or byte-size arithmetic overflowed.
    SizeOverflow,
    /// Frozen jump-fusion facts or a submitted region contradict trusted Core.
    InconsistentJumpFusion(&'static str),
    /// Retained for source compatibility with older pass code.
    InvalidResult(Diagnostics),
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for EditError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::core::{BlockTarget, CoreOp, FunctionBuilder};

    fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
        builder.body().block(block).unwrap().parameters[index].value
    }

    fn defining_instruction(body: &FunctionBody, value: ValueId) -> InstId {
        match body.value(value).unwrap().definition {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => panic!("expected instruction result"),
        }
    }

    fn one_i32_parameter_body() -> (
        SourceContext,
        CoreProgram,
        FunctionId,
        FunctionBody,
        ValueId,
    ) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("one"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let value = parameter(&builder, entry, 0);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        (sources, program, function, body, value)
    }

    #[test]
    fn resolves_twenty_thousand_replacements_without_host_recursion() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("chain"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let values = (0..=20_000)
            .map(|value| builder.i32_constant(value, OriginId::UNKNOWN).unwrap())
            .collect::<Vec<_>>();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![values[0]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let edits = values
            .windows(2)
            .map(|pair| (pair[0], ValueReplacement::Existing(pair[1])))
            .collect();
        let statistics = FunctionEditor::new(&program, &sources, function, &mut body)
            .unwrap()
            .replace_values_batch_with_statistics(edits)
            .unwrap();
        assert_eq!(statistics.placement_indices_built, 1);
        assert_eq!(statistics.erasure_ownership_scans, 0);
        assert_eq!(statistics.erasure_closure_scans, 0);
        assert_eq!(statistics.application_scans, 1);
        assert_eq!(
            statistics.attached_instruction_memberships_scanned,
            values.len()
        );
        assert_eq!(statistics.attached_blocks_scanned, 1);
        let TerminatorKind::Return(results) = &body
            .block(body.entry)
            .unwrap()
            .terminator
            .as_ref()
            .unwrap()
            .kind
        else {
            panic!("expected return");
        };
        assert_eq!(results, &[values[20_000]]);
    }

    #[test]
    fn erases_twenty_thousand_instructions_with_one_analysis_and_application_scan() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("erasure-scale"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let mut value = parameter(&builder, entry, 0);
        let mut instructions = Vec::with_capacity(20_000);
        for _ in 0..20_000 {
            value = builder.bool_not(value, OriginId::UNKNOWN).unwrap();
            instructions.push(defining_instruction(builder.body(), value));
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let statistics = FunctionEditor::new(&program, &sources, function, &mut body)
            .unwrap()
            .erase_discardable_inst_set_with_statistics(instructions)
            .unwrap();

        assert_eq!(statistics.placement_indices_built, 1);
        assert_eq!(statistics.erasure_ownership_scans, 1);
        assert_eq!(statistics.erasure_ownership_memberships_scanned, 20_000);
        assert_eq!(statistics.erasure_closure_scans, 1);
        assert_eq!(
            statistics.erasure_closure_instruction_memberships_scanned,
            20_000
        );
        assert_eq!(statistics.erasure_closure_blocks_scanned, 1);
        assert_eq!(statistics.application_scans, 1);
        assert_eq!(statistics.attached_instruction_memberships_scanned, 20_000);
        assert_eq!(statistics.attached_blocks_scanned, 1);
        assert_eq!(body.instruction_counts().attached, 0);
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn constants_are_distinct_definition_site_ordered_and_input_order_independent() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("constants"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let first = parameter(&builder, entry, 0);
        let second = parameter(&builder, entry, 1);
        let sum = builder
            .i32_add_wrapping(first, second, OriginId::UNKNOWN)
            .unwrap();
        let sum_instruction = defining_instruction(builder.body(), sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let original = builder.finish().unwrap();
        let mut forward = original.clone();
        let mut reverse = original;
        let make_edits = |reversed: bool| {
            let mut edits = vec![
                (first, ValueReplacement::Constant(TypedCoreConstant::I32(7))),
                (
                    second,
                    ValueReplacement::Constant(TypedCoreConstant::I32(7)),
                ),
            ];
            if reversed {
                edits.reverse();
            }
            edits
        };
        FunctionEditor::new(&program, &sources, function, &mut forward)
            .unwrap()
            .replace_values_batch(make_edits(false))
            .unwrap();
        FunctionEditor::new(&program, &sources, function, &mut reverse)
            .unwrap()
            .replace_values_batch(make_edits(true))
            .unwrap();
        assert_eq!(format!("{forward:#?}"), format!("{reverse:#?}"));

        let entry_data = forward.block(entry).unwrap();
        assert_eq!(entry_data.instructions.len(), 3);
        let operands = &forward.instruction(sum_instruction).unwrap().operands;
        assert_ne!(operands[0], operands[1]);
        for (instruction, operand) in entry_data.instructions[..2].iter().zip(operands) {
            let data = forward.instruction(*instruction).unwrap();
            assert_eq!(data.op, CoreOp::I32Constant(7));
            assert_eq!(data.results, vec![*operand]);
        }
    }

    #[test]
    fn mixed_existing_constant_chain_materializes_each_source_at_its_own_site() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("mixed-constant-chain"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let first = parameter(&builder, entry, 0);
        let second = parameter(&builder, entry, 1);
        let sum = builder
            .i32_add_wrapping(first, second, OriginId::UNKNOWN)
            .unwrap();
        let sum_instruction = defining_instruction(builder.body(), sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        FunctionEditor::new(&program, &sources, function, &mut body)
            .unwrap()
            .replace_values_batch(vec![
                (first, ValueReplacement::Existing(second)),
                (
                    second,
                    ValueReplacement::Constant(TypedCoreConstant::I32(7)),
                ),
            ])
            .unwrap();

        let operands = &body.instruction(sum_instruction).unwrap().operands;
        assert_ne!(operands[0], operands[1]);
        for operand in operands {
            let instruction = defining_instruction(&body, *operand);
            assert_eq!(
                body.instruction(instruction).unwrap().op,
                CoreOp::I32Constant(7)
            );
        }
        verify_function(&program, &sources, function, &body).unwrap();
    }

    #[test]
    fn value_batch_rejects_detached_endpoints_and_leaves_detached_uses_untouched() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("detached-history"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let parameter = parameter(&builder, entry, 0);
        let dead = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(dead).unwrap();
        let detached_constant = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let detached_sum = builder
            .i32_add_wrapping(parameter, detached_constant, OriginId::UNKNOWN)
            .unwrap();
        let detached_sum_instruction = defining_instruction(builder.body(), detached_sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Unreachable,
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        FunctionEditor::new(&program, &sources, function, &mut body)
            .unwrap()
            .detach_unreachable_blocks()
            .unwrap();
        let original = format!("{body:#?}");

        let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();
        assert_eq!(
            editor
                .replace_values_batch(vec![(
                    detached_constant,
                    ValueReplacement::Existing(parameter),
                )])
                .unwrap_err(),
            EditError::DetachedDefinition {
                value: detached_constant
            }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
        assert_eq!(
            editor
                .replace_values_batch(vec![(
                    parameter,
                    ValueReplacement::Existing(detached_constant),
                )])
                .unwrap_err(),
            EditError::DetachedDefinition {
                value: detached_constant
            }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);

        editor
            .replace_values_batch(vec![(
                parameter,
                ValueReplacement::Constant(TypedCoreConstant::I32(9)),
            )])
            .unwrap();
        assert_eq!(
            editor
                .body()
                .instruction(detached_sum_instruction)
                .unwrap()
                .operands[0],
            parameter,
            "detached diagnostic history must not be rewritten"
        );
        verify_function(&program, &sources, function, editor.body()).unwrap();
    }

    #[test]
    fn cycle_duplicate_and_capacity_failures_are_atomic() {
        let (sources, program, function, mut body, value) = one_i32_parameter_body();
        let original = format!("{body:#?}");
        let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();
        assert_eq!(
            editor
                .replace_values_batch(vec![(value, ValueReplacement::Existing(value))])
                .unwrap_err(),
            EditError::ReplacementCycle { value }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
        assert_eq!(
            editor
                .replace_values_batch(vec![
                    (value, ValueReplacement::Constant(TypedCoreConstant::I32(1))),
                    (value, ValueReplacement::Constant(TypedCoreConstant::I32(2))),
                ])
                .unwrap_err(),
            EditError::DuplicateValueReplacement { value }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
        assert_eq!(
            editor
                .replace_values_batch(vec![(
                    value,
                    ValueReplacement::Constant(TypedCoreConstant::Bool(true)),
                )])
                .unwrap_err(),
            EditError::ConstantTypeMismatch {
                value,
                value_type: CoreType::I32,
                constant_type: CoreType::Bool,
            }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
        let current_values = editor.body().value_counts().allocated as u64;
        assert_eq!(
            editor
                .replace_values_batch_with_entity_limit(
                    vec![(value, ValueReplacement::Constant(TypedCoreConstant::I32(3)),)],
                    current_values,
                )
                .unwrap_err(),
            EditError::EntityLimit
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
    }

    #[test]
    fn operand_batch_rejects_late_self_use_without_applying_earlier_edit() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("operands"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let two = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let first_sum = builder
            .i32_add_wrapping(one, one, OriginId::UNKNOWN)
            .unwrap();
        let final_sum = builder
            .i32_add_wrapping(first_sum, two, OriginId::UNKNOWN)
            .unwrap();
        let first_instruction = defining_instruction(builder.body(), first_sum);
        let final_instruction = defining_instruction(builder.body(), final_sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![final_sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let original = format!("{body:#?}");
        let error = FunctionEditor::new(&program, &sources, function, &mut body)
            .unwrap()
            .rewrite_inst_operands_batch(vec![
                (first_instruction, vec![two, two]),
                (final_instruction, vec![final_sum, two]),
            ])
            .unwrap_err();
        assert!(matches!(error, EditError::DoesNotDominate { value, .. } if value == final_sum));
        assert_eq!(format!("{body:#?}"), original);
    }

    #[test]
    fn operand_batch_rejects_duplicate_arity_and_type_errors_atomically() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("operand-contracts"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let boolean = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(one, one, OriginId::UNKNOWN)
            .unwrap();
        let instruction = defining_instruction(builder.body(), sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let original = format!("{body:#?}");
        let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();

        assert_eq!(
            editor
                .rewrite_inst_operands_batch(vec![
                    (instruction, vec![one, one]),
                    (instruction, vec![one, one]),
                ])
                .unwrap_err(),
            EditError::DuplicateInstructionEdit { instruction }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
        assert_eq!(
            editor
                .rewrite_inst_operands_batch(vec![(instruction, vec![one])])
                .unwrap_err(),
            EditError::OperandCount {
                instruction,
                expected: 2,
                actual: 1,
            }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
        assert_eq!(
            editor
                .rewrite_inst_operands_batch(vec![(instruction, vec![boolean, one])])
                .unwrap_err(),
            EditError::OperandType {
                instruction,
                operand_index: 0,
                expected: CoreType::I32,
                actual: CoreType::Bool,
            }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
    }

    #[test]
    fn erasure_accepts_internal_use_closure_and_rejects_escaping_results() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("erase"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let two = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(one, two, OriginId::UNKNOWN)
            .unwrap();
        let one_instruction = defining_instruction(builder.body(), one);
        let sum_instruction = defining_instruction(builder.body(), sum);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let original = format!("{body:#?}");
        let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();
        assert_eq!(
            editor
                .erase_discardable_inst_set(vec![one_instruction])
                .unwrap_err(),
            EditError::ValueStillUsed { value: one }
        );
        assert_eq!(format!("{:#?}", editor.body()), original);
        editor
            .erase_discardable_inst_set(vec![sum_instruction, one_instruction])
            .unwrap();
        assert!(!PlacementIndex::new(editor.body()).is_instruction_attached(one_instruction));
        assert!(!PlacementIndex::new(editor.body()).is_instruction_attached(sum_instruction));
        assert!(
            PlacementIndex::new(editor.body())
                .is_instruction_attached(defining_instruction(editor.body(), two))
        );
    }

    #[test]
    fn erasure_treats_terminator_values_as_live_roots() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("terminator-root"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let instruction = defining_instruction(builder.body(), value);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let original = format!("{body:#?}");

        assert_eq!(
            FunctionEditor::new(&program, &sources, function, &mut body)
                .unwrap()
                .erase_discardable_inst_set(vec![instruction])
                .unwrap_err(),
            EditError::ValueStillUsed { value }
        );
        assert_eq!(format!("{body:#?}"), original);
    }

    #[test]
    fn projected_cfg_rejects_newly_reachable_bad_use_and_accepts_joint_rewrite() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("projected"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let defining = builder.create_block(OriginId::UNKNOWN).unwrap();
        let unreachable_use = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(defining, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(defining).unwrap();
        let value = builder.i32_constant(9, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(unreachable_use).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        let original = format!("{body:#?}");
        let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();
        assert!(matches!(
            editor.set_terminator(
                entry,
                Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(unreachable_use, vec![])),
                    OriginId::UNKNOWN,
                ),
            ),
            Err(EditError::DoesNotDominate { value: rejected, .. }) if rejected == value
        ));
        assert_eq!(format!("{:#?}", editor.body()), original);
        editor
            .set_terminators_batch(vec![
                (
                    unreachable_use,
                    Terminator::new(
                        TerminatorKind::Jump(BlockTarget::new(defining, vec![])),
                        OriginId::UNKNOWN,
                    ),
                ),
                (
                    entry,
                    Terminator::new(
                        TerminatorKind::Jump(BlockTarget::new(unreachable_use, vec![])),
                        OriginId::UNKNOWN,
                    ),
                ),
            ])
            .unwrap();
        verify_function(&program, &sources, function, editor.body()).unwrap();
        editor
            .set_terminator(
                unreachable_use,
                Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(unreachable_use, vec![])),
                    OriginId::UNKNOWN,
                ),
            )
            .unwrap();
        verify_function(&program, &sources, function, editor.body()).unwrap();
    }

    #[test]
    fn editor_batches_and_detachment_do_not_hide_verifier_calls() {
        let (sources, program, function, mut body, _) = one_i32_parameter_body();
        super::super::verify::reset_verifier_counters();
        let entry = body.entry;
        let existing = body.block(entry).unwrap().terminator.clone().unwrap();
        let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();
        assert_eq!(super::super::verify::verifier_counters(), (1, 0));
        editor.rewrite_inst_operands_batch(vec![]).unwrap();
        editor
            .set_terminators_batch(vec![(entry, existing)])
            .unwrap();
        editor.detach_unreachable_blocks().unwrap();
        assert_eq!(super::super::verify::verifier_counters(), (1, 0));
    }

    fn jump_fusion_chain_with_instruction() -> (
        SourceContext,
        CoreProgram,
        FunctionId,
        FunctionBody,
        BlockId,
        BlockId,
    ) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("editor-fusion"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let middle = builder.create_block(OriginId::UNKNOWN).unwrap();
        let tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(middle, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(middle).unwrap();
        builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(tail, vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(tail).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        (sources, program, function, body, middle, tail)
    }

    #[test]
    fn fusion_batch_rejects_nonmaximal_and_overlapping_regions_atomically() {
        let (sources, program, function, body, _, _) = jump_fusion_chain_with_instruction();

        let mut nonmaximal_body = body.clone();
        let before = format!("{nonmaximal_body:#?}");
        let error = {
            let mut editor =
                FunctionEditor::new(&program, &sources, function, &mut nonmaximal_body).unwrap();
            let mut prepared = match editor.prepare_jump_fusion(None).unwrap() {
                JumpFusionPreparation::Ready(prepared) => prepared,
                JumpFusionPreparation::StoppedAtFactTableLimit { .. } => unreachable!(),
            };
            let mut discovery = prepared.discover_maximal_regions(usize::MAX).unwrap();
            discovery.regions[0].blocks.pop();
            prepared
                .fuse_jump_regions_batch(&discovery.regions)
                .unwrap_err()
        };
        assert!(matches!(error, EditError::InconsistentJumpFusion(_)));
        assert_eq!(format!("{nonmaximal_body:#?}"), before);

        let mut overlapping_body = body;
        let before = format!("{overlapping_body:#?}");
        let error = {
            let mut editor =
                FunctionEditor::new(&program, &sources, function, &mut overlapping_body).unwrap();
            let mut prepared = match editor.prepare_jump_fusion(None).unwrap() {
                JumpFusionPreparation::Ready(prepared) => prepared,
                JumpFusionPreparation::StoppedAtFactTableLimit { .. } => unreachable!(),
            };
            let mut discovery = prepared.discover_maximal_regions(usize::MAX).unwrap();
            discovery.regions.push(discovery.regions[0].clone());
            prepared
                .fuse_jump_regions_batch(&discovery.regions)
                .unwrap_err()
        };
        assert!(matches!(error, EditError::InconsistentJumpFusion(_)));
        assert_eq!(format!("{overlapping_body:#?}"), before);
    }

    #[test]
    fn fusion_size_failure_and_stale_edge_are_atomic() {
        let (sources, program, function, body, middle, _) = jump_fusion_chain_with_instruction();

        let mut limited_body = body.clone();
        let before = format!("{limited_body:#?}");
        let error = {
            let mut editor =
                FunctionEditor::new(&program, &sources, function, &mut limited_body).unwrap();
            let mut prepared = match editor.prepare_jump_fusion(None).unwrap() {
                JumpFusionPreparation::Ready(prepared) => prepared,
                JumpFusionPreparation::StoppedAtFactTableLimit { .. } => unreachable!(),
            };
            let discovery = prepared.discover_maximal_regions(usize::MAX).unwrap();
            prepared
                .fuse_jump_regions_batch_with_instruction_limit(&discovery.regions, 0)
                .unwrap_err()
        };
        assert_eq!(error, EditError::SizeOverflow);
        assert_eq!(format!("{limited_body:#?}"), before);

        let mut stale_body = body;
        let error = {
            let mut editor =
                FunctionEditor::new(&program, &sources, function, &mut stale_body).unwrap();
            let mut prepared = match editor.prepare_jump_fusion(None).unwrap() {
                JumpFusionPreparation::Ready(prepared) => prepared,
                JumpFusionPreparation::StoppedAtFactTableLimit { .. } => unreachable!(),
            };
            let discovery = prepared.discover_maximal_regions(usize::MAX).unwrap();
            prepared.body.block_mut(middle).unwrap().terminator = Some(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ));
            let corrupted = format!("{:#?}", prepared.body);
            let error = prepared
                .fuse_jump_regions_batch(&discovery.regions)
                .unwrap_err();
            (error, corrupted)
        };
        assert!(matches!(error.0, EditError::InconsistentJumpFusion(_)));
        assert_eq!(format!("{stale_body:#?}"), error.1);
    }

    #[test]
    fn fusion_detects_corrupted_substitution_cycle_before_semantic_mutation() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("fusion-cycle"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let argument = parameter(&builder, entry, 0);
        let destination = builder.create_block(OriginId::UNKNOWN).unwrap();
        let destination_parameter = builder
            .append_block_parameter(destination, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(destination, vec![argument])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(destination).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![destination_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();

        let (error, corrupted) = {
            let mut editor = FunctionEditor::new(&program, &sources, function, &mut body).unwrap();
            let mut prepared = match editor.prepare_jump_fusion(None).unwrap() {
                JumpFusionPreparation::Ready(prepared) => prepared,
                JumpFusionPreparation::StoppedAtFactTableLimit { .. } => unreachable!(),
            };
            let discovery = prepared.discover_maximal_regions(usize::MAX).unwrap();
            let TerminatorKind::Jump(target) = &mut prepared
                .body
                .block_mut(entry)
                .unwrap()
                .terminator
                .as_mut()
                .unwrap()
                .kind
            else {
                unreachable!()
            };
            target.arguments[0] = destination_parameter;
            let corrupted = format!("{:#?}", prepared.body);
            let error = prepared
                .fuse_jump_regions_batch(&discovery.regions)
                .unwrap_err();
            (error, corrupted)
        };
        assert!(
            matches!(error, EditError::InconsistentJumpFusion(message) if message.contains("cycle"))
        );
        assert_eq!(format!("{body:#?}"), corrupted);
    }

    #[test]
    fn fusion_rejects_corrupted_arity_and_consumed_tail_successor_atomically() {
        let (sources, program, function, body, middle, tail) = jump_fusion_chain_with_instruction();

        let mut arity_body = body.clone();
        let (error, corrupted) = {
            let mut editor =
                FunctionEditor::new(&program, &sources, function, &mut arity_body).unwrap();
            let mut prepared = match editor.prepare_jump_fusion(None).unwrap() {
                JumpFusionPreparation::Ready(prepared) => prepared,
                JumpFusionPreparation::StoppedAtFactTableLimit { .. } => unreachable!(),
            };
            let discovery = prepared.discover_maximal_regions(usize::MAX).unwrap();
            let instruction = prepared.body.block(middle).unwrap().instructions[0];
            let result = prepared.body.instruction(instruction).unwrap().results[0];
            let entry = prepared.body.entry;
            let TerminatorKind::Jump(target) = &mut prepared
                .body
                .block_mut(entry)
                .unwrap()
                .terminator
                .as_mut()
                .unwrap()
                .kind
            else {
                unreachable!()
            };
            target.arguments.push(result);
            let corrupted = format!("{:#?}", prepared.body);
            let error = prepared
                .fuse_jump_regions_batch(&discovery.regions)
                .unwrap_err();
            (error, corrupted)
        };
        assert!(
            matches!(error, EditError::InconsistentJumpFusion(message) if message.contains("arity"))
        );
        assert_eq!(format!("{arity_body:#?}"), corrupted);

        let mut tail_target_body = body;
        let (error, corrupted) = {
            let mut editor =
                FunctionEditor::new(&program, &sources, function, &mut tail_target_body).unwrap();
            let mut prepared = match editor.prepare_jump_fusion(None).unwrap() {
                JumpFusionPreparation::Ready(prepared) => prepared,
                JumpFusionPreparation::StoppedAtFactTableLimit { .. } => unreachable!(),
            };
            let discovery = prepared.discover_maximal_regions(usize::MAX).unwrap();
            prepared.body.block_mut(tail).unwrap().terminator = Some(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(middle, vec![])),
                OriginId::UNKNOWN,
            ));
            let corrupted = format!("{:#?}", prepared.body);
            let error = prepared
                .fuse_jump_regions_batch(&discovery.regions)
                .unwrap_err();
            (error, corrupted)
        };
        assert!(
            matches!(error, EditError::InconsistentJumpFusion(message) if message.contains("consumed"))
        );
        assert_eq!(format!("{tail_target_body:#?}"), corrupted);
    }
}
