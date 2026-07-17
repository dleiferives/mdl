//! Bounded sparse semantic liveness for Minecraft home coalescing.
//!
//! Liveness is an optional optimization proof. Every published function result is
//! either a complete fixed point with complete segments, or an explicit request for
//! the distinct-home fallback. Incomplete facts never cross this module boundary.

use std::collections::VecDeque;
use std::fmt;

use crate::entity::{EntityId, EntityLimitError, EntityVec};
use crate::ir::core::{
    BlockId, CoreProgram, FunctionBody, FunctionId, InstId, TerminatorKind, ValueId,
};

use super::MinecraftOptimizationLevel;
use super::analysis::{FunctionSemanticInventory, SemanticInventory};
use super::demand::{DemandError, RuntimeDemand};

/// A block-local liveness boundary.
///
/// For a block with `n` instructions, block parameters are defined at zero,
/// instruction `i` uses operands at `2i + 1` and defines all results at `2i + 2`,
/// terminator/edge operands are used at `2n + 1`, and `2n + 2` is the exclusive
/// block end. This makes an ordinary last input use and result definition adjacent
/// but nonoverlapping; target recipe timing is checked separately.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BlockPoint(u64);

impl BlockPoint {
    const ENTRY: Self = Self(0);

    const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(point) => Some(Self(point)),
            None => None,
        }
    }
}

/// One nonempty half-open range in one reachable block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LiveSegment {
    block: BlockId,
    block_ordinal: usize,
    from: BlockPoint,
    to: BlockPoint,
}

impl LiveSegment {
    pub(crate) const fn coordinates(self) -> (usize, u64, u64) {
        (self.block_ordinal, self.from.0, self.to.0)
    }

    #[cfg(test)]
    pub(crate) const fn block(self) -> BlockId {
        self.block
    }

    #[cfg(test)]
    pub(crate) const fn from(self) -> BlockPoint {
        self.from
    }

    #[cfg(test)]
    pub(crate) const fn to(self) -> BlockPoint {
        self.to
    }

    fn overlaps(self, other: Self) -> bool {
        self.block == other.block && self.from < other.to && other.from < self.to
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SegmentRange {
    start: usize,
    end: usize,
}

/// Exact converged entry/end facts for one block.
///
/// `live_in` is the state immediately after simultaneous block-parameter
/// definitions. `live_out` is the state immediately before the terminator and its
/// edge-specific simultaneous argument uses.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BlockLiveness {
    live_in: Box<[ValueId]>,
    live_out: Box<[ValueId]>,
}

#[cfg(test)]
impl BlockLiveness {
    pub(crate) fn live_in(&self) -> &[ValueId] {
        &self.live_in
    }

    pub(crate) fn live_out(&self) -> &[ValueId] {
        &self.live_out
    }
}

/// Complete sparse liveness for one function.
#[derive(Clone, Debug)]
pub(crate) struct CompletedFunctionLiveness {
    /// Retaining the fixed-point sets would duplicate the segment representation in
    /// production. Unit tests keep them only as an independent boundary oracle.
    #[cfg(test)]
    blocks: Box<[Option<BlockLiveness>]>,
    value_segment_ranges: Box<[Option<SegmentRange>]>,
    segments: Box<[LiveSegment]>,
    statistics: FunctionLivenessStatistics,
}

impl CompletedFunctionLiveness {
    #[cfg(test)]
    pub(crate) fn block(&self, block: BlockId) -> Option<&BlockLiveness> {
        entity_index(block)
            .and_then(|index| self.blocks.get(index))
            .and_then(Option::as_ref)
    }

    pub(crate) fn segments(&self, value: ValueId) -> &[LiveSegment] {
        let Some(range) = entity_index(value)
            .and_then(|index| self.value_segment_ranges.get(index))
            .copied()
            .flatten()
        else {
            return &[];
        };
        self.segments.get(range.start..range.end).unwrap_or(&[])
    }

    pub(crate) fn values_interfere(&self, left: ValueId, right: ValueId) -> bool {
        if left == right {
            return true;
        }
        segments_intersect(self.segments(left), self.segments(right))
    }

    /// Whether the current realization of `value` must survive one instruction's
    /// result-definition boundary. This is the exact caller-live predicate used by
    /// recursive spill planning.
    pub(crate) fn is_live_after_instruction(
        &self,
        body: &FunctionBody,
        instruction: InstId,
        value: ValueId,
    ) -> bool {
        let Some((block, ordinal)) = body.block_order().iter().copied().find_map(|block| {
            body.block(block).and_then(|data| {
                data.instructions()
                    .iter()
                    .position(|candidate| *candidate == instruction)
                    .map(|ordinal| (block, ordinal))
            })
        }) else {
            return false;
        };
        let Some(point) = u64::try_from(ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_mul(2))
            .and_then(|point| point.checked_add(2))
            .map(BlockPoint)
        else {
            return false;
        };
        self.segments(value)
            .iter()
            .any(|segment| segment.block == block && segment.from <= point && point < segment.to)
    }

    pub(crate) const fn statistics(&self) -> FunctionLivenessStatistics {
        self.statistics
    }
}

/// Stable reason one function selected distinct homes instead of using partial facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LivenessFallbackReason {
    PropagationEvents,
    RetainedSegments,
    DerivedBoundOverflow,
}

impl LivenessFallbackReason {
    /// Stable machine-readable spelling used by deterministic reports.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::PropagationEvents => "propagation-events",
            Self::RetainedSegments => "retained-segments",
            Self::DerivedBoundOverflow => "derived-bound-overflow",
        }
    }
}

impl fmt::Display for LivenessFallbackReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// Complete-or-fallback result for one function.
#[derive(Clone, Debug)]
pub(crate) enum FunctionLiveness {
    Complete(CompletedFunctionLiveness),
    ConservativeFallback {
        reason: LivenessFallbackReason,
        statistics: FunctionLivenessStatistics,
    },
}

impl FunctionLiveness {
    pub(crate) const fn completed(&self) -> Option<&CompletedFunctionLiveness> {
        match self {
            Self::Complete(completed) => Some(completed),
            Self::ConservativeFallback { .. } => None,
        }
    }

    pub(crate) const fn fallback_reason(&self) -> Option<LivenessFallbackReason> {
        match self {
            Self::Complete(_) => None,
            Self::ConservativeFallback { reason, .. } => Some(*reason),
        }
    }

    pub(crate) const fn statistics(&self) -> FunctionLivenessStatistics {
        match self {
            Self::Complete(completed) => completed.statistics(),
            Self::ConservativeFallback { statistics, .. } => *statistics,
        }
    }
}

/// Immutable Baseline liveness results aligned with Core functions.
#[derive(Clone, Debug)]
pub(crate) struct LivenessResult {
    level: MinecraftOptimizationLevel,
    functions: EntityVec<FunctionId, FunctionLiveness>,
}

