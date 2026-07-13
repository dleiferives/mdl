//! Bounded backward semantic demand for Minecraft physical planning.

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;

use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{
    BlockId, CoreProgram, FunctionId, InstId, TerminatorKind, ValueDef, ValueId,
};

use super::MinecraftOptimizationLevel;
use super::analysis::{FunctionSemanticInventory, SemanticInventory};

/// Whether one demand-analysis limit is derived from the immutable input or
/// explicitly selected by an internal caller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeDemandLimit<T> {
    Derived,
    #[allow(
        dead_code,
        reason = "demand unit tests exercise exact private boundaries while production derives its caps"
    )]
    Explicit(T),
}

/// Maximum logical entries in the complete dense demand result.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RuntimeDemandTableLimit(u64);

impl RuntimeDemandTableLimit {
    #[cfg(test)]
    pub(crate) const fn new(entries: u64) -> Self {
        Self(entries)
    }

    const fn get(self) -> u64 {
        self.0
    }
}

/// Maximum successfully processed propagation events.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RuntimeDemandEventLimit(u64);

impl RuntimeDemandEventLimit {
    #[cfg(test)]
    pub(crate) const fn new(events: u64) -> Self {
        Self(events)
    }

    const fn get(self) -> u64 {
        self.0
    }
}

/// Private resource policy for one whole-program demand computation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeDemandLimits {
    tables: RuntimeDemandLimit<RuntimeDemandTableLimit>,
    events: RuntimeDemandLimit<RuntimeDemandEventLimit>,
}

impl RuntimeDemandLimits {
    pub(crate) const fn derived() -> Self {
        Self {
            tables: RuntimeDemandLimit::Derived,
            events: RuntimeDemandLimit::Derived,
        }
    }

    #[cfg(test)]
    pub(crate) const fn with_table_limit(mut self, limit: RuntimeDemandTableLimit) -> Self {
        self.tables = RuntimeDemandLimit::Explicit(limit);
        self
    }

    #[cfg(test)]
    pub(crate) const fn with_event_limit(mut self, limit: RuntimeDemandEventLimit) -> Self {
        self.events = RuntimeDemandLimit::Explicit(limit);
        self
    }
}

impl Default for RuntimeDemandLimits {
    fn default() -> Self {
        Self::derived()
    }
}

/// Stable reason an optional precise demand computation selected all-reachable
/// physicalization instead.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeDemandFallbackReason {
    DenseTables,
    PropagationEvents,
    DerivedBoundOverflow,
}

/// Deterministic whole-program demand counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RuntimeDemandStatistics {
    dense_table_entries: Option<u64>,
    derived_event_upper_bound: Option<u64>,
    events_used: u64,
    seed_attempts: u64,
    first_marks: u64,
    dequeues: u64,
    operand_attempts: u64,
    incoming_edge_argument_attempts: u64,
    demanded_values: u64,
    demanded_instructions: u64,
    maximum_queue_length: u64,
}

impl RuntimeDemandStatistics {
    #[cfg(test)]
    const fn dense_table_entries(self) -> Option<u64> {
        self.dense_table_entries
    }

    #[cfg(test)]
    const fn derived_event_upper_bound(self) -> Option<u64> {
        self.derived_event_upper_bound
    }

    #[cfg(test)]
    const fn events_used(self) -> u64 {
        self.events_used
    }
}

/// A complete precise demand result for one function.
#[derive(Clone, Debug)]
struct FunctionRuntimeDemand {
    demanded_values: Box<[bool]>,
    demanded_instructions: Box<[bool]>,
}

/// Type boundary proving every retained bitset reached the backward fixed point.
#[derive(Clone, Debug)]
pub(crate) struct CompletedRuntimeDemand {
    functions: EntityVec<FunctionId, FunctionRuntimeDemand>,
    #[allow(
        dead_code,
        reason = "exact demand accounting is asserted by tests but is not retained in the final plan"
    )]
    statistics: RuntimeDemandStatistics,
}

/// Complete semantic demand policy consumed by physical planning.
#[derive(Clone, Debug)]
pub(crate) enum RuntimeDemand {
    /// Minecraft optimization is disabled; preserve every reachable entity.
    OptimizationDisabled {
        #[allow(
            dead_code,
            reason = "exact demand accounting is asserted by tests but is not retained in the final plan"
        )]
        statistics: RuntimeDemandStatistics,
    },
    /// Baseline analysis reached its complete backward fixed point.
    Complete(CompletedRuntimeDemand),
    /// Optional analysis stopped; preserve every reachable entity.
    ConservativeFallback {
        #[allow(
            dead_code,
            reason = "the typed fallback is test-observable while production consumes its conservative policy"
        )]
        reason: RuntimeDemandFallbackReason,
        #[allow(
            dead_code,
            reason = "exact demand accounting is asserted by tests but is not retained in the final plan"
        )]
        statistics: RuntimeDemandStatistics,
    },
}

impl RuntimeDemand {
    /// Computes one complete policy or returns a mandatory structural failure.
    pub(crate) fn for_level(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        level: MinecraftOptimizationLevel,
        limits: RuntimeDemandLimits,
    ) -> Result<Self, DemandError> {
        if level == MinecraftOptimizationLevel::None {
            return Ok(Self::optimization_disabled());
        }

        let preflight = match Preflight::new(core, inventory) {
            Ok(preflight) => preflight,
            Err(PreflightFailure::Overflow(statistics)) => {
                return Ok(Self::ConservativeFallback {
                    reason: RuntimeDemandFallbackReason::DerivedBoundOverflow,
                    statistics,
                });
            }
            Err(PreflightFailure::Invariant(error)) => return Err(error),
        };
        let mut statistics = RuntimeDemandStatistics {
            dense_table_entries: Some(preflight.dense_table_entries),
            derived_event_upper_bound: Some(preflight.event_upper_bound),
            ..RuntimeDemandStatistics::default()
        };
        let table_limit = match limits.tables {
            RuntimeDemandLimit::Derived => preflight.dense_table_entries,
            RuntimeDemandLimit::Explicit(limit) => limit.get(),
        };
        if preflight.dense_table_entries > table_limit {
            return Ok(Self::ConservativeFallback {
                reason: RuntimeDemandFallbackReason::DenseTables,
                statistics,
            });
        }
        let event_limit = match limits.events {
            RuntimeDemandLimit::Derived => preflight.event_upper_bound,
            RuntimeDemandLimit::Explicit(limit) => limit.get(),
        };

        let mut solver = Solver::new(core, inventory, event_limit, statistics);
        match solver.solve() {
            Ok(functions) => {
                statistics = solver.statistics;
                solver.verify_completion(&functions, preflight.event_upper_bound)?;
                Ok(Self::Complete(CompletedRuntimeDemand {
                    functions,
                    statistics,
                }))
            }
            Err(SolveFailure::Limit) => Ok(Self::ConservativeFallback {
                reason: RuntimeDemandFallbackReason::PropagationEvents,
                statistics: solver.statistics,
            }),
            Err(SolveFailure::Invariant(error)) => Err(error),
        }
    }

