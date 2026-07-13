//! Deterministic analyses derived from authoritative body layout.

use super::{BlockId, FunctionBody, InstId, TerminatorKind, ValueDef, ValueId};
use crate::entity::EntityId;

/// Attached parent block and instruction position for every allocated instruction.
#[derive(Debug)]
pub struct PlacementIndex<'a> {
    body: &'a FunctionBody,
    instructions: Vec<Option<(BlockId, usize)>>,
    attached_blocks: Vec<bool>,
}

impl<'a> PlacementIndex<'a> {
    /// Derives placement from block and instruction order.
    #[must_use]
    pub fn new(body: &'a FunctionBody) -> Self {
        let mut instructions = vec![None; body.instructions.len()];
        let mut attached_blocks = vec![false; body.blocks.len()];
        for block in &body.block_order {
            let Some(block_index) = usize::try_from(block.index()).ok() else {
                continue;
            };
            let Some(attached) = attached_blocks.get_mut(block_index) else {
                continue;
            };
            if *attached {
                continue;
            }
            *attached = true;
            let Some(data) = body.block(*block) else {
                continue;
            };
            for (position, instruction) in data.instructions.iter().copied().enumerate() {
                let Some(index) = usize::try_from(instruction.index()).ok() else {
                    continue;
                };
                if let Some(slot) = instructions.get_mut(index) {
                    if slot.is_none() {
                        *slot = Some((*block, position));
                    }
                }
            }
        }
        Self {
            body,
            instructions,
            attached_blocks,
        }
    }

    /// Returns the attached parent and local position of an instruction.
    #[must_use]
    pub fn instruction(&self, instruction: InstId) -> Option<(BlockId, usize)> {
        let index = usize::try_from(instruction.index()).ok()?;
        self.instructions.get(index).copied().flatten()
    }

    /// Returns whether a block is attached exactly once in the derived view.
    #[must_use]
    pub fn is_block_attached(&self, block: BlockId) -> bool {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| self.attached_blocks.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// Returns whether an instruction is attached to a valid attached block.
    #[must_use]
    pub fn is_instruction_attached(&self, instruction: InstId) -> bool {
        self.instruction(instruction).is_some()
    }

    /// Returns the body from which this borrowed analysis was derived.
    #[must_use]
    pub const fn body(&self) -> &'a FunctionBody {
        self.body
    }
}

/// One use of an SSA value in attached code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UseSite {
    /// Operand of an ordinary instruction.
    InstructionOperand {
        /// Block containing the instruction.
        block: BlockId,
        /// User instruction.
        instruction: InstId,
        /// Zero-based operand index.
        operand_index: usize,
    },
    /// Boolean condition of a branch terminator.
    BranchCondition {
        /// Block containing the terminator.
        block: BlockId,
    },
    /// Argument on one successor edge.
    EdgeArgument {
        /// Block containing the terminator.
        block: BlockId,
        /// Zero-based successor index (then before else).
        successor_index: usize,
        /// Zero-based argument index.
        argument_index: usize,
    },
    /// Value returned by a terminator.
    Return {
        /// Block containing the terminator.
        block: BlockId,
        /// Zero-based result index.
        result_index: usize,
    },
}

impl UseSite {
    /// Returns the attached block containing this use.
    #[must_use]
    pub const fn block(self) -> BlockId {
        match self {
            Self::InstructionOperand { block, .. }
            | Self::BranchCondition { block }
            | Self::EdgeArgument { block, .. }
            | Self::Return { block, .. } => block,
        }
    }
}

/// All attached uses indexed by allocated value identity.
#[derive(Debug)]
pub struct UseIndex<'a> {
    body: &'a FunctionBody,
    uses: Vec<Vec<UseSite>>,
}

impl<'a> UseIndex<'a> {
    /// Derives attached value uses in execution/layout order.
    #[must_use]
    pub fn new(body: &'a FunctionBody) -> Self {
        let placement = PlacementIndex::new(body);
        Self::from_placement(&placement)
    }