impl LivenessResult {
    #[allow(
        dead_code,
        reason = "baseline-specific tests and coalescing helpers retain the narrower constructor"
    )]
    pub(crate) fn for_baseline(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        demand: &RuntimeDemand,
        limits: LivenessLimits,
    ) -> Result<Self, LivenessError> {
        Self::for_level(
            core,
            inventory,
            demand,
            MinecraftOptimizationLevel::Baseline,
            limits,
        )
    }

    /// Computes semantic liveness for physical call-boundary planning at either
    /// lowering policy. `None` still uses distinct score homes; its liveness is
    /// needed only to avoid spilling undefined/dead homes at recursive calls.
    pub(crate) fn for_level(
        core: &CoreProgram,
        inventory: &SemanticInventory,
        demand: &RuntimeDemand,
        level: MinecraftOptimizationLevel,
        limits: LivenessLimits,
    ) -> Result<Self, LivenessError> {
        if !demand.matches_level(level) {
            return Err(LivenessError::DemandPolicyMismatch);
        }
        let mut functions = EntityVec::new();
        for (function, declaration) in core.functions() {
            let body = declaration
                .body()
                .ok_or(LivenessError::MissingDefinition { function })?;
            let function_inventory = inventory
                .function(function)
                .ok_or(LivenessError::MissingInventory { function })?;
            let analyzed = analyze_function(
                function,
                body,
                function_inventory,
                inventory,
                demand,
                limits,
            )?;
            let inserted = functions
                .push(analyzed)
                .map_err(|EntityLimitError| LivenessError::EntityLimit)?;
            debug_assert_eq!(inserted, function);
        }
        Ok(Self { level, functions })
    }

    pub(crate) fn function(&self, function: FunctionId) -> Option<&FunctionLiveness> {
        self.functions.get(function)
    }

    pub(crate) fn len(&self) -> usize {
        self.functions.len()
    }

    pub(crate) fn matches_level(&self, level: MinecraftOptimizationLevel) -> bool {
        self.level == level
    }
}

/// Per-function counters retained for deterministic reports and limit tests.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct FunctionLivenessStatistics {
    tracked_values: u64,
    reachable_blocks: u64,
    propagation_event_limit: u64,
    propagation_events: u64,
    worklist_dequeues: u64,
    maximum_live_set: u64,
    segment_limit: u64,
    retained_segments: u64,
}

impl FunctionLivenessStatistics {
    pub(crate) const fn tracked_values(self) -> u64 {
        self.tracked_values
    }

    pub(crate) const fn propagation_events(self) -> u64 {
        self.propagation_events
    }

    pub(crate) const fn retained_segments(self) -> u64 {
        self.retained_segments
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LivenessLimit<T> {
    Derived,
    #[allow(
        dead_code,
        reason = "private boundary tests select explicit limits while production derives them"
    )]
    Explicit(T),
}

/// Maximum charged propagation/set-element events for one function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LivenessEventLimit(u64);

impl LivenessEventLimit {
    #[cfg(test)]
    pub(crate) const fn new(events: u64) -> Self {
        Self(events)
    }
}

/// Maximum retained half-open segments for one function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LivenessSegmentLimit(u64);

impl LivenessSegmentLimit {
    #[cfg(test)]
    pub(crate) const fn new(segments: u64) -> Self {
        Self(segments)
    }
}

/// Independent per-computation resource policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LivenessLimits {
    events: LivenessLimit<LivenessEventLimit>,
    segments: LivenessLimit<LivenessSegmentLimit>,
}

impl LivenessLimits {
    pub(crate) const fn derived() -> Self {
        Self {
            events: LivenessLimit::Derived,
            segments: LivenessLimit::Derived,
        }
    }

    #[cfg(test)]
    pub(crate) const fn with_event_limit(mut self, limit: LivenessEventLimit) -> Self {
        self.events = LivenessLimit::Explicit(limit);
        self
    }

    #[cfg(test)]
    pub(crate) const fn with_segment_limit(mut self, limit: LivenessSegmentLimit) -> Self {
        self.segments = LivenessLimit::Explicit(limit);
        self
    }
}

impl Default for LivenessLimits {
    fn default() -> Self {
        Self::derived()
    }
}

#[derive(Clone, Debug)]
struct BlockFacts {
    tracked_parameters: Box<[ValueId]>,
    instruction_defs: Box<[ValueId]>,
    uses_before_instruction_defs: Box<[ValueId]>,
    terminator_uses: Box<[ValueId]>,
}

type BlockFactTable = Box<[Option<BlockFacts>]>;
type RetainedInstructionBits = Box<[bool]>;
type EdgeSubstitution = Box<[(ValueId, ValueId)]>;
type EdgeSubstitutionTable = Box<[EdgeSubstitution]>;
type SegmentTable = (Box<[Option<SegmentRange>]>, Box<[LiveSegment]>);

#[derive(Clone, Copy, Debug)]
struct DerivedLimits {
    events: u64,
    segments: u64,
}

const MAX_DERIVED_LIVENESS_EVENTS: u64 = 16_000_000;
const MAX_DERIVED_LIVE_SEGMENTS: u64 = 1_000_000;

