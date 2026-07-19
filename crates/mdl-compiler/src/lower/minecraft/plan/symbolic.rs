//! Independent symbolic validation of frozen physical-home contents.
//!
//! This checker intentionally depends only on verified Core, the final lowering
//! plan, and verifier-owned reachability. It does not replay any planning phase.
//! The abstract domain follows regalloc2's checker: a sparse home maps to every
//! SSA name that is guaranteed to denote its contents, joins intersect facts,
//! and an absent block-entry state is the sole `Top`/unvisited state.

use std::collections::{HashMap, VecDeque};

use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, BlockTarget, CoreOp, CoreProgram, FunctionBody, FunctionId, InstData, InstId,
    TerminatorKind, ValueId,
};
use crate::source::OriginId;

use super::minimum_demand::{FunctionMinimumDemand, MinimumSemanticDemand};
use super::{
    CallResultDestination, EdgeTransfer, HomeId, HomeRole, InstructionPlan, LoweringPlan, MoveStep,
    ScalarResultPlacement,
};

const MAX_FINDINGS: usize = 64;

/// Proves that every semantic read performed by the frozen plan observes the
/// corresponding Core value and that every edge schedule implements its parallel
/// assignment.
pub(super) fn verify_symbolic_home_contents(
    core: &CoreProgram,
    plan: &LoweringPlan,
    minimum: &MinimumSemanticDemand,
) -> Result<(), Diagnostics> {
    let mut checker = SymbolicChecker::new(core, plan, minimum);
    if let Err(issue) = checker.run() {
        checker.record(issue);
    }
    checker.finish()
}

struct SymbolicChecker<'a> {
    core: &'a CoreProgram,
    plan: &'a LoweringPlan,
    minimum: &'a MinimumSemanticDemand,
    findings: Vec<Diagnostic>,
    findings_truncated: bool,
    #[cfg(test)]
    statistics: SymbolicStatistics,
    #[cfg(test)]
    capture_entry_states: bool,
    #[cfg(test)]
    captured_entry_states: Vec<(FunctionId, Vec<Option<SparseState>>)>,
}

impl<'a> SymbolicChecker<'a> {
    fn new(
        core: &'a CoreProgram,
        plan: &'a LoweringPlan,
        minimum: &'a MinimumSemanticDemand,
    ) -> Self {
        let mut findings = Vec::new();
        let _ = findings.try_reserve_exact(MAX_FINDINGS + 1);
        Self {
            core,
            plan,
            minimum,
            findings,
            findings_truncated: false,
            #[cfg(test)]
            statistics: SymbolicStatistics::default(),
            #[cfg(test)]
            capture_entry_states: false,
            #[cfg(test)]
            captured_entry_states: Vec::new(),
        }
    }