    /// Derives attached value uses while reusing an existing placement fact.
    #[must_use]
    pub(crate) fn from_placement(placement: &PlacementIndex<'a>) -> Self {
        let body = placement.body();
        let mut uses = vec![vec![]; body.values.len()];
        let mut push = |value: ValueId, site: UseSite| {
            if let Some(value_uses) = usize::try_from(value.index())
                .ok()
                .and_then(|index| uses.get_mut(index))
            {
                value_uses.push(site);
            }
        };

        for block in &body.block_order {
            if !placement.is_block_attached(*block) {
                continue;
            }
            let Some(data) = body.block(*block) else {
                continue;
            };
            for instruction in &data.instructions {
                if placement.instruction(*instruction).map(|placed| placed.0) != Some(*block) {
                    continue;
                }
                let Some(data) = body.instruction(*instruction) else {
                    continue;
                };
                for (operand_index, operand) in data.operands.iter().copied().enumerate() {
                    push(
                        operand,
                        UseSite::InstructionOperand {
                            block: *block,
                            instruction: *instruction,
                            operand_index,
                        },
                    );
                }
            }
            let Some(terminator) = &data.terminator else {
                continue;
            };
            match &terminator.kind {
                TerminatorKind::Jump(target) => {
                    for (argument_index, value) in target.arguments.iter().copied().enumerate() {
                        push(
                            value,
                            UseSite::EdgeArgument {
                                block: *block,
                                successor_index: 0,
                                argument_index,
                            },
                        );
                    }
                }
                TerminatorKind::Branch {
                    condition,
                    then_target,
                    else_target,
                } => {
                    push(*condition, UseSite::BranchCondition { block: *block });
                    for (successor_index, target) in
                        [then_target, else_target].into_iter().enumerate()
                    {
                        for (argument_index, value) in target.arguments.iter().copied().enumerate()
                        {
                            push(
                                value,
                                UseSite::EdgeArgument {
                                    block: *block,
                                    successor_index,
                                    argument_index,
                                },
                            );
                        }
                    }
                }
                TerminatorKind::Return(results) => {
                    for (result_index, value) in results.iter().copied().enumerate() {
                        push(
                            value,
                            UseSite::Return {
                                block: *block,
                                result_index,
                            },
                        );
                    }
                }
                TerminatorKind::Unreachable => {}
            }
        }
        Self { body, uses }
    }

    /// Returns attached uses of one value, or an empty slice for an invalid ID.
    #[must_use]
    pub fn uses(&self, value: ValueId) -> &[UseSite] {
        usize::try_from(value.index())
            .ok()
            .and_then(|index| self.uses.get(index))
            .map_or(&[], Vec::as_slice)
    }

    /// Returns the body from which this borrowed analysis was derived.
    #[must_use]
    pub const fn body(&self) -> &'a FunctionBody {
        self.body
    }
}

/// Attached successor and predecessor edges.
#[derive(Debug)]
pub struct ControlFlowGraph<'a> {
    body: &'a FunctionBody,
    successors: Vec<Vec<BlockId>>,
    predecessors: Vec<Vec<BlockId>>,
    attached: Vec<bool>,
}

impl<'a> ControlFlowGraph<'a> {
    /// Derives a CFG from valid attached successor targets.
    #[must_use]
    pub fn new(body: &'a FunctionBody) -> Self {
        let placement = PlacementIndex::new(body);
        Self::from_placement(&placement)
    }

    /// Derives a CFG while reusing an existing placement fact.
    #[must_use]
    pub(crate) fn from_placement(placement: &PlacementIndex<'a>) -> Self {
        Self::from_placement_and_terminators(placement, &[])
    }

    /// Derives the projected CFG produced by a dense set of terminator overrides.
    ///
    /// An entry in `overrides` replaces the terminator of the block with the same
    /// dense ID. Missing entries and `None` entries use the body's current
    /// terminator. This constructor only derives edges; callers remain responsible
    /// for validating the complete projected terminator contracts.
    #[must_use]
    pub(crate) fn with_terminator_overrides(
        placement: &PlacementIndex<'a>,
        overrides: &[Option<&TerminatorKind>],
    ) -> Self {
        Self::from_placement_and_terminators(placement, overrides)
    }

    fn from_placement_and_terminators(
        placement: &PlacementIndex<'a>,
        overrides: &[Option<&TerminatorKind>],
    ) -> Self {
        let body = placement.body();
        let mut successors = vec![vec![]; body.blocks.len()];
        let mut predecessors = vec![vec![]; body.blocks.len()];
        let attached = body
            .blocks
            .keys()
            .map(|block| placement.is_block_attached(block))
            .collect::<Vec<_>>();

        for block in &body.block_order {
            if !placement.is_block_attached(*block) {
                continue;
            }
            let Some(block_index) = usize::try_from(block.index()).ok() else {
                continue;
            };
            let projected = overrides.get(block_index).copied().flatten().or_else(|| {
                body.block(*block)
                    .and_then(|data| data.terminator.as_ref())
                    .map(|terminator| &terminator.kind)
            });
            let Some(terminator) = projected else {
                continue;
            };
            terminator.for_each_successor(|target| {
                if !placement.is_block_attached(target.block) {
                    return;
                }
                let Some(target_index) = usize::try_from(target.block.index()).ok() else {
                    return;
                };
                successors[block_index].push(target.block);
                predecessors[target_index].push(*block);
            });
        }
        Self {
            body,
            successors,
            predecessors,
            attached,
        }
    }