fn analyze_function(
    function: FunctionId,
    body: &FunctionBody,
    function_inventory: &FunctionSemanticInventory,
    inventory: &SemanticInventory,
    demand: &RuntimeDemand,
    limits: LivenessLimits,
) -> Result<FunctionLiveness, LivenessError> {
    let tracked = tracked_values(function, body, function_inventory, inventory, demand)?;
    let tracked_count = tracked.iter().filter(|tracked| **tracked).count();
    let mut statistics = FunctionLivenessStatistics {
        tracked_values: u64::try_from(tracked_count).unwrap_or(u64::MAX),
        reachable_blocks: u64::try_from(function_inventory.reachable_blocks().len())
            .unwrap_or(u64::MAX),
        ..FunctionLivenessStatistics::default()
    };
    let Some(derived) = derive_limits(function_inventory, tracked_count) else {
        return Ok(FunctionLiveness::ConservativeFallback {
            reason: LivenessFallbackReason::DerivedBoundOverflow,
            statistics,
        });
    };
    statistics.propagation_event_limit = match limits.events {
        LivenessLimit::Derived => derived.events,
        LivenessLimit::Explicit(limit) => limit.0,
    };
    statistics.segment_limit = match limits.segments {
        LivenessLimit::Derived => derived.segments,
        LivenessLimit::Explicit(limit) => limit.0,
    };
    if statistics.segment_limit < statistics.tracked_values {
        return Ok(FunctionLiveness::ConservativeFallback {
            reason: LivenessFallbackReason::RetainedSegments,
            statistics,
        });
    }

    let (facts, retained_instructions) = build_block_facts(
        function,
        body,
        function_inventory,
        inventory,
        demand,
        &tracked,
    )?;
    let edge_substitutions = build_edge_substitutions(function, body, function_inventory)?;
    let mut budget = EventBudget::new(statistics.propagation_event_limit);
    let Some(fixed_point) = solve_fixed_point(
        function,
        body,
        function_inventory,
        &facts,
        &edge_substitutions,
        &tracked,
        &mut budget,
        &mut statistics,
    )?
    else {
        statistics.propagation_events = budget.used();
        return Ok(FunctionLiveness::ConservativeFallback {
            reason: LivenessFallbackReason::PropagationEvents,
            statistics,
        });
    };
    statistics.propagation_events = budget.used();
    let Some((ranges, segments)) = build_segments(
        function,
        body,
        function_inventory,
        &tracked,
        &retained_instructions,
        &fixed_point,
        statistics.segment_limit,
        &mut statistics,
    )?
    else {
        // The fallback publishes no segment facts. Keep the report about the
        // retained result, rather than exposing a discarded partial prefix.
        statistics.retained_segments = 0;
        return Ok(FunctionLiveness::ConservativeFallback {
            reason: LivenessFallbackReason::RetainedSegments,
            statistics,
        });
    };
    #[cfg(test)]
    let blocks = fixed_point
        .iter()
        .map(|facts| {
            facts.as_ref().map(|(live_in, live_out)| BlockLiveness {
                live_in: live_in.clone().into_boxed_slice(),
                live_out: live_out.clone().into_boxed_slice(),
            })
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(FunctionLiveness::Complete(CompletedFunctionLiveness {
        #[cfg(test)]
        blocks,
        value_segment_ranges: ranges,
        segments,
        statistics,
    }))
}

fn derive_limits(
    inventory: &FunctionSemanticInventory,
    tracked_values: usize,
) -> Option<DerivedLimits> {
    let incidences = inventory.demand_incidences();
    let base = 1_u64
        .checked_add(u64::try_from(inventory.reachable_blocks().len()).ok()?)?
        .checked_add(u64::try_from(tracked_values).ok()?)?
        .checked_add(u64::try_from(incidences.reachable_instructions()).ok()?)?
        .checked_add(u64::try_from(incidences.instruction_operands()).ok()?)?
        .checked_add(u64::try_from(incidences.edge_arguments()).ok()?)?
        .checked_add(u64::try_from(incidences.return_operands()).ok()?)?
        .checked_add(u64::try_from(incidences.branch_conditions()).ok()?)?;
    let events = base.checked_mul(64)?.min(MAX_DERIVED_LIVENESS_EVENTS);
    let segments = base.checked_mul(8)?.min(MAX_DERIVED_LIVE_SEGMENTS);
    Some(DerivedLimits { events, segments })
}

fn tracked_values(
    function: FunctionId,
    body: &FunctionBody,
    function_inventory: &FunctionSemanticInventory,
    inventory: &SemanticInventory,
    demand: &RuntimeDemand,
) -> Result<Box<[bool]>, LivenessError> {
    let mut tracked = vec![false; body.value_counts().allocated];
    let entry = body
        .block(body.entry())
        .ok_or(LivenessError::InvalidCoreEntity { function })?;
    for parameter in entry.parameters() {
        set_tracked(function, &mut tracked, parameter.value())?;
    }
    for value in function_inventory.reachable_values().iter().copied() {
        if demand.requires_value(function, value, inventory)? {
            set_tracked(function, &mut tracked, value)?;
        }
    }
    Ok(tracked.into_boxed_slice())
}

fn build_block_facts(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    program_inventory: &SemanticInventory,
    demand: &RuntimeDemand,
    tracked: &[bool],
) -> Result<(BlockFactTable, RetainedInstructionBits), LivenessError> {
    let mut facts = vec![None; body.block_counts().allocated];
    let mut retained_instructions = vec![false; body.instruction_counts().allocated];
    let mut locally_defined = vec![false; body.value_counts().allocated];
    let mut definitions_to_clear = Vec::new();

    for block in inventory.reachable_blocks().iter().copied() {
        let data = body
            .block(block)
            .ok_or(LivenessError::InvalidCoreEntity { function })?;
        let mut parameters = Vec::new();
        let mut instruction_defs = Vec::new();
        let mut local_uses = Vec::new();
        for parameter in data.parameters() {
            let value = parameter.value();
            mark_local_definition(
                function,
                &mut locally_defined,
                &mut definitions_to_clear,
                value,
            )?;
            if is_tracked(tracked, value) {
                parameters.push(value);
            }
        }
        for instruction in data.instructions().iter().copied() {
            let instruction_data = body
                .instruction(instruction)
                .ok_or(LivenessError::InvalidCoreEntity { function })?;
            let retained = demand.requires_instruction(function, instruction, program_inventory)?;
            if retained {
                set_indexed_bool(function, &mut retained_instructions, instruction, true)?;
                for operand in instruction_data.operands().iter().copied() {
                    if !is_tracked(tracked, operand) {
                        return Err(LivenessError::UntrackedRequiredValue {
                            function,
                            value: operand,
                        });
                    }
                    if !is_marked(&locally_defined, operand) {
                        local_uses.push(operand);
                    }
                }
            }
            for result in instruction_data.results().iter().copied() {
                mark_local_definition(
                    function,
                    &mut locally_defined,
                    &mut definitions_to_clear,
                    result,
                )?;
                if is_tracked(tracked, result) {
                    instruction_defs.push(result);
                }
            }
        }
        let terminator = data
            .terminator()
            .ok_or(LivenessError::InvalidCoreEntity { function })?;
        let mut terminator_uses = Vec::new();
        match terminator.kind() {
            TerminatorKind::Branch { condition, .. } => {
                require_tracked(function, tracked, *condition)?;
                terminator_uses.push(*condition);
            }
            TerminatorKind::Return(values) => {
                for value in values.iter().copied() {
                    require_tracked(function, tracked, value)?;
                    terminator_uses.push(value);
                }
            }
            TerminatorKind::Jump(_) | TerminatorKind::Unreachable => {}
        }
        sort_dedup(&mut parameters);
        sort_dedup(&mut instruction_defs);
        sort_dedup(&mut local_uses);
        sort_dedup(&mut terminator_uses);
        set_indexed_slot(
            function,
            &mut facts,
            block,
            Some(BlockFacts {
                tracked_parameters: parameters.into_boxed_slice(),
                instruction_defs: instruction_defs.into_boxed_slice(),
                uses_before_instruction_defs: local_uses.into_boxed_slice(),
                terminator_uses: terminator_uses.into_boxed_slice(),
            }),
        )?;
        for value in definitions_to_clear.drain(..) {
            set_indexed_bool(function, &mut locally_defined, value, false)?;
        }
    }
    Ok((
        facts.into_boxed_slice(),
        retained_instructions.into_boxed_slice(),
    ))
}

fn build_edge_substitutions(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
) -> Result<EdgeSubstitutionTable, LivenessError> {
    inventory
        .edges()
        .iter()
        .map(|edge| {
            let destination = body
                .block(edge.destination())
                .ok_or(LivenessError::InvalidCoreEntity { function })?;
            if destination.parameters().len() != edge.arguments().len() {
                return Err(LivenessError::InvalidEdge { function });
            }
            let mut substitutions = destination
                .parameters()
                .iter()
                .zip(edge.arguments().iter().copied())
                .map(|(parameter, argument)| (parameter.value(), argument))
                .collect::<Vec<_>>();
            substitutions.sort_unstable_by_key(|(parameter, _)| *parameter);
            Ok(substitutions.into_boxed_slice())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Vec::into_boxed_slice)
}

type FixedPoint = Box<[Option<(Vec<ValueId>, Vec<ValueId>)>]>;

#[allow(
    clippy::too_many_arguments,
    reason = "the sparse solver keeps its immutable authorities and bounded state explicit"
)]
fn solve_fixed_point(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    facts: &[Option<BlockFacts>],
    edge_substitutions: &[Box<[(ValueId, ValueId)]>],
    tracked: &[bool],
    budget: &mut EventBudget,
    statistics: &mut FunctionLivenessStatistics,
) -> Result<Option<FixedPoint>, LivenessError> {
    let mut states = vec![None; body.block_counts().allocated];
    let mut queued = vec![false; body.block_counts().allocated];
    let mut queue = VecDeque::new();
    for block in inventory.reachable_blocks().iter().rev().copied() {
        set_indexed_slot(function, &mut states, block, Some((Vec::new(), Vec::new())))?;
        enqueue(function, &mut queue, &mut queued, block)?;
    }

    while let Some(block) = queue.pop_front() {
        set_indexed_bool(function, &mut queued, block, false)?;
        if !budget.charge(1) {
            return Ok(None);
        }
        statistics.worklist_dequeues = statistics.worklist_dequeues.saturating_add(1);
        let block_facts = indexed_ref(facts, block)
            .and_then(Option::as_ref)
            .ok_or(LivenessError::InvalidCoreEntity { function })?;
        let Some(new_live_out) = compute_live_out(
            function,
            inventory,
            block,
            block_facts,
            edge_substitutions,
            &states,
            tracked,
            budget,
        )?
        else {
            return Ok(None);
        };
        let Some(without_defs) =
            difference_sorted(&new_live_out, &block_facts.instruction_defs, budget)
        else {
            return Ok(None);
        };
        let Some(new_live_in) = union_three_sorted(
            &block_facts.tracked_parameters,
            &block_facts.uses_before_instruction_defs,
            &without_defs,
            budget,
        ) else {
            return Ok(None);
        };
        statistics.maximum_live_set = statistics
            .maximum_live_set
            .max(u64::try_from(new_live_in.len().max(new_live_out.len())).unwrap_or(u64::MAX));
        let state = indexed_mut(&mut states, block)
            .and_then(Option::as_mut)
            .ok_or(LivenessError::InvalidCoreEntity { function })?;
        let live_in_changed = state.0 != new_live_in;
        if !live_in_changed && state.1 == new_live_out {
            continue;
        }
        state.0 = new_live_in;
        state.1 = new_live_out;
        if !live_in_changed {
            continue;
        }
        let incoming = inventory
            .incoming_edge_indices(block)
            .ok_or(LivenessError::InvalidCoreEntity { function })?;
        for edge_index in incoming.iter().copied() {
            let edge = inventory
                .edges()
                .get(edge_index)
                .ok_or(LivenessError::InvalidEdge { function })?;
            enqueue(function, &mut queue, &mut queued, edge.source())?;
        }
    }
    Ok(Some(states.into_boxed_slice()))
}

#[allow(
    clippy::too_many_arguments,
    reason = "edge-specific block-parameter substitution requires all fixed-point authorities"
)]
fn compute_live_out(
    function: FunctionId,
    inventory: &FunctionSemanticInventory,
    block: BlockId,
    facts: &BlockFacts,
    edge_substitutions: &[Box<[(ValueId, ValueId)]>],
    states: &[Option<(Vec<ValueId>, Vec<ValueId>)>],
    tracked: &[bool],
    budget: &mut EventBudget,
) -> Result<Option<Vec<ValueId>>, LivenessError> {
    let mut live_out = facts.terminator_uses.to_vec();
    let outgoing = inventory
        .outgoing_edge_indices(block)
        .ok_or(LivenessError::InvalidCoreEntity { function })?;
    for edge_index in outgoing.iter().copied() {
        let edge = inventory
            .edges()
            .get(edge_index)
            .ok_or(LivenessError::InvalidEdge { function })?;
        let substitutions = edge_substitutions
            .get(edge_index)
            .ok_or(LivenessError::InvalidEdge { function })?;
        let successor_live_in = indexed_ref(states, edge.destination())
            .and_then(Option::as_ref)
            .map_or(&[][..], |state| state.0.as_slice());
        let Some(edge_live) =
            substitute_edge_live_in(function, successor_live_in, substitutions, tracked, budget)?
        else {
            return Ok(None);
        };
        let Some(union) = union_sorted(&live_out, &edge_live, budget) else {
            return Ok(None);
        };
        live_out = union;
    }
    Ok(Some(live_out))
}

