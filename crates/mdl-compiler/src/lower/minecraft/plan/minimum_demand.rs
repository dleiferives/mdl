use std::collections::VecDeque;

use crate::entity::{EntityId, EntityVec};
use crate::ir::core::{
    BlockId, BlockTarget, CoreProgram, FunctionBody, FunctionId, InstId, TerminatorKind, ValueDef,
    ValueId,
};

/// Verifier-owned least semantic demand, derived independently from Core.
#[derive(Clone, Debug)]
pub(crate) struct MinimumSemanticDemand {
    functions: EntityVec<FunctionId, FunctionMinimumDemand>,
}

/// Reachability and least backward closure for one Core function.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FunctionMinimumDemand {
    reachable_blocks: Box<[BlockId]>,
    reachable_block_bits: Box<[bool]>,
    reachable_instruction_bits: Box<[bool]>,
    reachable_value_bits: Box<[bool]>,
    demanded_instruction_bits: Box<[bool]>,
    demanded_value_bits: Box<[bool]>,
}

#[derive(Debug)]
struct ReachabilityFacts {
    ordered_blocks: Vec<BlockId>,
    block_bits: Vec<bool>,
}

#[derive(Debug)]
struct ReachableCensus {
    instruction_bits: Vec<bool>,
    value_bits: Vec<bool>,
    instruction_count: usize,
    value_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuccessorOccurrence {
    Jump,
    Then,
    Else,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IncomingEdgeOccurrence {
    source: BlockId,
    successor: SuccessorOccurrence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkItem {
    Instruction(InstId),
    Value(ValueId),
}

impl MinimumSemanticDemand {
    /// Recomputes the verifier's complete mandatory demand from Core alone.
    pub(crate) fn new(core: &CoreProgram) -> Result<Self, MinimumDemandError> {
        let mut functions = program_vec(MinimumDemandTable::Functions, core.len())?;
        for (function, declaration) in core.functions() {
            let expected: FunctionId = dense_id(functions.len())
                .ok_or(MinimumDemandError::FunctionAlignment { function })?;
            if expected != function || functions.len() == core.len() {
                return Err(MinimumDemandError::FunctionAlignment { function });
            }
            let body = declaration
                .body()
                .ok_or(MinimumDemandError::MissingDefinition { function })?;
            functions.push(analyze_function(function, body)?);
        }
        if functions.len() != core.len() {
            return Err(MinimumDemandError::ProgramFunctionCountMismatch {
                expected: core.len(),
                actual: functions.len(),
            });
        }
        Ok(Self {
            functions: EntityVec::from_constrained_values(functions),
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.functions.len()
    }

    pub(crate) fn function(&self, function: FunctionId) -> Option<&FunctionMinimumDemand> {
        self.functions.get(function)
    }
}

impl FunctionMinimumDemand {
    /// Reachable blocks in authoritative Core layout order.
    pub(crate) fn reachable_blocks(&self) -> &[BlockId] {
        &self.reachable_blocks
    }

    pub(crate) fn is_block_reachable(&self, block: BlockId) -> Option<bool> {
        get_bit(&self.reachable_block_bits, block)
    }

    pub(crate) fn is_instruction_reachable(&self, instruction: InstId) -> Option<bool> {
        get_bit(&self.reachable_instruction_bits, instruction)
    }

    pub(crate) fn is_value_reachable(&self, value: ValueId) -> Option<bool> {
        get_bit(&self.reachable_value_bits, value)
    }

    #[allow(
        dead_code,
        reason = "minimum-demand unit tests inspect the complete reachable instruction set"
    )]
    pub(crate) fn reachable_instructions(&self) -> impl Iterator<Item = InstId> + '_ {
        marked_ids(&self.reachable_instruction_bits)
    }

    #[allow(
        dead_code,
        reason = "minimum-demand unit tests inspect the complete reachable value set"
    )]
    pub(crate) fn reachable_values(&self) -> impl Iterator<Item = ValueId> + '_ {
        marked_ids(&self.reachable_value_bits)
    }

    pub(crate) fn requires_instruction(&self, instruction: InstId) -> Option<bool> {
        get_bit(&self.demanded_instruction_bits, instruction)
    }

    pub(crate) fn requires_value(&self, value: ValueId) -> Option<bool> {
        get_bit(&self.demanded_value_bits, value)
    }

    #[allow(
        dead_code,
        reason = "minimum-demand unit tests inspect the complete mandatory instruction closure"
    )]
    pub(crate) fn demanded_instructions(&self) -> impl Iterator<Item = InstId> + '_ {
        marked_ids(&self.demanded_instruction_bits)
    }

    #[allow(
        dead_code,
        reason = "minimum-demand unit tests inspect the complete mandatory value closure"
    )]
    pub(crate) fn demanded_values(&self) -> impl Iterator<Item = ValueId> + '_ {
        marked_ids(&self.demanded_value_bits)
    }

    pub(crate) fn instruction_slots(&self) -> usize {
        self.demanded_instruction_bits.len()
    }

    pub(crate) fn value_slots(&self) -> usize {
        self.demanded_value_bits.len()
    }

    pub(crate) fn block_slots(&self) -> usize {
        self.reachable_block_bits.len()
    }
}

fn analyze_function(
    function: FunctionId,
    body: &FunctionBody,
) -> Result<FunctionMinimumDemand, MinimumDemandError> {
    let attached = derive_attached_blocks(function, body)?;
    let reachability = derive_reachability(function, body, &attached)?;
    let census = derive_reachable_census(function, body, &reachability)?;
    let incoming = derive_incoming_edges(function, body, &reachability, &census)?;
    let (demanded_instruction_bits, demanded_value_bits) =
        solve_backward_closure(function, body, &reachability, &census, &incoming)?;
    validate_no_unreachable_marks(
        function,
        &census,
        &demanded_instruction_bits,
        &demanded_value_bits,
    )?;
    Ok(FunctionMinimumDemand {
        reachable_blocks: reachability.ordered_blocks.into_boxed_slice(),
        reachable_block_bits: reachability.block_bits.into_boxed_slice(),
        reachable_instruction_bits: census.instruction_bits.into_boxed_slice(),
        reachable_value_bits: census.value_bits.into_boxed_slice(),
        demanded_instruction_bits: demanded_instruction_bits.into_boxed_slice(),
        demanded_value_bits: demanded_value_bits.into_boxed_slice(),
    })
}

