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
                if let Some(slot) = instructions.get_mut(index)
                    && slot.is_none()
                {
                    *slot = Some((*block, position));
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
        let mut uses = vec![vec![]; body.values.len()];
        let mut push = |value: ValueId, site: UseSite| {
            if let Ok(index) = usize::try_from(value.index())
                && let Some(value_uses) = uses.get_mut(index)
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
            let Some(terminator) = body.block(*block).and_then(|data| data.terminator.as_ref())
            else {
                continue;
            };
            terminator.kind.for_each_successor(|target| {
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
    cfg: &'a ControlFlowGraph<'a>,
    reachable: Reachability<'a>,
    immediate: Vec<Option<BlockId>>,
}

impl<'a> DominatorTree<'a> {
    /// Computes Cooper-Harvey-Kennedy immediate dominators.
    #[must_use]
    pub fn new(cfg: &'a ControlFlowGraph<'a>) -> Self {
        let reachable = Reachability::new(cfg);
        let rpo = cfg.reverse_postorder();
        let mut immediate = vec![None; cfg.body.blocks.len()];
        let mut rpo_position = vec![None; cfg.body.blocks.len()];
        for (position, block) in rpo.iter().copied().enumerate() {
            if let Ok(index) = usize::try_from(block.index()) {
                rpo_position[index] = Some(position);
            }
        }
        if let Some(entry) = rpo.first().copied()
            && let Ok(index) = usize::try_from(entry.index())
        {
            immediate[index] = Some(entry);
        }

        let mut changed = true;
        while changed {
            changed = false;
            for block in rpo.iter().copied().skip(1) {
                let mut defined_predecessors = cfg
                    .predecessors(block)
                    .iter()
                    .copied()
                    .filter(|predecessor| idom(*predecessor, &immediate).is_some());
                let Some(mut new_idom) = defined_predecessors.next() else {
                    continue;
                };
                for predecessor in defined_predecessors {
                    new_idom = intersect(predecessor, new_idom, &immediate, &rpo_position);
                }
                let Ok(index) = usize::try_from(block.index()) else {
                    continue;
                };
                if immediate[index] != Some(new_idom) {
                    immediate[index] = Some(new_idom);
                    changed = true;
                }
            }
        }
        Self {
            cfg,
            reachable,
            immediate,
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
        let entry = self.cfg.body.entry;
        let mut cursor = use_block;
        loop {
            if cursor == definition {
                return Dominance::Dominates;
            }
            if cursor == entry {
                return Dominance::DoesNotDominate;
            }
            let Some(parent) = self.immediate_dominator(cursor) else {
                return Dominance::DoesNotDominate;
            };
            cursor = parent;
        }
    }

    /// Returns the borrowed reachability view.
    #[must_use]
    pub const fn reachability(&self) -> &Reachability<'a> {
        &self.reachable
    }
}

fn idom(block: BlockId, immediate: &[Option<BlockId>]) -> Option<BlockId> {
    let index = usize::try_from(block.index()).ok()?;
    immediate.get(index).copied().flatten()
}

fn rpo_index(block: BlockId, positions: &[Option<usize>]) -> usize {
    usize::try_from(block.index())
        .ok()
        .and_then(|index| positions.get(index))
        .copied()
        .flatten()
        .unwrap_or(usize::MAX)
}

fn intersect(
    mut left: BlockId,
    mut right: BlockId,
    immediate: &[Option<BlockId>],
    positions: &[Option<usize>],
) -> BlockId {
    while left != right {
        while rpo_index(left, positions) > rpo_index(right, positions) {
            left = idom(left, immediate).unwrap_or(left);
        }
        while rpo_index(right, positions) > rpo_index(left, positions) {
            right = idom(right, immediate).unwrap_or(right);
        }
    }
    left
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
