//! Stable, bounded block-copy coalescing for Baseline home assignment.

use std::collections::BTreeSet;
use std::fmt;

use crate::entity::EntityId;
use crate::ir::core::{
    BlockId, CoreOp, CoreType, FunctionBody, FunctionId, InstId, ValueDef, ValueId,
};

use super::analysis::FunctionSemanticInventory;
use super::liveness::{CompletedFunctionLiveness, LivenessFallbackReason};
use super::scalar::scalar_access_contract;

const MAX_DERIVED_COALESCING_WORK: u64 = 8_000_000;
const MAX_RETAINED_COALESCING_PAIRS: usize = 262_144;

/// Why one function retained the distinct-home policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CoalescingFallbackReason {
    Liveness(LivenessFallbackReason),
    WorkLimit,
    DerivedBoundOverflow,
}

impl CoalescingFallbackReason {
    /// Stable machine-readable spelling used by deterministic reports.
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Liveness(LivenessFallbackReason::PropagationEvents) => {
                "liveness-propagation-events"
            }
            Self::Liveness(LivenessFallbackReason::RetainedSegments) => {
                "liveness-retained-segments"
            }
            Self::Liveness(LivenessFallbackReason::DerivedBoundOverflow) => {
                "liveness-derived-bound-overflow"
            }
            Self::WorkLimit => "work-limit",
            Self::DerivedBoundOverflow => "derived-bound-overflow",
        }
    }
}

impl fmt::Display for CoalescingFallbackReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

/// Deterministic coalescing counters retained with assignment decisions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CoalescingStatistics {
    candidates_considered: u64,
    merges_accepted: u64,
    work_limit: u64,
    work_used: u64,
}

impl CoalescingStatistics {
    pub(crate) const fn candidates_considered(self) -> u64 {
        self.candidates_considered
    }

    pub(crate) const fn merges_accepted(self) -> u64 {
        self.merges_accepted
    }

    pub(crate) const fn work_used(self) -> u64 {
        self.work_used
    }
}

/// Complete grouping or an all-distinct fallback request.
#[derive(Clone, Debug)]
pub(crate) enum CoalescingOutcome {
    Complete(CoalescedGroups),
    DistinctFallback {
        reason: CoalescingFallbackReason,
        statistics: CoalescingStatistics,
    },
}

impl CoalescingOutcome {
    pub(crate) const fn groups(&self) -> Option<&CoalescedGroups> {
        match self {
            Self::Complete(groups) => Some(groups),
            Self::DistinctFallback { .. } => None,
        }
    }

    pub(crate) const fn fallback_reason(&self) -> Option<CoalescingFallbackReason> {
        match self {
            Self::Complete(_) => None,
            Self::DistinctFallback { reason, .. } => Some(*reason),
        }
    }

    pub(crate) const fn statistics(&self) -> CoalescingStatistics {
        match self {
            Self::Complete(groups) => groups.statistics,
            Self::DistinctFallback { statistics, .. } => *statistics,
        }
    }
}

/// Frozen function-local semantic equivalence groups.
#[derive(Clone, Debug)]
pub(crate) struct CoalescedGroups {
    group_for_value: Box<[Option<usize>]>,
    group_count: usize,
    statistics: CoalescingStatistics,
}

impl CoalescedGroups {
    pub(crate) fn group(&self, value: ValueId) -> Option<usize> {
        entity_index(value)
            .and_then(|index| self.group_for_value.get(index))
            .copied()
            .flatten()
    }