fn derive_attached_blocks(
    function: FunctionId,
    body: &FunctionBody,
) -> Result<Vec<bool>, MinimumDemandError> {
    let mut attached = false_table(
        function,
        MinimumDemandTable::AttachedBlocks,
        body.block_counts().allocated,
    )?;
    for block in body.block_order().iter().copied() {
        if body.block(block).is_none() {
            return Err(MinimumDemandError::InvalidBlock { function, block });
        }
        let slot = bit_mut(&mut attached, block)
            .ok_or(MinimumDemandError::InvalidBlock { function, block })?;
        if std::mem::replace(slot, true) {
            return Err(MinimumDemandError::DuplicateAttachedBlock { function, block });
        }
    }
    if get_bit(&attached, body.entry()) != Some(true) {
        return Err(MinimumDemandError::InvalidEntry {
            function,
            entry: body.entry(),
        });
    }
    Ok(attached)
}

fn derive_reachability(
    function: FunctionId,
    body: &FunctionBody,
    attached: &[bool],
) -> Result<ReachabilityFacts, MinimumDemandError> {
    let mut block_bits = false_table(
        function,
        MinimumDemandTable::ReachableBlocks,
        body.block_counts().allocated,
    )?;
    let mut queue = fallible_queue(
        function,
        MinimumDemandTable::ReachabilityQueue,
        body.block_counts().allocated,
    )?;
    mark_reachable_block(
        function,
        body.entry(),
        attached,
        &mut block_bits,
        &mut queue,
    )?;

    while let Some(block) = queue.pop_front() {
        let data = body
            .block(block)
            .ok_or(MinimumDemandError::InvalidBlock { function, block })?;
        let terminator = data
            .terminator()
            .ok_or(MinimumDemandError::MissingTerminator { function, block })?;
        match terminator.kind() {
            TerminatorKind::Jump(target) => mark_reachable_block(
                function,
                target.block(),
                attached,
                &mut block_bits,
                &mut queue,
            )?,
            TerminatorKind::Branch {
                then_target,
                else_target,
                ..
            } => {
                mark_reachable_block(
                    function,
                    then_target.block(),
                    attached,
                    &mut block_bits,
                    &mut queue,
                )?;
                mark_reachable_block(
                    function,
                    else_target.block(),
                    attached,
                    &mut block_bits,
                    &mut queue,
                )?;
            }
            TerminatorKind::Return(_) | TerminatorKind::Unreachable => {}
        }
    }

    let mut ordered_blocks = fallible_vec(
        function,
        MinimumDemandTable::ReachableBlockOrder,
        body.block_order().len(),
    )?;
    ordered_blocks.extend(
        body.block_order()
            .iter()
            .copied()
            .filter(|block| get_bit(&block_bits, *block) == Some(true)),
    );
    Ok(ReachabilityFacts {
        ordered_blocks,
        block_bits,
    })
}

fn mark_reachable_block(
    function: FunctionId,
    block: BlockId,
    attached: &[bool],
    reachable: &mut [bool],
    queue: &mut VecDeque<BlockId>,
) -> Result<(), MinimumDemandError> {
    if get_bit(attached, block) != Some(true) {
        return Err(MinimumDemandError::InvalidSuccessor {
            function,
            destination: block,
        });
    }
    let slot = bit_mut(reachable, block).ok_or(MinimumDemandError::InvalidSuccessor {
        function,
        destination: block,
    })?;
    if !std::mem::replace(slot, true) {
        queue.push_back(block);
    }
    Ok(())
}

fn derive_reachable_census(
    function: FunctionId,
    body: &FunctionBody,
    reachability: &ReachabilityFacts,
) -> Result<ReachableCensus, MinimumDemandError> {
    let mut instruction_bits = false_table(
        function,
        MinimumDemandTable::ReachableInstructions,
        body.instruction_counts().allocated,
    )?;
    let mut value_bits = false_table(
        function,
        MinimumDemandTable::ReachableValues,
        body.value_counts().allocated,
    )?;
    let mut instruction_count = 0_usize;
    let mut value_count = 0_usize;

    for block in reachability.ordered_blocks.iter().copied() {
        let data = body
            .block(block)
            .ok_or(MinimumDemandError::InvalidBlock { function, block })?;
        for (parameter_index, parameter) in data.parameters().iter().enumerate() {
            validate_block_parameter(function, body, block, parameter_index, parameter.value())?;
            record_value(
                function,
                parameter.value(),
                &mut value_bits,
                &mut value_count,
            )?;
        }
        for instruction in data.instructions().iter().copied() {
            let instruction_data =
                body.instruction(instruction)
                    .ok_or(MinimumDemandError::InvalidInstruction {
                        function,
                        instruction,
                    })?;
            let slot = bit_mut(&mut instruction_bits, instruction).ok_or(
                MinimumDemandError::InvalidInstruction {
                    function,
                    instruction,
                },
            )?;
            if std::mem::replace(slot, true) {
                return Err(MinimumDemandError::DuplicateReachableInstruction {
                    function,
                    instruction,
                });
            }
            instruction_count =
                instruction_count
                    .checked_add(1)
                    .ok_or(MinimumDemandError::CountOverflow {
                        function,
                        table: MinimumDemandTable::ReachableInstructions,
                    })?;
            for (result_index, result) in instruction_data.results().iter().copied().enumerate() {
                validate_instruction_result(function, body, instruction, result_index, result)?;
                record_value(function, result, &mut value_bits, &mut value_count)?;
            }
        }
    }
    Ok(ReachableCensus {
        instruction_bits,
        value_bits,
        instruction_count,
        value_count,
    })
}