    /// Returns valid attached successors in semantic order.
    #[must_use]
    pub fn successors(&self, block: BlockId) -> &[BlockId] {
        Self::block_slice(&self.successors, block)
    }

    /// Returns attached predecessors in stable block/edge order.
    #[must_use]
    pub fn predecessors(&self, block: BlockId) -> &[BlockId] {
        Self::block_slice(&self.predecessors, block)
    }

    /// Returns whether a block participates in this attached CFG.
    #[must_use]
    pub fn is_attached(&self, block: BlockId) -> bool {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| self.attached.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// Computes deterministic DFS postorder from entry.
    #[must_use]
    pub fn postorder(&self) -> Vec<BlockId> {
        if !self.is_attached(self.body.entry) {
            return vec![];
        }
        let mut visited = vec![false; self.body.blocks.len()];
        let mut output = vec![];
        let mut stack = vec![(self.body.entry, 0_usize)];
        if let Ok(index) = usize::try_from(self.body.entry.index()) {
            visited[index] = true;
        }

        while let Some((block, next_successor)) = stack.last_mut() {
            let successors = self.successors(*block);
            if *next_successor < successors.len() {
                let successor = successors[*next_successor];
                *next_successor += 1;
                let Ok(index) = usize::try_from(successor.index()) else {
                    continue;
                };
                if !visited[index] {
                    visited[index] = true;
                    stack.push((successor, 0));
                }
            } else {
                output.push(*block);
                stack.pop();
            }
        }
        output
    }

    /// Computes deterministic reverse postorder from entry.
    #[must_use]
    pub fn reverse_postorder(&self) -> Vec<BlockId> {
        let mut blocks = self.postorder();
        blocks.reverse();
        blocks
    }

    fn block_slice(storage: &[Vec<BlockId>], block: BlockId) -> &[BlockId] {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| storage.get(index))
            .map_or(&[], Vec::as_slice)
    }
}

/// Entry-rooted attached block reachability.
#[derive(Debug)]
pub struct Reachability<'a> {
    cfg: &'a ControlFlowGraph<'a>,
    reachable: Vec<bool>,
}

impl<'a> Reachability<'a> {
    /// Computes reachability from the function entry.
    #[must_use]
    pub fn new(cfg: &'a ControlFlowGraph<'a>) -> Self {
        let mut reachable = vec![false; cfg.body.blocks.len()];
        for block in cfg.reverse_postorder() {
            if let Ok(index) = usize::try_from(block.index()) {
                reachable[index] = true;
            }
        }
        Self { cfg, reachable }
    }

    /// Returns whether the attached block is reachable from entry.
    #[must_use]
    pub fn contains(&self, block: BlockId) -> bool {
        usize::try_from(block.index())
            .ok()
            .and_then(|index| self.reachable.get(index))
            .copied()
            .unwrap_or(false)
    }

    /// Returns the borrowed CFG used by this analysis.
    #[must_use]
    pub const fn cfg(&self) -> &'a ControlFlowGraph<'a> {
        self.cfg
    }
}

/// Result of asking whether one definition block dominates a use block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Dominance {
    /// The definition dominates the reachable use.
    Dominates,
    /// The definition does not dominate the reachable use.
    DoesNotDominate,
    /// The use is unreachable, so cross-block dominance is intentionally undefined.
    UnreachableUse,
}

/// Immediate dominators for entry-reachable attached blocks.
#[derive(Debug)]
pub struct DominatorTree<'a> {
    reachable: Reachability<'a>,
    immediate: Vec<Option<BlockId>>,
    intervals: Vec<Option<(usize, usize)>>,
}