    pub(crate) fn len(&self) -> usize {
        self.group_count
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum DefinitionSite {
    Block(BlockId),
    Instruction(InstId),
}

#[derive(Clone, Debug)]
struct GroupState {
    members: Vec<ValueId>,
    definitions: Vec<DefinitionSite>,
    ty: CoreType,
    pinned: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Candidate {
    low: ValueId,
    high: ValueId,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ForbiddenPair {
    low: ValueId,
    high: ValueId,
}

impl ForbiddenPair {
    fn new(left: ValueId, right: ValueId) -> Option<Self> {
        match left.cmp(&right) {
            std::cmp::Ordering::Less => Some(Self {
                low: left,
                high: right,
            }),
            std::cmp::Ordering::Greater => Some(Self {
                low: right,
                high: left,
            }),
            std::cmp::Ordering::Equal => None,
        }
    }
}

pub(crate) fn coalesce_function(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    liveness: &CompletedFunctionLiveness,
) -> Result<CoalescingOutcome, CoalescingError> {
    let Some(derived_limit) = derive_work_limit(inventory, liveness) else {
        return Ok(CoalescingOutcome::DistinctFallback {
            reason: CoalescingFallbackReason::DerivedBoundOverflow,
            statistics: CoalescingStatistics::default(),
        });
    };
    coalesce_with_limit(function, body, inventory, liveness, derived_limit)
}

fn derive_work_limit(
    inventory: &FunctionSemanticInventory,
    liveness: &CompletedFunctionLiveness,
) -> Option<u64> {
    let incidences = inventory.demand_incidences();
    let base = 1_u64
        .checked_add(liveness.statistics().tracked_values())?
        .checked_add(u64::try_from(inventory.reachable_values().len()).ok()?)?
        .checked_add(u64::try_from(inventory.edges().len()).ok()?)?
        .checked_add(u64::try_from(incidences.edge_arguments()).ok()?)?
        .checked_add(u64::try_from(incidences.instruction_operands()).ok()?)?
        .checked_add(u64::try_from(incidences.reachable_instructions()).ok()?)?;
    base.checked_mul(64)
        .map(|limit| limit.min(MAX_DERIVED_COALESCING_WORK))
}

fn coalesce_with_limit(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    liveness: &CompletedFunctionLiveness,
    work_limit: u64,
) -> Result<CoalescingOutcome, CoalescingError> {
    let mut budget = WorkBudget::new(work_limit);
    let mut statistics = CoalescingStatistics {
        work_limit,
        ..CoalescingStatistics::default()
    };
    // Account once for the value walk used by either DSU construction or the
    // candidate-free dense freeze below.
    if inventory.demand_incidences().edge_arguments() > MAX_RETAINED_COALESCING_PAIRS
        || !budget.charge(u64::try_from(inventory.reachable_values().len()).unwrap_or(u64::MAX))
    {
        statistics.work_used = budget.used();
        return Ok(work_limit_fallback(statistics));
    }
    let Some(candidates) = candidates(function, body, inventory, liveness, &mut budget)? else {
        statistics.work_used = budget.used();
        return Ok(work_limit_fallback(statistics));
    };
    if candidates.is_empty() {
        // With no copy relation to merge, scalar alias constraints and singleton
        // DSU groups cannot affect the all-distinct answer.
        statistics.work_used = budget.used();
        return Ok(CoalescingOutcome::Complete(all_distinct_groups(
            function, body, inventory, liveness, statistics,
        )?));
    }
    let Some(forbidden) = forbidden_scalar_pairs(function, body, inventory, liveness, &mut budget)?
    else {
        statistics.work_used = budget.used();
        return Ok(work_limit_fallback(statistics));
    };
    let mut dsu = GroupDsu::new(function, body, inventory, liveness)?;

    for candidate in candidates {
        statistics.candidates_considered = statistics.candidates_considered.saturating_add(1);
        // `root` performs one find walk and, when needed, one compression walk.
        let root_work = search_work(dsu.len()).saturating_mul(4);
        if !budget.charge(root_work.saturating_add(1)) {
            statistics.work_used = budget.used();
            return Ok(work_limit_fallback(statistics));
        }
        let left = dsu.root(candidate.low)?;
        let right = dsu.root(candidate.high)?;
        if left == right {
            continue;
        }
        let legality = merge_is_legal(
            &dsu.groups[left],
            &dsu.groups[right],
            liveness,
            &forbidden,
            &mut budget,
        );
        let Some(legal) = legality else {
            statistics.work_used = budget.used();
            return Ok(work_limit_fallback(statistics));
        };
        if legal {
            if !dsu.union_roots(left, right, &mut budget) {
                statistics.work_used = budget.used();
                return Ok(work_limit_fallback(statistics));
            }
            statistics.merges_accepted = statistics.merges_accepted.saturating_add(1);
        }
    }
    if !audit_edges(function, body, inventory, liveness, &dsu, &mut budget)? {
        statistics.work_used = budget.used();
        return Ok(work_limit_fallback(statistics));
    }
    let freeze_work = u64::try_from(inventory.reachable_values().len())
        .unwrap_or(u64::MAX)
        .saturating_mul(search_work(dsu.len()).saturating_add(1))
        .saturating_add(liveness.statistics().tracked_values());
    if !budget.charge(freeze_work) {
        statistics.work_used = budget.used();
        return Ok(work_limit_fallback(statistics));
    }
    statistics.work_used = budget.used();
    Ok(CoalescingOutcome::Complete(
        dsu.freeze(body, inventory, statistics)?,
    ))
}

fn all_distinct_groups(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    liveness: &CompletedFunctionLiveness,
    statistics: CoalescingStatistics,
) -> Result<CoalescedGroups, CoalescingError> {
    let mut group_for_value = vec![None; body.value_counts().allocated];
    let mut group_count = 0_usize;
    for value in inventory.reachable_values().iter().copied() {
        if liveness.segments(value).is_empty() {
            continue;
        }
        body.value(value)
            .ok_or(CoalescingError::InvalidCoreEntity { function })?;
        let Some(index) = entity_index(value) else {
            return Err(CoalescingError::InvalidCoreEntity { function });
        };
        let slot = group_for_value
            .get_mut(index)
            .ok_or(CoalescingError::InvalidCoreEntity { function })?;
        if slot.replace(group_count).is_some() {
            return Err(CoalescingError::InvalidCoreEntity { function });
        }
        group_count += 1;
    }
    Ok(CoalescedGroups {
        group_for_value: group_for_value.into_boxed_slice(),
        group_count,
        statistics,
    })
}

const fn work_limit_fallback(mut statistics: CoalescingStatistics) -> CoalescingOutcome {
    // A fallback publishes distinct homes, so no tentative union was retained.
    statistics.merges_accepted = 0;
    CoalescingOutcome::DistinctFallback {
        reason: CoalescingFallbackReason::WorkLimit,
        statistics,
    }
}

fn candidates(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    liveness: &CompletedFunctionLiveness,
    budget: &mut WorkBudget,
) -> Result<Option<Vec<Candidate>>, CoalescingError> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for edge in inventory.edges() {
        if !budget.charge(1) {
            return Ok(None);
        }
        let destination = body
            .block(edge.destination())
            .ok_or(CoalescingError::InvalidCoreEntity { function })?;
        if destination.parameters().len() != edge.arguments().len() {
            return Err(CoalescingError::InvalidEdge { function });
        }
        for (parameter, argument) in destination
            .parameters()
            .iter()
            .zip(edge.arguments().iter().copied())
        {
            if !budget.charge(1) {
                return Ok(None);
            }
            let destination = parameter.value();
            if destination == argument
                || liveness.segments(destination).is_empty()
                || liveness.segments(argument).is_empty()
            {
                continue;
            }
            let (low, high) = if destination < argument {
                (destination, argument)
            } else {
                (argument, destination)
            };
            let candidate = Candidate { low, high };
            if !budget.charge(search_work(seen.len())) {
                return Ok(None);
            }
            if seen.insert(candidate) {
                if candidates.len() == MAX_RETAINED_COALESCING_PAIRS {
                    return Ok(None);
                }
                candidates.push(candidate);
            }
        }
    }
    Ok(Some(candidates))
}

fn forbidden_scalar_pairs(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    liveness: &CompletedFunctionLiveness,
    budget: &mut WorkBudget,
) -> Result<Option<Vec<ForbiddenPair>>, CoalescingError> {
    let mut forbidden = Vec::new();
    for instruction in inventory.reachable_instructions().iter().copied() {
        if !budget.charge(1) {
            return Ok(None);
        }
        let data = body
            .instruction(instruction)
            .ok_or(CoalescingError::InvalidCoreEntity { function })?;
        let Some(contract) = scalar_access_contract(data.op()) else {
            debug_assert!(matches!(data.op(), CoreOp::Call(_)));
            continue;
        };
        for (result_index, result) in data.results().iter().copied().enumerate() {
            if liveness.segments(result).is_empty() {
                continue;
            }
            for operand in data.operands().iter().copied() {
                if !budget.charge(1) {
                    return Ok(None);
                }
                if liveness.segments(operand).is_empty() {
                    continue;
                }
                let reuse_allowed = data.operands().iter().copied().enumerate().any(
                    |(operand_index, candidate)| {
                        candidate == operand
                            && contract.reusable_operand(result_index) == Some(operand_index)
                    },
                );
                if !reuse_allowed {
                    if let Some(pair) = ForbiddenPair::new(result, operand) {
                        if forbidden.len() == MAX_RETAINED_COALESCING_PAIRS {
                            return Ok(None);
                        }
                        forbidden.push(pair);
                    }
                }
            }
        }
    }
    if !budget.charge(sort_work(forbidden.len())) {
        return Ok(None);
    }
    forbidden.sort_unstable();
    forbidden.dedup();
    Ok(Some(forbidden))
}

fn sort_work(len: usize) -> u64 {
    let len = u64::try_from(len).unwrap_or(u64::MAX);
    if len < 2 {
        return len;
    }
    let levels = u64::from(u64::BITS - (len - 1).leading_zeros());
    len.saturating_mul(levels)
}

fn search_work(len: usize) -> u64 {
    let len = u64::try_from(len).unwrap_or(u64::MAX);
    if len < 2 {
        1
    } else {
        u64::from(u64::BITS - (len - 1).leading_zeros())
    }
}

fn merge_is_legal(
    left: &GroupState,
    right: &GroupState,
    liveness: &CompletedFunctionLiveness,
    forbidden: &[ForbiddenPair],
    budget: &mut WorkBudget,
) -> Option<bool> {
    if left.ty != right.ty || left.pinned || right.pinned {
        return Some(false);
    }
    if sorted_slices_intersect(&left.definitions, &right.definitions, budget)? {
        return Some(false);
    }
    for left_value in left.members.iter().copied() {
        for right_value in right.members.iter().copied() {
            let segment_work = u64::try_from(liveness.segments(left_value).len())
                .unwrap_or(u64::MAX)
                .saturating_add(
                    u64::try_from(liveness.segments(right_value).len()).unwrap_or(u64::MAX),
                )
                .saturating_add(1);
            if !budget.charge(segment_work) {
                return None;
            }
            if liveness.values_interfere(left_value, right_value) {
                return Some(false);
            }
            if !budget.charge(search_work(forbidden.len())) {
                return None;
            }
            if let Some(pair) = ForbiddenPair::new(left_value, right_value) {
                if forbidden.binary_search(&pair).is_ok() {
                    return Some(false);
                }
            }
        }
    }
    Some(true)
}

fn sorted_slices_intersect<T: Ord>(
    left: &[T],
    right: &[T],
    budget: &mut WorkBudget,
) -> Option<bool> {
    if !budget.charge(
        u64::try_from(left.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(right.len()).unwrap_or(u64::MAX)),
    ) {
        return None;
    }
    let (mut left_index, mut right_index) = (0, 0);
    while let (Some(left_value), Some(right_value)) = (left.get(left_index), right.get(right_index))
    {
        match left_value.cmp(right_value) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => return Some(true),
        }
    }
    Some(false)
}

fn audit_edges(
    function: FunctionId,
    body: &FunctionBody,
    inventory: &FunctionSemanticInventory,
    liveness: &CompletedFunctionLiveness,
    dsu: &GroupDsu,
    budget: &mut WorkBudget,
) -> Result<bool, CoalescingError> {
    for edge in inventory.edges() {
        if !budget.charge(1) {
            return Ok(false);
        }
        let destination = body
            .block(edge.destination())
            .ok_or(CoalescingError::InvalidCoreEntity { function })?;
        let mut destination_roots = Vec::new();
        for (parameter, argument) in destination
            .parameters()
            .iter()
            .zip(edge.arguments().iter().copied())
        {
            let value = parameter.value();
            if liveness.segments(value).is_empty() {
                continue;
            }
            let root_work = search_work(dsu.len()).saturating_mul(2);
            if !budget.charge(root_work.saturating_add(1)) {
                return Ok(false);
            }
            let destination_root = dsu.root_readonly(value)?;
            let source_root = dsu.root_readonly(argument)?;
            if dsu.groups[destination_root].ty != dsu.groups[source_root].ty {
                return Err(CoalescingError::EdgeTypeMismatch { function });
            }
            destination_roots.push(destination_root);
        }
        if !budget.charge(sort_work(destination_roots.len())) {
            return Ok(false);
        }
        destination_roots.sort_unstable();
        if destination_roots.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(CoalescingError::AliasedEdgeDestinations { function });
        }
    }
    Ok(true)
}

struct GroupDsu {
    value_to_node: Vec<Option<usize>>,
    parents: Vec<usize>,
    sizes: Vec<usize>,
    groups: Vec<GroupState>,
}

impl GroupDsu {
    fn len(&self) -> usize {
        self.parents.len()
    }