fn validate_block_parameter(
    function: FunctionId,
    body: &FunctionBody,
    block: BlockId,
    parameter_index: usize,
    value: ValueId,
) -> Result<(), MinimumDemandError> {
    let expected_index = u32::try_from(parameter_index)
        .map_err(|_| MinimumDemandError::InvalidValueDefinition { function, value })?;
    let data = body
        .value(value)
        .ok_or(MinimumDemandError::InvalidValue { function, value })?;
    if data.definition()
        != (ValueDef::BlockParam {
            block,
            parameter_index: expected_index,
        })
    {
        return Err(MinimumDemandError::InvalidValueDefinition { function, value });
    }
    Ok(())
}

fn validate_instruction_result(
    function: FunctionId,
    body: &FunctionBody,
    instruction: InstId,
    result_index: usize,
    value: ValueId,
) -> Result<(), MinimumDemandError> {
    let expected_index = u32::try_from(result_index)
        .map_err(|_| MinimumDemandError::InvalidValueDefinition { function, value })?;
    let data = body
        .value(value)
        .ok_or(MinimumDemandError::InvalidValue { function, value })?;
    if data.definition()
        != (ValueDef::InstResult {
            instruction,
            result_index: expected_index,
        })
    {
        return Err(MinimumDemandError::InvalidValueDefinition { function, value });
    }
    Ok(())
}

fn record_value(
    function: FunctionId,
    value: ValueId,
    bits: &mut [bool],
    count: &mut usize,
) -> Result<(), MinimumDemandError> {
    let slot = bit_mut(bits, value).ok_or(MinimumDemandError::InvalidValue { function, value })?;
    if std::mem::replace(slot, true) {
        return Err(MinimumDemandError::DuplicateReachableValue { function, value });
    }
    *count = count
        .checked_add(1)
        .ok_or(MinimumDemandError::CountOverflow {
            function,
            table: MinimumDemandTable::ReachableValues,
        })?;
    Ok(())
}

fn derive_incoming_edges(
    function: FunctionId,
    body: &FunctionBody,
    reachability: &ReachabilityFacts,
    census: &ReachableCensus,
) -> Result<Vec<Vec<IncomingEdgeOccurrence>>, MinimumDemandError> {
    let mut incoming = fallible_nested_vec(
        function,
        MinimumDemandTable::IncomingEdgeHeads,
        body.block_counts().allocated,
    )?;
    for source in reachability.ordered_blocks.iter().copied() {
        let data = body.block(source).ok_or(MinimumDemandError::InvalidBlock {
            function,
            block: source,
        })?;
        for instruction in data.instructions().iter().copied() {
            let instruction_data =
                body.instruction(instruction)
                    .ok_or(MinimumDemandError::InvalidInstruction {
                        function,
                        instruction,
                    })?;
            for operand in instruction_data.operands().iter().copied() {
                require_reachable_value(function, census, operand)?;
            }
        }
        let terminator = data
            .terminator()
            .ok_or(MinimumDemandError::MissingTerminator {
                function,
                block: source,
            })?;
        match terminator.kind() {
            TerminatorKind::Jump(target) => record_incoming_edge(
                function,
                body,
                reachability,
                census,
                &mut incoming,
                source,
                SuccessorOccurrence::Jump,
                target,
            )?,
            TerminatorKind::Branch {
                condition,
                then_target,
                else_target,
            } => {
                require_reachable_value(function, census, *condition)?;
                record_incoming_edge(
                    function,
                    body,
                    reachability,
                    census,
                    &mut incoming,
                    source,
                    SuccessorOccurrence::Then,
                    then_target,
                )?;
                record_incoming_edge(
                    function,
                    body,
                    reachability,
                    census,
                    &mut incoming,
                    source,
                    SuccessorOccurrence::Else,
                    else_target,
                )?;
            }
            TerminatorKind::Return(values) => {
                for value in values.iter().copied() {
                    require_reachable_value(function, census, value)?;
                }
            }
            TerminatorKind::Unreachable => {}
        }
    }
    Ok(incoming)
}

#[allow(
    clippy::too_many_arguments,
    reason = "one checked edge occurrence correlates independent reachability, Core types, and incoming storage"
)]
fn record_incoming_edge(
    function: FunctionId,
    body: &FunctionBody,
    reachability: &ReachabilityFacts,
    census: &ReachableCensus,
    incoming: &mut [Vec<IncomingEdgeOccurrence>],
    source: BlockId,
    successor: SuccessorOccurrence,
    target: &BlockTarget,
) -> Result<(), MinimumDemandError> {
    if target.block() == body.entry() {
        return Err(MinimumDemandError::EdgeTargetsEntry { function, source });
    }
    if get_bit(&reachability.block_bits, target.block()) != Some(true) {
        return Err(MinimumDemandError::InvalidSuccessor {
            function,
            destination: target.block(),
        });
    }
    let destination = body
        .block(target.block())
        .ok_or(MinimumDemandError::InvalidBlock {
            function,
            block: target.block(),
        })?;
    if destination.parameters().len() != target.arguments().len() {
        return Err(MinimumDemandError::InvalidEdgeArity {
            function,
            source,
            destination: target.block(),
        });
    }
    for (parameter, argument) in destination.parameters().iter().zip(target.arguments()) {
        require_reachable_value(function, census, *argument)?;
        let parameter_type = body
            .value(parameter.value())
            .ok_or(MinimumDemandError::InvalidValue {
                function,
                value: parameter.value(),
            })?
            .ty();
        let argument_type = body
            .value(*argument)
            .ok_or(MinimumDemandError::InvalidValue {
                function,
                value: *argument,
            })?
            .ty();
        if parameter_type != argument_type {
            return Err(MinimumDemandError::EdgeTypeMismatch {
                function,
                source,
                destination: target.block(),
            });
        }
    }
    let destination_index = entity_index(target.block())
        .and_then(|index| incoming.get_mut(index))
        .ok_or(MinimumDemandError::InvalidBlock {
            function,
            block: target.block(),
        })?;
    destination_index
        .try_reserve(1)
        .map_err(|_| MinimumDemandError::Capacity {
            function,
            table: MinimumDemandTable::IncomingEdgeOccurrences,
        })?;
    destination_index.push(IncomingEdgeOccurrence { source, successor });
    Ok(())
}