impl<'a> DominatorTree<'a> {
    /// Computes immediate dominators with iterative simple Lengauer-Tarjan.
    ///
    /// The link-eval forest uses path compression and explicit worklists. For
    /// `B` allocated blocks, `V` entry-reachable blocks, and `E` attached CFG
    /// edge occurrences, this takes `O(B + (V + E) log V)` time
    /// and `O(B + V)` temporary space beyond the already-built CFG. The retained
    /// reachability, immediate-dominator, and interval tables use `O(B)` space.
    #[must_use]
    pub fn new(cfg: &'a ControlFlowGraph<'a>) -> Self {
        let reachable = Reachability::new(cfg);
        let immediate = lengauer_tarjan_immediate_dominators(cfg);
        let intervals = dominance_intervals(cfg, &immediate, &reachable);
        Self {
            reachable,
            immediate,
            intervals,
        }
    }

    /// Returns a block's immediate dominator.
    ///
    /// Entry dominates itself. Detached and unreachable blocks return `None`.
    #[must_use]
    pub fn immediate_dominator(&self, block: BlockId) -> Option<BlockId> {
        idom(block, &self.immediate)
    }

    /// Queries entry-rooted block dominance with explicit unreachable-use handling.
    #[must_use]
    pub fn dominates(&self, definition: BlockId, use_block: BlockId) -> Dominance {
        if !self.reachable.contains(use_block) {
            return Dominance::UnreachableUse;
        }
        if !self.reachable.contains(definition) {
            return Dominance::DoesNotDominate;
        }
        let interval = |block: BlockId| {
            usize::try_from(block.index())
                .ok()
                .and_then(|index| self.intervals.get(index))
                .copied()
                .flatten()
        };
        match (interval(definition), interval(use_block)) {
            (Some((definition_pre, definition_post)), Some((use_pre, use_post)))
                if definition_pre <= use_pre && use_post <= definition_post =>
            {
                Dominance::Dominates
            }
            _ => Dominance::DoesNotDominate,
        }
    }

    /// Returns the borrowed reachability view.
    #[must_use]
    pub const fn reachability(&self) -> &Reachability<'a> {
        &self.reachable
    }
}

/// One entry-reachable block in Lengauer-Tarjan DFS-preorder space.
///
/// Index zero is a virtual root. A real node's `parent` remains the immutable
/// DFS-tree parent while `ancestor` is the path-compressed link-eval forest.
#[derive(Clone, Copy, Debug)]
struct LengauerTarjanNode {
    block: Option<BlockId>,
    parent: usize,
    ancestor: usize,
    label: usize,
    semi: usize,
    idom: usize,
}

impl LengauerTarjanNode {
    const VIRTUAL_ROOT: Self = Self {
        block: None,
        parent: 0,
        ancestor: 0,
        label: 0,
        semi: 0,
        idom: 0,
    };

    const fn discovered(block: BlockId, parent: usize, preorder: usize) -> Self {
        Self {
            block: Some(block),
            parent,
            ancestor: 0,
            label: preorder,
            semi: preorder,
            idom: 0,
        }
    }
}

fn lengauer_tarjan_immediate_dominators(cfg: &ControlFlowGraph<'_>) -> Vec<Option<BlockId>> {
    let mut immediate = vec![None; cfg.body.blocks.len()];
    let (mut nodes, preorder) = lengauer_tarjan_spanning_tree(cfg);
    if nodes.len() == 1 {
        return immediate;
    }

    // Simple Lengauer-Tarjan computes semidominators and relative dominators in
    // reverse DFS preorder. Each node enters exactly one intrusive bucket; two
    // dense arrays avoid both per-bucket allocations and superlinear scratch.
    let mut bucket_heads = vec![0_usize; nodes.len()];
    let mut bucket_next = vec![0_usize; nodes.len()];
    let mut eval_stack = Vec::new();
    for node in (2..nodes.len()).rev() {
        let block = nodes[node]
            .block
            .expect("only the Lengauer-Tarjan virtual root lacks a block");
        for predecessor in cfg.predecessors(block) {
            let Some(predecessor) = dense_block_index(*predecessor)
                .and_then(|index| preorder.get(index))
                .copied()
                .filter(|preorder| *preorder != 0)
            else {
                continue;
            };
            let candidate = lengauer_tarjan_eval(&mut nodes, predecessor, &mut eval_stack);
            nodes[node].semi = nodes[node].semi.min(nodes[candidate].semi);
        }
        let parent = nodes[node].parent;
        let semi = nodes[node].semi;
        debug_assert!((1..node).contains(&semi));
        bucket_next[node] = bucket_heads[semi];
        bucket_heads[semi] = node;
        nodes[node].ancestor = parent;

        let mut pending = std::mem::take(&mut bucket_heads[parent]);
        while pending != 0 {
            let next = bucket_next[pending];
            let candidate = lengauer_tarjan_eval(&mut nodes, pending, &mut eval_stack);
            nodes[pending].idom = if nodes[candidate].semi < nodes[pending].semi {
                candidate
            } else {
                parent
            };
            pending = next;
        }
    }

    // Convert relative dominators into immediate dominators in preorder, where
    // every referenced immediate dominator has already been finalized.
    nodes[1].idom = 1;
    for node in 2..nodes.len() {
        let relative = nodes[node].idom;
        debug_assert!((1..node).contains(&relative));
        if relative != nodes[node].semi {
            nodes[node].idom = nodes[relative].idom;
        }
    }

    for node in 1..nodes.len() {
        let block = nodes[node]
            .block
            .expect("only the Lengauer-Tarjan virtual root lacks a block");
        let dominator = nodes[nodes[node].idom]
            .block
            .expect("every reachable block is dominated by the entry");
        if let Some(index) = dense_block_index(block) {
            immediate[index] = Some(dominator);
        }
    }
    immediate
}