fn substitute_edge_live_in(
    function: FunctionId,
    successor_live_in: &[ValueId],
    substitutions: &[(ValueId, ValueId)],
    tracked: &[bool],
    budget: &mut EventBudget,
) -> Result<Option<Vec<ValueId>>, LivenessError> {
    if !budget.charge(
        u64::try_from(successor_live_in.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(substitutions.len()).unwrap_or(u64::MAX)),
    ) {
        return Ok(None);
    }
    let mut edge_live = Vec::with_capacity(successor_live_in.len());
    let mut substitution_index = 0;
    for value in successor_live_in.iter().copied() {
        while substitutions
            .get(substitution_index)
            .is_some_and(|(parameter, _)| *parameter < value)
        {
            substitution_index += 1;
        }
        if let Some((_, argument)) = substitutions
            .get(substitution_index)
            .filter(|(parameter, _)| *parameter == value)
        {
            let argument = *argument;
            require_tracked(function, tracked, argument)?;
            edge_live.push(argument);
        } else {
            require_tracked(function, tracked, value)?;
            edge_live.push(value);
        }
    }
    sort_dedup(&mut edge_live);
    Ok(Some(edge_live))
}

#[derive(Clone, Copy, Debug)]
struct RawSegment {
    value: ValueId,
    segment: LiveSegment,
}

#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one reverse layout walk constructs every exact half-open segment boundary"
)]
fn build_segments(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    tracked: &[bool],
    retained_instructions: &[bool],
    fixed_point: &[Option<(Vec<ValueId>, Vec<ValueId>)>],
    segment_limit: u64,
    statistics: &mut FunctionLivenessStatistics,
) -> Result<Option<SegmentTable>, LivenessError> {
    let mut raw = Vec::new();
    let mut ends = vec![None::<BlockPoint>; body.value_counts().allocated];
    let mut active_ends = Vec::new();
    for (block_ordinal, block) in inventory.reachable_blocks().iter().copied().enumerate() {
        let data = body
            .block(block)
            .ok_or(LivenessError::InvalidCoreEntity { function })?;
        let state = indexed_ref(fixed_point, block)
            .and_then(Option::as_ref)
            .ok_or(LivenessError::InvalidCoreEntity { function })?;
        let block_end = block_end_point(data.instructions().len())
            .ok_or(LivenessError::ProgramPointOverflow { function, block })?;
        for value in state.1.iter().copied() {
            set_end_if_absent(function, &mut ends, &mut active_ends, value, block_end)?;
        }
        for (instruction_ordinal, instruction) in
            data.instructions().iter().copied().enumerate().rev()
        {
            let instruction_data = body
                .instruction(instruction)
                .ok_or(LivenessError::InvalidCoreEntity { function })?;
            let definition = instruction_def_point(instruction_ordinal)
                .ok_or(LivenessError::ProgramPointOverflow { function, block })?;
            for result in instruction_data.results().iter().copied() {
                if !is_tracked(tracked, result) {
                    continue;
                }
                let end = take_indexed(function, &mut ends, result)?.unwrap_or(
                    definition
                        .next()
                        .ok_or(LivenessError::ProgramPointOverflow { function, block })?,
                );
                if !push_segment(
                    &mut raw,
                    result,
                    LiveSegment {
                        block,
                        block_ordinal,
                        from: definition,
                        to: end,
                    },
                    segment_limit,
                    statistics,
                ) {
                    return Ok(None);
                }
            }
            if indexed_bool(retained_instructions, instruction) {
                let use_point = instruction_use_point(instruction_ordinal)
                    .ok_or(LivenessError::ProgramPointOverflow { function, block })?;
                let end = use_point
                    .next()
                    .ok_or(LivenessError::ProgramPointOverflow { function, block })?;
                for operand in instruction_data.operands().iter().copied() {
                    require_tracked(function, tracked, operand)?;
                    if indexed_ref(&ends, operand).copied().flatten().is_none() {
                        set_end_if_absent(function, &mut ends, &mut active_ends, operand, end)?;
                    }
                }
            }
        }
        let mut tracked_parameters = data
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .filter(|value| is_tracked(tracked, *value))
            .collect::<Vec<_>>();
        sort_dedup(&mut tracked_parameters);
        for value in state.0.iter().copied() {
            let end = take_indexed(function, &mut ends, value)?.or_else(|| {
                tracked_parameters
                    .binary_search(&value)
                    .is_ok()
                    .then_some(BlockPoint(1))
            });
            let Some(end) = end else {
                return Err(LivenessError::InvalidFixedPoint { function, value });
            };
            if !push_segment(
                &mut raw,
                value,
                LiveSegment {
                    block,
                    block_ordinal,
                    from: BlockPoint::ENTRY,
                    to: end,
                },
                segment_limit,
                statistics,
            ) {
                return Ok(None);
            }
        }
        if let Some(value) = active_ends
            .iter()
            .copied()
            .find(|value| indexed_ref(&ends, *value).is_some_and(Option::is_some))
        {
            return Err(LivenessError::InvalidFixedPoint { function, value });
        }
        active_ends.clear();
    }
    raw.sort_by_key(|raw| {
        (
            raw.value.index(),
            raw.segment.block_ordinal,
            raw.segment.from,
            raw.segment.to,
        )
    });
    let mut ranges = vec![None; body.value_counts().allocated];
    let mut segments = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        let value = raw[index].value;
        let start = segments.len();
        while index < raw.len() && raw[index].value == value {
            let segment = raw[index].segment;
            if segment.from >= segment.to {
                return Err(LivenessError::InvalidFixedPoint { function, value });
            }
            segments.push(segment);
            index += 1;
        }
        let end = segments.len();
        set_indexed_slot(
            function,
            &mut ranges,
            value,
            Some(SegmentRange { start, end }),
        )?;
    }
    Ok(Some((
        ranges.into_boxed_slice(),
        segments.into_boxed_slice(),
    )))
}