fn require_reachable_value(
    function: FunctionId,
    census: &ReachableCensus,
    value: ValueId,
) -> Result<(), MinimumDemandError> {
    if get_bit(&census.value_bits, value) != Some(true) {
        return Err(MinimumDemandError::UnreachableValueUse { function, value });
    }
    Ok(())
}

fn solve_backward_closure(
    function: FunctionId,
    body: &FunctionBody,
    reachability: &ReachabilityFacts,
    census: &ReachableCensus,
    incoming: &[Vec<IncomingEdgeOccurrence>],
) -> Result<(Vec<bool>, Vec<bool>), MinimumDemandError> {
    let mut demanded_instructions = false_table(
        function,
        MinimumDemandTable::DemandedInstructions,
        census.instruction_bits.len(),
    )?;
    let mut demanded_values = false_table(
        function,
        MinimumDemandTable::DemandedValues,
        census.value_bits.len(),
    )?;
    let queue_capacity = census
        .instruction_count
        .checked_add(census.value_count)
        .ok_or(MinimumDemandError::CountOverflow {
            function,
            table: MinimumDemandTable::DemandWorklist,
        })?;
    let mut queue = fallible_queue(function, MinimumDemandTable::DemandWorklist, queue_capacity)?;
    seed_roots(
        function,
        body,
        reachability,
        census,
        &mut demanded_instructions,
        &mut demanded_values,
        &mut queue,
    )?;
    drain_worklist(
        function,
        body,
        census,
        incoming,
        &mut demanded_instructions,
        &mut demanded_values,
        &mut queue,
    )?;
    Ok((demanded_instructions, demanded_values))
}