    pub(crate) const fn optimization_disabled() -> Self {
        Self::OptimizationDisabled {
            statistics: RuntimeDemandStatistics {
                dense_table_entries: None,
                derived_event_upper_bound: None,
                events_used: 0,
                seed_attempts: 0,
                first_marks: 0,
                dequeues: 0,
                operand_attempts: 0,
                incoming_edge_argument_attempts: 0,
                demanded_values: 0,
                demanded_instructions: 0,
                maximum_queue_length: 0,
            },
        }
    }

    /// Checks that the computed phase state belongs to the selected lowering policy.
    pub(crate) const fn matches_level(&self, level: MinecraftOptimizationLevel) -> bool {
        matches!(
            (level, self),
            (
                MinecraftOptimizationLevel::None,
                Self::OptimizationDisabled { .. }
            ) | (
                MinecraftOptimizationLevel::Baseline,
                Self::Complete(_) | Self::ConservativeFallback { .. }
            )
        )
    }

    pub(crate) fn requires_value(
        &self,
        function: FunctionId,
        value: ValueId,
        inventory: &SemanticInventory,
    ) -> Result<bool, DemandError> {
        let function_inventory = inventory
            .function(function)
            .ok_or(DemandError::MissingInventory { function })?;
        match self {
            Self::OptimizationDisabled { .. } | Self::ConservativeFallback { .. } => {
                if entity_index(value).is_none_or(|index| {
                    index >= function_inventory.demand_incidences().allocated_values()
                }) {
                    return Err(DemandError::InvalidValue { function, value });
                }
                Ok(function_inventory.contains_value(value))
            }
            Self::Complete(completed) => {
                let function_demand = completed
                    .functions
                    .get(function)
                    .ok_or(DemandError::FunctionAlignment { function })?;
                if function_demand.demanded_values.len()
                    != function_inventory.demand_incidences().allocated_values()
                {
                    return Err(DemandError::DemandTableMismatch { function });
                }
                get_bit(&function_demand.demanded_values, value)
                    .ok_or(DemandError::InvalidValue { function, value })
            }
        }
    }

    pub(crate) fn requires_instruction(
        &self,
        function: FunctionId,
        instruction: InstId,
        inventory: &SemanticInventory,
    ) -> Result<bool, DemandError> {
        let function_inventory = inventory
            .function(function)
            .ok_or(DemandError::MissingInventory { function })?;
        match self {
            Self::OptimizationDisabled { .. } | Self::ConservativeFallback { .. } => {
                if entity_index(instruction).is_none_or(|index| {
                    index
                        >= function_inventory
                            .demand_incidences()
                            .allocated_instructions()
                }) {
                    return Err(DemandError::InvalidInstruction {
                        function,
                        instruction,
                    });
                }
                Ok(function_inventory.contains_instruction(instruction))
            }
            Self::Complete(completed) => {
                let function_demand = completed
                    .functions
                    .get(function)
                    .ok_or(DemandError::FunctionAlignment { function })?;
                if function_demand.demanded_instructions.len()
                    != function_inventory
                        .demand_incidences()
                        .allocated_instructions()
                {
                    return Err(DemandError::DemandTableMismatch { function });
                }
                get_bit(&function_demand.demanded_instructions, instruction).ok_or(
                    DemandError::InvalidInstruction {
                        function,
                        instruction,
                    },
                )
            }
        }
    }

    #[cfg(test)]
    pub(crate) const fn statistics(&self) -> RuntimeDemandStatistics {
        match self {
            Self::OptimizationDisabled { statistics }
            | Self::ConservativeFallback { statistics, .. } => *statistics,
            Self::Complete(completed) => completed.statistics,
        }
    }

    #[cfg(test)]
    pub(crate) const fn completion(&self) -> RuntimeDemandCompletion {
        match self {
            Self::OptimizationDisabled { .. } => RuntimeDemandCompletion::OptimizationDisabled,
            Self::Complete(_) => RuntimeDemandCompletion::Complete,
            Self::ConservativeFallback { reason, .. } => {
                RuntimeDemandCompletion::ConservativeFallback(*reason)
            }
        }
    }
}

/// Externally inspectable completion class without exposing precise bitset storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(test)]
pub(crate) enum RuntimeDemandCompletion {
    OptimizationDisabled,
    Complete,
    ConservativeFallback(RuntimeDemandFallbackReason),
}

#[derive(Clone, Copy, Debug)]
struct Preflight {
    dense_table_entries: u64,
    event_upper_bound: u64,
}

enum PreflightFailure {
    Overflow(RuntimeDemandStatistics),
    Invariant(DemandError),
}

impl From<DemandError> for PreflightFailure {
    fn from(error: DemandError) -> Self {
        Self::Invariant(error)
    }
}