fn push_segment(
    segments: &mut Vec<RawSegment>,
    value: ValueId,
    segment: LiveSegment,
    limit: u64,
    statistics: &mut FunctionLivenessStatistics,
) -> bool {
    let next = statistics.retained_segments.saturating_add(1);
    if next > limit {
        return false;
    }
    statistics.retained_segments = next;
    segments.push(RawSegment { value, segment });
    true
}

fn segments_intersect(left: &[LiveSegment], right: &[LiveSegment]) -> bool {
    let mut left_index = 0;
    let mut right_index = 0;
    while let (Some(left_segment), Some(right_segment)) =
        (left.get(left_index), right.get(right_index))
    {
        if left_segment.overlaps(*right_segment) {
            return true;
        }
        let left_key = (left_segment.block_ordinal, left_segment.to);
        let right_key = (right_segment.block_ordinal, right_segment.to);
        if left_key <= right_key {
            left_index += 1;
        } else {
            right_index += 1;
        }
    }
    false
}

struct EventBudget {
    limit: u64,
    used: u64,
}

impl EventBudget {
    const fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    fn charge(&mut self, amount: u64) -> bool {
        let Some(next) = self.used.checked_add(amount) else {
            self.used = self.limit;
            return false;
        };
        if next > self.limit {
            return false;
        }
        self.used = next;
        true
    }

    const fn used(&self) -> u64 {
        self.used
    }
}

fn union_three_sorted(
    first: &[ValueId],
    second: &[ValueId],
    third: &[ValueId],
    budget: &mut EventBudget,
) -> Option<Vec<ValueId>> {
    let first_two = union_sorted(first, second, budget)?;
    union_sorted(&first_two, third, budget)
}

fn union_sorted(
    left: &[ValueId],
    right: &[ValueId],
    budget: &mut EventBudget,
) -> Option<Vec<ValueId>> {
    if !budget.charge(
        u64::try_from(left.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(right.len()).unwrap_or(u64::MAX)),
    ) {
        return None;
    }
    let mut merged = Vec::with_capacity(left.len().saturating_add(right.len()));
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() || right_index < right.len() {
        match (left.get(left_index), right.get(right_index)) {
            (Some(left_value), Some(right_value)) if left_value < right_value => {
                merged.push(*left_value);
                left_index += 1;
            }
            (Some(left_value), Some(right_value)) if right_value < left_value => {
                merged.push(*right_value);
                right_index += 1;
            }
            (Some(value), Some(_)) => {
                merged.push(*value);
                left_index += 1;
                right_index += 1;
            }
            (Some(value), None) => {
                merged.push(*value);
                left_index += 1;
            }
            (None, Some(value)) => {
                merged.push(*value);
                right_index += 1;
            }
            (None, None) => break,
        }
    }
    Some(merged)
}

fn difference_sorted(
    values: &[ValueId],
    removed: &[ValueId],
    budget: &mut EventBudget,
) -> Option<Vec<ValueId>> {
    if !budget.charge(
        u64::try_from(values.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(removed.len()).unwrap_or(u64::MAX)),
    ) {
        return None;
    }
    let mut difference = Vec::with_capacity(values.len());
    let mut removed_index = 0;
    for value in values.iter().copied() {
        while removed
            .get(removed_index)
            .is_some_and(|removed| *removed < value)
        {
            removed_index += 1;
        }
        if removed.get(removed_index).copied() != Some(value) {
            difference.push(value);
        }
    }
    Some(difference)
}

fn instruction_use_point(ordinal: usize) -> Option<BlockPoint> {
    u64::try_from(ordinal)
        .ok()?
        .checked_mul(2)?
        .checked_add(1)
        .map(BlockPoint)
}

fn instruction_def_point(ordinal: usize) -> Option<BlockPoint> {
    u64::try_from(ordinal)
        .ok()?
        .checked_mul(2)?
        .checked_add(2)
        .map(BlockPoint)
}

fn block_end_point(instructions: usize) -> Option<BlockPoint> {
    u64::try_from(instructions)
        .ok()?
        .checked_mul(2)?
        .checked_add(2)
        .map(BlockPoint)
}

fn enqueue(
    function: FunctionId,
    queue: &mut VecDeque<BlockId>,
    queued: &mut [bool],
    block: BlockId,
) -> Result<(), LivenessError> {
    if indexed_bool(queued, block) {
        return Ok(());
    }
    set_indexed_bool(function, queued, block, true)?;
    queue.push_back(block);
    Ok(())
}

fn sort_dedup(values: &mut Vec<ValueId>) {
    values.sort_unstable();
    values.dedup();
}

fn is_tracked(tracked: &[bool], value: ValueId) -> bool {
    indexed_bool(tracked, value)
}

fn require_tracked(
    function: FunctionId,
    tracked: &[bool],
    value: ValueId,
) -> Result<(), LivenessError> {
    if is_tracked(tracked, value) {
        Ok(())
    } else {
        Err(LivenessError::UntrackedRequiredValue { function, value })
    }
}

fn set_tracked(
    function: FunctionId,
    tracked: &mut [bool],
    value: ValueId,
) -> Result<(), LivenessError> {
    set_indexed_bool(function, tracked, value, true)
}

fn mark_local_definition(
    function: FunctionId,
    definitions: &mut [bool],
    to_clear: &mut Vec<ValueId>,
    value: ValueId,
) -> Result<(), LivenessError> {
    set_indexed_bool(function, definitions, value, true)?;
    to_clear.push(value);
    Ok(())
}

fn is_marked(definitions: &[bool], value: ValueId) -> bool {
    indexed_bool(definitions, value)
}

fn indexed_bool<I: EntityId>(values: &[bool], id: I) -> bool {
    entity_index(id)
        .and_then(|index| values.get(index))
        .copied()
        .unwrap_or(false)
}

fn set_indexed_bool<I: EntityId>(
    function: FunctionId,
    values: &mut [bool],
    id: I,
    value: bool,
) -> Result<(), LivenessError> {
    let slot = entity_index(id)
        .and_then(|index| values.get_mut(index))
        .ok_or(LivenessError::InvalidCoreEntity { function })?;
    *slot = value;
    Ok(())
}

fn indexed_ref<I: EntityId, T>(values: &[T], id: I) -> Option<&T> {
    entity_index(id).and_then(|index| values.get(index))
}

fn indexed_mut<I: EntityId, T>(values: &mut [T], id: I) -> Option<&mut T> {
    entity_index(id).and_then(|index| values.get_mut(index))
}

fn set_indexed_slot<I: EntityId, T>(
    function: FunctionId,
    values: &mut [T],
    id: I,
    value: T,
) -> Result<(), LivenessError> {
    let slot = indexed_mut(values, id).ok_or(LivenessError::InvalidCoreEntity { function })?;
    *slot = value;
    Ok(())
}