#[allow(
    clippy::too_many_arguments,
    reason = "root seeding writes both dense demand domains through one deterministic queue"
)]
fn seed_roots(
    function: FunctionId,
    body: &FunctionBody,
    reachability: &ReachabilityFacts,
    census: &ReachableCensus,
    demanded_instructions: &mut [bool],
    demanded_values: &mut [bool],
    queue: &mut VecDeque<WorkItem>,
) -> Result<(), MinimumDemandError> {
    for block in reachability.ordered_blocks.iter().copied() {
        let data = body
            .block(block)
            .ok_or(MinimumDemandError::InvalidBlock { function, block })?;
        for instruction in data.instructions().iter().copied() {
            let instruction_data =
                body.instruction(instruction)
                    .ok_or(MinimumDemandError::InvalidInstruction {
                        function,
                        instruction,
                    })?;
            if !instruction_data.op().is_trivially_discardable() {
                mark_instruction(function, census, demanded_instructions, queue, instruction)?;
            }
        }
        let terminator = data
            .terminator()
            .ok_or(MinimumDemandError::MissingTerminator { function, block })?;
        match terminator.kind() {
            TerminatorKind::Branch { condition, .. } => {
                mark_value(function, census, demanded_values, queue, *condition)?;
            }
            TerminatorKind::Return(values) => {
                for value in values.iter().copied() {
                    mark_value(function, census, demanded_values, queue, value)?;
                }
            }
            TerminatorKind::Jump(_) | TerminatorKind::Unreachable => {}
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "the fixed-point worker updates two typed mark domains from one ordered queue"
)]
fn drain_worklist(
    function: FunctionId,
    body: &FunctionBody,
    census: &ReachableCensus,
    incoming: &[Vec<IncomingEdgeOccurrence>],
    demanded_instructions: &mut [bool],
    demanded_values: &mut [bool],
    queue: &mut VecDeque<WorkItem>,
) -> Result<(), MinimumDemandError> {
    while let Some(item) = queue.pop_front() {
        match item {
            WorkItem::Instruction(instruction) => {
                let data = body.instruction(instruction).ok_or(
                    MinimumDemandError::InvalidInstruction {
                        function,
                        instruction,
                    },
                )?;
                for operand in data.operands().iter().copied() {
                    mark_value(function, census, demanded_values, queue, operand)?;
                }
            }
            WorkItem::Value(value) => {
                propagate_value(
                    function,
                    body,
                    census,
                    incoming,
                    demanded_instructions,
                    demanded_values,
                    queue,
                    value,
                )?;
            }
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "value propagation validates its Core definition before updating either mark domain"
)]
fn propagate_value(
    function: FunctionId,
    body: &FunctionBody,
    census: &ReachableCensus,
    incoming: &[Vec<IncomingEdgeOccurrence>],
    demanded_instructions: &mut [bool],
    demanded_values: &mut [bool],
    queue: &mut VecDeque<WorkItem>,
    value: ValueId,
) -> Result<(), MinimumDemandError> {
    let data = body
        .value(value)
        .ok_or(MinimumDemandError::InvalidValue { function, value })?;
    match data.definition() {
        ValueDef::InstResult {
            instruction,
            result_index,
        } => {
            let result_index = usize::try_from(result_index)
                .map_err(|_| MinimumDemandError::InvalidValueDefinition { function, value })?;
            let producer =
                body.instruction(instruction)
                    .ok_or(MinimumDemandError::InvalidInstruction {
                        function,
                        instruction,
                    })?;
            if producer.results().get(result_index).copied() != Some(value) {
                return Err(MinimumDemandError::InvalidValueDefinition { function, value });
            }
            mark_instruction(function, census, demanded_instructions, queue, instruction)?;
        }
        ValueDef::BlockParam {
            block,
            parameter_index,
        } => propagate_block_parameter(
            function,
            body,
            census,
            incoming,
            demanded_values,
            queue,
            value,
            block,
            parameter_index,
        )?,
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "each incoming occurrence is independently re-read from Core for one parameter index"
)]
fn propagate_block_parameter(
    function: FunctionId,
    body: &FunctionBody,
    census: &ReachableCensus,
    incoming: &[Vec<IncomingEdgeOccurrence>],
    demanded_values: &mut [bool],
    queue: &mut VecDeque<WorkItem>,
    value: ValueId,
    block: BlockId,
    parameter_index: u32,
) -> Result<(), MinimumDemandError> {
    let parameter_index = usize::try_from(parameter_index)
        .map_err(|_| MinimumDemandError::InvalidValueDefinition { function, value })?;
    let block_data = body
        .block(block)
        .ok_or(MinimumDemandError::InvalidBlock { function, block })?;
    if block_data
        .parameters()
        .get(parameter_index)
        .map(crate::ir::core::BlockParam::value)
        != Some(value)
    {
        return Err(MinimumDemandError::InvalidValueDefinition { function, value });
    }
    let occurrences = entity_index(block)
        .and_then(|index| incoming.get(index))
        .ok_or(MinimumDemandError::InvalidBlock { function, block })?;
    for occurrence in occurrences.iter().copied() {
        let target = occurrence_target(function, body, occurrence)?;
        if target.block() != block || target.arguments().len() != block_data.parameters().len() {
            return Err(MinimumDemandError::InvalidIncomingEdge {
                function,
                source: occurrence.source,
                destination: block,
            });
        }
        let argument = target.arguments().get(parameter_index).copied().ok_or(
            MinimumDemandError::InvalidIncomingEdge {
                function,
                source: occurrence.source,
                destination: block,
            },
        )?;
        mark_value(function, census, demanded_values, queue, argument)?;
    }
    Ok(())
}

fn occurrence_target(
    function: FunctionId,
    body: &FunctionBody,
    occurrence: IncomingEdgeOccurrence,
) -> Result<&BlockTarget, MinimumDemandError> {
    let terminator = body
        .block(occurrence.source)
        .ok_or(MinimumDemandError::InvalidBlock {
            function,
            block: occurrence.source,
        })?
        .terminator()
        .ok_or(MinimumDemandError::MissingTerminator {
            function,
            block: occurrence.source,
        })?;
    match (terminator.kind(), occurrence.successor) {
        (TerminatorKind::Jump(target), SuccessorOccurrence::Jump)
        | (
            TerminatorKind::Branch {
                then_target: target,
                ..
            },
            SuccessorOccurrence::Then,
        )
        | (
            TerminatorKind::Branch {
                else_target: target,
                ..
            },
            SuccessorOccurrence::Else,
        ) => Ok(target),
        _ => Err(MinimumDemandError::InvalidIncomingEdge {
            function,
            source: occurrence.source,
            destination: occurrence.source,
        }),
    }
}

fn mark_instruction(
    function: FunctionId,
    census: &ReachableCensus,
    demanded: &mut [bool],
    queue: &mut VecDeque<WorkItem>,
    instruction: InstId,
) -> Result<(), MinimumDemandError> {
    if get_bit(&census.instruction_bits, instruction) != Some(true) {
        return Err(MinimumDemandError::UnreachableInstructionMark {
            function,
            instruction,
        });
    }
    let slot = bit_mut(demanded, instruction).ok_or(MinimumDemandError::InvalidInstruction {
        function,
        instruction,
    })?;
    if !std::mem::replace(slot, true) {
        queue.push_back(WorkItem::Instruction(instruction));
    }
    Ok(())
}

fn mark_value(
    function: FunctionId,
    census: &ReachableCensus,
    demanded: &mut [bool],
    queue: &mut VecDeque<WorkItem>,
    value: ValueId,
) -> Result<(), MinimumDemandError> {
    if get_bit(&census.value_bits, value) != Some(true) {
        return Err(MinimumDemandError::UnreachableValueMark { function, value });
    }
    let slot =
        bit_mut(demanded, value).ok_or(MinimumDemandError::InvalidValue { function, value })?;
    if !std::mem::replace(slot, true) {
        queue.push_back(WorkItem::Value(value));
    }
    Ok(())
}

fn validate_no_unreachable_marks(
    function: FunctionId,
    census: &ReachableCensus,
    demanded_instructions: &[bool],
    demanded_values: &[bool],
) -> Result<(), MinimumDemandError> {
    for (index, demanded) in demanded_instructions.iter().copied().enumerate() {
        if demanded && census.instruction_bits.get(index).copied() != Some(true) {
            let instruction = dense_id(index).ok_or(MinimumDemandError::CountOverflow {
                function,
                table: MinimumDemandTable::DemandedInstructions,
            })?;
            return Err(MinimumDemandError::UnreachableInstructionMark {
                function,
                instruction,
            });
        }
    }
    for (index, demanded) in demanded_values.iter().copied().enumerate() {
        if demanded && census.value_bits.get(index).copied() != Some(true) {
            let value = dense_id(index).ok_or(MinimumDemandError::CountOverflow {
                function,
                table: MinimumDemandTable::DemandedValues,
            })?;
            return Err(MinimumDemandError::UnreachableValueMark { function, value });
        }
    }
    Ok(())
}

fn false_table(
    function: FunctionId,
    table: MinimumDemandTable,
    length: usize,
) -> Result<Vec<bool>, MinimumDemandError> {
    let mut values = fallible_vec(function, table, length)?;
    values.resize(length, false);
    Ok(values)
}

fn fallible_vec<T>(
    function: FunctionId,
    table: MinimumDemandTable,
    capacity: usize,
) -> Result<Vec<T>, MinimumDemandError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| MinimumDemandError::Capacity { function, table })?;
    Ok(values)
}

fn program_vec<T>(
    table: MinimumDemandTable,
    capacity: usize,
) -> Result<Vec<T>, MinimumDemandError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| MinimumDemandError::ProgramCapacity { table })?;
    Ok(values)
}

fn fallible_nested_vec<T>(
    function: FunctionId,
    table: MinimumDemandTable,
    length: usize,
) -> Result<Vec<Vec<T>>, MinimumDemandError> {
    let mut values = fallible_vec(function, table, length)?;
    values.resize_with(length, Vec::new);
    Ok(values)
}

fn fallible_queue<T>(
    function: FunctionId,
    table: MinimumDemandTable,
    capacity: usize,
) -> Result<VecDeque<T>, MinimumDemandError> {
    let mut queue = VecDeque::new();
    queue
        .try_reserve_exact(capacity)
        .map_err(|_| MinimumDemandError::Capacity { function, table })?;
    Ok(queue)
}

fn entity_index(id: impl EntityId) -> Option<usize> {
    usize::try_from(id.index()).ok()
}