fn lengauer_tarjan_spanning_tree(
    cfg: &ControlFlowGraph<'_>,
) -> (Vec<LengauerTarjanNode>, Vec<usize>) {
    let mut nodes = vec![LengauerTarjanNode::VIRTUAL_ROOT];
    let mut preorder = vec![0_usize; cfg.body.blocks.len()];
    if !cfg.is_attached(cfg.body.entry) {
        return (nodes, preorder);
    }
    let Some(entry_index) = dense_block_index(cfg.body.entry) else {
        return (nodes, preorder);
    };

    preorder[entry_index] = 1;
    nodes.push(LengauerTarjanNode::discovered(cfg.body.entry, 0, 1));
    let mut stack = vec![(cfg.body.entry, 0_usize)];
    while let Some((block, next_successor)) = stack.last_mut() {
        let successors = cfg.successors(*block);
        if *next_successor == successors.len() {
            stack.pop();
            continue;
        }

        let successor = successors[*next_successor];
        *next_successor += 1;
        let Some(successor_index) = dense_block_index(successor) else {
            continue;
        };
        if preorder[successor_index] != 0 {
            continue;
        }
        let parent = dense_block_index(*block)
            .and_then(|index| preorder.get(index))
            .copied()
            .expect("an active DFS frame has a preorder number");
        let successor_preorder = nodes.len();
        preorder[successor_index] = successor_preorder;
        nodes.push(LengauerTarjanNode::discovered(
            successor,
            parent,
            successor_preorder,
        ));
        stack.push((successor, 0));
    }
    (nodes, preorder)
}

/// Path-compressed simple link-eval over the DFS-parent forest.
///
/// Across the complete Lengauer-Tarjan construction this gives the standard
/// `O((V + E) log V)` simple-algorithm bound. Balanced linking can improve the
/// bound to inverse-Ackermann time but needs more state and complexity.
fn lengauer_tarjan_eval(
    nodes: &mut [LengauerTarjanNode],
    node: usize,
    stack: &mut Vec<usize>,
) -> usize {
    debug_assert!(stack.is_empty());
    let mut cursor = node;
    while nodes[nodes[cursor].ancestor].ancestor != 0 {
        stack.push(cursor);
        cursor = nodes[cursor].ancestor;
    }

    while let Some(current) = stack.pop() {
        let ancestor = nodes[current].ancestor;
        if nodes[nodes[ancestor].label].semi < nodes[nodes[current].label].semi {
            nodes[current].label = nodes[ancestor].label;
        }
        nodes[current].ancestor = nodes[ancestor].ancestor;
    }
    nodes[node].label
}

fn dense_block_index(block: BlockId) -> Option<usize> {
    usize::try_from(block.index()).ok()
}

fn dominance_intervals(
    cfg: &ControlFlowGraph<'_>,
    immediate: &[Option<BlockId>],
    reachable: &Reachability<'_>,
) -> Vec<Option<(usize, usize)>> {
    let mut children = vec![vec![]; cfg.body.blocks.len()];
    for block in cfg.body.blocks.keys() {
        if block == cfg.body.entry || !reachable.contains(block) {
            continue;
        }
        let Some(parent) = idom(block, immediate) else {
            continue;
        };
        if let Ok(parent_index) = usize::try_from(parent.index()) {
            children[parent_index].push(block);
        }
    }

    let mut intervals = vec![None; cfg.body.blocks.len()];
    if !reachable.contains(cfg.body.entry) {
        return intervals;
    }

    let mut timestamp = 0_usize;
    let mut stack = vec![(cfg.body.entry, 0_usize)];
    while let Some((block, next_child)) = stack.last_mut() {
        let Ok(index) = usize::try_from(block.index()) else {
            stack.pop();
            continue;
        };
        if *next_child == 0 {
            intervals[index] = Some((timestamp, timestamp));
            timestamp += 1;
        }
        if *next_child < children[index].len() {
            let child = children[index][*next_child];
            *next_child += 1;
            stack.push((child, 0));
        } else {
            if let Some((preorder, _)) = intervals[index] {
                intervals[index] = Some((preorder, timestamp));
                timestamp += 1;
            }
            stack.pop();
        }
    }
    intervals
}