    fn new(
        function: FunctionId,
        body: &FunctionBody,
        inventory: &FunctionSemanticInventory,
        liveness: &CompletedFunctionLiveness,
    ) -> Result<Self, CoalescingError> {
        let value_count = body.value_counts().allocated;
        let mut value_to_node = vec![None; value_count];
        let mut parents = Vec::new();
        let mut sizes = Vec::new();
        let mut groups = Vec::new();
        for value in inventory.reachable_values().iter().copied() {
            if liveness.segments(value).is_empty() {
                continue;
            }
            let data = body
                .value(value)
                .ok_or(CoalescingError::InvalidCoreEntity { function })?;
            let Some(index) = entity_index(value) else {
                return Err(CoalescingError::InvalidCoreEntity { function });
            };
            let node = parents.len();
            let slot = value_to_node
                .get_mut(index)
                .ok_or(CoalescingError::InvalidCoreEntity { function })?;
            if slot.replace(node).is_some() {
                return Err(CoalescingError::InvalidCoreEntity { function });
            }
            parents.push(node);
            sizes.push(1);
            groups.push(GroupState {
                members: vec![value],
                definitions: vec![definition_site(data.definition())],
                ty: data.ty(),
                pinned: matches!(
                    data.definition(),
                    ValueDef::BlockParam { block, .. } if block == body.entry()
                ),
            });
        }
        Ok(Self {
            value_to_node,
            parents,
            sizes,
            groups,
        })
    }