fn get_bit(bits: &[bool], id: impl EntityId) -> Option<bool> {
    bits.get(entity_index(id)?).copied()
}

fn bit_mut(bits: &mut [bool], id: impl EntityId) -> Option<&mut bool> {
    bits.get_mut(entity_index(id)?)
}

fn dense_id<I: EntityId>(index: usize) -> Option<I> {
    u32::try_from(index).ok().map(I::from_index)
}

#[allow(
    dead_code,
    reason = "shared iterator implementation for the test-observable minimum-demand sets"
)]
fn marked_ids<I: EntityId>(bits: &[bool]) -> impl Iterator<Item = I> + '_ {
    bits.iter()
        .copied()
        .enumerate()
        .filter(|(_, marked)| *marked)
        .map(|(index, _)| {
            let index = u32::try_from(index)
                .expect("Core-owned dense table indices remain in the u32 entity domain");
            I::from_index(index)
        })
}

/// Dense scratch class whose mandatory allocation failed or overflowed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MinimumDemandTable {
    Functions,
    AttachedBlocks,
    ReachableBlocks,
    ReachabilityQueue,
    ReachableBlockOrder,
    ReachableInstructions,
    ReachableValues,
    IncomingEdgeHeads,
    IncomingEdgeOccurrences,
    DemandedInstructions,
    DemandedValues,
    DemandWorklist,
}

/// Mandatory structural or capacity failure in verifier-owned demand derivation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MinimumDemandError {
    MissingDefinition {
        function: FunctionId,
    },
    FunctionAlignment {
        function: FunctionId,
    },
    ProgramFunctionCountMismatch {
        expected: usize,
        actual: usize,
    },
    InvalidEntry {
        function: FunctionId,
        entry: BlockId,
    },
    InvalidBlock {
        function: FunctionId,
        block: BlockId,
    },
    DuplicateAttachedBlock {
        function: FunctionId,
        block: BlockId,
    },
    MissingTerminator {
        function: FunctionId,
        block: BlockId,
    },
    InvalidSuccessor {
        function: FunctionId,
        destination: BlockId,
    },
    EdgeTargetsEntry {
        function: FunctionId,
        source: BlockId,
    },
    InvalidEdgeArity {
        function: FunctionId,
        source: BlockId,
        destination: BlockId,
    },
    EdgeTypeMismatch {
        function: FunctionId,
        source: BlockId,
        destination: BlockId,
    },
    InvalidIncomingEdge {
        function: FunctionId,
        source: BlockId,
        destination: BlockId,
    },
    InvalidInstruction {
        function: FunctionId,
        instruction: InstId,
    },
    DuplicateReachableInstruction {
        function: FunctionId,
        instruction: InstId,
    },
    InvalidValue {
        function: FunctionId,
        value: ValueId,
    },
    InvalidValueDefinition {
        function: FunctionId,
        value: ValueId,
    },
    DuplicateReachableValue {
        function: FunctionId,
        value: ValueId,
    },
    UnreachableValueUse {
        function: FunctionId,
        value: ValueId,
    },
    UnreachableInstructionMark {
        function: FunctionId,
        instruction: InstId,
    },
    UnreachableValueMark {
        function: FunctionId,
        value: ValueId,
    },
    CountOverflow {
        function: FunctionId,
        table: MinimumDemandTable,
    },
    Capacity {
        function: FunctionId,
        table: MinimumDemandTable,
    },
    ProgramCapacity {
        table: MinimumDemandTable,
    },
}