impl Preflight {
    fn new(core: &CoreProgram, inventory: &SemanticInventory) -> Result<Self, PreflightFailure> {
        let mut dense_table_entries = checked_usize_to_u64(core.len())
            .ok_or_else(|| PreflightFailure::Overflow(RuntimeDemandStatistics::default()))?;
        let mut seeds = 0_u64;
        let mut markable_entities = 0_u64;
        let mut instruction_operands = 0_u64;
        let mut edge_arguments = 0_u64;

        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(DemandError::MissingDefinition { function })?;
            let function_inventory = inventory
                .function(function)
                .ok_or(DemandError::MissingInventory { function })?;
            let counts = function_inventory.demand_incidences();
            if counts.allocated_values() != body.value_counts().allocated
                || counts.allocated_instructions() != body.instruction_counts().allocated
                || counts.reachable_values() != function_inventory.reachable_values().len()
                || counts.reachable_instructions()
                    != function_inventory.reachable_instructions().len()
            {
                return Err(DemandError::IncidenceMismatch { function }.into());
            }
            counts
                .reachable_values()
                .checked_add(counts.reachable_instructions())
                .ok_or_else(|| PreflightFailure::Overflow(RuntimeDemandStatistics::default()))?;

            dense_table_entries = checked_add_usize(dense_table_entries, counts.allocated_values())
                .and_then(|value| checked_add_usize(value, counts.allocated_instructions()))
                .ok_or_else(|| PreflightFailure::Overflow(RuntimeDemandStatistics::default()))?;
            seeds = checked_add_usize(seeds, counts.return_operands())
                .and_then(|value| checked_add_usize(value, counts.branch_conditions()))
                .and_then(|value| checked_add_usize(value, counts.non_discardable_instructions()))
                .ok_or_else(|| PreflightFailure::Overflow(RuntimeDemandStatistics::default()))?;
            markable_entities = checked_add_usize(markable_entities, counts.reachable_values())
                .and_then(|value| checked_add_usize(value, counts.reachable_instructions()))
                .ok_or_else(|| PreflightFailure::Overflow(RuntimeDemandStatistics::default()))?;
            instruction_operands =
                checked_add_usize(instruction_operands, counts.instruction_operands()).ok_or_else(
                    || PreflightFailure::Overflow(RuntimeDemandStatistics::default()),
                )?;
            edge_arguments = checked_add_usize(edge_arguments, counts.edge_arguments())
                .ok_or_else(|| PreflightFailure::Overflow(RuntimeDemandStatistics::default()))?;
        }

        let event_upper_bound = derive_event_upper_bound(
            seeds,
            markable_entities,
            instruction_operands,
            edge_arguments,
        )
        .ok_or_else(|| {
            PreflightFailure::Overflow(RuntimeDemandStatistics {
                dense_table_entries: Some(dense_table_entries),
                ..RuntimeDemandStatistics::default()
            })
        })?;
        Ok(Self {
            dense_table_entries,
            event_upper_bound,
        })
    }
}

const fn derive_event_upper_bound(
    seeds: u64,
    markable_entities: u64,
    instruction_operands: u64,
    edge_arguments: u64,
) -> Option<u64> {
    let Some(marks_and_dequeues) = markable_entities.checked_mul(2) else {
        return None;
    };
    let Some(value) = seeds.checked_add(marks_and_dequeues) else {
        return None;
    };
    let Some(value) = value.checked_add(instruction_operands) else {
        return None;
    };
    value.checked_add(edge_arguments)
}

fn checked_usize_to_u64(value: usize) -> Option<u64> {
    u64::try_from(value).ok()
}

fn checked_add_usize(total: u64, value: usize) -> Option<u64> {
    total.checked_add(checked_usize_to_u64(value)?)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkItem {
    Value(ValueId),
    Instruction(InstId),
}

#[derive(Clone, Copy)]
enum EventKind {
    Seed,
    FirstMarkValue,
    FirstMarkInstruction,
    Dequeue,
    Operand,
    IncomingEdgeArgument,
}

struct Solver<'a> {
    core: &'a CoreProgram,
    inventory: &'a SemanticInventory,
    event_limit: u64,
    statistics: RuntimeDemandStatistics,
}

impl<'a> Solver<'a> {
    const fn new(
        core: &'a CoreProgram,
        inventory: &'a SemanticInventory,
        event_limit: u64,
        statistics: RuntimeDemandStatistics,
    ) -> Self {
        Self {
            core,
            inventory,
            event_limit,
            statistics,
        }
    }

    fn solve(&mut self) -> Result<EntityVec<FunctionId, FunctionRuntimeDemand>, SolveFailure> {
        let mut functions = EntityVec::new();
        for (function, declaration) in self.core.functions() {
            let body = declaration
                .body()
                .ok_or(DemandError::MissingDefinition { function })?;
            let inventory = self
                .inventory
                .function(function)
                .ok_or(DemandError::MissingInventory { function })?;
            let counts = inventory.demand_incidences();
            let queue_capacity = counts
                .reachable_values()
                .checked_add(counts.reachable_instructions())
                .ok_or(DemandError::QueueCapacityOverflow { function })?;
            let mut demand = FunctionRuntimeDemand {
                demanded_values: vec![false; counts.allocated_values()].into_boxed_slice(),
                demanded_instructions: vec![false; counts.allocated_instructions()]
                    .into_boxed_slice(),
            };
            let mut queue = VecDeque::with_capacity(queue_capacity);
            self.seed_function(function, body, inventory, &mut demand, &mut queue)?;
            self.drain_function(function, body, inventory, &mut demand, &mut queue)?;
            let inserted = functions
                .push(demand)
                .map_err(|EntityLimitError| DemandError::EntityLimit)?;
            if inserted != function {
                return Err(DemandError::FunctionAlignment { function }.into());
            }
        }
        Ok(functions)
    }

    fn seed_function(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        inventory: &FunctionSemanticInventory,
        demand: &mut FunctionRuntimeDemand,
        queue: &mut VecDeque<WorkItem>,
    ) -> Result<(), SolveFailure> {
        for block in inventory.reachable_blocks().iter().copied() {
            let block_data = body
                .block(block)
                .ok_or(DemandError::InvalidBlock { function, block })?;
            if !inventory.contains_block(block) {
                return Err(DemandError::InvalidBlock { function, block }.into());
            }
            for instruction in block_data.instructions().iter().copied() {
                let instruction_data =
                    body.instruction(instruction)
                        .ok_or(DemandError::InvalidInstruction {
                            function,
                            instruction,
                        })?;
                if !inventory.contains_instruction(instruction) {
                    return Err(DemandError::UnreachableInstruction {
                        function,
                        instruction,
                    }
                    .into());
                }
                if !instruction_data.op().is_trivially_discardable() {
                    self.charge(EventKind::Seed)?;
                    self.mark_instruction(function, inventory, demand, queue, instruction)?;
                }
            }
            let terminator = block_data
                .terminator()
                .ok_or(DemandError::MissingTerminator { function, block })?;
            match terminator.kind() {
                TerminatorKind::Branch { condition, .. } => {
                    self.charge(EventKind::Seed)?;
                    self.mark_value(function, inventory, demand, queue, *condition)?;
                }
                TerminatorKind::Return(values) => {
                    for value in values.iter().copied() {
                        self.charge(EventKind::Seed)?;
                        self.mark_value(function, inventory, demand, queue, value)?;
                    }
                }
                TerminatorKind::Jump(_) | TerminatorKind::Unreachable => {}
            }
        }
        Ok(())
    }