    fn run(&mut self) -> CheckResult<()> {
        if self.minimum.len() != self.core.len() {
            self.record(SymbolicIssue::shape(
                None,
                None,
                "verifier-owned reachability is not aligned with Core",
                OriginId::UNKNOWN,
            ));
            return Ok(());
        }
        for (function, declaration) in self.core.functions() {
            let Some(body) = declaration.body() else {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    None,
                    "Core function has no definition",
                    declaration.origin(),
                ));
                continue;
            };
            let Some(reachability) = self.minimum.function(function) else {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    None,
                    "verifier-owned reachability has no function entry",
                    declaration.origin(),
                ));
                continue;
            };
            if self.plan.functions.get(function).is_none() {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    None,
                    "lowering plan has no function layout",
                    declaration.origin(),
                ));
                continue;
            }
            self.verify_function(function, body, reachability)?;
        }
        Ok(())
    }

    fn verify_function(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        reachability: &FunctionMinimumDemand,
    ) -> CheckResult<()> {
        let mut entry_states = fallible_none_table(
            function,
            SymbolicTable::BlockEntryStates,
            body.block_counts().allocated,
        )?;
        let mut queued = fallible_bool_table(
            function,
            SymbolicTable::QueuedBlocks,
            body.block_counts().allocated,
        )?;
        let mut queue = VecDeque::new();
        queue
            .try_reserve_exact(reachability.reachable_blocks().len())
            .map_err(|_| capacity(function, SymbolicTable::BlockWorklist))?;

        let entry = body.entry();
        let entry_state = self.initial_entry_state(function, body, false)?;
        set_block_state(function, &mut entry_states, entry, entry_state)?;
        enqueue(function, &mut queue, &mut queued, entry)?;

        while let Some(block) = queue.pop_front() {
            set_queued(function, &mut queued, block, false)?;
            let Some(entry_state) = block_state(&entry_states, block) else {
                continue;
            };
            let mut state = entry_state.try_clone(function)?;
            self.transfer_block_definitions(function, body, block, &mut state)?;
            self.propagate_successors(
                function,
                body,
                block,
                &state,
                &mut entry_states,
                &mut queued,
                &mut queue,
            )?;
            #[cfg(test)]
            {
                self.statistics.processed_blocks =
                    self.statistics.processed_blocks.saturating_add(1);
                self.statistics.max_local_home_facts =
                    self.statistics.max_local_home_facts.max(state.fact_count());
            }
        }

        #[cfg(test)]
        {
            self.statistics.retained_entry_home_facts =
                self.statistics.retained_entry_home_facts.saturating_add(
                    entry_states
                        .iter()
                        .filter_map(Option::as_ref)
                        .map(SparseState::fact_count)
                        .sum::<usize>(),
                );
            if self.capture_entry_states {
                self.captured_entry_states
                    .push((function, entry_states.clone()));
            }
        }

        // Reads are checked only after convergence. Reporting during the first
        // pass would turn transient, pre-meet facts into order-dependent errors.
        for block in reachability.reachable_blocks().iter().copied() {
            let Some(entry_state) = block_state(&entry_states, block) else {
                let origin = body
                    .block(block)
                    .map_or(OriginId::UNKNOWN, crate::ir::core::BlockData::origin);
                self.record(SymbolicIssue::missing_state(function, block, origin));
                continue;
            };
            let mut state = entry_state.try_clone(function)?;
            self.validate_block(function, body, block, &mut state)?;
        }
        Ok(())
    }

    fn initial_entry_state(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        report: bool,
    ) -> CheckResult<SparseState> {
        let mut state = SparseState::new();
        let Some(layout) = self.plan.functions.get(function) else {
            return Ok(state);
        };
        let Some(entry_data) = body.block(body.entry()) else {
            return Ok(state);
        };
        if report && layout.abi.entry_block != body.entry() {
            self.record(SymbolicIssue::abi(
                function,
                "function ABI entry does not match Core entry",
                entry_data.origin(),
            ));
        }
        if report && layout.abi.parameters.len() != entry_data.parameters().len() {
            self.record(SymbolicIssue::abi(
                function,
                "function ABI parameter arity does not match Core entry",
                entry_data.origin(),
            ));
        }
        let mut definitions = fallible_vec(
            function,
            SymbolicTable::Definitions,
            layout
                .abi
                .parameters
                .len()
                .min(entry_data.parameters().len()),
        )?;
        for (parameter, home) in entry_data
            .parameters()
            .iter()
            .zip(layout.abi.parameters.iter().copied())
        {
            definitions.push(Definition {
                value: parameter.value(),
                home,
            });
        }
        if report {
            self.validate_distinct_definitions(
                function,
                &definitions,
                "entry parameters",
                entry_data.origin(),
            )?;
        }
        state.define_simultaneously(function, &definitions)?;
        Ok(state)
    }

    fn transfer_block_definitions(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        block: BlockId,
        state: &mut SparseState,
    ) -> CheckResult<()> {
        let Some(block_data) = body.block(block) else {
            return Ok(());
        };
        for instruction in block_data.instructions().iter().copied() {
            let Some(data) = body.instruction(instruction) else {
                continue;
            };
            self.transfer_instruction_definitions(function, instruction, data, state)?;
        }
        Ok(())
    }

    fn transfer_instruction_definitions(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &InstData,
        state: &mut SparseState,
    ) -> CheckResult<()> {
        let Some(plan) = self.plan.instruction_plan(function, instruction) else {
            return Ok(());
        };
        match plan {
            InstructionPlan::OmittedPure | InstructionPlan::External { .. } => Ok(()),
            InstructionPlan::Minecraft { results, .. }
            | InstructionPlan::Scalar { results, .. } => {
                self.finish_scalar_outputs(function, data, results, state, false)
            }
            InstructionPlan::Call {
                result_destinations,
                ..
            } => self.finish_call_outputs(function, data, result_destinations, state, false),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "successor propagation keeps all bounded dense tables explicit"
    )]
    fn propagate_successors(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        block: BlockId,
        state: &SparseState,
        entries: &mut [Option<SparseState>],
        queued: &mut [bool],
        queue: &mut VecDeque<BlockId>,
    ) -> CheckResult<()> {
        let Some(block_data) = body.block(block) else {
            return Ok(());
        };
        let Some(terminator) = block_data.terminator() else {
            return Ok(());
        };
        match terminator.kind() {
            TerminatorKind::Jump(target) => {
                let steps = match self.plan.edge_transfer(function, block) {
                    Some(EdgeTransfer::Jump { steps }) => steps.as_ref(),
                    _ => &[],
                };
                self.propagate_edge(function, body, state, target, steps, entries, queued, queue)
            }
            TerminatorKind::Branch {
                then_target,
                else_target,
                ..
            } => {
                let (then_steps, else_steps) = match self.plan.edge_transfer(function, block) {
                    Some(EdgeTransfer::Branch {
                        then_edge,
                        else_edge,
                    }) => (then_edge.steps(), else_edge.steps()),
                    _ => (&[][..], &[][..]),
                };
                self.propagate_edge(
                    function,
                    body,
                    state,
                    then_target,
                    then_steps,
                    entries,
                    queued,
                    queue,
                )?;
                self.propagate_edge(
                    function,
                    body,
                    state,
                    else_target,
                    else_steps,
                    entries,
                    queued,
                    queue,
                )
            }
            TerminatorKind::Return(_) | TerminatorKind::Unreachable => Ok(()),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one edge transfer correlates Core, physical moves, and fixed-point tables"
    )]
    fn propagate_edge(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        source_state: &SparseState,
        target: &BlockTarget,
        steps: &[MoveStep],
        entries: &mut [Option<SparseState>],
        queued: &mut [bool],
        queue: &mut VecDeque<BlockId>,
    ) -> CheckResult<()> {
        let mut edge_state = source_state.try_clone(function)?;
        let assignments = self.edge_assignments(function, body, target, false)?;
        self.apply_edge(
            function,
            &mut edge_state,
            &assignments,
            steps,
            false,
            OriginId::UNKNOWN,
        )?;
        if meet_block_state(function, entries, target.block(), &edge_state)? {
            enqueue(function, queue, queued, target.block())?;
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one deterministic replay keeps block-entry, instruction, and terminator diagnostics in execution order"
    )]
    fn validate_block(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        block: BlockId,
        state: &mut SparseState,
    ) -> CheckResult<()> {
        let Some(block_data) = body.block(block) else {
            self.record(SymbolicIssue::shape(
                Some(function),
                Some(block),
                "reachable block is absent",
                OriginId::UNKNOWN,
            ));
            return Ok(());
        };
        if block == body.entry() {
            // Rebuild solely to run entry-ABI correlation checks. The converged
            // state remains authoritative for all subsequent reads.
            let _ = self.initial_entry_state(function, body, true)?;
        }
        for parameter in block_data.parameters() {
            if let Some(home) = self.plan.value_home(function, parameter.value()) {
                self.require_value(
                    function,
                    None,
                    parameter.value(),
                    home,
                    state,
                    parameter.origin(),
                    "block parameter",
                );
            }
        }

        for instruction in block_data.instructions().iter().copied() {
            let Some(data) = body.instruction(instruction) else {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    Some(block),
                    format!("block references absent {instruction:?}"),
                    block_data.origin(),
                ));
                continue;
            };
            self.validate_instruction(function, instruction, data, state)?;
        }

        let Some(terminator) = block_data.terminator() else {
            self.record(SymbolicIssue::shape(
                Some(function),
                Some(block),
                "reachable block has no terminator",
                block_data.origin(),
            ));
            return Ok(());
        };
        match terminator.kind() {
            TerminatorKind::Jump(target) => {
                let steps = if let Some(EdgeTransfer::Jump { steps }) =
                    self.plan.edge_transfer(function, block)
                {
                    steps.as_ref()
                } else {
                    self.record(SymbolicIssue::transfer(
                        function,
                        block,
                        "jump has no matching physical transfer",
                        terminator.origin(),
                    ));
                    &[]
                };
                self.validate_edge(
                    function,
                    body,
                    block,
                    state,
                    target,
                    steps,
                    terminator.origin(),
                )?;
            }
            TerminatorKind::Branch {
                condition,
                then_target,
                else_target,
            } => {
                self.require_mapped_value(
                    function,
                    None,
                    *condition,
                    state,
                    terminator.origin(),
                    "branch condition",
                );
                let (then_steps, else_steps) = if let Some(EdgeTransfer::Branch {
                    then_edge,
                    else_edge,
                }) = self.plan.edge_transfer(function, block)
                {
                    (then_edge.steps(), else_edge.steps())
                } else {
                    self.record(SymbolicIssue::transfer(
                        function,
                        block,
                        "branch has no matching per-arm physical transfer",
                        terminator.origin(),
                    ));
                    (&[][..], &[][..])
                };
                self.validate_edge(
                    function,
                    body,
                    block,
                    state,
                    then_target,
                    then_steps,
                    terminator.origin(),
                )?;
                self.validate_edge(
                    function,
                    body,
                    block,
                    state,
                    else_target,
                    else_steps,
                    terminator.origin(),
                )?;
            }
            TerminatorKind::Return(values) => {
                self.validate_return(function, values, state, terminator.origin())?;
                if self.plan.edge_transfer(function, block).is_some() {
                    self.record(SymbolicIssue::transfer(
                        function,
                        block,
                        "return unexpectedly owns an edge transfer",
                        terminator.origin(),
                    ));
                }
            }
            TerminatorKind::Unreachable => {
                if self.plan.edge_transfer(function, block).is_some() {
                    self.record(SymbolicIssue::transfer(
                        function,
                        block,
                        "unreachable terminator unexpectedly owns an edge transfer",
                        terminator.origin(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_instruction(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &InstData,
        state: &mut SparseState,
    ) -> CheckResult<()> {
        let Some(plan) = self.plan.instruction_plan(function, instruction) else {
            self.record(SymbolicIssue::shape(
                Some(function),
                None,
                format!("reachable {instruction:?} has no instruction plan"),
                data.origin(),
            ));
            return Ok(());
        };
        match plan {
            InstructionPlan::OmittedPure => {
                if !data.op().is_trivially_discardable() {
                    self.record(SymbolicIssue::shape(
                        Some(function),
                        None,
                        format!("non-discardable {instruction:?} is omitted"),
                        data.origin(),
                    ));
                }
                Ok(())
            }
            InstructionPlan::External { .. } => {
                if !matches!(data.op(), CoreOp::External(_))
                    || !data.operands().is_empty()
                    || !data.results().is_empty()
                {
                    self.record(SymbolicIssue::shape(
                        Some(function),
                        None,
                        format!("{instruction:?} has an invalid external plan shape"),
                        data.origin(),
                    ));
                }
                Ok(())
            }
            InstructionPlan::Minecraft { results, .. } => {
                if !matches!(data.op(), CoreOp::External(_)) || !data.operands().is_empty() {
                    self.record(SymbolicIssue::shape(
                        Some(function),
                        None,
                        format!("{instruction:?} has an invalid Minecraft command plan shape"),
                        data.origin(),
                    ));
                }
                for result in results {
                    state.kill(result.home());
                }
                self.finish_scalar_outputs(function, data, results, state, true)
            }
            InstructionPlan::Scalar { operands, results } => {
                self.validate_scalar(function, instruction, data, operands, results, state)
            }
            InstructionPlan::Call {
                arguments,
                result_destinations,
            } => self.validate_call(
                function,
                instruction,
                data,
                arguments,
                result_destinations,
                state,
            ),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the checker intentionally keeps the closed scalar recipe timing table exhaustive and independent"
    )]
    fn validate_scalar(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &InstData,
        operands: &[HomeId],
        results: &[ScalarResultPlacement],
        state: &mut SparseState,
    ) -> CheckResult<()> {
        if matches!(data.op(), CoreOp::Call(_) | CoreOp::External(_)) {
            self.record(SymbolicIssue::shape(
                Some(function),
                None,
                format!("non-scalar {instruction:?} has a scalar plan"),
                data.origin(),
            ));
            return Ok(());
        }
        if operands.len() != data.operands().len() || results.len() != data.results().len() {
            self.record(SymbolicIssue::shape(
                Some(function),
                None,
                format!("scalar {instruction:?} has incorrect physical arity"),
                data.origin(),
            ));
        }
        self.validate_scalar_placements(function, instruction, data, results)?;

        match data.op() {
            CoreOp::BoolConstant(_) | CoreOp::I32Constant(_) | CoreOp::StringConstant(_) => {
                if let Some(output) = placement_home(results, 0) {
                    state.kill(output);
                }
            }
            CoreOp::I32AddWrapping | CoreOp::I32SubWrapping => {
                let Some((left_value, left_home)) = operand(data, operands, 0) else {
                    return self.finish_scalar_outputs(function, data, results, state, true);
                };
                let Some((right_value, right_home)) = operand(data, operands, 1) else {
                    return self.finish_scalar_outputs(function, data, results, state, true);
                };
                self.require_value(
                    function,
                    Some(instruction),
                    left_value,
                    left_home,
                    state,
                    data.origin(),
                    "wrapping arithmetic left operand",
                );
                if let Some(output) = placement_home(results, 0) {
                    state.copy_home(function, left_home, output)?;
                    if output == right_home
                        && !(left_value == right_value && left_home == right_home)
                    {
                        self.record(SymbolicIssue::timing(
                            function,
                            instruction,
                            "wrapping arithmetic result aliases the late-read right operand",
                            data.origin(),
                        ));
                    }
                }
                self.require_value(
                    function,
                    Some(instruction),
                    right_value,
                    right_home,
                    state,
                    data.origin(),
                    "wrapping arithmetic right operand",
                );
            }
            CoreOp::I32AddOverflowing => {
                let Some((left_value, left_home)) = operand(data, operands, 0) else {
                    return self.finish_scalar_outputs(function, data, results, state, true);
                };
                let Some((right_value, right_home)) = operand(data, operands, 1) else {
                    return self.finish_scalar_outputs(function, data, results, state, true);
                };
                self.require_value(
                    function,
                    Some(instruction),
                    left_value,
                    left_home,
                    state,
                    data.origin(),
                    "overflowing-add left operand",
                );
                if let Some(sum_home) = placement_home(results, 0) {
                    if sum_home == left_home || sum_home == right_home {
                        self.record(SymbolicIssue::timing(
                            function,
                            instruction,
                            "overflowing-add sum aliases an operand read after the sum write",
                            data.origin(),
                        ));
                    }
                    state.copy_home(function, left_home, sum_home)?;
                }
                self.require_value(
                    function,
                    Some(instruction),
                    right_value,
                    right_home,
                    state,
                    data.origin(),
                    "overflowing-add right operand",
                );
                if let Some(sum_home) = placement_home(results, 0) {
                    state.kill(sum_home);
                }
                if let Some(overflow_home) = placement_home(results, 1) {
                    if overflow_home == left_home || overflow_home == right_home {
                        self.record(SymbolicIssue::timing(
                            function,
                            instruction,
                            "overflow flag aliases an operand read after its early write",
                            data.origin(),
                        ));
                    }
                    state.kill(overflow_home);
                }
                for (value, home, description) in [
                    (left_value, left_home, "overflowing-add late left operand"),
                    (
                        right_value,
                        right_home,
                        "overflowing-add late right operand",
                    ),
                ] {
                    self.require_value(
                        function,
                        Some(instruction),
                        value,
                        home,
                        state,
                        data.origin(),
                        description,
                    );
                }
            }
            CoreOp::I32Compare(_) => {
                if let Some(output) = placement_home(results, 0) {
                    if operands.contains(&output) {
                        self.record(SymbolicIssue::timing(
                            function,
                            instruction,
                            "comparison result aliases an operand read after the result write",
                            data.origin(),
                        ));
                    }
                    state.kill(output);
                }
                for index in 0..2 {
                    if let Some((value, home)) = operand(data, operands, index) {
                        self.require_value(
                            function,
                            Some(instruction),
                            value,
                            home,
                            state,
                            data.origin(),
                            "comparison operand",
                        );
                    }
                }
            }
            CoreOp::BoolNot => {
                if let Some(output) = placement_home(results, 0) {
                    if operands.first() == Some(&output) {
                        self.record(SymbolicIssue::timing(
                            function,
                            instruction,
                            "Boolean-not result aliases its late-read operand",
                            data.origin(),
                        ));
                    }
                    state.kill(output);
                }
                if let Some((value, home)) = operand(data, operands, 0) {
                    self.require_value(
                        function,
                        Some(instruction),
                        value,
                        home,
                        state,
                        data.origin(),
                        "Boolean-not operand",
                    );
                }
            }
            CoreOp::ListI32Empty
            | CoreOp::ListI32Length
            | CoreOp::ListI32Push
            | CoreOp::ListI32LastOrZero
            | CoreOp::ListI32WithoutLast => {
                if let Some(output) = placement_home(results, 0) {
                    if operands.contains(&output) {
                        self.record(SymbolicIssue::timing(
                            function,
                            instruction,
                            "list result aliases an operand despite a no-reuse recipe contract",
                            data.origin(),
                        ));
                    }
                    state.kill(output);
                }
                for index in 0..data.operands().len() {
                    if let Some((value, home)) = operand(data, operands, index) {
                        self.require_value(
                            function,
                            Some(instruction),
                            value,
                            home,
                            state,
                            data.origin(),
                            "list operation operand",
                        );
                    }
                }
            }
            CoreOp::StringLength
            | CoreOp::StringEndsWithAscii(_)
            | CoreOp::StringWithoutLastUnit => {
                if let Some(output) = placement_home(results, 0) {
                    if operands.contains(&output) {
                        self.record(SymbolicIssue::timing(
                            function,
                            instruction,
                            "string result aliases an operand despite a no-reuse recipe contract",
                            data.origin(),
                        ));
                    }
                    state.kill(output);
                }
                if let Some((value, home)) = operand(data, operands, 0) {
                    self.require_value(
                        function,
                        Some(instruction),
                        value,
                        home,
                        state,
                        data.origin(),
                        "string operation operand",
                    );
                }
            }
            CoreOp::Call(_) | CoreOp::External(_) => {
                unreachable!("non-scalar operation rejected before exhaustive scalar match")
            }
        }
        self.finish_scalar_outputs(function, data, results, state, true)
    }

    fn validate_scalar_placements(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &InstData,
        results: &[ScalarResultPlacement],
    ) -> CheckResult<()> {
        let mut seen_indices = fallible_vec(function, SymbolicTable::Definitions, results.len())?;
        let mut seen_homes = fallible_vec(function, SymbolicTable::Definitions, results.len())?;
        for placement in results.iter().copied() {
            let index = placement.result_index();
            if index >= data.results().len() || seen_indices.contains(&index) {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    None,
                    format!("scalar {instruction:?} has a duplicate or invalid result index"),
                    data.origin(),
                ));
            } else {
                seen_indices.push(index);
            }
            let home = placement.home();
            if seen_homes.contains(&home) {
                self.record(SymbolicIssue::definition_alias(
                    function,
                    format!("scalar {instruction:?} writes two simultaneous outputs to {home:?}"),
                    data.origin(),
                ));
            } else {
                seen_homes.push(home);
            }
            if let ScalarResultPlacement::Semantic { value, .. } = placement {
                let expected = data.results().get(index).copied();
                if expected != Some(value) || self.plan.value_home(function, value) != Some(home) {
                    self.record(SymbolicIssue::shape(
                        Some(function),
                        None,
                        format!("scalar {instruction:?} has an uncorrelated semantic result"),
                        data.origin(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn finish_scalar_outputs(
        &mut self,
        function: FunctionId,
        data: &InstData,
        placements: &[ScalarResultPlacement],
        state: &mut SparseState,
        report: bool,
    ) -> CheckResult<()> {
        let mut definitions = fallible_vec(function, SymbolicTable::Definitions, placements.len())?;
        for placement in placements.iter().copied() {
            state.kill(placement.home());
            let ScalarResultPlacement::Semantic {
                result_index,
                value,
                home,
            } = placement
            else {
                continue;
            };
            if data.results().get(result_index).copied() == Some(value) {
                definitions.push(Definition { value, home });
            }
        }
        if report {
            self.validate_distinct_definitions(
                function,
                &definitions,
                "scalar results",
                data.origin(),
            )?;
        }
        state.define_simultaneously(function, &definitions)
    }

    fn validate_call(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &InstData,
        arguments: &[HomeId],
        destinations: &[Option<CallResultDestination>],
        state: &mut SparseState,
    ) -> CheckResult<()> {
        let CoreOp::Call(callee) = data.op() else {
            self.record(SymbolicIssue::shape(
                Some(function),
                None,
                format!("non-call {instruction:?} has a call plan"),
                data.origin(),
            ));
            return Ok(());
        };
        if arguments.len() != data.operands().len() || destinations.len() != data.results().len() {
            self.record(SymbolicIssue::shape(
                Some(function),
                None,
                format!("call {instruction:?} has incorrect physical arity"),
                data.origin(),
            ));
        }

        // All caller arguments are read before any caller result definition.
        for (value, home) in data
            .operands()
            .iter()
            .copied()
            .zip(arguments.iter().copied())
        {
            self.require_value(
                function,
                Some(instruction),
                value,
                home,
                state,
                data.origin(),
                "call argument",
            );
        }
        self.validate_callee_abi(*callee, data.origin())?;
        self.validate_call_destinations(function, instruction, data, destinations)?;
        self.finish_call_outputs(function, data, destinations, state, true)
    }

    fn validate_callee_abi(&mut self, callee: FunctionId, origin: OriginId) -> CheckResult<()> {
        let Some(declaration) = self.core.function(callee) else {
            self.record(SymbolicIssue::abi(
                callee,
                "call references an absent callee",
                origin,
            ));
            return Ok(());
        };
        let Some(layout) = self.plan.functions.get(callee) else {
            self.record(SymbolicIssue::abi(
                callee,
                "call references a callee without a physical layout",
                origin,
            ));
            return Ok(());
        };
        if layout.abi.parameters.len() != declaration.parameters().len()
            || layout.abi.results.len() != declaration.results().len()
        {
            self.record(SymbolicIssue::abi(
                callee,
                "callee ABI is incomplete",
                origin,
            ));
        }
        self.validate_distinct_homes(
            callee,
            &layout.abi.parameters,
            "callee parameter ABI",
            origin,
        )?;
        self.validate_distinct_homes(callee, &layout.abi.results, "callee result ABI", origin)
    }

    fn validate_call_destinations(
        &mut self,
        function: FunctionId,
        instruction: InstId,
        data: &InstData,
        destinations: &[Option<CallResultDestination>],
    ) -> CheckResult<()> {
        let mut seen_homes =
            fallible_vec(function, SymbolicTable::Definitions, destinations.len())?;
        for (index, destination) in destinations.iter().copied().enumerate() {
            let Some(destination) = destination else {
                continue;
            };
            if destination.result_index() != index
                || data.results().get(index).copied() != Some(destination.value())
                || self.plan.value_home(function, destination.value()) != Some(destination.home())
            {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    None,
                    format!("call {instruction:?} has an uncorrelated result destination"),
                    data.origin(),
                ));
            }
            if seen_homes.contains(&destination.home()) {
                self.record(SymbolicIssue::definition_alias(
                    function,
                    format!(
                        "call {instruction:?} defines distinct simultaneous results in one home"
                    ),
                    data.origin(),
                ));
            } else {
                seen_homes.push(destination.home());
            }
        }
        Ok(())
    }

    fn finish_call_outputs(
        &mut self,
        function: FunctionId,
        data: &InstData,
        destinations: &[Option<CallResultDestination>],
        state: &mut SparseState,
        report: bool,
    ) -> CheckResult<()> {
        let mut definitions =
            fallible_vec(function, SymbolicTable::Definitions, destinations.len())?;
        for destination in destinations.iter().copied().flatten() {
            if data.results().get(destination.result_index()).copied() == Some(destination.value())
            {
                definitions.push(Definition {
                    value: destination.value(),
                    home: destination.home(),
                });
            }
        }
        if report {
            self.validate_distinct_definitions(
                function,
                &definitions,
                "call results",
                data.origin(),
            )?;
        }
        state.define_simultaneously(function, &definitions)
    }

    fn validate_return(
        &mut self,
        function: FunctionId,
        values: &[ValueId],
        state: &mut SparseState,
        origin: OriginId,
    ) -> CheckResult<()> {
        let Some(layout) = self.plan.functions.get(function) else {
            return Ok(());
        };
        if layout.abi.results.len() != values.len() {
            self.record(SymbolicIssue::abi(
                function,
                "return arity does not match result ABI",
                origin,
            ));
        }
        self.validate_distinct_homes(function, &layout.abi.results, "function result ABI", origin)?;
        for (value, destination) in values
            .iter()
            .copied()
            .zip(layout.abi.results.iter().copied())
        {
            let Some(source) = self.plan.value_home(function, value) else {
                self.record(SymbolicIssue::missing_home(
                    function,
                    None,
                    value,
                    "return value",
                    origin,
                ));
                continue;
            };
            self.require_value(function, None, value, source, state, origin, "return value");
            state.copy_home(function, source, destination)?;
        }
        for (value, result_home) in values
            .iter()
            .copied()
            .zip(layout.abi.results.iter().copied())
        {
            self.require_contents(
                function,
                None,
                value,
                result_home,
                state,
                origin,
                "completed return slot",
            );
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "edge validation keeps semantic source, physical schedule, and provenance explicit"
    )]
    fn validate_edge(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        source: BlockId,
        source_state: &SparseState,
        target: &BlockTarget,
        steps: &[MoveStep],
        origin: OriginId,
    ) -> CheckResult<()> {
        let assignments = self.edge_assignments(function, body, target, true)?;
        for assignment in &assignments {
            self.require_value(
                function,
                None,
                assignment.source_value,
                assignment.source_home,
                source_state,
                origin,
                "edge argument",
            );
        }
        self.validate_edge_destination_aliases(function, source, &assignments, origin)?;
        self.prove_move_schedule(function, source, &assignments, steps, origin)?;
        let mut edge_state = source_state.try_clone(function)?;
        self.apply_edge(function, &mut edge_state, &assignments, steps, true, origin)?;
        for assignment in &assignments {
            self.require_value(
                function,
                None,
                assignment.destination_value,
                assignment.destination_home,
                &edge_state,
                origin,
                "edge-defined block parameter",
            );
        }
        Ok(())
    }

    fn edge_assignments(
        &mut self,
        function: FunctionId,
        body: &FunctionBody,
        target: &BlockTarget,
        report: bool,
    ) -> CheckResult<Vec<EdgeAssignment>> {
        let Some(destination) = body.block(target.block()) else {
            return Ok(Vec::new());
        };
        let mut assignments = fallible_vec(
            function,
            SymbolicTable::EdgeAssignments,
            destination.parameters().len().min(target.arguments().len()),
        )?;
        for (parameter, source_value) in destination
            .parameters()
            .iter()
            .zip(target.arguments().iter().copied())
        {
            let Some(destination_home) = self.plan.value_home(function, parameter.value()) else {
                continue;
            };
            let Some(source_home) = self.plan.value_home(function, source_value) else {
                if report {
                    self.record(SymbolicIssue::missing_home(
                        function,
                        None,
                        source_value,
                        "retained edge argument",
                        parameter.origin(),
                    ));
                }
                continue;
            };
            assignments.push(EdgeAssignment {
                destination_value: parameter.value(),
                destination_home,
                source_value,
                source_home,
            });
        }
        Ok(assignments)
    }

    fn apply_edge(
        &mut self,
        function: FunctionId,
        state: &mut SparseState,
        assignments: &[EdgeAssignment],
        steps: &[MoveStep],
        report: bool,
        origin: OriginId,
    ) -> CheckResult<()> {
        let Some(layout) = self.plan.functions.get(function) else {
            return Ok(());
        };
        for temporary in layout.edge_temporaries.iter().copied() {
            state.kill(temporary);
        }
        for step in steps.iter().copied() {
            if report && !self.valid_move_home(function, step.source()) {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    None,
                    format!("move reads invalid or foreign {:?}", step.source()),
                    origin,
                ));
            }
            if report && !self.valid_move_home(function, step.destination()) {
                self.record(SymbolicIssue::shape(
                    Some(function),
                    None,
                    format!("move writes invalid or foreign {:?}", step.destination()),
                    origin,
                ));
            }
            state.copy_home(function, step.source(), step.destination())?;
        }
        let substitution = Substitution::new(function, assignments)?;
        state.substitute(function, &substitution)?;
        for temporary in layout.edge_temporaries.iter().copied() {
            state.kill(temporary);
        }
        Ok(())
    }

    fn prove_move_schedule(
        &mut self,
        function: FunctionId,
        source: BlockId,
        assignments: &[EdgeAssignment],
        steps: &[MoveStep],
        origin: OriginId,
    ) -> CheckResult<()> {
        let mut tokens = TokenState::new();
        for step in steps.iter().copied() {
            let token = tokens.read(self, function, step.source());
            if token.is_none() {
                self.record(SymbolicIssue::scratch(
                    function,
                    format!("edge from {source:?} reads uninitialized scratch"),
                    origin,
                ));
            }
            tokens.write(function, step.destination(), token)?;
        }
        for assignment in assignments {
            let actual = tokens.read(self, function, assignment.destination_home);
            if actual != Some(assignment.source_home) {
                self.record(SymbolicIssue::transfer(
                    function,
                    source,
                    format!(
                        "move sequence leaves {:?} with {actual:?}, expected original {:?}",
                        assignment.destination_home, assignment.source_home
                    ),
                    origin,
                ));
            }
        }
        Ok(())
    }

    fn validate_edge_destination_aliases(
        &mut self,
        function: FunctionId,
        source: BlockId,
        assignments: &[EdgeAssignment],
        origin: OriginId,
    ) -> CheckResult<()> {
        let mut definitions =
            fallible_vec(function, SymbolicTable::Definitions, assignments.len())?;
        definitions.extend(assignments.iter().map(|assignment| Definition {
            value: assignment.destination_value,
            home: assignment.destination_home,
        }));
        definitions.sort_unstable_by_key(|definition| (definition.home, definition.value));
        for pair in definitions.windows(2) {
            if pair[0].home == pair[1].home && pair[0].value != pair[1].value {
                self.record(SymbolicIssue::definition_alias(
                    function,
                    format!(
                        "edge from {source:?} defines distinct block parameters in {:?}",
                        pair[0].home
                    ),
                    origin,
                ));
            }
        }
        Ok(())
    }

    fn validate_distinct_definitions(
        &mut self,
        function: FunctionId,
        definitions: &[Definition],
        context: &'static str,
        origin: OriginId,
    ) -> CheckResult<()> {
        let mut sorted = fallible_vec(function, SymbolicTable::Definitions, definitions.len())?;
        sorted.extend_from_slice(definitions);
        sorted.sort_unstable_by_key(|definition| (definition.home, definition.value));
        for pair in sorted.windows(2) {
            if pair[0].home == pair[1].home && pair[0].value != pair[1].value {
                self.record(SymbolicIssue::definition_alias(
                    function,
                    format!("distinct simultaneous {context} share {:?}", pair[0].home),
                    origin,
                ));
            }
        }
        Ok(())
    }

    fn validate_distinct_homes(
        &mut self,
        function: FunctionId,
        homes: &[HomeId],
        context: &'static str,
        origin: OriginId,
    ) -> CheckResult<()> {
        let mut sorted = fallible_vec(function, SymbolicTable::Definitions, homes.len())?;
        sorted.extend_from_slice(homes);
        sorted.sort_unstable();
        for pair in sorted.windows(2) {
            if pair[0] == pair[1] {
                self.record(SymbolicIssue::abi(
                    function,
                    format!("{context} aliases {:?}", pair[0]),
                    origin,
                ));
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a failed semantic read needs its complete stable diagnostic context"
    )]
    fn require_value(
        &mut self,
        function: FunctionId,
        instruction: Option<InstId>,
        value: ValueId,
        home: HomeId,
        state: &SparseState,
        origin: OriginId,
        context: &'static str,
    ) {
        if self.plan.value_home(function, value) != Some(home) {
            self.record(SymbolicIssue::shape(
                Some(function),
                None,
                format!("{context} for {value:?} does not use its assigned home"),
                origin,
            ));
        }
        self.require_contents(function, instruction, value, home, state, origin, context);
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "physical ABI slots also need complete stable read context without semantic-home correlation"
    )]
    fn require_contents(
        &mut self,
        function: FunctionId,
        instruction: Option<InstId>,
        value: ValueId,
        home: HomeId,
        state: &SparseState,
        origin: OriginId,
        context: &'static str,
    ) {
        if !state.contains(home, value) {
            self.record(SymbolicIssue::read(
                function,
                instruction,
                value,
                home,
                context,
                origin,
            ));
        }
    }

    fn require_mapped_value(
        &mut self,
        function: FunctionId,
        instruction: Option<InstId>,
        value: ValueId,
        state: &SparseState,
        origin: OriginId,
        context: &'static str,
    ) {
        let Some(home) = self.plan.value_home(function, value) else {
            self.record(SymbolicIssue::missing_home(
                function,
                instruction,
                value,
                context,
                origin,
            ));
            return;
        };
        self.require_value(function, instruction, value, home, state, origin, context);
    }

    fn valid_move_home(&self, function: FunctionId, home: HomeId) -> bool {
        matches!(
            self.plan.homes.get(home).map(|home| home.role),
            Some(
                HomeRole::Value { function: owner, .. }
                    | HomeRole::Register { function: owner, .. }
                    | HomeRole::EdgeTemporary { function: owner }
                    | HomeRole::TypedEdgeTemporary { function: owner, .. }
            ) if owner == function
        )
    }

    fn is_edge_temporary(&self, function: FunctionId, home: HomeId) -> bool {
        matches!(
            self.plan.homes.get(home).map(|home| home.role),
            Some(
                HomeRole::EdgeTemporary { function: owner }
                    | HomeRole::TypedEdgeTemporary { function: owner, .. }
            ) if owner == function
        )
    }

    fn record(&mut self, issue: SymbolicIssue) {
        if self.findings.len() < MAX_FINDINGS {
            self.findings.push(issue.into_diagnostic());
        } else {
            self.findings_truncated = true;
        }
    }

    fn finish(mut self) -> Result<(), Diagnostics> {
        if self.findings_truncated {
            self.findings.push(Diagnostic::new(
                "lower.plan.symbolic-findings-truncated",
                format!("symbolic plan verification stopped retaining detail after {MAX_FINDINGS} findings"),
                OriginId::UNKNOWN,
            ));
        }
        match Diagnostics::from_findings(self.findings) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Definition {
    value: ValueId,
    home: HomeId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EdgeAssignment {
    destination_value: ValueId,
    destination_home: HomeId,
    source_value: ValueId,
    source_home: HomeId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SparseState {
    /// Sorted by `HomeId`; every symbol vector is sorted and nonempty.
    facts: Vec<HomeFact>,
    /// The exact inverse incidence. Keeping both sparse
    /// indexes makes a static definition in a 20,000-instruction block O(log n)
    /// rather than rescanning every preceding home.
    reverse: HashMap<ValueId, Vec<HomeId>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HomeFact {
    home: HomeId,
    symbols: Vec<ValueId>,
}

impl SparseState {
    fn new() -> Self {
        Self {
            facts: Vec::new(),
            reverse: HashMap::new(),
        }
    }

    #[cfg(test)]
    fn fact_count(&self) -> usize {
        self.facts.len()
    }

    fn try_clone(&self, function: FunctionId) -> CheckResult<Self> {
        let mut facts = fallible_vec(function, SymbolicTable::StateHomeFacts, self.facts.len())?;
        for fact in &self.facts {
            let mut symbols =
                fallible_vec(function, SymbolicTable::StateSymbols, fact.symbols.len())?;
            symbols.extend_from_slice(&fact.symbols);
            facts.push(HomeFact {
                home: fact.home,
                symbols,
            });
        }
        Self::from_facts(function, facts)
    }

    fn contains(&self, home: HomeId, value: ValueId) -> bool {
        self.fact_index(home)
            .ok()
            .and_then(|index| self.facts.get(index))
            .is_some_and(|fact| fact.symbols.binary_search(&value).is_ok())
    }

    fn symbols(&self, home: HomeId) -> &[ValueId] {
        self.fact_index(home)
            .ok()
            .and_then(|index| self.facts.get(index))
            .map_or(&[], |fact| fact.symbols.as_slice())
    }

    fn kill(&mut self, home: HomeId) {
        if let Ok(index) = self.fact_index(home) {
            let removed = self.facts.remove(index);
            for value in removed.symbols {
                self.remove_reverse_home(value, home);
            }
        }
    }

    fn copy_home(
        &mut self,
        function: FunctionId,
        source: HomeId,
        destination: HomeId,
    ) -> CheckResult<()> {
        if source == destination {
            return Ok(());
        }
        let source_symbols = self.symbols(source);
        let mut copied = fallible_vec(function, SymbolicTable::StateSymbols, source_symbols.len())?;
        copied.extend_from_slice(source_symbols);
        self.set_symbols(function, destination, copied)
    }

    fn define_simultaneously(
        &mut self,
        function: FunctionId,
        definitions: &[Definition],
    ) -> CheckResult<()> {
        let mut defined_values =
            fallible_vec(function, SymbolicTable::Definitions, definitions.len())?;
        defined_values.extend(definitions.iter().map(|definition| definition.value));
        defined_values.sort_unstable();
        defined_values.dedup();
        self.remove_symbols(&defined_values);

        for definition in definitions {
            let mut singleton = fallible_vec(function, SymbolicTable::StateSymbols, 1)?;
            singleton.push(definition.value);
            self.set_symbols(function, definition.home, singleton)?;
        }
        Ok(())
    }

    fn substitute(&mut self, function: FunctionId, substitution: &Substitution) -> CheckResult<()> {
        let before = self.try_clone(function)?;
        self.remove_symbols(&substitution.destinations);
        for (source, destination) in &substitution.by_source {
            let homes = before.homes(*source);
            for home in homes {
                self.add_symbol(function, *home, *destination)?;
            }
        }
        Ok(())
    }

    fn meet_with(&mut self, function: FunctionId, other: &Self) -> CheckResult<bool> {
        let mut intersection = fallible_vec(
            function,
            SymbolicTable::StateHomeFacts,
            self.facts.len().min(other.facts.len()),
        )?;
        let (mut left, mut right) = (0, 0);
        while left < self.facts.len() && right < other.facts.len() {
            let left_fact = &self.facts[left];
            let right_fact = &other.facts[right];
            match left_fact.home.cmp(&right_fact.home) {
                std::cmp::Ordering::Less => left += 1,
                std::cmp::Ordering::Greater => right += 1,
                std::cmp::Ordering::Equal => {
                    let symbols =
                        intersect_symbols(function, &left_fact.symbols, &right_fact.symbols)?;
                    if !symbols.is_empty() {
                        intersection.push(HomeFact {
                            home: left_fact.home,
                            symbols,
                        });
                    }
                    left += 1;
                    right += 1;
                }
            }
        }
        let changed = intersection != self.facts;
        if changed {
            *self = Self::from_facts(function, intersection)?;
        }
        Ok(changed)
    }

    fn remove_symbols(&mut self, removed: &[ValueId]) {
        for value in removed.iter().copied() {
            self.remove_symbol(value);
        }
    }

    fn add_symbol(
        &mut self,
        function: FunctionId,
        home: HomeId,
        addition: ValueId,
    ) -> CheckResult<()> {
        match self.fact_index(home) {
            Ok(index) => match self.facts[index].symbols.binary_search(&addition) {
                Ok(_) => return Ok(()),
                Err(symbol_index) => {
                    self.facts[index]
                        .symbols
                        .try_reserve(1)
                        .map_err(|_| capacity(function, SymbolicTable::StateSymbols))?;
                    self.facts[index].symbols.insert(symbol_index, addition);
                }
            },
            Err(index) => {
                let mut symbols = fallible_vec(function, SymbolicTable::StateSymbols, 1)?;
                symbols.push(addition);
                self.facts
                    .try_reserve(1)
                    .map_err(|_| capacity(function, SymbolicTable::StateHomeFacts))?;
                self.facts.insert(index, HomeFact { home, symbols });
            }
        }
        self.add_reverse_home(function, addition, home)?;
        Ok(())
    }

    fn set_symbols(
        &mut self,
        function: FunctionId,
        home: HomeId,
        symbols: Vec<ValueId>,
    ) -> CheckResult<()> {
        self.kill(home);
        for symbol in symbols {
            self.add_symbol(function, home, symbol)?;
        }
        Ok(())
    }

    fn homes(&self, value: ValueId) -> &[HomeId] {
        self.reverse.get(&value).map_or(&[], Vec::as_slice)
    }

    fn remove_symbol(&mut self, value: ValueId) {
        let Some(homes) = self.reverse.remove(&value) else {
            return;
        };
        for home in homes {
            let Ok(fact_index) = self.fact_index(home) else {
                continue;
            };
            let Ok(symbol_index) = self.facts[fact_index].symbols.binary_search(&value) else {
                continue;
            };
            self.facts[fact_index].symbols.remove(symbol_index);
            if self.facts[fact_index].symbols.is_empty() {
                self.facts.remove(fact_index);
            }
        }
    }

    fn add_reverse_home(
        &mut self,
        function: FunctionId,
        value: ValueId,
        home: HomeId,
    ) -> CheckResult<()> {
        if let Some(homes) = self.reverse.get_mut(&value) {
            match homes.binary_search(&home) {
                Ok(_) => {}
                Err(home_index) => {
                    homes
                        .try_reserve(1)
                        .map_err(|_| capacity(function, SymbolicTable::StateValueHomes))?;
                    homes.insert(home_index, home);
                }
            }
            return Ok(());
        }
        let mut homes = fallible_vec(function, SymbolicTable::StateValueHomes, 1)?;
        homes.push(home);
        self.reverse
            .try_reserve(1)
            .map_err(|_| capacity(function, SymbolicTable::StateValueFacts))?;
        self.reverse.insert(value, homes);
        Ok(())
    }

    fn remove_reverse_home(&mut self, value: ValueId, home: HomeId) {
        let Some(homes) = self.reverse.get_mut(&value) else {
            return;
        };
        if let Ok(home_index) = homes.binary_search(&home) {
            homes.remove(home_index);
        }
        if homes.is_empty() {
            self.reverse.remove(&value);
        }
    }

    fn from_facts(function: FunctionId, facts: Vec<HomeFact>) -> CheckResult<Self> {
        let incidence_count = facts
            .iter()
            .try_fold(0_usize, |total, fact| total.checked_add(fact.symbols.len()));
        let Some(incidence_count) = incidence_count else {
            return Err(capacity(function, SymbolicTable::StateValueHomes));
        };
        let mut reverse = HashMap::<ValueId, Vec<HomeId>>::new();
        reverse
            .try_reserve(incidence_count)
            .map_err(|_| capacity(function, SymbolicTable::StateValueFacts))?;
        for fact in &facts {
            for value in fact.symbols.iter().copied() {
                if let Some(homes) = reverse.get_mut(&value) {
                    homes
                        .try_reserve(1)
                        .map_err(|_| capacity(function, SymbolicTable::StateValueHomes))?;
                    homes.push(fact.home);
                } else {
                    let mut homes = fallible_vec(function, SymbolicTable::StateValueHomes, 1)?;
                    homes.push(fact.home);
                    reverse.insert(value, homes);
                }
            }
        }
        Ok(Self { facts, reverse })
    }

    fn fact_index(&self, home: HomeId) -> Result<usize, usize> {
        self.facts.binary_search_by_key(&home, |fact| fact.home)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Substitution {
    /// `(source, destination)` pairs sorted by both IDs.
    by_source: Vec<(ValueId, ValueId)>,
    /// Sorted unique overwritten destination symbols.
    destinations: Vec<ValueId>,
}

impl Substitution {
    fn new(function: FunctionId, assignments: &[EdgeAssignment]) -> CheckResult<Self> {
        let mut by_source =
            fallible_vec(function, SymbolicTable::Substitutions, assignments.len())?;
        let mut destinations =
            fallible_vec(function, SymbolicTable::Substitutions, assignments.len())?;
        for assignment in assignments {
            by_source.push((assignment.source_value, assignment.destination_value));
            destinations.push(assignment.destination_value);
        }
        by_source.sort_unstable();
        destinations.sort_unstable();
        destinations.dedup();
        Ok(Self {
            by_source,
            destinations,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TokenState {
    /// Sorted explicit overrides. Absence means the home's original token, except
    /// for edge temporaries, whose absence means uninitialized.
    overrides: Vec<(HomeId, Option<HomeId>)>,
}

impl TokenState {
    const fn new() -> Self {
        Self {
            overrides: Vec::new(),
        }
    }

    fn read(
        &self,
        checker: &SymbolicChecker<'_>,
        function: FunctionId,
        home: HomeId,
    ) -> Option<HomeId> {
        match self
            .overrides
            .binary_search_by_key(&home, |(home, _)| *home)
        {
            Ok(index) => self.overrides[index].1,
            Err(_) if checker.is_edge_temporary(function, home) => None,
            Err(_) => Some(home),
        }
    }

    fn write(
        &mut self,
        function: FunctionId,
        home: HomeId,
        token: Option<HomeId>,
    ) -> CheckResult<()> {
        match self
            .overrides
            .binary_search_by_key(&home, |(home, _)| *home)
        {
            Ok(index) => self.overrides[index].1 = token,
            Err(index) => {
                self.overrides
                    .try_reserve(1)
                    .map_err(|_| capacity(function, SymbolicTable::MoveTokens))?;
                self.overrides.insert(index, (home, token));
            }
        }
        Ok(())
    }
}

fn placement_home(placements: &[ScalarResultPlacement], result_index: usize) -> Option<HomeId> {
    placements
        .iter()
        .copied()
        .find(|placement| placement.result_index() == result_index)
        .map(ScalarResultPlacement::home)
}

fn operand(data: &InstData, homes: &[HomeId], index: usize) -> Option<(ValueId, HomeId)> {
    Some((*data.operands().get(index)?, *homes.get(index)?))
}

fn intersect_symbols(
    function: FunctionId,
    left: &[ValueId],
    right: &[ValueId],
) -> CheckResult<Vec<ValueId>> {
    let mut result = fallible_vec(
        function,
        SymbolicTable::StateSymbols,
        left.len().min(right.len()),
    )?;
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                result.push(left[left_index]);
                left_index += 1;
                right_index += 1;
            }
        }
    }
    Ok(result)
}

fn set_block_state(
    function: FunctionId,
    entries: &mut [Option<SparseState>],
    block: BlockId,
    state: SparseState,
) -> CheckResult<()> {
    let slot = block_slot_mut(entries, block)
        .ok_or_else(|| SymbolicIssue::invalid_block(function, block))?;
    *slot = Some(state);
    Ok(())
}

fn meet_block_state(
    function: FunctionId,
    entries: &mut [Option<SparseState>],
    block: BlockId,
    incoming: &SparseState,
) -> CheckResult<bool> {
    let slot = block_slot_mut(entries, block)
        .ok_or_else(|| SymbolicIssue::invalid_block(function, block))?;
    if let Some(current) = slot {
        current.meet_with(function, incoming)
    } else {
        *slot = Some(incoming.try_clone(function)?);
        Ok(true)
    }
}

fn block_state(entries: &[Option<SparseState>], block: BlockId) -> Option<&SparseState> {
    usize::try_from(block.index())
        .ok()
        .and_then(|index| entries.get(index))
        .and_then(Option::as_ref)
}

fn block_slot_mut<T>(entries: &mut [T], block: BlockId) -> Option<&mut T> {
    usize::try_from(block.index())
        .ok()
        .and_then(|index| entries.get_mut(index))
}

fn enqueue(
    function: FunctionId,
    queue: &mut VecDeque<BlockId>,
    queued: &mut [bool],
    block: BlockId,
) -> CheckResult<()> {
    let slot = block_slot_mut(queued, block)
        .ok_or_else(|| SymbolicIssue::invalid_block(function, block))?;
    if !std::mem::replace(slot, true) {
        queue.push_back(block);
    }
    Ok(())
}

fn set_queued(
    function: FunctionId,
    queued: &mut [bool],
    block: BlockId,
    value: bool,
) -> CheckResult<()> {
    *block_slot_mut(queued, block)
        .ok_or_else(|| SymbolicIssue::invalid_block(function, block))? = value;
    Ok(())
}

fn fallible_none_table<T>(
    function: FunctionId,
    table: SymbolicTable,
    length: usize,
) -> CheckResult<Vec<Option<T>>> {
    let mut values = fallible_vec(function, table, length)?;
    values.resize_with(length, || None);
    Ok(values)
}

fn fallible_bool_table(
    function: FunctionId,
    table: SymbolicTable,
    length: usize,
) -> CheckResult<Vec<bool>> {
    let mut values = fallible_vec(function, table, length)?;
    values.resize(length, false);
    Ok(values)
}

fn fallible_vec<T>(
    function: FunctionId,
    table: SymbolicTable,
    requested_capacity: usize,
) -> CheckResult<Vec<T>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(requested_capacity)
        .map_err(|_| capacity(function, table))?;
    Ok(values)
}

type CheckResult<T> = Result<T, SymbolicIssue>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SymbolicTable {
    BlockEntryStates,
    QueuedBlocks,
    BlockWorklist,
    StateHomeFacts,
    StateSymbols,
    StateValueFacts,
    StateValueHomes,
    Definitions,
    EdgeAssignments,
    Substitutions,
    MoveTokens,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SymbolicIssue {
    code: &'static str,
    message: String,
    origin: OriginId,
}

impl SymbolicIssue {
    fn capacity(function: FunctionId, table: SymbolicTable) -> Self {
        Self {
            code: "lower.plan.symbolic-capacity",
            message: format!("symbolic verification could not allocate {table:?} for {function:?}"),
            origin: OriginId::UNKNOWN,
        }
    }

    fn invalid_block(function: FunctionId, block: BlockId) -> Self {
        Self {
            code: "lower.plan.symbolic-shape",
            message: format!("symbolic verification encountered invalid {block:?} in {function:?}"),
            origin: OriginId::UNKNOWN,
        }
    }

    fn shape(
        function: Option<FunctionId>,
        block: Option<BlockId>,
        detail: impl Into<String>,
        origin: OriginId,
    ) -> Self {
        Self {
            code: "lower.plan.symbolic-shape",
            message: format!(
                "symbolic plan shape failure at {function:?} {block:?}: {}",
                detail.into()
            ),
            origin,
        }
    }

    fn missing_state(function: FunctionId, block: BlockId, origin: OriginId) -> Self {
        Self {
            code: "lower.plan.symbolic-missing-state",
            message: format!("reachable {function:?} {block:?} has no converged entry state"),
            origin,
        }
    }

    fn read(
        function: FunctionId,
        instruction: Option<InstId>,
        value: ValueId,
        home: HomeId,
        context: &'static str,
        origin: OriginId,
    ) -> Self {
        Self {
            code: "lower.plan.symbolic-read",
            message: format!(
                "{context} in {function:?} {instruction:?} reads {home:?} without {value:?}"
            ),
            origin,
        }
    }

    fn missing_home(
        function: FunctionId,
        instruction: Option<InstId>,
        value: ValueId,
        context: &'static str,
        origin: OriginId,
    ) -> Self {
        Self {
            code: "lower.plan.symbolic-missing-home",
            message: format!("{context} in {function:?} {instruction:?} has no home for {value:?}"),
            origin,
        }
    }

    fn timing(
        function: FunctionId,
        instruction: InstId,
        detail: impl Into<String>,
        origin: OriginId,
    ) -> Self {
        Self {
            code: "lower.plan.symbolic-recipe-timing",
            message: format!(
                "scalar timing failure in {function:?} {instruction:?}: {}",
                detail.into()
            ),
            origin,
        }
    }

    fn definition_alias(function: FunctionId, detail: impl Into<String>, origin: OriginId) -> Self {
        Self {
            code: "lower.plan.symbolic-definition-alias",
            message: format!(
                "simultaneous definition failure in {function:?}: {}",
                detail.into()
            ),
            origin,
        }
    }

    fn transfer(
        function: FunctionId,
        block: BlockId,
        detail: impl Into<String>,
        origin: OriginId,
    ) -> Self {
        Self {
            code: "lower.plan.symbolic-transfer",
            message: format!(
                "edge transfer failure in {function:?} {block:?}: {}",
                detail.into()
            ),
            origin,
        }
    }

    fn scratch(function: FunctionId, detail: impl Into<String>, origin: OriginId) -> Self {
        Self {
            code: "lower.plan.symbolic-scratch",
            message: format!("scratch failure in {function:?}: {}", detail.into()),
            origin,
        }
    }

    fn abi(function: FunctionId, detail: impl Into<String>, origin: OriginId) -> Self {
        Self {
            code: "lower.plan.symbolic-abi",
            message: format!("ABI failure in {function:?}: {}", detail.into()),
            origin,
        }
    }

    fn into_diagnostic(self) -> Diagnostic {
        Diagnostic::new(self.code, self.message, self.origin)
    }
}

fn capacity(function: FunctionId, table: SymbolicTable) -> SymbolicIssue {
    SymbolicIssue::capacity(function, table)
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SymbolicStatistics {
    processed_blocks: usize,
    retained_entry_home_facts: usize,
    max_local_home_facts: usize,
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use super::{
        Definition, EdgeAssignment, SparseState, Substitution,
        verify_symbolic_home_contents as verify_symbolic_home_contents_with_minimum,
    };
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBody, FunctionBuilder, FunctionId,
        I32Predicate, InstData, InstId, Terminator, TerminatorKind, ValueDef, ValueId,
    };
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::assignment::HomeAssignment;
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::lower::minecraft::edge_transfer::EdgeTransferPlan;
    use crate::lower::minecraft::plan::{
        CallResultDestination, EdgeTransfer, Home, HomeId, HomeRole, InstructionPlan, LoweringPlan,
        MoveStep, ScalarResultPlacement,
    };
    use crate::lower::minecraft::resources::ResourceInventory;
    use crate::lower::minecraft::{GeneratedNames, LoweringOptions, MinecraftOptimizationLevel};
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    trait GeneratedFixtureResult<T> {
        #[track_caller]
        fn expect_generated(self, context: &str) -> T;
    }

    impl<T, E: Debug> GeneratedFixtureResult<T> for Result<T, E> {
        #[track_caller]
        fn expect_generated(self, context: &str) -> T {
            self.unwrap_or_else(|error| {
                panic!("{context}: generated fixture construction failed: {error:?}")
            })
        }
    }

    #[track_caller]
    fn run_generated_case<T>(context: &str, case: impl FnOnce() -> T) -> T {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(case)).unwrap_or_else(|payload| {
            if let Some(message) = payload.downcast_ref::<String>() {
                panic!("{context}: generated symbolic case failed: {message}");
            }
            if let Some(message) = payload.downcast_ref::<&str>() {
                panic!("{context}: generated symbolic case failed: {message}");
            }
            panic!("{context}: generated symbolic case failed with a non-string panic payload");
        })
    }

    fn verify_symbolic_home_contents(
        core: &CoreProgram,
        plan: &LoweringPlan,
    ) -> Result<(), crate::diagnostic::Diagnostics> {
        let minimum = super::MinimumSemanticDemand::new(core)
            .expect("symbolic verifier fixtures must have valid Core control flow");
        verify_symbolic_home_contents_with_minimum(core, plan, &minimum)
    }

    #[test]
    fn sparse_meet_matches_a_dense_intersection_oracle() {
        let function = FunctionId::from_index(0);
        // Two symbols in three homes gives 64 states and 4,096 complete joins.
        for left_mask in 0_u8..64 {
            for right_mask in 0_u8..64 {
                let mut sparse = state_from_mask(function, left_mask, 3, 2);
                let other = state_from_mask(function, right_mask, 3, 2);
                let changed = sparse.meet_with(function, &other).unwrap();
                assert_eq!(changed, left_mask & right_mask != left_mask);
                assert_state_mask(&sparse, left_mask & right_mask, 3, 2);
                assert_inverse_is_exact(&sparse);
            }
        }
    }

    #[test]
    fn simultaneous_substitution_matches_a_dense_oracle() {
        let function = FunctionId::from_index(0);
        let assignments = [
            EdgeAssignment {
                destination_value: ValueId::from_index(1),
                destination_home: HomeId::from_index(0),
                source_value: ValueId::from_index(0),
                source_home: HomeId::from_index(0),
            },
            EdgeAssignment {
                destination_value: ValueId::from_index(2),
                destination_home: HomeId::from_index(1),
                source_value: ValueId::from_index(1),
                source_home: HomeId::from_index(1),
            },
        ];
        let substitution = Substitution::new(function, &assignments).unwrap();
        for mask in 0_u16..512 {
            let mut state = state_from_mask(function, mask, 3, 3);
            let before = dense_from_mask(mask, 3, 3);
            state.substitute(function, &substitution).unwrap();

            let mut expected = before.clone();
            for row in &mut expected {
                row[1] = false;
                row[2] = false;
            }
            for home in 0..3 {
                if before[home][0] {
                    expected[home][1] = true;
                }
                if before[home][1] {
                    expected[home][2] = true;
                }
            }
            assert_dense_state(&state, &expected);
            assert_inverse_is_exact(&state);
        }
    }

    #[test]
    fn generated_small_cfgs_match_an_independent_dense_symbolic_oracle() {
        const GENERATOR_VERSION: u32 = 1;

        for seed in 0_u64..12 {
            let shape = if seed & 1 == 0 { "diamond" } else { "loop" };
            let fixture_context = format!(
                "generator_version={GENERATOR_VERSION} seed={seed} shape={shape} inputs=symbolic:[bool,i32,i32]"
            );
            let (core, function, corruptible_edge) = run_generated_case(&fixture_context, || {
                generated_dense_oracle_cfg(GENERATOR_VERSION, seed, &fixture_context)
            });
            for level in [
                MinecraftOptimizationLevel::None,
                MinecraftOptimizationLevel::Baseline,
            ] {
                let context = format!("{fixture_context} level={level:?} case=valid");
                run_generated_case(&context, || {
                    let plan = freeze_generated(&core, level, &context);
                    assert_dense_oracle_agreement(&core, &plan, true, &context);
                });
            }

            let context = format!("{fixture_context} level=None case=erased-edge");
            run_generated_case(&context, || {
                let mut corrupted =
                    freeze_generated(&core, MinecraftOptimizationLevel::None, &context);
                erase_edge_schedule(&mut corrupted, function, corruptible_edge, &context);
                assert_dense_oracle_agreement(&core, &corrupted, false, &context);
            });
        }
    }

    #[test]
    fn twenty_thousand_straight_line_definitions_remain_sparse_and_linear_sized() {
        let function = FunctionId::from_index(0);
        let mut state = SparseState::new();
        for index in 0..20_000 {
            state
                .define_simultaneously(
                    function,
                    &[Definition {
                        value: ValueId::from_index(index),
                        home: HomeId::from_index(index),
                    }],
                )
                .unwrap();
        }
        assert_eq!(state.facts.len(), 20_000);
        assert_eq!(state.reverse.len(), 20_000);
        assert_eq!(
            state
                .facts
                .iter()
                .map(|fact| fact.symbols.len())
                .sum::<usize>(),
            20_000
        );
        assert_inverse_is_exact(&state);
    }

    #[test]
    fn wrapping_add_accepts_left_reuse_but_rejects_distinct_right_reuse() {
        let (core, function, result) = wrapping_add_program();
        let mut left_reuse = freeze(&core, MinecraftOptimizationLevel::None);
        alias_scalar_result(&mut left_reuse, function, result, 0, 0);
        verify_symbolic_home_contents(&core, &left_reuse).unwrap();

        let mut right_reuse = freeze(&core, MinecraftOptimizationLevel::None);
        alias_scalar_result(&mut right_reuse, function, result, 0, 1);
        let diagnostics = verify_symbolic_home_contents(&core, &right_reuse).unwrap_err();
        assert!(diagnostics.contains_code("lower.plan.symbolic-recipe-timing"));
        assert!(
            diagnostics.contains_code("lower.plan.symbolic-read"),
            "{diagnostics:#?}"
        );
    }

    #[test]
    fn full_verifier_rejects_recipe_legal_alias_when_fabricated_liveness_kills_a_live_left() {
        let (core, function, result, dead) = wrapping_add_with_live_left_program();
        let mut plan = freeze(&core, MinecraftOptimizationLevel::Baseline);
        let old_result_home = plan.value_home(function, result).unwrap();
        alias_scalar_result(&mut plan, function, result, 0, 0);
        let dead_instruction =
            instruction_of(core.function(function).unwrap().body().unwrap(), dead);
        let layout = plan.functions.get_mut(function).unwrap();
        layout.value_homes[dead.index() as usize] = Some(old_result_home);
        layout.instruction_plans[dead_instruction.index() as usize] =
            Some(InstructionPlan::Scalar {
                operands: Box::new([]),
                results: Box::new([ScalarResultPlacement::Semantic {
                    result_index: 0,
                    value: dead,
                    home: old_result_home,
                }]),
            });

        let diagnostics = super::super::verify::verify_plan(&core, &plan).unwrap_err();

        assert!(
            diagnostics.contains_code("lower.plan.symbolic-read"),
            "{diagnostics:#?}"
        );
        assert!(!diagnostics.contains_code("lower.plan.symbolic-recipe-timing"));
    }

    #[test]
    fn overflowing_add_rejects_sum_alias_before_late_sign_reads() {
        let (core, function, sum) = overflowing_add_program();
        let mut plan = freeze(&core, MinecraftOptimizationLevel::None);
        alias_scalar_result(&mut plan, function, sum, 0, 0);

        let diagnostics = verify_symbolic_home_contents(&core, &plan).unwrap_err();

        assert!(diagnostics.contains_code("lower.plan.symbolic-recipe-timing"));
        assert!(diagnostics.contains_code("lower.plan.symbolic-read"));
    }

    #[test]
    fn compare_and_bool_not_reject_early_output_aliases() {
        let (compare_core, compare_function, compared) = compare_program();
        let mut compare_plan = freeze(&compare_core, MinecraftOptimizationLevel::None);
        alias_scalar_result(&mut compare_plan, compare_function, compared, 0, 0);
        let diagnostics = verify_symbolic_home_contents(&compare_core, &compare_plan).unwrap_err();
        assert!(diagnostics.contains_code("lower.plan.symbolic-read"));

        let (not_core, not_function, negated) = bool_not_program();
        let mut not_plan = freeze(&not_core, MinecraftOptimizationLevel::None);
        alias_scalar_result(&mut not_plan, not_function, negated, 0, 0);
        let diagnostics = verify_symbolic_home_contents(&not_core, &not_plan).unwrap_err();
        assert!(diagnostics.contains_code("lower.plan.symbolic-read"));
    }

    #[test]
    fn partial_overflow_recipe_temporary_is_not_a_semantic_definition() {
        let (core, _, _) = overflowing_add_program();
        let plan = freeze(&core, MinecraftOptimizationLevel::Baseline);

        verify_symbolic_home_contents(&core, &plan).unwrap();
    }

    #[test]
    fn calls_read_arguments_first_and_reject_duplicate_result_definitions() {
        let (core, caller, call, second_result) = call_program();
        let mut plan = freeze(&core, MinecraftOptimizationLevel::None);
        let layout = plan.functions.get_mut(caller).unwrap();
        let InstructionPlan::Call {
            result_destinations,
            ..
        } = layout.instruction_plans[call.index() as usize]
            .as_mut()
            .unwrap()
        else {
            panic!("fixture call must have a call plan");
        };
        let first = result_destinations[0].unwrap();
        result_destinations[1] = Some(CallResultDestination {
            result_index: 1,
            value: second_result,
            home: first.home(),
        });
        layout.value_homes[second_result.index() as usize] = Some(first.home());

        let diagnostics = verify_symbolic_home_contents(&core, &plan).unwrap_err();

        assert!(diagnostics.contains_code("lower.plan.symbolic-definition-alias"));
    }

    #[test]
    fn corrupted_swap_schedule_and_uninitialized_scratch_are_rejected() {
        let (core, function, entry) = swap_edge_program();
        let valid = freeze(&core, MinecraftOptimizationLevel::None);
        verify_symbolic_home_contents(&core, &valid).unwrap();

        let mut missing_moves = valid.clone();
        let layout = missing_moves.functions.get_mut(function).unwrap();
        layout.edge_transfers[entry.index() as usize] = Some(EdgeTransfer::Jump {
            steps: Box::new([]),
        });
        let diagnostics = verify_symbolic_home_contents(&core, &missing_moves).unwrap_err();
        assert!(diagnostics.contains_code("lower.plan.symbolic-transfer"));

        let mut uninitialized = valid;
        let temporary = uninitialized
            .homes
            .push(Home {
                holder: GeneratedNames::edge_temporary_holder(function),
                role: HomeRole::EdgeTemporary { function },
            })
            .unwrap();
        let layout = uninitialized.functions.get_mut(function).unwrap();
        layout.parallel_copy_temp = Some(temporary);
        layout.edge_temporaries = vec![temporary].into_boxed_slice();
        let EdgeTransfer::Jump { steps } = layout.edge_transfers[entry.index() as usize]
            .as_mut()
            .unwrap()
        else {
            panic!("fixture edge must be a jump");
        };
        let destination = steps[0].destination();
        *steps = vec![MoveStep::new(destination, temporary)].into_boxed_slice();
        let diagnostics = verify_symbolic_home_contents(&core, &uninitialized).unwrap_err();
        assert!(diagnostics.contains_code("lower.plan.symbolic-scratch"));
    }

    fn freeze(core: &CoreProgram, level: MinecraftOptimizationLevel) -> LoweringPlan {
        let options = options(level);
        let inventory = SemanticInventory::new(core).unwrap();
        let assignment = match level {
            MinecraftOptimizationLevel::None => HomeAssignment::for_none(core, &inventory).unwrap(),
            MinecraftOptimizationLevel::Baseline => {
                let demand = RuntimeDemand::for_level(
                    core,
                    &inventory,
                    level,
                    RuntimeDemandLimits::derived(),
                )
                .unwrap();
                HomeAssignment::for_baseline_derived_liveness(core, &inventory, &demand).unwrap()
            }
        };
        let transfers = match level {
            MinecraftOptimizationLevel::None => {
                EdgeTransferPlan::for_none(core, &inventory, &assignment).unwrap()
            }
            MinecraftOptimizationLevel::Baseline => {
                EdgeTransferPlan::for_baseline(core, &inventory, &assignment).unwrap()
            }
        };
        let resources = ResourceInventory::new(core, &inventory, &transfers, &options).unwrap();
        LoweringPlan::from_parts(core, &options, assignment, transfers, resources).unwrap()
    }

    fn freeze_generated(
        core: &CoreProgram,
        level: MinecraftOptimizationLevel,
        context: &str,
    ) -> LoweringPlan {
        let options = generated_options(level, context);
        let inventory = SemanticInventory::new(core).expect_generated(context);
        let assignment = match level {
            MinecraftOptimizationLevel::None => {
                HomeAssignment::for_none(core, &inventory).expect_generated(context)
            }
            MinecraftOptimizationLevel::Baseline => {
                let demand = RuntimeDemand::for_level(
                    core,
                    &inventory,
                    level,
                    RuntimeDemandLimits::derived(),
                )
                .expect_generated(context);
                HomeAssignment::for_baseline_derived_liveness(core, &inventory, &demand)
                    .expect_generated(context)
            }
        };
        let transfers = match level {
            MinecraftOptimizationLevel::None => {
                EdgeTransferPlan::for_none(core, &inventory, &assignment).expect_generated(context)
            }
            MinecraftOptimizationLevel::Baseline => {
                EdgeTransferPlan::for_baseline(core, &inventory, &assignment)
                    .expect_generated(context)
            }
        };
        let resources = ResourceInventory::new(core, &inventory, &transfers, &options)
            .expect_generated(context);
        LoweringPlan::from_parts(core, &options, assignment, transfers, resources)
            .expect_generated(context)
    }

    fn generated_options(level: MinecraftOptimizationLevel, context: &str) -> LoweringOptions {
        let namespace = PackNamespace::new("mdl").expect_generated(context);
        let objective = ObjectiveName::new("mdl.reg").expect_generated(context);
        LoweringOptions::new(JavaEditionTarget::V26_2, namespace, objective)
            .expect_generated(context)
            .with_optimization_level(level)
    }

    fn options(level: MinecraftOptimizationLevel) -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
        .with_optimization_level(level)
    }

    fn wrapping_add_program() -> (CoreProgram, FunctionId, ValueId) {
        scalar_binary_program(CoreType::I32, |builder, left, right| {
            builder.i32_add_wrapping(left, right, OriginId::UNKNOWN)
        })
    }

    fn wrapping_add_with_live_left_program() -> (CoreProgram, FunctionId, ValueId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("wrapping-live-left"),
                vec![],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = builder.i32_constant(4, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let result = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        let dead = builder.i32_constant(9, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result, left]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, result, dead)
    }

    fn overflowing_add_program() -> (CoreProgram, FunctionId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("overflow"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let parameters = entry_parameters(&builder);
        let (sum, _overflowed) = builder
            .i32_add_overflowing(parameters[0], parameters[1], OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, sum)
    }

    fn compare_program() -> (CoreProgram, FunctionId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("compare"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let parameters = entry_parameters(&builder);
        let result = builder
            .i32_compare(
                I32Predicate::SignedLt,
                parameters[0],
                parameters[1],
                OriginId::UNKNOWN,
            )
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, result)
    }

    fn bool_not_program() -> (CoreProgram, FunctionId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("not"),
                vec![CoreType::Bool],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let parameter = entry_parameters(&builder)[0];
        let result = builder.bool_not(parameter, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, result)
    }

    fn scalar_binary_program(
        result_type: CoreType,
        build: impl FnOnce(
            &mut FunctionBuilder<'_>,
            ValueId,
            ValueId,
        ) -> Result<ValueId, crate::ir::core::BuildError>,
    ) -> (CoreProgram, FunctionId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("binary"),
                vec![CoreType::I32, CoreType::I32],
                vec![result_type],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let parameters = entry_parameters(&builder);
        let result = build(&mut builder, parameters[0], parameters[1]).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, result)
    }

    fn call_program() -> (CoreProgram, FunctionId, InstId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let producer_function = core
            .declare_function(
                Some("callee"),
                vec![],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let consumer_function = core
            .declare_function(
                Some("caller"),
                vec![],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut producer_body = FunctionBuilder::new(&core, &sources, producer_function).unwrap();
        let first = producer_body.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let second = producer_body.i32_constant(2, OriginId::UNKNOWN).unwrap();
        producer_body
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![first, second]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(producer_function, producer_body.finish().unwrap())
            .unwrap();

        let mut consumer_body = FunctionBuilder::new(&core, &sources, consumer_function).unwrap();
        let results = consumer_body
            .call(producer_function, vec![], OriginId::UNKNOWN)
            .unwrap();
        let call = instruction_of(consumer_body.body(), results[0]);
        consumer_body
            .terminate(Terminator::new(
                TerminatorKind::Return(results.clone()),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let second_result = results[1];
        core.define_function(consumer_function, consumer_body.finish().unwrap())
            .unwrap();
        (core, consumer_function, call, second_result)
    }

    fn swap_edge_program() -> (CoreProgram, FunctionId, crate::ir::core::BlockId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("swap"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let parameters = entry_parameters(&builder);
        let destination = builder.create_block(OriginId::UNKNOWN).unwrap();
        let left = builder
            .append_block_parameter(destination, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let right = builder
            .append_block_parameter(destination, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    destination,
                    vec![parameters[1], parameters[0]],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(destination).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![left, right]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, entry)
    }

    fn generated_dense_oracle_cfg(
        generator_version: u32,
        seed: u64,
        context: &str,
    ) -> (CoreProgram, FunctionId, BlockId) {
        assert_eq!(
            generator_version, 1,
            "{context}: unsupported generator version"
        );
        if seed & 1 == 0 {
            generated_dense_diamond(seed, context)
        } else {
            generated_dense_loop(seed, context)
        }
    }

    fn generated_dense_diamond(seed: u64, context: &str) -> (CoreProgram, FunctionId, BlockId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("generated-dense-diamond"),
                vec![CoreType::Bool, CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let mut builder = FunctionBuilder::new(&core, &sources, function).expect_generated(context);
        let entry = builder.entry_block();
        let parameters = generated_entry_parameters(&builder, context);
        let left_block = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let left_first = builder
            .append_block_parameter(left_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let left_second = builder
            .append_block_parameter(left_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let right_block = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let right_first = builder
            .append_block_parameter(right_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let right_second = builder
            .append_block_parameter(right_block, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let join = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let ordered = [parameters[1], parameters[2]];
        let swapped = [parameters[2], parameters[1]];
        let then_arguments = if seed & 2 == 0 { ordered } else { swapped };
        let else_arguments = if seed & 4 == 0 { swapped } else { ordered };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: parameters[0],
                    then_target: BlockTarget::new(left_block, then_arguments.to_vec()),
                    else_target: BlockTarget::new(right_block, else_arguments.to_vec()),
                },
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder
            .switch_to_block(left_block)
            .expect_generated(context);
        let left_result = builder
            .i32_add_wrapping(left_first, left_second, OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![left_result])),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder
            .switch_to_block(right_block)
            .expect_generated(context);
        let right_result = builder
            .i32_add_wrapping(right_first, right_second, OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![right_result])),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder.switch_to_block(join).expect_generated(context);
        let offset = builder
            .i32_constant(
                i32::try_from(seed).expect_generated(context),
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let result = builder
            .i32_add_wrapping(joined, offset, OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);
        let body = builder.finish().expect_generated(context);
        core.define_function(function, body)
            .expect_generated(context);
        (core, function, entry)
    }

    fn generated_dense_loop(seed: u64, context: &str) -> (CoreProgram, FunctionId, BlockId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("generated-dense-loop"),
                vec![CoreType::Bool, CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let mut builder = FunctionBuilder::new(&core, &sources, function).expect_generated(context);
        let entry = builder.entry_block();
        let parameters = generated_entry_parameters(&builder, context);
        let header = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let first = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let second = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let exit = builder
            .create_block(OriginId::UNKNOWN)
            .expect_generated(context);
        let completed = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .expect_generated(context);
        let initial = if seed & 2 == 0 {
            vec![parameters[1], parameters[2]]
        } else {
            vec![parameters[2], parameters[1]]
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, initial)),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder.switch_to_block(header).expect_generated(context);
        let next = builder
            .i32_add_wrapping(first, second, OriginId::UNKNOWN)
            .expect_generated(context);
        let backedge = if seed & 4 == 0 {
            vec![next, second]
        } else {
            vec![second, next]
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: parameters[0],
                    then_target: BlockTarget::new(header, backedge),
                    else_target: BlockTarget::new(exit, vec![next]),
                },
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);

        builder.switch_to_block(exit).expect_generated(context);
        let offset = builder
            .i32_constant(
                i32::try_from(seed >> 1).expect_generated(context),
                OriginId::UNKNOWN,
            )
            .expect_generated(context);
        let result = builder
            .i32_add_wrapping(completed, offset, OriginId::UNKNOWN)
            .expect_generated(context);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .expect_generated(context);
        let body = builder.finish().expect_generated(context);
        core.define_function(function, body)
            .expect_generated(context);
        (core, function, entry)
    }

    fn generated_entry_parameters(builder: &FunctionBuilder<'_>, context: &str) -> Vec<ValueId> {
        let parameters = builder
            .body()
            .block(builder.entry_block())
            .unwrap_or_else(|| panic!("{context}: generated entry block is missing"))
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect::<Vec<_>>();
        assert_eq!(
            parameters.len(),
            3,
            "{context}: generated fixture must have three entry parameters"
        );
        parameters
    }

    fn entry_parameters(builder: &FunctionBuilder<'_>) -> Vec<ValueId> {
        builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect()
    }

    fn instruction_of(body: &crate::ir::core::FunctionBody, value: ValueId) -> InstId {
        let ValueDef::InstResult { instruction, .. } = body.value(value).unwrap().definition()
        else {
            panic!("fixture value must be an instruction result");
        };
        instruction
    }

    fn alias_scalar_result(
        plan: &mut LoweringPlan,
        function: FunctionId,
        value: ValueId,
        result_index: usize,
        operand_index: usize,
    ) {
        let instruction = {
            let layout = plan.functions.get(function).unwrap();
            layout
                .instruction_plans
                .iter()
                .enumerate()
                .find_map(|(index, plan)| {
                    let Some(InstructionPlan::Scalar { results, .. }) = plan else {
                        return None;
                    };
                    results
                        .iter()
                        .any(|result| result.value() == Some(value))
                        .then(|| u32::try_from(index).ok().map(InstId::from_index))
                        .flatten()
                })
                .unwrap()
        };
        let layout = plan.functions.get_mut(function).unwrap();
        let InstructionPlan::Scalar { operands, results } = layout.instruction_plans
            [instruction.index() as usize]
            .as_mut()
            .unwrap()
        else {
            panic!("fixture instruction must be scalar");
        };
        let home = operands[operand_index];
        results[result_index] = ScalarResultPlacement::Semantic {
            result_index,
            value,
            home,
        };
        layout.value_homes[value.index() as usize] = Some(home);
    }

    fn assert_dense_oracle_agreement(
        core: &CoreProgram,
        plan: &LoweringPlan,
        expected_acceptance: bool,
        context: &str,
    ) {
        let minimum = super::MinimumSemanticDemand::new(core).unwrap_or_else(|error| {
            panic!("{context}: minimum-demand construction failed: {error:?}")
        });
        let mut sparse = super::SymbolicChecker::new(core, plan, &minimum);
        sparse.capture_entry_states = true;
        if let Err(issue) = sparse.run() {
            sparse.record(issue);
        }
        let sparse_accepts = sparse.findings.is_empty();
        let sparse_entries = sparse.captured_entry_states;

        assert_eq!(sparse_accepts, expected_acceptance, "{context}: sparse");
        for (function, declaration) in core.functions() {
            let body = declaration.body().unwrap_or_else(|| {
                panic!("{context}: generated function {function:?} has no definition")
            });
            let dense = dense_symbolic_oracle(body, plan, function);
            assert_eq!(dense.accepts, expected_acceptance, "{context}: dense");
            let (_, sparse_function_entries) = sparse_entries
                .iter()
                .find(|(candidate, _)| *candidate == function)
                .unwrap_or_else(|| {
                    panic!("{context}: sparse oracle omitted function {function:?}")
                });
            assert_eq!(
                sparse_function_entries.len(),
                dense.entries.len(),
                "{context}: entry table length"
            );
            for (block_index, (sparse_entry, dense_entry)) in sparse_function_entries
                .iter()
                .zip(&dense.entries)
                .enumerate()
            {
                assert_eq!(
                    sparse_entry.is_some(),
                    dense_entry.is_some(),
                    "{context}: reachability at block {block_index}"
                );
                let (Some(sparse_entry), Some(dense_entry)) = (sparse_entry, dense_entry) else {
                    continue;
                };
                for home_index in 0..plan.homes.len() {
                    let home =
                        HomeId::from_index(u32::try_from(home_index).expect_generated(context));
                    for value_index in 0..body.value_counts().allocated {
                        let value = ValueId::from_index(
                            u32::try_from(value_index).expect_generated(context),
                        );
                        assert_eq!(
                            sparse_entry.contains(home, value),
                            dense_entry.contains(home, value),
                            "{context}: block {block_index}, home {home_index}, value {value_index}"
                        );
                    }
                }
            }
        }
    }

    fn erase_edge_schedule(
        plan: &mut LoweringPlan,
        function: FunctionId,
        source: BlockId,
        context: &str,
    ) {
        let layout = plan
            .functions
            .get_mut(function)
            .unwrap_or_else(|| panic!("{context}: generated plan omitted function {function:?}"));
        let edge = layout
            .edge_transfers
            .get_mut(source.index() as usize)
            .unwrap_or_else(|| panic!("{context}: generated plan omitted source {source:?}"))
            .as_mut()
            .unwrap_or_else(|| panic!("{context}: generated source {source:?} has no edge plan"));
        match edge {
            EdgeTransfer::Jump { steps } => {
                assert!(
                    !steps.is_empty(),
                    "{context}: generated jump edge has no schedule to erase"
                );
                *steps = Box::new([]);
            }
            EdgeTransfer::Branch { then_edge, .. } => {
                assert!(
                    !then_edge.steps.is_empty(),
                    "{context}: generated branch edge has no schedule to erase"
                );
                then_edge.steps = Box::new([]);
            }
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct DenseState {
        facts: Vec<Vec<bool>>,
    }

    impl DenseState {
        fn new(home_count: usize, value_count: usize) -> Self {
            Self {
                facts: vec![vec![false; value_count]; home_count],
            }
        }

        fn contains(&self, home: HomeId, value: ValueId) -> bool {
            self.facts
                .get(home.index() as usize)
                .and_then(|row| row.get(value.index() as usize))
                .copied()
                .unwrap_or(false)
        }

        fn kill(&mut self, home: HomeId) -> bool {
            let Some(row) = self.facts.get_mut(home.index() as usize) else {
                return false;
            };
            row.fill(false);
            true
        }

        fn copy(&mut self, source: HomeId, destination: HomeId) -> bool {
            let Some(symbols) = self.facts.get(source.index() as usize).cloned() else {
                return false;
            };
            let Some(destination) = self.facts.get_mut(destination.index() as usize) else {
                return false;
            };
            *destination = symbols;
            true
        }

        fn define(&mut self, definitions: &[(ValueId, HomeId)]) -> bool {
            let mut valid = true;
            for (value, _) in definitions {
                let value_index = value.index() as usize;
                for row in &mut self.facts {
                    if let Some(symbol) = row.get_mut(value_index) {
                        *symbol = false;
                    } else {
                        valid = false;
                    }
                }
            }
            for (value, home) in definitions {
                valid &= self.kill(*home);
                let Some(symbol) = self
                    .facts
                    .get_mut(home.index() as usize)
                    .and_then(|row| row.get_mut(value.index() as usize))
                else {
                    valid = false;
                    continue;
                };
                *symbol = true;
            }
            valid
        }

        fn substitute(&mut self, assignments: &[DenseEdgeAssignment]) -> bool {
            let before = self.clone();
            let mut valid = true;
            for assignment in assignments {
                let value_index = assignment.destination_value.index() as usize;
                for row in &mut self.facts {
                    if let Some(symbol) = row.get_mut(value_index) {
                        *symbol = false;
                    } else {
                        valid = false;
                    }
                }
            }
            for assignment in assignments {
                for home_index in 0..self.facts.len() {
                    let home = HomeId::from_index(u32::try_from(home_index).unwrap());
                    if !before.contains(home, assignment.source_value) {
                        continue;
                    }
                    let Some(symbol) = self.facts[home_index]
                        .get_mut(assignment.destination_value.index() as usize)
                    else {
                        valid = false;
                        continue;
                    };
                    *symbol = true;
                }
            }
            valid
        }

        fn meet(&mut self, other: &Self) -> bool {
            let mut changed = false;
            for (row, other_row) in self.facts.iter_mut().zip(&other.facts) {
                for (fact, other_fact) in row.iter_mut().zip(other_row) {
                    let intersection = *fact && *other_fact;
                    changed |= intersection != *fact;
                    *fact = intersection;
                }
            }
            changed
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct DenseEdgeAssignment {
        destination_value: ValueId,
        destination_home: HomeId,
        source_value: ValueId,
    }

    struct DenseObservation {
        accepts: bool,
        entries: Vec<Option<DenseState>>,
    }

    fn dense_symbolic_oracle(
        body: &FunctionBody,
        plan: &LoweringPlan,
        function: FunctionId,
    ) -> DenseObservation {
        let mut accepts = true;
        let mut entries = vec![None; body.block_counts().allocated];
        let mut initial = DenseState::new(plan.homes.len(), body.value_counts().allocated);
        let layout = plan.functions.get(function).unwrap();
        let entry = body.block(body.entry()).unwrap();
        let definitions: Vec<_> = entry
            .parameters()
            .iter()
            .zip(layout.abi.parameters.iter().copied())
            .map(|(parameter, home)| (parameter.value(), home))
            .collect();
        accepts &= initial.define(&definitions);
        entries[body.entry().index() as usize] = Some(initial);

        loop {
            let mut changed = false;
            for block in body.block_order().iter().copied() {
                let Some(mut state) = entries[block.index() as usize].clone() else {
                    continue;
                };
                accepts &= dense_transfer_definitions(body, plan, function, block, &mut state);
                let terminator = body.block(block).unwrap().terminator().unwrap();
                match terminator.kind() {
                    TerminatorKind::Jump(target) => {
                        let Some(EdgeTransfer::Jump { steps }) =
                            plan.edge_transfer(function, block)
                        else {
                            accepts = false;
                            continue;
                        };
                        accepts &= dense_propagate_edge(
                            body,
                            plan,
                            function,
                            &state,
                            target,
                            steps,
                            &mut entries,
                            &mut changed,
                        );
                    }
                    TerminatorKind::Branch {
                        then_target,
                        else_target,
                        ..
                    } => {
                        let Some(EdgeTransfer::Branch {
                            then_edge,
                            else_edge,
                        }) = plan.edge_transfer(function, block)
                        else {
                            accepts = false;
                            continue;
                        };
                        accepts &= dense_propagate_edge(
                            body,
                            plan,
                            function,
                            &state,
                            then_target,
                            then_edge.steps(),
                            &mut entries,
                            &mut changed,
                        );
                        accepts &= dense_propagate_edge(
                            body,
                            plan,
                            function,
                            &state,
                            else_target,
                            else_edge.steps(),
                            &mut entries,
                            &mut changed,
                        );
                    }
                    TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
                }
            }
            if !changed {
                break;
            }
        }

        for block in body.block_order().iter().copied() {
            let Some(state) = entries[block.index() as usize].clone() else {
                continue;
            };
            accepts &= dense_validate_block(body, plan, function, block, state);
        }
        DenseObservation { accepts, entries }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the test oracle keeps its naïve fixed-point tables explicit"
    )]
    fn dense_propagate_edge(
        body: &FunctionBody,
        plan: &LoweringPlan,
        function: FunctionId,
        source: &DenseState,
        target: &BlockTarget,
        steps: &[MoveStep],
        entries: &mut [Option<DenseState>],
        changed: &mut bool,
    ) -> bool {
        let (assignments, mut valid) = dense_edge_assignments(body, plan, function, target);
        let mut state = source.clone();
        valid &= dense_apply_edge(plan, function, &mut state, &assignments, steps);
        let target_index = target.block().index() as usize;
        let Some(entry) = entries.get_mut(target_index) else {
            return false;
        };
        if let Some(entry) = entry {
            *changed |= entry.meet(&state);
        } else {
            *entry = Some(state);
            *changed = true;
        }
        valid
    }

    fn dense_transfer_definitions(
        body: &FunctionBody,
        plan: &LoweringPlan,
        function: FunctionId,
        block: BlockId,
        state: &mut DenseState,
    ) -> bool {
        let mut valid = true;
        for instruction in body.block(block).unwrap().instructions().iter().copied() {
            let data = body.instruction(instruction).unwrap();
            let Some(instruction_plan) = plan.instruction_plan(function, instruction) else {
                valid = false;
                continue;
            };
            valid &= dense_finish_instruction(data, instruction_plan, state);
        }
        valid
    }

    fn dense_finish_instruction(
        data: &InstData,
        plan: &InstructionPlan,
        state: &mut DenseState,
    ) -> bool {
        match plan {
            InstructionPlan::OmittedPure
            | InstructionPlan::External { .. }
            | InstructionPlan::Minecraft { .. } => true,
            InstructionPlan::Scalar { results, .. } => {
                let mut valid = true;
                for result in results.iter().copied() {
                    valid &= state.kill(result.home());
                }
                let definitions: Vec<_> = results
                    .iter()
                    .copied()
                    .filter_map(|result| match result {
                        ScalarResultPlacement::Semantic {
                            result_index,
                            value,
                            home,
                        } if data.results().get(result_index).copied() == Some(value) => {
                            Some((value, home))
                        }
                        _ => None,
                    })
                    .collect();
                valid & state.define(&definitions)
            }
            InstructionPlan::Call {
                result_destinations,
                ..
            } => {
                let definitions: Vec<_> = result_destinations
                    .iter()
                    .copied()
                    .flatten()
                    .filter(|destination| {
                        data.results().get(destination.result_index()).copied()
                            == Some(destination.value())
                    })
                    .map(|destination| (destination.value(), destination.home()))
                    .collect();
                state.define(&definitions)
            }
        }
    }

    fn dense_edge_assignments(
        body: &FunctionBody,
        plan: &LoweringPlan,
        function: FunctionId,
        target: &BlockTarget,
    ) -> (Vec<DenseEdgeAssignment>, bool) {
        let mut assignments = Vec::new();
        let mut valid = true;
        let destination = body.block(target.block()).unwrap();
        for (parameter, source_value) in destination
            .parameters()
            .iter()
            .zip(target.arguments().iter().copied())
        {
            let Some(destination_home) = plan.value_home(function, parameter.value()) else {
                continue;
            };
            if plan.value_home(function, source_value).is_none() {
                valid = false;
                continue;
            }
            assignments.push(DenseEdgeAssignment {
                destination_value: parameter.value(),
                destination_home,
                source_value,
            });
        }
        (assignments, valid)
    }

    fn dense_apply_edge(
        plan: &LoweringPlan,
        function: FunctionId,
        state: &mut DenseState,
        assignments: &[DenseEdgeAssignment],
        steps: &[MoveStep],
    ) -> bool {
        let layout = plan.functions.get(function).unwrap();
        let mut valid = true;
        for temporary in layout.edge_temporaries.iter().copied() {
            valid &= state.kill(temporary);
        }
        for step in steps.iter().copied() {
            valid &= state.copy(step.source(), step.destination());
        }
        valid &= state.substitute(assignments);
        for temporary in layout.edge_temporaries.iter().copied() {
            valid &= state.kill(temporary);
        }
        valid
    }

    fn dense_validate_block(
        body: &FunctionBody,
        plan: &LoweringPlan,
        function: FunctionId,
        block: BlockId,
        mut state: DenseState,
    ) -> bool {
        let block_data = body.block(block).unwrap();
        let mut valid = block_data.parameters().iter().all(|parameter| {
            plan.value_home(function, parameter.value())
                .is_none_or(|home| state.contains(home, parameter.value()))
        });
        for instruction in block_data.instructions().iter().copied() {
            let data = body.instruction(instruction).unwrap();
            let Some(instruction_plan) = plan.instruction_plan(function, instruction) else {
                valid = false;
                continue;
            };
            let operand_homes: &[HomeId] = match instruction_plan {
                InstructionPlan::OmittedPure
                | InstructionPlan::External { .. }
                | InstructionPlan::Minecraft { .. } => &[],
                InstructionPlan::Scalar { operands, .. } => operands,
                InstructionPlan::Call { arguments, .. } => arguments,
            };
            valid &= data
                .operands()
                .iter()
                .copied()
                .zip(operand_homes.iter().copied())
                .all(|(value, home)| {
                    plan.value_home(function, value) == Some(home) && state.contains(home, value)
                });
            valid &= dense_finish_instruction(data, instruction_plan, &mut state);
        }
        let terminator = block_data.terminator().unwrap();
        match terminator.kind() {
            TerminatorKind::Jump(target) => {
                let Some(EdgeTransfer::Jump { steps }) = plan.edge_transfer(function, block) else {
                    return false;
                };
                valid & dense_validate_edge(body, plan, function, &state, target, steps)
            }
            TerminatorKind::Branch {
                condition,
                then_target,
                else_target,
            } => {
                valid &= plan
                    .value_home(function, *condition)
                    .is_some_and(|home| state.contains(home, *condition));
                let Some(EdgeTransfer::Branch {
                    then_edge,
                    else_edge,
                }) = plan.edge_transfer(function, block)
                else {
                    return false;
                };
                valid
                    & dense_validate_edge(
                        body,
                        plan,
                        function,
                        &state,
                        then_target,
                        then_edge.steps(),
                    )
                    & dense_validate_edge(
                        body,
                        plan,
                        function,
                        &state,
                        else_target,
                        else_edge.steps(),
                    )
            }
            TerminatorKind::Return(values) => {
                valid
                    & values.iter().copied().all(|value| {
                        plan.value_home(function, value)
                            .is_some_and(|home| state.contains(home, value))
                    })
            }
            TerminatorKind::Unreachable => valid,
        }
    }

    fn dense_validate_edge(
        body: &FunctionBody,
        plan: &LoweringPlan,
        function: FunctionId,
        source: &DenseState,
        target: &BlockTarget,
        steps: &[MoveStep],
    ) -> bool {
        let (assignments, mut valid) = dense_edge_assignments(body, plan, function, target);
        for assignment in &assignments {
            let source_home = plan.value_home(function, assignment.source_value).unwrap();
            valid &= source.contains(source_home, assignment.source_value);
        }
        let layout = plan.functions.get(function).unwrap();
        let mut moved = source.clone();
        for temporary in layout.edge_temporaries.iter().copied() {
            valid &= moved.kill(temporary);
        }
        for step in steps.iter().copied() {
            valid &= moved.copy(step.source(), step.destination());
        }
        for assignment in &assignments {
            valid &= moved.contains(assignment.destination_home, assignment.source_value);
        }
        valid &= moved.substitute(&assignments);
        for temporary in layout.edge_temporaries.iter().copied() {
            valid &= moved.kill(temporary);
        }
        for assignment in &assignments {
            valid &= moved.contains(assignment.destination_home, assignment.destination_value);
        }
        valid
    }

    fn state_from_mask(
        function: FunctionId,
        mask: impl Into<u64>,
        homes: usize,
        values: usize,
    ) -> SparseState {
        let mask = mask.into();
        let mut state = SparseState::new();
        for home in 0..homes {
            for value in 0..values {
                let bit = home * values + value;
                if mask & (1_u64 << bit) != 0 {
                    state
                        .add_symbol(
                            function,
                            HomeId::from_index(u32::try_from(home).unwrap()),
                            ValueId::from_index(u32::try_from(value).unwrap()),
                        )
                        .unwrap();
                }
            }
        }
        state
    }

    fn dense_from_mask(mask: impl Into<u64>, homes: usize, values: usize) -> Vec<Vec<bool>> {
        let mask = mask.into();
        (0..homes)
            .map(|home| {
                (0..values)
                    .map(|value| mask & (1_u64 << (home * values + value)) != 0)
                    .collect()
            })
            .collect()
    }

    fn assert_state_mask(state: &SparseState, mask: impl Into<u64>, homes: usize, values: usize) {
        assert_dense_state(state, &dense_from_mask(mask, homes, values));
    }

    fn assert_dense_state(state: &SparseState, expected: &[Vec<bool>]) {
        for (home, values) in expected.iter().enumerate() {
            for (value, present) in values.iter().copied().enumerate() {
                assert_eq!(
                    state.contains(
                        HomeId::from_index(u32::try_from(home).unwrap()),
                        ValueId::from_index(u32::try_from(value).unwrap())
                    ),
                    present,
                    "home {home}, value {value}"
                );
            }
        }
    }

    fn assert_inverse_is_exact(state: &SparseState) {
        for fact in &state.facts {
            for value in &fact.symbols {
                assert!(state.homes(*value).contains(&fact.home));
            }
        }
        for (value, homes) in &state.reverse {
            for home in homes {
                assert!(state.contains(*home, *value));
            }
        }
        assert_eq!(
            state
                .facts
                .iter()
                .map(|fact| fact.symbols.len())
                .sum::<usize>(),
            state.reverse.values().map(Vec::len).sum::<usize>()
        );
    }
}