#[cfg(test)]
mod tests {
    use super::{MinimumDemandError, MinimumSemanticDemand};
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionEditor, FunctionId,
        InstId, Terminator, TerminatorKind, ValueDef, ValueId,
    };
    use crate::lower::minecraft::MinecraftOptimizationLevel;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::source::{OriginId, SourceContext};

    #[test]
    fn unused_pure_chain_and_results_are_outside_the_minimum() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = declare(&mut core, "unused", vec![], vec![]);
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let two = builder
            .i32_add_wrapping(one, one, OriginId::UNKNOWN)
            .unwrap();
        let three = builder
            .i32_add_wrapping(two, one, OriginId::UNKNOWN)
            .unwrap();
        let instructions =
            [one, two, three].map(|value| defining_instruction(builder.body(), value));
        builder.terminate(returning(vec![])).unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let demand = MinimumSemanticDemand::new(&core).unwrap();
        let function_demand = demand.function(function).unwrap();
        assert!(
            [one, two, three]
                .into_iter()
                .all(|value| function_demand.requires_value(value) == Some(false))
        );
        assert!(
            instructions.into_iter().all(|instruction| function_demand
                .requires_instruction(instruction)
                == Some(false))
        );
    }

    #[test]
    fn one_multi_result_use_demands_the_producer_but_not_its_sibling_result() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = declare(&mut core, "multi", vec![], vec![CoreType::I32]);
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (sum, overflow) = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap();
        let instruction = defining_instruction(builder.body(), sum);
        builder.terminate(returning(vec![sum])).unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let demand = MinimumSemanticDemand::new(&core).unwrap();
        let function_demand = demand.function(function).unwrap();
        assert_eq!(function_demand.requires_value(sum), Some(true));
        assert_eq!(function_demand.requires_value(overflow), Some(false));
        assert_eq!(
            function_demand.requires_instruction(instruction),
            Some(true)
        );
        assert_eq!(function_demand.requires_value(left), Some(true));
        assert_eq!(function_demand.requires_value(right), Some(true));
    }

    #[test]
    fn calls_seed_arguments_but_not_unused_results_and_uncalled_returns_still_root() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let target_function = declare(
            &mut core,
            "callee",
            vec![CoreType::I32],
            vec![CoreType::I32],
        );
        let uncalled = declare(&mut core, "uncalled", vec![], vec![CoreType::I32]);
        let invoking_function = declare(&mut core, "caller", vec![], vec![]);

        let mut target_builder = FunctionBuilder::new(&core, &sources, target_function).unwrap();
        let target_parameter = block_values(&target_builder, target_builder.entry_block())[0];
        target_builder
            .terminate(returning(vec![target_parameter]))
            .unwrap();
        core.define_function(target_function, target_builder.finish().unwrap())
            .unwrap();

        let mut uncalled_builder = FunctionBuilder::new(&core, &sources, uncalled).unwrap();
        let uncalled_result = uncalled_builder.i32_constant(9, OriginId::UNKNOWN).unwrap();
        uncalled_builder
            .terminate(returning(vec![uncalled_result]))
            .unwrap();
        core.define_function(uncalled, uncalled_builder.finish().unwrap())
            .unwrap();

        let mut invoking_builder =
            FunctionBuilder::new(&core, &sources, invoking_function).unwrap();
        let argument = invoking_builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let call_result = invoking_builder
            .call(target_function, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
        let call = defining_instruction(invoking_builder.body(), call_result);
        invoking_builder.terminate(returning(vec![])).unwrap();
        core.define_function(invoking_function, invoking_builder.finish().unwrap())
            .unwrap();

        let demand = MinimumSemanticDemand::new(&core).unwrap();
        let invoking_demand = demand.function(invoking_function).unwrap();
        assert_eq!(invoking_demand.requires_instruction(call), Some(true));
        assert_eq!(invoking_demand.requires_value(argument), Some(true));
        assert_eq!(invoking_demand.requires_value(call_result), Some(false));
        assert_eq!(
            demand
                .function(target_function)
                .unwrap()
                .requires_value(target_parameter),
            Some(true)
        );
        assert_eq!(
            demand
                .function(uncalled)
                .unwrap()
                .requires_value(uncalled_result),
            Some(true)
        );
    }

    #[test]
    fn rooted_block_parameter_cycle_closes_but_unrooted_cycle_stays_empty() {
        let (core, rooted, rooted_values, unrooted, unrooted_values) = cycle_programs();
        let demand = MinimumSemanticDemand::new(&core).unwrap();
        let rooted_demand = demand.function(rooted).unwrap();
        let unrooted_demand = demand.function(unrooted).unwrap();

        assert!(
            rooted_values
                .into_iter()
                .all(|value| rooted_demand.requires_value(value) == Some(true))
        );
        assert!(
            unrooted_values
                .into_iter()
                .all(|value| unrooted_demand.requires_value(value) == Some(false))
        );
        assert_eq!(unrooted_demand.demanded_values().count(), 0);
        assert_eq!(unrooted_demand.demanded_instructions().count(), 0);
    }

    #[test]
    fn detached_and_unreachable_history_remains_allocated_but_never_marked() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = declare(&mut core, "history", vec![], vec![]);
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let reachable = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let detached = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let detached_instruction = defining_instruction(builder.body(), detached);
        let unreachable_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder.terminate(returning(vec![])).unwrap();
        builder.switch_to_block(unreachable_block).unwrap();
        let unreachable = builder.i32_constant(3, OriginId::UNKNOWN).unwrap();
        builder.terminate(returning(vec![])).unwrap();
        let mut body = builder.finish().unwrap();
        FunctionEditor::new(&core, &sources, function, &mut body)
            .unwrap()
            .erase_pure_inst(detached_instruction)
            .unwrap();
        core.define_function(function, body).unwrap();

        let demand = MinimumSemanticDemand::new(&core).unwrap();
        let function_demand = demand.function(function).unwrap();
        assert_eq!(function_demand.reachable_blocks(), &[entry]);
        assert_eq!(function_demand.block_slots(), 2);
        assert_eq!(
            function_demand.reachable_values().collect::<Vec<_>>(),
            vec![reachable]
        );
        assert_eq!(function_demand.reachable_instructions().count(), 1);
        assert_eq!(function_demand.is_value_reachable(reachable), Some(true));
        assert_eq!(function_demand.requires_value(reachable), Some(false));
        assert_eq!(function_demand.is_value_reachable(detached), Some(false));
        assert_eq!(function_demand.requires_value(detached), Some(false));
        assert_eq!(function_demand.is_value_reachable(unreachable), Some(false));
        assert_eq!(function_demand.requires_value(unreachable), Some(false));
        assert_eq!(
            function_demand.is_instruction_reachable(detached_instruction),
            Some(false)
        );
        assert_eq!(
            function_demand.is_block_reachable(unreachable_block),
            Some(false)
        );
    }

    #[test]
    fn duplicate_destination_edge_occurrences_propagate_each_indexed_argument() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = declare(&mut core, "duplicates", vec![], vec![CoreType::I32]);
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let condition = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let then_value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let else_value = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(join, vec![then_value]),
                    else_target: BlockTarget::new(join, vec![else_value]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder.terminate(returning(vec![joined])).unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let demand = MinimumSemanticDemand::new(&core).unwrap();
        let function_demand = demand.function(function).unwrap();
        assert_eq!(function_demand.requires_value(joined), Some(true));
        assert_eq!(function_demand.requires_value(then_value), Some(true));
        assert_eq!(function_demand.requires_value(else_value), Some(true));
    }

    #[test]
    fn twenty_thousand_value_chain_uses_a_linear_nonrecursive_worklist() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = declare(&mut core, "deep", vec![], vec![CoreType::I32]);
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let mut value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        for _ in 1..20_000 {
            value = builder
                .i32_add_wrapping(value, value, OriginId::UNKNOWN)
                .unwrap();
        }
        builder.terminate(returning(vec![value])).unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let demand = MinimumSemanticDemand::new(&core).unwrap();
        let function_demand = demand.function(function).unwrap();
        assert_eq!(function_demand.instruction_slots(), 20_000);
        assert_eq!(function_demand.value_slots(), 20_000);
        assert_eq!(function_demand.demanded_instructions().count(), 20_000);
        assert_eq!(function_demand.demanded_values().count(), 20_000);
    }

    #[test]
    fn independent_result_is_deterministic_and_matches_completed_runtime_demand() {
        for seed in 0..32 {
            let core = generated_program(seed);
            let first = MinimumSemanticDemand::new(&core).unwrap();
            let second = MinimumSemanticDemand::new(&core).unwrap();
            assert_eq!(format!("{first:#?}"), format!("{second:#?}"));
            assert_matches_runtime_demand(&core, &first);
        }
        let (cycles, ..) = cycle_programs();
        let independent = MinimumSemanticDemand::new(&cycles).unwrap();
        assert_matches_runtime_demand(&cycles, &independent);
    }

    #[test]
    fn missing_definition_is_a_typed_mandatory_error() {
        let mut core = CoreProgram::new();
        let function = declare(&mut core, "decl-only", vec![], vec![]);
        assert!(matches!(
            MinimumSemanticDemand::new(&core),
            Err(MinimumDemandError::MissingDefinition { function: actual })
                if actual == function
        ));
    }

    fn cycle_programs() -> (
        CoreProgram,
        FunctionId,
        [ValueId; 5],
        FunctionId,
        [ValueId; 2],
    ) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let rooted = declare(&mut core, "rooted", vec![], vec![CoreType::I32]);
        let unrooted = declare(&mut core, "unrooted", vec![], vec![]);

        let mut rooted_builder = FunctionBuilder::new(&core, &sources, rooted).unwrap();
        let initial_value = rooted_builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let initial_condition = rooted_builder
            .bool_constant(true, OriginId::UNKNOWN)
            .unwrap();
        let loop_block = rooted_builder.create_block(OriginId::UNKNOWN).unwrap();
        let loop_value = rooted_builder
            .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let loop_condition = rooted_builder
            .append_block_parameter(loop_block, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        let exit = rooted_builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit_value = rooted_builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        rooted_builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    loop_block,
                    vec![initial_value, initial_condition],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        rooted_builder.switch_to_block(loop_block).unwrap();
        rooted_builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: loop_condition,
                    then_target: BlockTarget::new(loop_block, vec![loop_value, loop_condition]),
                    else_target: BlockTarget::new(exit, vec![loop_value]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        rooted_builder.switch_to_block(exit).unwrap();
        rooted_builder
            .terminate(returning(vec![exit_value]))
            .unwrap();
        core.define_function(rooted, rooted_builder.finish().unwrap())
            .unwrap();

        let mut unrooted_builder = FunctionBuilder::new(&core, &sources, unrooted).unwrap();
        let unrooted_initial = unrooted_builder.i32_constant(3, OriginId::UNKNOWN).unwrap();
        let unrooted_loop = unrooted_builder.create_block(OriginId::UNKNOWN).unwrap();
        let unrooted_parameter = unrooted_builder
            .append_block_parameter(unrooted_loop, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        unrooted_builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(unrooted_loop, vec![unrooted_initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        unrooted_builder.switch_to_block(unrooted_loop).unwrap();
        unrooted_builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(unrooted_loop, vec![unrooted_parameter])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(unrooted, unrooted_builder.finish().unwrap())
            .unwrap();

        (
            core,
            rooted,
            [
                initial_value,
                initial_condition,
                loop_value,
                loop_condition,
                exit_value,
            ],
            unrooted,
            [unrooted_initial, unrooted_parameter],
        )
    }

    fn generated_program(seed: u32) -> CoreProgram {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = declare(&mut core, "generated", vec![], vec![CoreType::I32]);
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let mut value = builder
            .i32_constant(i32::try_from(seed).unwrap(), OriginId::UNKNOWN)
            .unwrap();
        for index in 0..(seed % 7) {
            let other = builder
                .i32_constant(i32::try_from(index).unwrap() + 1, OriginId::UNKNOWN)
                .unwrap();
            value = if (seed + index) % 2 == 0 {
                builder
                    .i32_add_wrapping(value, other, OriginId::UNKNOWN)
                    .unwrap()
            } else {
                builder
                    .i32_add_overflowing(value, other, OriginId::UNKNOWN)
                    .unwrap()
                    .0
            };
        }
        for index in 0..(seed % 4) {
            let unused = builder
                .bool_constant(index % 2 == 0, OriginId::UNKNOWN)
                .unwrap();
            builder.bool_not(unused, OriginId::UNKNOWN).unwrap();
        }
        builder.terminate(returning(vec![value])).unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        core
    }

    fn assert_matches_runtime_demand(core: &CoreProgram, independent: &MinimumSemanticDemand) {
        let inventory = SemanticInventory::new(core).unwrap();
        let runtime = RuntimeDemand::for_level(
            core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        assert_eq!(independent.len(), core.len());
        for (function, declaration) in core.functions() {
            let body = declaration.body().unwrap();
            let minimum = independent.function(function).unwrap();
            assert_eq!(minimum.value_slots(), body.value_counts().allocated);
            assert_eq!(
                minimum.instruction_slots(),
                body.instruction_counts().allocated
            );
            for index in 0..body.value_counts().allocated {
                let value = ValueId::from_index(u32::try_from(index).unwrap());
                assert_eq!(
                    minimum.requires_value(value),
                    Some(runtime.requires_value(function, value, &inventory).unwrap()),
                    "value mismatch in {function:?} at {value:?}"
                );
            }
            for index in 0..body.instruction_counts().allocated {
                let instruction = InstId::from_index(u32::try_from(index).unwrap());
                assert_eq!(
                    minimum.requires_instruction(instruction),
                    Some(
                        runtime
                            .requires_instruction(function, instruction, &inventory)
                            .unwrap()
                    ),
                    "instruction mismatch in {function:?} at {instruction:?}"
                );
            }
        }
    }

    fn declare(
        core: &mut CoreProgram,
        name: &str,
        parameters: Vec<CoreType>,
        results: Vec<CoreType>,
    ) -> FunctionId {
        core.declare_function(Some(name), parameters, results, OriginId::UNKNOWN)
            .unwrap()
    }

    fn returning(values: Vec<ValueId>) -> Terminator {
        Terminator::new(TerminatorKind::Return(values), OriginId::UNKNOWN)
    }

    fn defining_instruction(body: &crate::ir::core::FunctionBody, value: ValueId) -> InstId {
        match body.value(value).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => panic!("expected instruction result"),
        }
    }

    fn block_values(builder: &FunctionBuilder<'_>, block: BlockId) -> Vec<ValueId> {
        builder
            .body()
            .block(block)
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect()
    }
}