fn idom(block: BlockId, immediate: &[Option<BlockId>]) -> Option<BlockId> {
    let index = usize::try_from(block.index()).ok()?;
    immediate.get(index).copied().flatten()
}

pub(crate) fn definition_block(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    value: ValueId,
) -> Option<BlockId> {
    match body.value(value)?.definition {
        ValueDef::BlockParam { block, .. } if placement.is_block_attached(block) => Some(block),
        ValueDef::InstResult { instruction, .. } => {
            placement.instruction(instruction).map(|placed| placed.0)
        }
        ValueDef::BlockParam { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::core::{
        BlockTarget, CoreProgram, CoreType, FunctionBuilder, Terminator, TerminatorKind,
    };
    use crate::source::{OriginId, SourceContext};

    fn body_from_successors(successors: &[Vec<usize>]) -> FunctionBody {
        assert!(!successors.is_empty());
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("generated-dominance"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = builder.body().block(entry).unwrap().parameters[0].value;
        let mut blocks = vec![entry];
        blocks.extend(
            (1..successors.len()).map(|_| builder.create_block(OriginId::UNKNOWN).unwrap()),
        );

        for (source, targets) in successors.iter().enumerate() {
            assert!(targets.len() <= 2);
            assert!(
                targets
                    .iter()
                    .all(|target| (1..blocks.len()).contains(target))
            );
            builder.switch_to_block(blocks[source]).unwrap();
            let target = |index: usize| BlockTarget::new(blocks[index], vec![]);
            let kind = match targets.as_slice() {
                [] => TerminatorKind::Return(vec![]),
                [only] => TerminatorKind::Jump(target(*only)),
                [then_block, else_block] => TerminatorKind::Branch {
                    condition,
                    then_target: target(*then_block),
                    else_target: target(*else_block),
                },
                _ => unreachable!("the successor-count assertion rejects this case"),
            };
            builder
                .terminate(Terminator::new(kind, OriginId::UNKNOWN))
                .unwrap();
        }
        builder.finish().unwrap()
    }

    fn oracle_dominator_sets(cfg: &ControlFlowGraph<'_>) -> Vec<Option<Vec<bool>>> {
        let block_count = cfg.body.blocks.len();
        let mut reachable = vec![false; block_count];
        let mut stack = vec![cfg.body.entry];
        while let Some(block) = stack.pop() {
            let Some(index) = dense_block_index(block) else {
                continue;
            };
            if reachable[index] || !cfg.is_attached(block) {
                continue;
            }
            reachable[index] = true;
            stack.extend(cfg.successors(block).iter().copied());
        }

        let all_reachable = reachable.clone();
        let mut dominators = reachable
            .iter()
            .map(|is_reachable| is_reachable.then(|| all_reachable.clone()))
            .collect::<Vec<_>>();
        let entry_index = dense_block_index(cfg.body.entry).unwrap();
        let mut entry_set = vec![false; block_count];
        entry_set[entry_index] = true;
        dominators[entry_index] = Some(entry_set);

        loop {
            let mut changed = false;
            for block in cfg.body.blocks.keys() {
                let index = dense_block_index(block).unwrap();
                if block == cfg.body.entry || !reachable[index] {
                    continue;
                }
                let mut predecessors = cfg.predecessors(block).iter().filter_map(|predecessor| {
                    let predecessor = dense_block_index(*predecessor)?;
                    dominators.get(predecessor)?.as_ref()
                });
                let Some(first) = predecessors.next() else {
                    continue;
                };
                let mut next = first.clone();
                for predecessor in predecessors {
                    for (value, predecessor_value) in next.iter_mut().zip(predecessor) {
                        *value &= *predecessor_value;
                    }
                }
                next[index] = true;
                if dominators[index].as_ref() != Some(&next) {
                    dominators[index] = Some(next);
                    changed = true;
                }
            }
            if !changed {
                return dominators;
            }
        }
    }

    fn oracle_immediate_dominator(
        cfg: &ControlFlowGraph<'_>,
        dominators: &[Option<Vec<bool>>],
        block: BlockId,
    ) -> Option<BlockId> {
        let block_index = dense_block_index(block)?;
        let block_dominators = dominators.get(block_index)?.as_ref()?;
        if block == cfg.body.entry {
            return Some(block);
        }
        cfg.body.blocks.keys().find(|candidate| {
            let candidate_index = dense_block_index(*candidate).unwrap();
            block_dominators[candidate_index]
                && *candidate != block
                && cfg.body.blocks.keys().all(|other| {
                    let other_index = dense_block_index(other).unwrap();
                    other == block
                        || other == *candidate
                        || !block_dominators[other_index]
                        || dominators[candidate_index]
                            .as_ref()
                            .is_some_and(|set| set[other_index])
                })
        })
    }

    fn assert_matches_oracle(body: &FunctionBody) {
        let cfg = ControlFlowGraph::new(body);
        assert_cfg_matches_oracle(&cfg);
    }

    fn assert_cfg_matches_oracle(cfg: &ControlFlowGraph<'_>) {
        let oracle = oracle_dominator_sets(cfg);
        let actual = DominatorTree::new(cfg);
        for block in cfg.body.blocks.keys() {
            assert_eq!(
                actual.immediate_dominator(block),
                oracle_immediate_dominator(cfg, &oracle, block),
                "immediate dominator differs for {block:?} in {:?}",
                cfg.body,
            );
            for use_block in cfg.body.blocks.keys() {
                let use_index = dense_block_index(use_block).unwrap();
                let block_index = dense_block_index(block).unwrap();
                let expected = match oracle[use_index].as_ref() {
                    None => Dominance::UnreachableUse,
                    Some(_) if oracle[block_index].is_none() => Dominance::DoesNotDominate,
                    Some(set) if set[block_index] => Dominance::Dominates,
                    Some(_) => Dominance::DoesNotDominate,
                };
                assert_eq!(
                    actual.dominates(block, use_block),
                    expected,
                    "dominance differs for {block:?} -> {use_block:?} in {:?}",
                    cfg.body,
                );
            }
        }
    }

    fn next_random(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        *state
    }

    #[test]
    fn dominance_intervals_match_a_diamond_and_explicit_unreachable_policy() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("dominance-diamond"),
                vec![CoreType::Bool],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let condition = builder.body().block(entry).unwrap().parameters[0].value;
        let left = builder.create_block(OriginId::UNKNOWN).unwrap();
        let right = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let unreachable = builder.create_block(OriginId::UNKNOWN).unwrap();
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
        for block in [left, right] {
            builder.switch_to_block(block).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(join, vec![])),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        for block in [join, unreachable] {
            builder.switch_to_block(block).unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
        }
        let body = builder.finish().unwrap();
        let cfg = ControlFlowGraph::new(&body);
        let dominators = DominatorTree::new(&cfg);

        assert_eq!(dominators.immediate_dominator(entry), Some(entry));
        assert_eq!(dominators.immediate_dominator(left), Some(entry));
        assert_eq!(dominators.immediate_dominator(right), Some(entry));
        assert_eq!(dominators.immediate_dominator(join), Some(entry));
        assert_eq!(dominators.immediate_dominator(unreachable), None);
        assert_eq!(dominators.dominates(entry, join), Dominance::Dominates);
        assert_eq!(dominators.dominates(left, left), Dominance::Dominates);
        assert_eq!(dominators.dominates(left, join), Dominance::DoesNotDominate);
        assert_eq!(
            dominators.dominates(unreachable, join),
            Dominance::DoesNotDominate
        );
        assert_eq!(
            dominators.dominates(entry, unreachable),
            Dominance::UnreachableUse
        );
    }

    #[test]
    fn deep_dominator_intervals_use_no_host_recursion() {
        const BLOCKS: usize = 20_000;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("deep-dominance"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let blocks = (0..BLOCKS)
            .map(|_| builder.create_block(OriginId::UNKNOWN).unwrap())
            .collect::<Vec<_>>();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(blocks[0], vec![])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        for (index, block) in blocks.iter().copied().enumerate() {
            builder.switch_to_block(block).unwrap();
            let kind = blocks.get(index + 1).map_or_else(
                || TerminatorKind::Return(vec![]),
                |successor| TerminatorKind::Jump(BlockTarget::new(*successor, vec![])),
            );
            builder
                .terminate(Terminator::new(kind, OriginId::UNKNOWN))
                .unwrap();
        }
        let body = builder.finish().unwrap();
        let cfg = ControlFlowGraph::new(&body);
        let dominators = DominatorTree::new(&cfg);
        let tail = *blocks.last().unwrap();

        assert_eq!(dominators.dominates(entry, tail), Dominance::Dominates);
        assert_eq!(
            dominators.immediate_dominator(tail),
            blocks.get(BLOCKS - 2).copied()
        );
    }

    #[test]
    fn simple_lengauer_tarjan_matches_an_independent_set_oracle_on_generated_valid_cfgs() {
        let explicit_irreducible =
            body_from_successors(&[vec![1, 2], vec![1, 3], vec![3, 1], vec![1, 2], vec![4, 4]]);
        assert_matches_oracle(&explicit_irreducible);

        let mut random = 0x9e37_79b9_7f4a_7c15_u64;
        for block_count in 1..=9 {
            for _ in 0..256 {
                let mut successors = vec![vec![]; block_count];
                if block_count > 1 {
                    for targets in &mut successors {
                        let successor_count =
                            usize::try_from(next_random(&mut random) % 3).unwrap();
                        let target_count = u64::try_from(block_count - 1).unwrap();
                        targets.extend((0..successor_count).map(|_| {
                            1 + usize::try_from(next_random(&mut random) % target_count).unwrap()
                        }));
                    }
                }
                let body = body_from_successors(&successors);
                assert_matches_oracle(&body);
            }
        }
    }

    #[test]
    fn duplicate_backedges_to_entry_preserve_root_dominance() {
        let body = body_from_successors(&[vec![1], vec![]]);
        let entry = body.entry();
        let loop_block = body.blocks.keys().nth(1).unwrap();
        let condition = body.block(entry).unwrap().parameters[0].value;
        let backedges = TerminatorKind::Branch {
            condition,
            then_target: BlockTarget::new(entry, vec![condition]),
            else_target: BlockTarget::new(entry, vec![condition]),
        };
        let placement = PlacementIndex::new(&body);
        let mut overrides = vec![None; body.blocks.len()];
        overrides[dense_block_index(loop_block).unwrap()] = Some(&backedges);
        let cfg = ControlFlowGraph::with_terminator_overrides(&placement, &overrides);

        assert_eq!(cfg.predecessors(entry), &[loop_block, loop_block]);
        assert_cfg_matches_oracle(&cfg);
    }

    #[test]
    fn duplicate_edges_self_loops_and_irreducible_cycles_are_deterministic() {
        let body =
            body_from_successors(&[vec![1, 2], vec![1, 3], vec![3, 1], vec![1, 2], vec![4, 4]]);
        let cfg = ControlFlowGraph::new(&body);
        let first = DominatorTree::new(&cfg);
        let second = DominatorTree::new(&cfg);

        assert_eq!(first.immediate, second.immediate);
        assert_eq!(first.intervals, second.intervals);
        assert_matches_oracle(&body);
    }

    #[test]
    fn wide_adversarial_graph_uses_linear_scratch_without_fixed_point_iteration() {
        const WIDTH: usize = 20_000;

        // Two disjoint fan-out chains reach every leaf. Every leaf's DFS parent
        // is deep in the first chain while its semidominator and immediate
        // dominator are the entry. This is quadratic for Semi-NCA's usual
        // parent-climb phase and creates 20,000 direct dominator-tree children;
        // simple Lengauer-Tarjan handles it with path compression and linear
        // auxiliary storage.
        let left_start = 1;
        let right_start = left_start + WIDTH;
        let leaf_start = right_start + WIDTH;
        let mut successors = vec![vec![]; 1 + 3 * WIDTH];
        successors[0] = vec![left_start, right_start];
        for offset in 0..WIDTH {
            let leaf = leaf_start + offset;
            successors[left_start + offset].push(leaf);
            successors[right_start + offset].push(leaf);
            if offset + 1 < WIDTH {
                successors[left_start + offset].push(left_start + offset + 1);
                successors[right_start + offset].push(right_start + offset + 1);
            }
        }

        let body = body_from_successors(&successors);
        let entry = body.entry();
        let cfg = ControlFlowGraph::new(&body);
        let dominators = DominatorTree::new(&cfg);
        for leaf in body.blocks.keys().skip(leaf_start) {
            assert_eq!(dominators.immediate_dominator(leaf), Some(entry));
        }
    }
}