    fn root(&mut self, value: ValueId) -> Result<usize, CoalescingError> {
        let node = entity_index(value)
            .and_then(|index| self.value_to_node.get(index))
            .copied()
            .flatten()
            .ok_or(CoalescingError::InvalidValue { value })?;
        let mut root = *self
            .parents
            .get(node)
            .ok_or(CoalescingError::InvalidValue { value })?;
        while self.parents[root] != root {
            root = self.parents[root];
        }
        let mut current = node;
        while self.parents[current] != root {
            let next = self.parents[current];
            self.parents[current] = root;
            current = next;
        }
        Ok(root)
    }

    fn root_readonly(&self, value: ValueId) -> Result<usize, CoalescingError> {
        let mut root = entity_index(value)
            .and_then(|index| self.value_to_node.get(index))
            .copied()
            .flatten()
            .ok_or(CoalescingError::InvalidValue { value })?;
        while self.parents[root] != root {
            root = self.parents[root];
        }
        Ok(root)
    }

    fn union_roots(&mut self, mut left: usize, mut right: usize, budget: &mut WorkBudget) -> bool {
        if self.sizes[left] < self.sizes[right]
            || (self.sizes[left] == self.sizes[right] && right < left)
        {
            std::mem::swap(&mut left, &mut right);
        }
        let merge_work = u64::try_from(self.groups[left].members.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(self.groups[right].members.len()).unwrap_or(u64::MAX))
            .saturating_add(u64::try_from(self.groups[left].definitions.len()).unwrap_or(u64::MAX))
            .saturating_add(
                u64::try_from(self.groups[right].definitions.len()).unwrap_or(u64::MAX),
            );
        if !budget.charge(merge_work) {
            return false;
        }
        self.parents[right] = left;
        self.sizes[left] = self.sizes[left].saturating_add(self.sizes[right]);
        let left_members = std::mem::take(&mut self.groups[left].members);
        let right_members = std::mem::take(&mut self.groups[right].members);
        let left_definitions = std::mem::take(&mut self.groups[left].definitions);
        let right_definitions = std::mem::take(&mut self.groups[right].definitions);
        self.groups[left].members = merge_sorted_owned(left_members, right_members);
        self.groups[left].definitions = merge_sorted_owned(left_definitions, right_definitions);
        true
    }