fn take_indexed<I: EntityId, T>(
    function: FunctionId,
    values: &mut [Option<T>],
    id: I,
) -> Result<Option<T>, LivenessError> {
    indexed_mut(values, id)
        .map(Option::take)
        .ok_or(LivenessError::InvalidCoreEntity { function })
}

fn set_end_if_absent(
    function: FunctionId,
    ends: &mut [Option<BlockPoint>],
    active: &mut Vec<ValueId>,
    value: ValueId,
    end: BlockPoint,
) -> Result<(), LivenessError> {
    let slot = indexed_mut(ends, value).ok_or(LivenessError::InvalidCoreEntity { function })?;
    if slot.is_none() {
        *slot = Some(end);
        active.push(value);
    }
    Ok(())
}

fn entity_index<I: EntityId>(id: I) -> Option<usize> {
    usize::try_from(id.index()).ok()
}

/// Mandatory structural failure while constructing optional liveness facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LivenessError {
    MissingDefinition {
        function: FunctionId,
    },
    MissingInventory {
        function: FunctionId,
    },
    InvalidCoreEntity {
        function: FunctionId,
    },
    InvalidEdge {
        function: FunctionId,
    },
    UntrackedRequiredValue {
        function: FunctionId,
        value: ValueId,
    },
    InvalidFixedPoint {
        function: FunctionId,
        value: ValueId,
    },
    ProgramPointOverflow {
        function: FunctionId,
        block: BlockId,
    },
    DemandPolicyMismatch,
    Demand(DemandError),
    EntityLimit,
}