    fn drain_function(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        inventory: &FunctionSemanticInventory,
        demand: &mut FunctionRuntimeDemand,
        queue: &mut VecDeque<WorkItem>,
    ) -> Result<(), SolveFailure> {
        while !queue.is_empty() {
            self.charge(EventKind::Dequeue)?;
            let item = queue
                .pop_front()
                .expect("the demand queue was checked nonempty");
            match item {
                WorkItem::Instruction(instruction) => {
                    let data =
                        body.instruction(instruction)
                            .ok_or(DemandError::InvalidInstruction {
                                function,
                                instruction,
                            })?;
                    if !inventory.contains_instruction(instruction) {
                        return Err(DemandError::UnreachableInstruction {
                            function,
                            instruction,
                        }
                        .into());
                    }
                    for operand in data.operands().iter().copied() {
                        self.charge(EventKind::Operand)?;
                        self.mark_value(function, inventory, demand, queue, operand)?;
                    }
                }
                WorkItem::Value(value) => {
                    let data = body
                        .value(value)
                        .ok_or(DemandError::InvalidValue { function, value })?;
                    if !inventory.contains_value(value) {
                        return Err(DemandError::UnreachableValue { function, value }.into());
                    }
                    match data.definition() {
                        ValueDef::InstResult {
                            instruction,
                            result_index,
                        } => {
                            let result_index = usize::try_from(result_index).map_err(|_| {
                                DemandError::InvalidValueDefinition { function, value }
                            })?;
                            let producer = body.instruction(instruction).ok_or(
                                DemandError::InvalidInstruction {
                                    function,
                                    instruction,
                                },
                            )?;
                            if !inventory.contains_instruction(instruction)
                                || producer.results().get(result_index).copied() != Some(value)
                            {
                                return Err(DemandError::InvalidValueDefinition {
                                    function,
                                    value,
                                }
                                .into());
                            }
                            self.mark_instruction(function, inventory, demand, queue, instruction)?;
                        }
                        ValueDef::BlockParam {
                            block,
                            parameter_index,
                        } => {
                            self.propagate_block_parameter(
                                function,
                                body,
                                inventory,
                                demand,
                                queue,
                                value,
                                block,
                                parameter_index,
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the checked block-parameter transfer keeps every typed phase authority explicit"
    )]
    fn propagate_block_parameter(
        &mut self,
        function: FunctionId,
        body: &crate::ir::core::FunctionBody,
        inventory: &FunctionSemanticInventory,
        demand: &mut FunctionRuntimeDemand,
        queue: &mut VecDeque<WorkItem>,
        value: ValueId,
        block: BlockId,
        parameter_index: u32,
    ) -> Result<(), SolveFailure> {
        let parameter_index = usize::try_from(parameter_index)
            .map_err(|_| DemandError::InvalidValueDefinition { function, value })?;
        let block_data = body
            .block(block)
            .ok_or(DemandError::InvalidBlock { function, block })?;
        if !inventory.contains_block(block)
            || block_data
                .parameters()
                .get(parameter_index)
                .map(crate::ir::core::BlockParam::value)
                != Some(value)
        {
            return Err(DemandError::InvalidValueDefinition { function, value }.into());
        }
        let incoming = inventory
            .incoming_edge_indices(block)
            .ok_or(DemandError::MissingIncomingEdges { function, block })?;
        for edge_index in incoming.iter().copied() {
            let edge =
                inventory
                    .edges()
                    .get(edge_index)
                    .ok_or(DemandError::InvalidIncomingEdge {
                        function,
                        block,
                        edge_index,
                    })?;
            if edge.destination() != block {
                return Err(DemandError::IncomingEdgeDestination {
                    function,
                    block,
                    edge_index,
                    actual: edge.destination(),
                }
                .into());
            }
            if edge.arguments().len() != block_data.parameters().len() {
                return Err(DemandError::IncomingEdgeArity {
                    function,
                    block,
                    edge_index,
                    expected: block_data.parameters().len(),
                    actual: edge.arguments().len(),
                }
                .into());
            }
            let argument = edge.arguments()[parameter_index];
            self.charge(EventKind::IncomingEdgeArgument)?;
            self.mark_value(function, inventory, demand, queue, argument)?;
        }
        Ok(())
    }

    fn mark_value(
        &mut self,
        function: FunctionId,
        inventory: &FunctionSemanticInventory,
        demand: &mut FunctionRuntimeDemand,
        queue: &mut VecDeque<WorkItem>,
        value: ValueId,
    ) -> Result<(), SolveFailure> {
        if !inventory.contains_value(value) {
            return Err(DemandError::UnreachableValue { function, value }.into());
        }
        let index = entity_index(value).ok_or(DemandError::InvalidValue { function, value })?;
        let marked = demand
            .demanded_values
            .get(index)
            .copied()
            .ok_or(DemandError::InvalidValue { function, value })?;
        if marked {
            return Ok(());
        }
        self.charge(EventKind::FirstMarkValue)?;
        demand.demanded_values[index] = true;
        queue.push_back(WorkItem::Value(value));
        self.record_queue_length(queue.len())?;
        Ok(())
    }

    fn mark_instruction(
        &mut self,
        function: FunctionId,
        inventory: &FunctionSemanticInventory,
        demand: &mut FunctionRuntimeDemand,
        queue: &mut VecDeque<WorkItem>,
        instruction: InstId,
    ) -> Result<(), SolveFailure> {
        if !inventory.contains_instruction(instruction) {
            return Err(DemandError::UnreachableInstruction {
                function,
                instruction,
            }
            .into());
        }
        let index = entity_index(instruction).ok_or(DemandError::InvalidInstruction {
            function,
            instruction,
        })?;
        let marked = demand.demanded_instructions.get(index).copied().ok_or(
            DemandError::InvalidInstruction {
                function,
                instruction,
            },
        )?;
        if marked {
            return Ok(());
        }
        self.charge(EventKind::FirstMarkInstruction)?;
        demand.demanded_instructions[index] = true;
        queue.push_back(WorkItem::Instruction(instruction));
        self.record_queue_length(queue.len())?;
        Ok(())
    }

    fn charge(&mut self, kind: EventKind) -> Result<(), SolveFailure> {
        if self.statistics.events_used == self.event_limit {
            return Err(SolveFailure::Limit);
        }
        self.statistics.events_used += 1;
        match kind {
            EventKind::Seed => self.statistics.seed_attempts += 1,
            EventKind::FirstMarkValue => {
                self.statistics.first_marks += 1;
                self.statistics.demanded_values += 1;
            }
            EventKind::FirstMarkInstruction => {
                self.statistics.first_marks += 1;
                self.statistics.demanded_instructions += 1;
            }
            EventKind::Dequeue => self.statistics.dequeues += 1,
            EventKind::Operand => self.statistics.operand_attempts += 1,
            EventKind::IncomingEdgeArgument => {
                self.statistics.incoming_edge_argument_attempts += 1;
            }
        }
        Ok(())
    }

    fn record_queue_length(&mut self, length: usize) -> Result<(), DemandError> {
        let length = u64::try_from(length).map_err(|_| DemandError::StatisticsOverflow)?;
        self.statistics.maximum_queue_length = self.statistics.maximum_queue_length.max(length);
        Ok(())
    }

    fn verify_completion(
        &self,
        functions: &EntityVec<FunctionId, FunctionRuntimeDemand>,
        event_upper_bound: u64,
    ) -> Result<(), DemandError> {
        let statistics = self.statistics;
        if statistics.events_used
            != statistics.seed_attempts
                + statistics.first_marks
                + statistics.dequeues
                + statistics.operand_attempts
                + statistics.incoming_edge_argument_attempts
            || statistics.first_marks != statistics.dequeues
            || statistics.first_marks
                != statistics.demanded_values + statistics.demanded_instructions
            || statistics.events_used > event_upper_bound
        {
            return Err(DemandError::StatisticsMismatch);
        }
        for (function, function_demand) in functions.iter() {
            let inventory = self
                .inventory
                .function(function)
                .ok_or(DemandError::MissingInventory { function })?;
            for (index, demanded) in function_demand.demanded_values.iter().copied().enumerate() {
                if demanded {
                    let index =
                        u32::try_from(index).map_err(|_| DemandError::StatisticsOverflow)?;
                    let value = ValueId::from_index(index);
                    if !inventory.contains_value(value) {
                        return Err(DemandError::UnreachableValue { function, value });
                    }
                }
            }
            for (index, demanded) in function_demand
                .demanded_instructions
                .iter()
                .copied()
                .enumerate()
            {
                if demanded {
                    let index =
                        u32::try_from(index).map_err(|_| DemandError::StatisticsOverflow)?;
                    let instruction = InstId::from_index(index);
                    if !inventory.contains_instruction(instruction) {
                        return Err(DemandError::UnreachableInstruction {
                            function,
                            instruction,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

enum SolveFailure {
    Limit,
    Invariant(DemandError),
}

impl From<DemandError> for SolveFailure {
    fn from(error: DemandError) -> Self {
        Self::Invariant(error)
    }
}

fn entity_index(id: impl EntityId) -> Option<usize> {
    usize::try_from(id.index()).ok()
}

fn get_bit(id_bits: &[bool], id: impl EntityId) -> Option<bool> {
    id_bits.get(entity_index(id)?).copied()
}

/// Mandatory mismatch between verified Core, its inventory, and demand propagation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DemandError {
    MissingDefinition {
        function: FunctionId,
    },
    MissingInventory {
        function: FunctionId,
    },
    FunctionAlignment {
        function: FunctionId,
    },
    InvalidBlock {
        function: FunctionId,
        block: BlockId,
    },
    MissingTerminator {
        function: FunctionId,
        block: BlockId,
    },
    InvalidInstruction {
        function: FunctionId,
        instruction: InstId,
    },
    UnreachableInstruction {
        function: FunctionId,
        instruction: InstId,
    },
    InvalidValue {
        function: FunctionId,
        value: ValueId,
    },
    UnreachableValue {
        function: FunctionId,
        value: ValueId,
    },
    InvalidValueDefinition {
        function: FunctionId,
        value: ValueId,
    },
    MissingIncomingEdges {
        function: FunctionId,
        block: BlockId,
    },
    InvalidIncomingEdge {
        function: FunctionId,
        block: BlockId,
        edge_index: usize,
    },
    IncomingEdgeDestination {
        function: FunctionId,
        block: BlockId,
        edge_index: usize,
        actual: BlockId,
    },
    IncomingEdgeArity {
        function: FunctionId,
        block: BlockId,
        edge_index: usize,
        expected: usize,
        actual: usize,
    },
    IncidenceMismatch {
        function: FunctionId,
    },
    DemandTableMismatch {
        function: FunctionId,
    },
    QueueCapacityOverflow {
        function: FunctionId,
    },
    EntityLimit,
    StatisticsOverflow,
    StatisticsMismatch,
}

impl fmt::Display for DemandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for DemandError {}

#[cfg(test)]
mod tests {
    use super::{
        DemandError, RuntimeDemand, RuntimeDemandCompletion, RuntimeDemandEventLimit,
        RuntimeDemandFallbackReason, RuntimeDemandLimits, RuntimeDemandTableLimit,
        derive_event_upper_bound,
    };
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, InstId, Terminator,
        TerminatorKind, ValueDef, ValueId,
    };
    use crate::lower::minecraft::MinecraftOptimizationLevel;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::source::{OriginId, SourceContext};

    fn analyze(
        program: &CoreProgram,
        level: MinecraftOptimizationLevel,
        limits: RuntimeDemandLimits,
    ) -> (SemanticInventory, RuntimeDemand) {
        let inventory = SemanticInventory::new(program).unwrap();
        let demand = RuntimeDemand::for_level(program, &inventory, level, limits).unwrap();
        (inventory, demand)
    }

    fn defining_instruction(program: &CoreProgram, function: FunctionId, value: ValueId) -> InstId {
        let body = program.function(function).unwrap().body().unwrap();
        match body.value(value).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => panic!("expected an instruction result"),
        }
    }

    fn one_returning_constant() -> (CoreProgram, FunctionId, ValueId, InstId) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("constant"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let value = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let instruction = match body.value(value).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => unreachable!(),
        };
        program.define_function(function, body).unwrap();
        (program, function, value, instruction)
    }

    #[test]
    fn optimization_disabled_is_explicit_and_all_reachable() {
        let (program, function, value, instruction) = one_returning_constant();
        let (inventory, demand) = analyze(
            &program,
            MinecraftOptimizationLevel::None,
            RuntimeDemandLimits::derived()
                .with_table_limit(RuntimeDemandTableLimit::new(0))
                .with_event_limit(RuntimeDemandEventLimit::new(0)),
        );

        assert_eq!(
            demand.completion(),
            RuntimeDemandCompletion::OptimizationDisabled
        );
        assert!(demand.requires_value(function, value, &inventory).unwrap());
        assert!(
            demand
                .requires_instruction(function, instruction, &inventory)
                .unwrap()
        );
        assert_eq!(demand.statistics().events_used(), 0);
    }

    #[test]
    fn dead_pure_work_completes_at_zero_actual_fuel() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("dead"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let value = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        let instruction = defining_instruction(&program, function, value);

        let (inventory, demand) = analyze(
            &program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived().with_event_limit(RuntimeDemandEventLimit::new(0)),
        );

        assert_eq!(demand.completion(), RuntimeDemandCompletion::Complete);
        assert!(!demand.requires_value(function, value, &inventory).unwrap());
        assert!(
            !demand
                .requires_instruction(function, instruction, &inventory)
                .unwrap()
        );
        assert_eq!(demand.statistics().events_used(), 0);
        assert_eq!(demand.statistics().derived_event_upper_bound(), Some(4));
    }

    #[test]
    fn returning_constant_has_exact_dense_and_event_boundaries() {
        let (program, function, value, instruction) = one_returning_constant();

        let (_, table_fallback) = analyze(
            &program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived().with_table_limit(RuntimeDemandTableLimit::new(2)),
        );
        assert_eq!(
            table_fallback.completion(),
            RuntimeDemandCompletion::ConservativeFallback(RuntimeDemandFallbackReason::DenseTables)
        );
        assert_eq!(table_fallback.statistics().dense_table_entries(), Some(3));

        let (_, event_fallback) = analyze(
            &program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived()
                .with_table_limit(RuntimeDemandTableLimit::new(3))
                .with_event_limit(RuntimeDemandEventLimit::new(4)),
        );
        assert_eq!(
            event_fallback.completion(),
            RuntimeDemandCompletion::ConservativeFallback(
                RuntimeDemandFallbackReason::PropagationEvents
            )
        );
        assert_eq!(event_fallback.statistics().events_used(), 4);

        let (inventory, complete) = analyze(
            &program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived()
                .with_table_limit(RuntimeDemandTableLimit::new(3))
                .with_event_limit(RuntimeDemandEventLimit::new(5)),
        );
        assert_eq!(complete.completion(), RuntimeDemandCompletion::Complete);
        assert_eq!(complete.statistics().events_used(), 5);
        assert_eq!(complete.statistics().derived_event_upper_bound(), Some(5));
        assert!(
            complete
                .requires_value(function, value, &inventory)
                .unwrap()
        );
        assert!(
            complete
                .requires_instruction(function, instruction, &inventory)
                .unwrap()
        );
    }

    #[test]
    fn otherwise_uncalled_function_return_remains_an_external_demand_root() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let caller = program
            .declare_function(Some("caller"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let uncalled = program
            .declare_function(
                Some("uncalled"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut caller_builder = FunctionBuilder::new(&program, &sources, caller).unwrap();
        caller_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(caller, caller_builder.finish().unwrap())
            .unwrap();

        let mut uncalled_builder = FunctionBuilder::new(&program, &sources, uncalled).unwrap();
        let returned = uncalled_builder
            .i32_constant(41, OriginId::UNKNOWN)
            .unwrap();
        uncalled_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![returned]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(uncalled, uncalled_builder.finish().unwrap())
            .unwrap();
        let instruction = defining_instruction(&program, uncalled, returned);

        let (inventory, demand) = analyze(
            &program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        );

        // A-014: every current function ABI remains externally observable even
        // when no internal call edge reaches it.
        assert!(inventory.function(caller).unwrap().call_sites().is_empty());
        assert!(
            inventory
                .function(uncalled)
                .unwrap()
                .call_sites()
                .is_empty()
        );
        assert!(
            demand
                .requires_value(uncalled, returned, &inventory)
                .unwrap()
        );
        assert!(
            demand
                .requires_instruction(uncalled, instruction, &inventory)
                .unwrap()
        );
    }

    fn overflow_mask_program(
        demand_sum: bool,
        demand_overflow: bool,
    ) -> (CoreProgram, FunctionId, [ValueId; 4], InstId) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let result_types = match (demand_sum, demand_overflow) {
            (false, false) => vec![],
            (true, false) => vec![CoreType::I32],
            (false, true) => vec![CoreType::Bool],
            (true, true) => vec![CoreType::I32, CoreType::Bool],
        };
        let function = program
            .declare_function(Some("overflow"), vec![], result_types, OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let left = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (sum, overflow) = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap();
        let returned = match (demand_sum, demand_overflow) {
            (false, false) => vec![],
            (true, false) => vec![sum],
            (false, true) => vec![overflow],
            (true, true) => vec![sum, overflow],
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(returned),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let instruction = match body.value(sum).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => unreachable!(),
        };
        program.define_function(function, body).unwrap();
        (program, function, [left, right, sum, overflow], instruction)
    }

    #[test]
    fn overflowing_add_results_are_semantically_independent() {
        for (demand_sum, demand_overflow) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (program, function, [left, right, sum, overflow], instruction) =
                overflow_mask_program(demand_sum, demand_overflow);
            let (inventory, demand) = analyze(
                &program,
                MinecraftOptimizationLevel::Baseline,
                RuntimeDemandLimits::derived(),
            );

            assert_eq!(
                demand.requires_value(function, sum, &inventory).unwrap(),
                demand_sum
            );
            assert_eq!(
                demand
                    .requires_value(function, overflow, &inventory)
                    .unwrap(),
                demand_overflow
            );
            let retained = demand_sum || demand_overflow;
            assert_eq!(
                demand
                    .requires_instruction(function, instruction, &inventory)
                    .unwrap(),
                retained
            );
            assert_eq!(
                demand.requires_value(function, left, &inventory).unwrap(),
                retained
            );
            assert_eq!(
                demand.requires_value(function, right, &inventory).unwrap(),
                retained
            );
        }
    }

    struct CallFixture {
        program: CoreProgram,
        caller_used: FunctionId,
        caller_unused: FunctionId,
        used_arguments: [ValueId; 2],
        used_results: [ValueId; 2],
        used_call: InstId,
        unused_results: [ValueId; 2],
        unused_call: InstId,
    }

    fn call_fixture() -> CallFixture {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let callee = program
            .declare_function(
                Some("callee"),
                vec![CoreType::I32, CoreType::Bool],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let caller_used = program
            .declare_function(
                Some("caller_used"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let caller_unused = program
            .declare_function(Some("caller_unused"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();

        let mut callee_builder = FunctionBuilder::new(&program, &sources, callee).unwrap();
        let parameters = callee_builder
            .body()
            .block(callee_builder.entry_block())
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect::<Vec<_>>();
        callee_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(parameters),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(callee, callee_builder.finish().unwrap())
            .unwrap();

        let mut used_builder = FunctionBuilder::new(&program, &sources, caller_used).unwrap();
        let integer = used_builder.i32_constant(9, OriginId::UNKNOWN).unwrap();
        let boolean = used_builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let results = used_builder
            .call(callee, vec![integer, boolean], OriginId::UNKNOWN)
            .unwrap();
        let used_results = [results[0], results[1]];
        used_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![results[1]]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let used_body = used_builder.finish().unwrap();
        let used_call = match used_body.value(results[0]).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => unreachable!(),
        };
        program.define_function(caller_used, used_body).unwrap();

        let mut unused_builder = FunctionBuilder::new(&program, &sources, caller_unused).unwrap();
        let integer_unused = unused_builder.i32_constant(11, OriginId::UNKNOWN).unwrap();
        let boolean_unused = unused_builder
            .bool_constant(false, OriginId::UNKNOWN)
            .unwrap();
        let results = unused_builder
            .call(
                callee,
                vec![integer_unused, boolean_unused],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let unused_results = [results[0], results[1]];
        unused_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let unused_body = unused_builder.finish().unwrap();
        let unused_call = match unused_body.value(results[0]).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => unreachable!(),
        };
        program.define_function(caller_unused, unused_body).unwrap();

        CallFixture {
            program,
            caller_used,
            caller_unused,
            used_arguments: [integer, boolean],
            used_results,
            used_call,
            unused_results,
            unused_call,
        }
    }

    #[test]
    fn calls_are_retained_without_fabricating_result_demand() {
        let fixture = call_fixture();
        let (inventory, demand) = analyze(
            &fixture.program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        );

        assert!(
            demand
                .requires_instruction(fixture.caller_used, fixture.used_call, &inventory)
                .unwrap()
        );
        assert!(
            demand
                .requires_instruction(fixture.caller_unused, fixture.unused_call, &inventory)
                .unwrap()
        );
        assert!(fixture.used_arguments.into_iter().all(|value| {
            demand
                .requires_value(fixture.caller_used, value, &inventory)
                .unwrap()
        }));
        assert!(
            !demand
                .requires_value(fixture.caller_used, fixture.used_results[0], &inventory)
                .unwrap()
        );
        assert!(
            demand
                .requires_value(fixture.caller_used, fixture.used_results[1], &inventory)
                .unwrap()
        );
        assert!(fixture.unused_results.into_iter().all(|value| {
            !demand
                .requires_value(fixture.caller_unused, value, &inventory)
                .unwrap()
        }));
    }

    fn block_parameter_cycle(rooted: bool) -> (CoreProgram, FunctionId, ValueId, ValueId, InstId) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("cycle"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let initial = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let loop_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
            .append_block_parameter(loop_block, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(loop_block, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(loop_block).unwrap();
        let loop_terminator = if rooted {
            TerminatorKind::Branch {
                condition: parameter,
                then_target: BlockTarget::new(loop_block, vec![parameter]),
                else_target: BlockTarget::new(exit, vec![]),
            }
        } else {
            TerminatorKind::Jump(BlockTarget::new(loop_block, vec![parameter]))
        };
        builder
            .terminate(Terminator::new(loop_terminator, OriginId::UNKNOWN))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let body = builder.finish().unwrap();
        let initial_instruction = match body.value(initial).unwrap().definition() {
            ValueDef::InstResult { instruction, .. } => instruction,
            ValueDef::BlockParam { .. } => unreachable!(),
        };
        program.define_function(function, body).unwrap();
        let _ = entry;
        (program, function, initial, parameter, initial_instruction)
    }

    #[test]
    fn block_parameter_cycles_are_root_sensitive_and_terminate() {
        for rooted in [false, true] {
            let (program, function, initial, parameter, initial_instruction) =
                block_parameter_cycle(rooted);
            let (inventory, demand) = analyze(
                &program,
                MinecraftOptimizationLevel::Baseline,
                RuntimeDemandLimits::derived(),
            );
            assert_eq!(
                demand
                    .requires_value(function, parameter, &inventory)
                    .unwrap(),
                rooted
            );
            assert_eq!(
                demand
                    .requires_value(function, initial, &inventory)
                    .unwrap(),
                rooted
            );
            assert_eq!(
                demand
                    .requires_instruction(function, initial_instruction, &inventory)
                    .unwrap(),
                rooted
            );
            assert_eq!(
                demand.statistics().incoming_edge_argument_attempts,
                if rooted { 2 } else { 0 }
            );
        }
    }

    #[test]
    fn duplicate_occurrences_charge_once_per_occurrence_and_mark_once() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("duplicates"),
                vec![],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(value, value, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum, sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let (_, demand) = analyze(
            &program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        );
        let statistics = demand.statistics();
        assert_eq!(statistics.seed_attempts, 2);
        assert_eq!(statistics.operand_attempts, 2);
        assert_eq!(statistics.demanded_values, 2);
        assert_eq!(statistics.demanded_instructions, 2);
        assert_eq!(statistics.events_used, 12);
    }

    struct HistoryFixture {
        program: CoreProgram,
        function: FunctionId,
        reachable: ValueId,
        attached_unreachable: ValueId,
        detached: ValueId,
    }

    fn history_fixture() -> HistoryFixture {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("history"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let reachable = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let attached_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let detached_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![reachable]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(attached_block).unwrap();
        let attached_unreachable = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![attached_unreachable]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(detached_block).unwrap();
        let detached = builder.i32_constant(3, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![detached]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        body.block_order.retain(|block| *block != detached_block);
        program.define_function(function, body).unwrap();
        HistoryFixture {
            program,
            function,
            reachable,
            attached_unreachable,
            detached,
        }
    }

    #[test]
    fn dense_capacity_includes_all_allocated_history_but_demand_does_not() {
        let fixture = history_fixture();
        let (inventory, fallback) = analyze(
            &fixture.program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived().with_table_limit(RuntimeDemandTableLimit::new(6)),
        );
        assert_eq!(fallback.statistics().dense_table_entries(), Some(7));
        assert_eq!(
            fallback.completion(),
            RuntimeDemandCompletion::ConservativeFallback(RuntimeDemandFallbackReason::DenseTables)
        );
        assert!(
            fallback
                .requires_value(fixture.function, fixture.reachable, &inventory)
                .unwrap()
        );
        assert!(
            !fallback
                .requires_value(fixture.function, fixture.attached_unreachable, &inventory)
                .unwrap()
        );
        assert!(
            !fallback
                .requires_value(fixture.function, fixture.detached, &inventory)
                .unwrap()
        );

        let (inventory, complete) = analyze(
            &fixture.program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived().with_table_limit(RuntimeDemandTableLimit::new(7)),
        );
        assert_eq!(complete.completion(), RuntimeDemandCompletion::Complete);
        assert!(
            complete
                .requires_value(fixture.function, fixture.reachable, &inventory)
                .unwrap()
        );
        assert!(
            !complete
                .requires_value(fixture.function, fixture.attached_unreachable, &inventory)
                .unwrap()
        );
        assert!(
            !complete
                .requires_value(fixture.function, fixture.detached, &inventory)
                .unwrap()
        );
    }

    #[test]
    fn a_late_event_limit_discards_every_earlier_precise_function() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let mut first_value = None;
        let mut first_function = None;
        for name in ["first", "second"] {
            let function = program
                .declare_function(Some(name), vec![], vec![CoreType::I32], OriginId::UNKNOWN)
                .unwrap();
            let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
            let value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![value]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            program
                .define_function(function, builder.finish().unwrap())
                .unwrap();
            first_function.get_or_insert(function);
            first_value.get_or_insert(value);
        }
        let inventory = SemanticInventory::new(&program).unwrap();
        let demand = RuntimeDemand::for_level(
            &program,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived().with_event_limit(RuntimeDemandEventLimit::new(9)),
        )
        .unwrap();

        assert_eq!(
            demand.completion(),
            RuntimeDemandCompletion::ConservativeFallback(
                RuntimeDemandFallbackReason::PropagationEvents
            )
        );
        assert_eq!(demand.statistics().events_used(), 9);
        assert!(
            demand
                .requires_value(first_function.unwrap(), first_value.unwrap(), &inventory)
                .unwrap()
        );
    }

    #[test]
    fn stale_core_definition_is_a_mandatory_typed_failure() {
        let (mut program, function, value, instruction) = one_returning_constant();
        let inventory = SemanticInventory::new(&program).unwrap();
        let body = program
            .function_mut(function)
            .unwrap()
            .body
            .as_mut()
            .unwrap();
        body.values.get_mut(value).unwrap().definition = ValueDef::InstResult {
            instruction,
            result_index: 99,
        };

        assert_eq!(
            RuntimeDemand::for_level(
                &program,
                &inventory,
                MinecraftOptimizationLevel::Baseline,
                RuntimeDemandLimits::derived(),
            )
            .unwrap_err(),
            DemandError::InvalidValueDefinition { function, value }
        );
    }

    #[test]
    fn checked_event_bound_reports_overflow_without_giant_ir() {
        assert_eq!(derive_event_upper_bound(0, u64::MAX, 0, 0), None);
        assert_eq!(
            derive_event_upper_bound(u64::MAX - 4, 1, 1, 1),
            Some(u64::MAX)
        );
        assert_eq!(derive_event_upper_bound(u64::MAX - 3, 1, 1, 1), None);
    }

    #[test]
    fn twenty_thousand_value_chain_is_linear_and_deterministic() {
        const DEPTH: usize = 20_000;
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("deep"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let first = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let mut value = first;
        for _ in 0..DEPTH {
            value = builder.bool_not(value, OriginId::UNKNOWN).unwrap();
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        let first_instruction = defining_instruction(&program, function, first);
        let last_instruction = defining_instruction(&program, function, value);
        let inventory = SemanticInventory::new(&program).unwrap();

        let first_run = RuntimeDemand::for_level(
            &program,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let second_run = RuntimeDemand::for_level(
            &program,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();

        assert_eq!(first_run.statistics(), second_run.statistics());
        assert_eq!(first_run.statistics().demanded_values, DEPTH as u64 + 1);
        assert_eq!(
            first_run.statistics().demanded_instructions,
            DEPTH as u64 + 1
        );
        assert_eq!(first_run.statistics().maximum_queue_length, 1);
        for instruction in [first_instruction, last_instruction] {
            assert!(
                first_run
                    .requires_instruction(function, instruction, &inventory)
                    .unwrap()
            );
            assert!(
                second_run
                    .requires_instruction(function, instruction, &inventory)
                    .unwrap()
            );
        }
    }

    #[test]
    fn queries_reject_ids_outside_the_allocated_domain() {
        let (program, function, _, _) = one_returning_constant();
        let (inventory, demand) = analyze(
            &program,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        );
        let invalid = ValueId::from_index(99);

        assert_eq!(
            demand.requires_value(function, invalid, &inventory),
            Err(DemandError::InvalidValue {
                function,
                value: invalid
            })
        );
    }
}