    fn freeze(
        self,
        body: &FunctionBody,
        inventory: &FunctionSemanticInventory,
        statistics: CoalescingStatistics,
    ) -> Result<CoalescedGroups, CoalescingError> {
        let mut root_to_group = vec![None; self.parents.len()];
        let mut group_for_value = vec![None; body.value_counts().allocated];
        let mut group_count = 0_usize;
        for value in inventory.reachable_values().iter().copied() {
            let Some(index) = entity_index(value) else {
                return Err(CoalescingError::InvalidValue { value });
            };
            if self.value_to_node.get(index).copied().flatten().is_none() {
                continue;
            }
            let root = self.root_readonly(value)?;
            let group = if let Some(group) = root_to_group[root] {
                group
            } else {
                let group = group_count;
                group_count += 1;
                root_to_group[root] = Some(group);
                group
            };
            group_for_value[index] = Some(group);
        }
        Ok(CoalescedGroups {
            group_for_value: group_for_value.into_boxed_slice(),
            group_count,
            statistics,
        })
    }
}

fn merge_sorted_owned<T: Ord>(left: Vec<T>, right: Vec<T>) -> Vec<T> {
    let capacity = left.len().saturating_add(right.len());
    let mut left = left.into_iter().peekable();
    let mut right = right.into_iter().peekable();
    let mut merged = Vec::with_capacity(capacity);
    while let (Some(left_value), Some(right_value)) = (left.peek(), right.peek()) {
        match left_value.cmp(right_value) {
            std::cmp::Ordering::Less => merged.push(left.next().unwrap()),
            std::cmp::Ordering::Greater => merged.push(right.next().unwrap()),
            std::cmp::Ordering::Equal => {
                merged.push(left.next().unwrap());
                let _ = right.next();
            }
        }
    }
    merged.extend(left);
    merged.extend(right);
    merged
}

const fn definition_site(definition: ValueDef) -> DefinitionSite {
    match definition {
        ValueDef::BlockParam { block, .. } => DefinitionSite::Block(block),
        ValueDef::InstResult { instruction, .. } => DefinitionSite::Instruction(instruction),
    }
}

struct WorkBudget {
    limit: u64,
    used: u64,
}

impl WorkBudget {
    const fn new(limit: u64) -> Self {
        Self { limit, used: 0 }
    }