impl From<DemandError> for LivenessError {
    fn from(error: DemandError) -> Self {
        Self::Demand(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BlockPoint, FunctionLiveness, LiveSegment, LivenessEventLimit, LivenessFallbackReason,
        LivenessLimits, LivenessResult, LivenessSegmentLimit,
    };
    use crate::entity::EntityId;
    use crate::ir::core::{
        BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind, ValueId,
    };
    use crate::lower::minecraft::MinecraftOptimizationLevel;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::assignment::{AssignmentError, HomeAssignment};
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::source::{OriginId, SourceContext};

    #[test]
    fn fallback_reason_codes_are_stable_kebab_case() {
        for (reason, expected) in [
            (
                LivenessFallbackReason::PropagationEvents,
                "propagation-events",
            ),
            (
                LivenessFallbackReason::RetainedSegments,
                "retained-segments",
            ),
            (
                LivenessFallbackReason::DerivedBoundOverflow,
                "derived-bound-overflow",
            ),
        ] {
            assert_eq!(reason.code(), expected);
            assert_eq!(reason.to_string(), expected);
        }
    }

    #[test]
    fn straight_line_points_are_half_open_and_keep_use_before_def_order() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("points"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let block = builder.entry_block();
        let left = builder.i32_constant(3, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(4, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let result = analyze(&core, LivenessLimits::derived());
        let completed = result.function(function).unwrap().completed().unwrap();
        assert_eq!(completed.block(block).unwrap().live_in(), &[]);
        assert_eq!(completed.block(block).unwrap().live_out(), &[sum]);
        assert_eq!(completed.segments(left), &[segment(block, 0, 2, 6)]);
        assert_eq!(completed.segments(right), &[segment(block, 0, 4, 6)]);
        assert_eq!(completed.segments(sum), &[segment(block, 0, 6, 8)]);
        assert_eq!(completed.segments(sum)[0].block(), block);
        assert_eq!(completed.segments(sum)[0].to(), BlockPoint(8));
        assert!(completed.values_interfere(left, right));
        assert!(!completed.values_interfere(left, sum));
    }

    #[test]
    fn block_parameters_substitute_edge_arguments_at_one_simultaneous_boundary() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("edge"), vec![], vec![CoreType::I32], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let source = builder.i32_constant(9, OriginId::UNKNOWN).unwrap();
        let target = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
            .append_block_parameter(target, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(target, vec![source])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(target).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let result = analyze(&core, LivenessLimits::derived());
        let completed = result.function(function).unwrap().completed().unwrap();
        assert_eq!(completed.block(entry).unwrap().live_out(), &[source]);
        assert_eq!(completed.block(target).unwrap().live_in(), &[parameter]);
        assert_eq!(completed.segments(source), &[segment(entry, 0, 2, 4)]);
        assert_eq!(completed.segments(parameter), &[segment(target, 1, 0, 2)]);
        assert!(!completed.values_interfere(source, parameter));
    }

    #[test]
    fn all_edge_arguments_are_read_before_simultaneous_parameter_definitions() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("multi-edge"),
                vec![],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let first_source = builder.i32_constant(3, OriginId::UNKNOWN).unwrap();
        let second_source = builder.i32_constant(4, OriginId::UNKNOWN).unwrap();
        let target = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first_parameter = builder
            .append_block_parameter(target, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let second_parameter = builder
            .append_block_parameter(target, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(target, vec![first_source, second_source])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(target).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![first_parameter, second_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let result = analyze(&core, LivenessLimits::derived());
        let completed = result.function(function).unwrap().completed().unwrap();
        assert_eq!(
            completed.block(entry).unwrap().live_out(),
            &[first_source, second_source]
        );
        assert!(completed.values_interfere(first_source, second_source));
        assert!(completed.values_interfere(first_parameter, second_parameter));
        assert!(!completed.values_interfere(first_source, first_parameter));
        assert!(!completed.values_interfere(second_source, second_parameter));
    }

    #[test]
    fn duplicate_successor_arms_keep_their_distinct_edge_arguments() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("duplicate-successor"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry = builder.entry_block();
        let then_value = builder.i32_constant(10, OriginId::UNKNOWN).unwrap();
        let else_value = builder.i32_constant(20, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
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
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let result = analyze(&core, LivenessLimits::derived());
        let completed = result.function(function).unwrap().completed().unwrap();
        assert_eq!(
            completed.block(entry).unwrap().live_out(),
            &[then_value, else_value, condition]
        );
        assert_eq!(completed.block(join).unwrap().live_in(), &[parameter]);
    }

    #[test]
    fn call_arguments_end_exactly_where_simultaneous_results_begin() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let identity_function = core
            .declare_function(
                Some("callee"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut callee_builder = FunctionBuilder::new(&core, &sources, identity_function).unwrap();
        let callee_parameter = callee_builder
            .body()
            .block(callee_builder.entry_block())
            .unwrap()
            .parameters()[0]
            .value();
        callee_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![callee_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(identity_function, callee_builder.finish().unwrap())
            .unwrap();

        let calling_function = core
            .declare_function(
                Some("caller"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, calling_function).unwrap();
        let argument = builder.i32_constant(8, OriginId::UNKNOWN).unwrap();
        let result = builder
            .call(identity_function, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let caller_entry = builder.entry_block();
        core.define_function(calling_function, builder.finish().unwrap())
            .unwrap();

        let liveness = analyze(&core, LivenessLimits::derived());
        let completed = liveness
            .function(calling_function)
            .unwrap()
            .completed()
            .unwrap();
        assert_eq!(
            completed.segments(argument),
            &[segment(caller_entry, 0, 2, 4)]
        );
        assert_eq!(
            completed.segments(result),
            &[segment(caller_entry, 0, 4, 6)]
        );
        assert!(!completed.values_interfere(argument, result));
    }

    #[test]
    fn simultaneous_multi_results_reserve_the_same_definition_boundary() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("multi"),
                vec![],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let left = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let right = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let (sum, overflow) = builder
            .i32_add_overflowing(left, right, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum, overflow]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let result = analyze(&core, LivenessLimits::derived());
        let completed = result.function(function).unwrap().completed().unwrap();
        assert_eq!(completed.segments(sum)[0].from(), BlockPoint(6));
        assert_eq!(completed.segments(overflow)[0].from(), BlockPoint(6));
        assert!(completed.values_interfere(sum, overflow));
    }

    #[test]
    fn either_optional_limit_discards_the_whole_function_solution() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("fallback"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let event = analyze(
            &core,
            LivenessLimits::derived().with_event_limit(LivenessEventLimit::new(0)),
        );
        assert!(matches!(
            event.function(function),
            Some(FunctionLiveness::ConservativeFallback {
                reason: LivenessFallbackReason::PropagationEvents,
                ..
            })
        ));
        let segment = analyze(
            &core,
            LivenessLimits::derived().with_segment_limit(LivenessSegmentLimit::new(0)),
        );
        assert!(matches!(
            segment.function(function),
            Some(FunctionLiveness::ConservativeFallback {
                reason: LivenessFallbackReason::RetainedSegments,
                ..
            })
        ));
    }

    #[test]
    fn exact_limits_complete_and_one_less_discards_late_partial_facts_deterministically() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("late-limit"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let value = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for block in [then_block, else_block] {
            builder.switch_to_block(block).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![value]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let complete = analyze(&core, LivenessLimits::derived());
        let statistics = complete
            .function(function)
            .unwrap()
            .completed()
            .unwrap()
            .statistics();
        let event_limit = statistics.propagation_events();
        let segment_limit = statistics.retained_segments();
        assert!(event_limit > 0);
        assert!(segment_limit > statistics.tracked_values());

        let exact_events = analyze(
            &core,
            LivenessLimits::derived().with_event_limit(LivenessEventLimit::new(event_limit)),
        );
        assert!(
            exact_events
                .function(function)
                .unwrap()
                .completed()
                .is_some()
        );
        let event_fallback = analyze(
            &core,
            LivenessLimits::derived().with_event_limit(LivenessEventLimit::new(event_limit - 1)),
        );
        let event_result = event_fallback.function(function).unwrap();
        assert_eq!(
            event_result.fallback_reason(),
            Some(LivenessFallbackReason::PropagationEvents)
        );
        assert!(event_result.statistics().propagation_events() > 0);
        assert_eq!(event_result.statistics().retained_segments(), 0);

        let exact_segments = analyze(
            &core,
            LivenessLimits::derived().with_segment_limit(LivenessSegmentLimit::new(segment_limit)),
        );
        assert!(
            exact_segments
                .function(function)
                .unwrap()
                .completed()
                .is_some()
        );
        let one_less = LivenessLimits::derived()
            .with_segment_limit(LivenessSegmentLimit::new(segment_limit - 1));
        let segment_fallback = analyze(&core, one_less);
        let repeated_segment_fallback = analyze(&core, one_less);
        for result in [&segment_fallback, &repeated_segment_fallback] {
            let function_result = result.function(function).unwrap();
            assert_eq!(
                function_result.fallback_reason(),
                Some(LivenessFallbackReason::RetainedSegments)
            );
            assert_eq!(function_result.statistics().retained_segments(), 0);
        }
        assert_eq!(
            segment_fallback.function(function).unwrap().statistics(),
            repeated_segment_fallback
                .function(function)
                .unwrap()
                .statistics()
        );
    }

    #[test]
    fn assignment_rejects_cross_policy_liveness_even_for_an_empty_shape() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(Some("empty"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        let inventory = SemanticInventory::new(&core).unwrap();
        let demand = RuntimeDemand::for_level(
            &core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let mut liveness =
            LivenessResult::for_baseline(&core, &inventory, &demand, LivenessLimits::derived())
                .unwrap();
        assert!(liveness.matches_level(MinecraftOptimizationLevel::Baseline));
        liveness.level = MinecraftOptimizationLevel::None;

        let error =
            HomeAssignment::for_baseline(&core, &inventory, &demand, &liveness).unwrap_err();
        assert_eq!(error, AssignmentError::LivenessAlignment);
    }

    #[test]
    fn irreducible_edge_liveness_matches_an_independent_dense_oracle() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("irreducible"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let initial = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let left = builder.create_block(OriginId::UNKNOWN).unwrap();
        let left_parameter = builder
            .append_block_parameter(left, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let right = builder.create_block(OriginId::UNKNOWN).unwrap();
        let right_parameter = builder
            .append_block_parameter(right, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(left, vec![initial]),
                    else_target: BlockTarget::new(right, vec![initial]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(left).unwrap();
        let left_next = builder
            .i32_add_wrapping(left_parameter, one, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(right, vec![left_next])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(right).unwrap();
        let right_next = builder
            .i32_add_wrapping(right_parameter, one, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(left, vec![right_next]),
                    else_target: BlockTarget::new(exit, vec![right_next]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![returned]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        assert_boundary_sets_match_dense_oracle(&core, function);
    }

    #[test]
    fn deep_chain_and_wide_join_complete_with_sparse_segments() {
        const DEPTH: usize = 2_000;
        const WIDTH: usize = 128;

        let (deep, deep_function) = deep_chain(DEPTH);
        let deep_result = analyze(&deep, LivenessLimits::derived());
        let deep_completed = deep_result
            .function(deep_function)
            .unwrap()
            .completed()
            .unwrap();
        assert_eq!(
            deep_completed.statistics().retained_segments(),
            u64::try_from(DEPTH + 1).unwrap()
        );

        let (wide, wide_function) = wide_join(WIDTH);
        let first = analyze(&wide, LivenessLimits::derived());
        let second = analyze(&wide, LivenessLimits::derived());
        let first_statistics = first
            .function(wide_function)
            .unwrap()
            .completed()
            .unwrap()
            .statistics();
        let second_statistics = second
            .function(wide_function)
            .unwrap()
            .completed()
            .unwrap()
            .statistics();
        assert_eq!(first_statistics, second_statistics);
        assert!(first_statistics.retained_segments() < 4 * u64::try_from(WIDTH).unwrap());
    }

    #[test]
    fn twenty_thousand_simultaneously_live_values_retain_linear_segments() {
        const VALUES: usize = 20_000;

        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("twenty-thousand-live"),
                vec![CoreType::I32; VALUES],
                vec![CoreType::I32; VALUES],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let returned = builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect::<Vec<_>>();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(returned.clone()),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let result = analyze(&core, LivenessLimits::derived());
        let completed = result.function(function).unwrap().completed().unwrap();
        assert_eq!(completed.statistics().tracked_values(), VALUES as u64);
        assert_eq!(completed.statistics().retained_segments(), VALUES as u64);
        assert_eq!(completed.segments(returned[0]).len(), 1);
        assert_eq!(completed.segments(returned[VALUES - 1]).len(), 1);
    }

    fn analyze(core: &CoreProgram, limits: LivenessLimits) -> LivenessResult {
        let inventory = SemanticInventory::new(core).unwrap();
        let demand = RuntimeDemand::for_level(
            core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        LivenessResult::for_baseline(core, &inventory, &demand, limits).unwrap()
    }

    fn assert_boundary_sets_match_dense_oracle(core: &CoreProgram, function: FunctionId) {
        let inventory = SemanticInventory::new(core).unwrap();
        let demand = RuntimeDemand::for_level(
            core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let result =
            LivenessResult::for_baseline(core, &inventory, &demand, LivenessLimits::derived())
                .unwrap();
        let expected = dense_boundary_oracle(core, function, &inventory, &demand);
        let completed = result.function(function).unwrap().completed().unwrap();
        for block in inventory
            .function(function)
            .unwrap()
            .reachable_blocks()
            .iter()
            .copied()
        {
            let index = usize::try_from(block.index()).unwrap();
            let (live_in, live_out) = expected[index].as_ref().unwrap();
            let actual = completed.block(block).unwrap();
            assert_eq!(actual.live_in(), live_in);
            assert_eq!(actual.live_out(), live_out);
        }
    }

    #[derive(Clone)]
    struct DenseBlockFacts {
        parameters: Vec<bool>,
        instruction_definitions: Vec<bool>,
        local_uses: Vec<bool>,
        terminator_uses: Vec<bool>,
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the test oracle deliberately restates liveness with dense Boolean equations"
    )]
    fn dense_boundary_oracle(
        core: &CoreProgram,
        function: FunctionId,
        inventory: &SemanticInventory,
        demand: &RuntimeDemand,
    ) -> Vec<Option<(Vec<ValueId>, Vec<ValueId>)>> {
        let body = core.function(function).unwrap().body().unwrap();
        let semantic = inventory.function(function).unwrap();
        let value_count = body.value_counts().allocated;
        let mut tracked = vec![false; value_count];
        for parameter in body.block(body.entry()).unwrap().parameters() {
            tracked[usize::try_from(parameter.value().index()).unwrap()] = true;
        }
        for value in semantic.reachable_values().iter().copied() {
            if demand.requires_value(function, value, inventory).unwrap() {
                tracked[usize::try_from(value.index()).unwrap()] = true;
            }
        }

        let mut block_facts = vec![None; body.block_counts().allocated];
        for block in semantic.reachable_blocks().iter().copied() {
            let data = body.block(block).unwrap();
            let mut parameters = vec![false; value_count];
            let mut instruction_definitions = vec![false; value_count];
            let mut local_definitions = vec![false; value_count];
            let mut local_uses = vec![false; value_count];
            let mut terminator_uses = vec![false; value_count];
            for parameter in data.parameters() {
                let index = usize::try_from(parameter.value().index()).unwrap();
                local_definitions[index] = true;
                parameters[index] = tracked[index];
            }
            for instruction in data.instructions().iter().copied() {
                let instruction_data = body.instruction(instruction).unwrap();
                if demand
                    .requires_instruction(function, instruction, inventory)
                    .unwrap()
                {
                    for operand in instruction_data.operands().iter().copied() {
                        let index = usize::try_from(operand.index()).unwrap();
                        assert!(tracked[index]);
                        if !local_definitions[index] {
                            local_uses[index] = true;
                        }
                    }
                }
                for result in instruction_data.results().iter().copied() {
                    let index = usize::try_from(result.index()).unwrap();
                    local_definitions[index] = true;
                    instruction_definitions[index] = tracked[index];
                }
            }
            match data.terminator().unwrap().kind() {
                TerminatorKind::Branch { condition, .. } => {
                    terminator_uses[usize::try_from(condition.index()).unwrap()] = true;
                }
                TerminatorKind::Return(values) => {
                    for value in values {
                        terminator_uses[usize::try_from(value.index()).unwrap()] = true;
                    }
                }
                TerminatorKind::Jump(_) | TerminatorKind::Unreachable => {}
            }
            block_facts[usize::try_from(block.index()).unwrap()] = Some(DenseBlockFacts {
                parameters,
                instruction_definitions,
                local_uses,
                terminator_uses,
            });
        }

        let mut states = vec![None; body.block_counts().allocated];
        for block in semantic.reachable_blocks().iter().copied() {
            states[usize::try_from(block.index()).unwrap()] =
                Some((vec![false; value_count], vec![false; value_count]));
        }
        loop {
            let mut changed = false;
            for block in semantic.reachable_blocks().iter().rev().copied() {
                let block_index = usize::try_from(block.index()).unwrap();
                let facts = block_facts[block_index].as_ref().unwrap();
                let mut live_out = facts.terminator_uses.clone();
                for edge_index in semantic.outgoing_edge_indices(block).unwrap() {
                    let edge = &semantic.edges()[*edge_index];
                    let destination = body.block(edge.destination()).unwrap();
                    let successor_live_in = &states
                        [usize::try_from(edge.destination().index()).unwrap()]
                    .as_ref()
                    .unwrap()
                    .0;
                    for (value_index, is_live) in successor_live_in.iter().copied().enumerate() {
                        if !is_live {
                            continue;
                        }
                        let value = ValueId::from_index(u32::try_from(value_index).unwrap());
                        let substituted = destination
                            .parameters()
                            .iter()
                            .position(|parameter| parameter.value() == value)
                            .map_or(value, |parameter_index| edge.arguments()[parameter_index]);
                        live_out[usize::try_from(substituted.index()).unwrap()] = true;
                    }
                }
                let mut live_in = vec![false; value_count];
                for value_index in 0..value_count {
                    live_in[value_index] = facts.parameters[value_index]
                        || facts.local_uses[value_index]
                        || (live_out[value_index] && !facts.instruction_definitions[value_index]);
                }
                let state = states[block_index].as_mut().unwrap();
                if state.0 != live_in || state.1 != live_out {
                    *state = (live_in, live_out);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        states
            .into_iter()
            .map(|state| {
                state.map(|(live_in, live_out)| (dense_values(&live_in), dense_values(&live_out)))
            })
            .collect()
    }

    fn dense_values(bits: &[bool]) -> Vec<ValueId> {
        bits.iter()
            .copied()
            .enumerate()
            .filter(|(_, present)| *present)
            .map(|(index, _)| ValueId::from_index(u32::try_from(index).unwrap()))
            .collect()
    }

    fn deep_chain(depth: usize) -> (CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("deep-chain"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let initial = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let mut blocks = Vec::with_capacity(depth);
        let mut parameters = Vec::with_capacity(depth);
        for _ in 0..depth {
            let block = builder.create_block(OriginId::UNKNOWN).unwrap();
            parameters.push(
                builder
                    .append_block_parameter(block, CoreType::I32, OriginId::UNKNOWN)
                    .unwrap(),
            );
            blocks.push(block);
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(blocks[0], vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for index in 0..depth {
            builder.switch_to_block(blocks[index]).unwrap();
            let terminator = if index + 1 == depth {
                TerminatorKind::Return(vec![parameters[index]])
            } else {
                TerminatorKind::Jump(BlockTarget::new(blocks[index + 1], vec![parameters[index]]))
            };
            builder
                .terminate(Terminator::new(terminator, OriginId::UNKNOWN))
                .unwrap();
        }
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    fn wide_join(width: usize) -> (CoreProgram, FunctionId) {
        assert!(width >= 2);
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("wide-join"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let payload = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let leaves = (0..width)
            .map(|_| builder.create_block(OriginId::UNKNOWN).unwrap())
            .collect::<Vec<_>>();
        let dispatches = (1..width - 1)
            .map(|_| builder.create_block(OriginId::UNKNOWN).unwrap())
            .collect::<Vec<_>>();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let mut current = builder.entry_block();
        for index in 0..width - 1 {
            if current != builder.entry_block() {
                builder.switch_to_block(current).unwrap();
            }
            let else_block = dispatches.get(index).copied().unwrap_or(leaves[width - 1]);
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Branch {
                        condition,
                        then_target: BlockTarget::new(leaves[index], vec![]),
                        else_target: BlockTarget::new(else_block, vec![]),
                    },
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            current = else_block;
        }
        for leaf in leaves {
            builder.switch_to_block(leaf).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(join, vec![payload])),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![joined]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    const fn segment(
        block: crate::ir::core::BlockId,
        block_ordinal: usize,
        from: u64,
        to: u64,
    ) -> LiveSegment {
        LiveSegment {
            block,
            block_ordinal,
            from: BlockPoint(from),
            to: BlockPoint(to),
        }
    }
}