    fn charge(&mut self, amount: u64) -> bool {
        let Some(next) = self.used.checked_add(amount) else {
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

fn entity_index<I: EntityId>(id: I) -> Option<usize> {
    usize::try_from(id.index()).ok()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CoalescingError {
    InvalidCoreEntity { function: FunctionId },
    InvalidEdge { function: FunctionId },
    EdgeTypeMismatch { function: FunctionId },
    AliasedEdgeDestinations { function: FunctionId },
    InvalidValue { value: ValueId },
}

#[cfg(test)]
mod tests {
    use super::{
        Candidate, CoalescingFallbackReason, CoalescingOutcome, WorkBudget, candidates,
        coalesce_with_limit,
    };
    use crate::ir::core::{
        BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, Terminator,
        TerminatorKind, ValueId,
    };
    use crate::lower::minecraft::MinecraftOptimizationLevel;
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::assignment::HomeAssignment;
    use crate::lower::minecraft::demand::{RuntimeDemand, RuntimeDemandLimits};
    use crate::lower::minecraft::liveness::{LivenessEventLimit, LivenessFallbackReason};
    use crate::lower::minecraft::liveness::{LivenessLimits, LivenessResult};
    use crate::source::{OriginId, SourceContext};

    #[derive(Clone, Copy)]
    enum AddShape {
        Left,
        Right,
        Same,
    }

    #[test]
    fn fallback_reason_codes_are_stable_kebab_case() {
        for (reason, expected) in [
            (
                CoalescingFallbackReason::Liveness(LivenessFallbackReason::PropagationEvents),
                "liveness-propagation-events",
            ),
            (
                CoalescingFallbackReason::Liveness(LivenessFallbackReason::RetainedSegments),
                "liveness-retained-segments",
            ),
            (
                CoalescingFallbackReason::Liveness(LivenessFallbackReason::DerivedBoundOverflow),
                "liveness-derived-bound-overflow",
            ),
            (CoalescingFallbackReason::WorkLimit, "work-limit"),
            (
                CoalescingFallbackReason::DerivedBoundOverflow,
                "derived-bound-overflow",
            ),
        ] {
            assert_eq!(reason.code(), expected);
            assert_eq!(reason.to_string(), expected);
        }
    }

    #[test]
    fn candidates_keep_first_authoritative_edge_occurrence_not_value_id_order() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("candidate-order"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let middle = builder.create_block(OriginId::UNKNOWN).unwrap();
        let tail = builder.create_block(OriginId::UNKNOWN).unwrap();
        let tail_parameter = builder
            .append_block_parameter(tail, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let middle_parameter = builder
            .append_block_parameter(middle, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let initial = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        assert!(tail_parameter < middle_parameter && middle_parameter < initial);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(middle, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(middle).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(tail, vec![middle_parameter])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(tail).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![tail_parameter]),
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
        let liveness =
            LivenessResult::for_baseline(&core, &inventory, &demand, LivenessLimits::derived())
                .unwrap();
        let body = core.function(function).unwrap().body().unwrap();
        let mut budget = WorkBudget::new(u64::MAX);
        let actual = candidates(
            function,
            body,
            inventory.function(function).unwrap(),
            liveness.function(function).unwrap().completed().unwrap(),
            &mut budget,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            actual,
            vec![
                Candidate {
                    low: middle_parameter,
                    high: initial,
                },
                Candidate {
                    low: tail_parameter,
                    high: middle_parameter,
                },
            ]
        );
    }

    #[test]
    fn transitive_group_checks_every_member_and_keeps_the_first_edge_winner() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("transitive-interference"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let first = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let second = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let left = builder.create_block(OriginId::UNKNOWN).unwrap();
        let right = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
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
        builder.switch_to_block(left).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![first])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(right).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![second])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![joined]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let assignment = assign(&core);
        assert_eq!(
            home(&assignment, function, first),
            home(&assignment, function, joined)
        );
        assert_ne!(
            home(&assignment, function, second),
            home(&assignment, function, joined)
        );
        let decision = assignment.function(function).unwrap().coalescing().unwrap();
        assert_eq!(decision.statistics().candidates_considered(), 2);
        assert_eq!(decision.statistics().merges_accepted(), 1);
    }

    #[test]
    fn entry_abi_parameter_is_an_exclusive_singleton_even_after_it_dies() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("pinned-entry"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let entry_parameter = builder
            .body()
            .block(builder.entry_block())
            .unwrap()
            .parameters()[0]
            .value();
        let next = builder.create_block(OriginId::UNKNOWN).unwrap();
        let next_parameter = builder
            .append_block_parameter(next, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(next, vec![entry_parameter])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(next).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![next_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let assignment = assign(&core);
        assert_ne!(
            home(&assignment, function, entry_parameter),
            home(&assignment, function, next_parameter)
        );
        let decision = assignment.function(function).unwrap().coalescing().unwrap();
        assert_eq!(decision.statistics().candidates_considered(), 1);
        assert_eq!(decision.statistics().merges_accepted(), 0);
    }

    #[test]
    fn loop_carried_wrapping_add_reuses_only_a_permitted_semantic_operand() {
        let left = wrapping_loop(AddShape::Left);
        let left_assignment = assign(&left.core);
        assert_eq!(
            home(&left_assignment, left.function, left.parameter),
            home(&left_assignment, left.function, left.result)
        );

        let right = wrapping_loop(AddShape::Right);
        let right_assignment = assign(&right.core);
        assert_ne!(
            home(&right_assignment, right.function, right.parameter),
            home(&right_assignment, right.function, right.result)
        );

        let same = wrapping_loop(AddShape::Same);
        let same_assignment = assign(&same.core);
        assert_eq!(
            home(&same_assignment, same.function, same.parameter),
            home(&same_assignment, same.function, same.result)
        );
    }

    #[test]
    fn simultaneous_block_parameter_definitions_never_share_a_home() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("duplicate-source"),
                vec![],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let source = builder.i32_constant(7, OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let second = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![source, source])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![first, second]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();

        let assignment = assign(&core);
        assert_ne!(
            home(&assignment, function, first),
            home(&assignment, function, second)
        );
    }

    #[test]
    fn late_read_scalar_recipes_reject_loop_carried_operand_reuse() {
        let (overflow_core, overflow_function, overflow_parameter, overflow_sum) =
            overflowing_loop();
        let overflow_assignment = assign(&overflow_core);
        assert_ne!(
            home(&overflow_assignment, overflow_function, overflow_parameter),
            home(&overflow_assignment, overflow_function, overflow_sum)
        );

        let (not_core, not_function, not_parameter, negated) = bool_not_loop();
        let not_assignment = assign(&not_core);
        assert_ne!(
            home(&not_assignment, not_function, not_parameter),
            home(&not_assignment, not_function, negated)
        );
    }

    #[test]
    fn a_call_result_reuses_only_an_argument_dead_before_result_definition() {
        let (dead_core, dead_caller, dead_parameter, dead_result) = call_loop(false);
        let dead_assignment = assign(&dead_core);
        assert_eq!(
            home(&dead_assignment, dead_caller, dead_parameter),
            home(&dead_assignment, dead_caller, dead_result)
        );

        let (live_core, live_caller, live_parameter, live_result) = call_loop(true);
        let live_assignment = assign(&live_core);
        assert_ne!(
            home(&live_assignment, live_caller, live_parameter),
            home(&live_assignment, live_caller, live_result)
        );
    }

    #[test]
    fn two_demanded_call_results_remain_distinct_through_loop_carried_merges() {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let pair_function = core
            .declare_function(
                Some("pair"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut pair_builder = FunctionBuilder::new(&core, &sources, pair_function).unwrap();
        let pair_parameters = pair_builder
            .body()
            .block(pair_builder.entry_block())
            .unwrap()
            .parameters()
            .iter()
            .map(crate::ir::core::BlockParam::value)
            .collect::<Vec<_>>();
        pair_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(pair_parameters),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(pair_function, pair_builder.finish().unwrap())
            .unwrap();

        let calling_function = core
            .declare_function(
                Some("pair-loop"),
                vec![],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, calling_function).unwrap();
        let first_initial = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let second_initial = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first_parameter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let second_parameter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let first_returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let second_returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    header,
                    vec![first_initial, second_initial],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let results = builder
            .call(
                pair_function,
                vec![first_parameter, second_parameter],
                OriginId::UNKNOWN,
            )
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(header, results.clone()),
                    else_target: BlockTarget::new(exit, results.clone()),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![first_returned, second_returned]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(calling_function, builder.finish().unwrap())
            .unwrap();

        let assignment = assign(&core);
        assert_ne!(
            home(&assignment, calling_function, results[0]),
            home(&assignment, calling_function, results[1])
        );
        assert_ne!(
            home(&assignment, calling_function, first_parameter),
            home(&assignment, calling_function, second_parameter)
        );
    }

    #[test]
    fn large_candidate_free_function_freezes_at_the_dense_mapping_work_bound() {
        const VALUE_COUNT: usize = 4_096;

        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("candidate-free"),
                vec![],
                vec![CoreType::I32; VALUE_COUNT],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let mut values = Vec::with_capacity(VALUE_COUNT);
        for value in 0..VALUE_COUNT {
            values.push(
                builder
                    .i32_constant(i32::try_from(value).unwrap(), OriginId::UNKNOWN)
                    .unwrap(),
            );
        }
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(values.clone()),
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
        let liveness =
            LivenessResult::for_baseline(&core, &inventory, &demand, LivenessLimits::derived())
                .unwrap();
        let body = core.function(function).unwrap().body().unwrap();
        let semantic = inventory.function(function).unwrap();
        assert!(semantic.edges().is_empty());
        let work_limit = u64::try_from(semantic.reachable_values().len()).unwrap();

        let outcome = coalesce_with_limit(
            function,
            body,
            semantic,
            liveness.function(function).unwrap().completed().unwrap(),
            work_limit,
        )
        .unwrap();

        let groups = outcome
            .groups()
            .expect("candidate-free fast path completes");
        assert_eq!(outcome.statistics().work_used(), work_limit);
        assert_eq!(outcome.statistics().candidates_considered(), 0);
        assert_eq!(outcome.statistics().merges_accepted(), 0);
        assert_eq!(groups.len(), VALUE_COUNT);
        for (index, value) in values.into_iter().enumerate() {
            assert_eq!(groups.group(value), Some(index));
        }
    }

    #[test]
    fn coalescing_work_exhaustion_publishes_no_partial_grouping() {
        let fixture = wrapping_loop(AddShape::Left);
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let demand = RuntimeDemand::for_level(
            &fixture.core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let liveness = LivenessResult::for_baseline(
            &fixture.core,
            &inventory,
            &demand,
            LivenessLimits::derived(),
        )
        .unwrap();
        let completed = liveness
            .function(fixture.function)
            .unwrap()
            .completed()
            .unwrap();
        let body = fixture
            .core
            .function(fixture.function)
            .unwrap()
            .body()
            .unwrap();
        let outcome = coalesce_with_limit(
            fixture.function,
            body,
            inventory.function(fixture.function).unwrap(),
            completed,
            0,
        )
        .unwrap();
        assert!(matches!(
            outcome,
            CoalescingOutcome::DistinctFallback {
                reason: CoalescingFallbackReason::WorkLimit,
                ..
            }
        ));
        assert!(outcome.groups().is_none());
    }

    #[test]
    fn late_work_exhaustion_discards_already_tentative_unions_and_reports_zero_merges() {
        let fixture = wrapping_loop(AddShape::Left);
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let demand = RuntimeDemand::for_level(
            &fixture.core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let liveness = LivenessResult::for_baseline(
            &fixture.core,
            &inventory,
            &demand,
            LivenessLimits::derived(),
        )
        .unwrap();
        let completed = liveness
            .function(fixture.function)
            .unwrap()
            .completed()
            .unwrap();
        let body = fixture
            .core
            .function(fixture.function)
            .unwrap()
            .body()
            .unwrap();
        let semantic = inventory.function(fixture.function).unwrap();
        let complete =
            coalesce_with_limit(fixture.function, body, semantic, completed, u64::MAX).unwrap();
        assert!(complete.statistics().merges_accepted() > 0);
        let full_work = complete.statistics().work_used();
        let limit = full_work - 1;
        let fallback =
            coalesce_with_limit(fixture.function, body, semantic, completed, limit).unwrap();
        assert_eq!(
            fallback.fallback_reason(),
            Some(CoalescingFallbackReason::WorkLimit)
        );
        assert!(fallback.groups().is_none());
        assert_eq!(fallback.statistics().merges_accepted(), 0);
        assert_eq!(
            fallback.statistics().candidates_considered(),
            complete.statistics().candidates_considered(),
            "one-less-than-complete work reaches the final freeze after every candidate"
        );
        let repeated =
            coalesce_with_limit(fixture.function, body, semantic, completed, limit).unwrap();
        assert!(repeated.groups().is_none());
        assert_eq!(repeated.fallback_reason(), fallback.fallback_reason());
        assert_eq!(repeated.statistics(), fallback.statistics());
    }

    #[test]
    fn incomplete_liveness_forces_distinct_assignment_for_the_whole_function() {
        let fixture = wrapping_loop(AddShape::Left);
        let inventory = SemanticInventory::new(&fixture.core).unwrap();
        let demand = RuntimeDemand::for_level(
            &fixture.core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let liveness = LivenessResult::for_baseline(
            &fixture.core,
            &inventory,
            &demand,
            LivenessLimits::derived().with_event_limit(LivenessEventLimit::new(0)),
        )
        .unwrap();
        let assignment =
            HomeAssignment::for_baseline(&fixture.core, &inventory, &demand, &liveness).unwrap();
        assert_ne!(
            home(&assignment, fixture.function, fixture.parameter),
            home(&assignment, fixture.function, fixture.result)
        );
        let decision = assignment
            .function(fixture.function)
            .unwrap()
            .coalescing()
            .unwrap();
        assert_eq!(
            decision.fallback_reason(),
            Some(CoalescingFallbackReason::Liveness(
                LivenessFallbackReason::PropagationEvents
            ))
        );
        assert_eq!(decision.statistics().merges_accepted(), 0);
    }

    struct WrappingLoop {
        core: CoreProgram,
        function: FunctionId,
        parameter: ValueId,
        result: ValueId,
    }

    fn wrapping_loop(shape: AddShape) -> WrappingLoop {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("wrapping-loop"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let initial = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let other = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let (left, right) = match shape {
            AddShape::Left => (parameter, other),
            AddShape::Right => (other, parameter),
            AddShape::Same => (parameter, parameter),
        };
        let result = builder
            .i32_add_wrapping(left, right, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(header, vec![result]),
                    else_target: BlockTarget::new(exit, vec![result]),
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
        WrappingLoop {
            core,
            function,
            parameter,
            result,
        }
    }

    fn overflowing_loop() -> (CoreProgram, FunctionId, ValueId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("overflow-loop"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let initial = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let (sum, overflowed) = builder
            .i32_add_overflowing(parameter, one, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: overflowed,
                    then_target: BlockTarget::new(header, vec![sum]),
                    else_target: BlockTarget::new(exit, vec![sum]),
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
        (core, function, parameter, sum)
    }

    fn bool_not_loop() -> (CoreProgram, FunctionId, ValueId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("not-loop"),
                vec![],
                vec![CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, function).unwrap();
        let initial = builder.bool_constant(true, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
            .append_block_parameter(header, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let returned = builder
            .append_block_parameter(exit, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let negated = builder.bool_not(parameter, OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: negated,
                    then_target: BlockTarget::new(header, vec![negated]),
                    else_target: BlockTarget::new(exit, vec![negated]),
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
        (core, function, parameter, negated)
    }

    fn call_loop(argument_live_after_call: bool) -> (CoreProgram, FunctionId, ValueId, ValueId) {
        let sources = SourceContext::new();
        let mut core = CoreProgram::new();
        let identity_function = core
            .declare_function(
                Some("identity"),
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
                Some("call-loop"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, &sources, calling_function).unwrap();
        let initial = builder.i32_constant(5, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let parameter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let returned = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![initial])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let result = builder
            .call(identity_function, vec![parameter], OriginId::UNKNOWN)
            .unwrap()[0];
        let returned_value = if argument_live_after_call {
            builder
                .i32_add_wrapping(result, parameter, OriginId::UNKNOWN)
                .unwrap()
        } else {
            result
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(header, vec![result]),
                    else_target: BlockTarget::new(exit, vec![returned_value]),
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
        core.define_function(calling_function, builder.finish().unwrap())
            .unwrap();
        (core, calling_function, parameter, result)
    }

    fn assign(core: &CoreProgram) -> HomeAssignment {
        let inventory = SemanticInventory::new(core).unwrap();
        let demand = RuntimeDemand::for_level(
            core,
            &inventory,
            MinecraftOptimizationLevel::Baseline,
            RuntimeDemandLimits::derived(),
        )
        .unwrap();
        let liveness =
            LivenessResult::for_baseline(core, &inventory, &demand, LivenessLimits::derived())
                .unwrap();
        HomeAssignment::for_baseline(core, &inventory, &demand, &liveness).unwrap()
    }

    fn home(
        assignment: &HomeAssignment,
        function: FunctionId,
        value: ValueId,
    ) -> crate::lower::minecraft::assignment::AssignedHomeId {
        assignment
            .function(function)
            .unwrap()
            .value_assignment(value)
            .unwrap()
            .home()
    }
}
