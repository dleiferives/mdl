//! In-place edits guarded by type, dominance, placement, and effect checks.

use std::error::Error;
use std::fmt;

use super::analysis::definition_block;
use super::{
    CoreProgram, Diagnostics, Dominance, DominatorTree, EffectClass, FunctionBody, FunctionId,
    InstId, PlacementIndex, Terminator, TerminatorKind, UseIndex, UseSite, ValueDef, ValueId,
    verify_function,
};
use crate::source::SourceContext;

/// A body-scoped, verified in-place editing surface.
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
        Ok(Self {
            program,
            sources,
            function,
            body,
        })
    }

    /// Returns a read-only view of the body being edited.
    #[must_use]
    pub const fn body(&self) -> &FunctionBody {
        self.body
    }

    #[cfg(test)]
    pub(crate) fn body_mut_for_test(&mut self) -> &mut FunctionBody {
        self.body
    }

    /// Replaces every attached use of one value with an equal-typed dominating value.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation for invalid values, a type mismatch, a
    /// detached replacement definition, or a reachable use not dominated by the
    /// replacement.
    pub fn replace_value(&mut self, old: ValueId, new: ValueId) -> Result<(), EditError> {
        self.preflight_value_replacement(old, new)?;
        self.apply_value_replacement(old, new);
        Ok(())
    }

    /// Replaces all results of one pure instruction with equal-typed results of
    /// another attached pure instruction, then detaches the old instruction.
    ///
    /// Semantic equivalence is the caller's proof obligation.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation unless both instructions are attached and
    /// pure, result contracts match, and every replacement dominates every use.
    pub fn replace_pure_inst(&mut self, old: InstId, replacement: InstId) -> Result<(), EditError> {
        if old == replacement {
            return Ok(());
        }
        let placement = PlacementIndex::new(self.body);
        if !placement.is_instruction_attached(old) {
            return Err(EditError::InvalidInstruction { instruction: old });
        }
        if !placement.is_instruction_attached(replacement) {
            return Err(EditError::InvalidInstruction {
                instruction: replacement,
            });
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
        let pairs = old_data
            .results
            .iter()
            .copied()
            .zip(replacement_data.results.iter().copied())
            .collect::<Vec<_>>();
        for (old_value, new_value) in &pairs {
            self.preflight_value_replacement(*old_value, *new_value)?;
        }

        for (old_value, new_value) in pairs {
            self.apply_value_replacement(old_value, new_value);
        }
        self.detach_instruction(old)?;
        Ok(())
    }

    /// Detaches an unused pure instruction.
    ///
    /// # Errors
    ///
    /// Returns an error before mutation for a detached instruction, unknown effects,
    /// or any attached use of a result.
    pub fn erase_pure_inst(&mut self, instruction: InstId) -> Result<(), EditError> {
        let placement = PlacementIndex::new(self.body);
        if !placement.is_instruction_attached(instruction) {
            return Err(EditError::InvalidInstruction { instruction });
        }
        let data = self
            .body
            .instruction(instruction)
            .ok_or(EditError::InvalidInstruction { instruction })?;
        if data.op.effects() != EffectClass::Pure {
            return Err(EditError::NotPure { instruction });
        }
        let uses = UseIndex::new(self.body);
        if let Some(result) = data
            .results
            .iter()
            .copied()
            .find(|result| !uses.uses(*result).is_empty())
        {
            return Err(EditError::ValueStillUsed { value: result });
        }
        self.detach_instruction(instruction)
    }

    /// Replaces one attached block terminator and keeps it only if the resulting
    /// function verifies.
    ///
    /// # Errors
    ///
    /// Returns an error with verifier diagnostics and restores the exact old
    /// terminator when the candidate is structurally invalid.
    pub fn set_terminator(
        &mut self,
        block: super::BlockId,
        replacement: Terminator,
    ) -> Result<(), EditError> {
        let old = {
            let data = self
                .body
                .block_mut(block)
                .ok_or(EditError::InvalidBlock { block })?;
            data.terminator.replace(replacement)
        };
        match verify_function(self.program, self.sources, self.function, self.body) {
            Ok(()) => Ok(()),
            Err(diagnostics) => {
                if let Some(data) = self.body.block_mut(block) {
                    data.terminator = old;
                }
                Err(EditError::InvalidTerminator(diagnostics))
            }
        }
    }

    /// Detaches every attached block not reachable from entry.
    ///
    /// The entire unreachable subgraph is removed from executable layout at once;
    /// allocated IDs and data remain available to the debug dumper.
    ///
    /// # Errors
    ///
    /// Returns an error only if the edited body was unexpectedly invalid.
    pub fn detach_unreachable_blocks(&mut self) -> Result<usize, EditError> {
        let cfg = super::ControlFlowGraph::new(self.body);
        let reachability = super::Reachability::new(&cfg);
        let retained = self
            .body
            .block_order
            .iter()
            .copied()
            .filter(|block| reachability.contains(*block))
            .collect::<Vec<_>>();
        let removed = self.body.block_order.len() - retained.len();
        drop(reachability);
        drop(cfg);
        self.body.block_order = retained;
        verify_function(self.program, self.sources, self.function, self.body)
            .map_err(EditError::InvalidResult)?;
        Ok(removed)
    }

    fn preflight_value_replacement(&self, old: ValueId, new: ValueId) -> Result<(), EditError> {
        let old_data = self
            .body
            .value(old)
            .ok_or(EditError::InvalidValue { value: old })?;
        let new_data = self
            .body
            .value(new)
            .ok_or(EditError::InvalidValue { value: new })?;
        if old_data.ty != new_data.ty {
            return Err(EditError::TypeMismatch {
                old: old_data.ty,
                new: new_data.ty,
            });
        }
        if old == new {
            return Ok(());
        }
        let placement = PlacementIndex::new(self.body);
        let Some(new_block) = definition_block(self.body, &placement, new) else {
            return Err(EditError::DetachedDefinition { value: new });
        };
        let uses = UseIndex::new(self.body);
        let cfg = super::ControlFlowGraph::new(self.body);
        let dominators = DominatorTree::new(&cfg);
        for use_site in uses.uses(old) {
            let use_block = use_site.block();
            if new_block == use_block {
                if !definition_precedes(self.body, &placement, new, *use_site) {
                    return Err(EditError::DoesNotDominate {
                        value: new,
                        use_block,
                    });
                }
            } else if dominators.dominates(new_block, use_block) == Dominance::DoesNotDominate {
                return Err(EditError::DoesNotDominate {
                    value: new,
                    use_block,
                });
            }
        }
        Ok(())
    }

    fn apply_value_replacement(&mut self, old: ValueId, new: ValueId) {
        if old == new {
            return;
        }
        let blocks = self.body.block_order.clone();
        for block in blocks {
            let instructions = self
                .body
                .block(block)
                .map(|data| data.instructions.clone())
                .unwrap_or_default();
            for instruction in instructions {
                if let Some(data) = self.body.instruction_mut(instruction) {
                    for operand in &mut data.operands {
                        if *operand == old {
                            *operand = new;
                        }
                    }
                }
            }
            let Some(data) = self.body.block_mut(block) else {
                continue;
            };
            let Some(terminator) = &mut data.terminator else {
                continue;
            };
            match &mut terminator.kind {
                TerminatorKind::Jump(target) => replace_in_slice(&mut target.arguments, old, new),
                TerminatorKind::Branch {
                    condition,
                    then_target,
                    else_target,
                } => {
                    if *condition == old {
                        *condition = new;
                    }
                    replace_in_slice(&mut then_target.arguments, old, new);
                    replace_in_slice(&mut else_target.arguments, old, new);
                }
                TerminatorKind::Return(values) => replace_in_slice(values, old, new),
                TerminatorKind::Unreachable => {}
            }
        }
    }

    fn detach_instruction(&mut self, instruction: InstId) -> Result<(), EditError> {
        let placement = PlacementIndex::new(self.body);
        let (block, _) = placement
            .instruction(instruction)
            .ok_or(EditError::InvalidInstruction { instruction })?;
        drop(placement);
        let data = self
            .body
            .block_mut(block)
            .ok_or(EditError::InvalidBlock { block })?;
        let Some(position) = data
            .instructions
            .iter()
            .position(|candidate| *candidate == instruction)
        else {
            return Err(EditError::InvalidInstruction { instruction });
        };
        data.instructions.remove(position);
        Ok(())
    }
}

fn replace_in_slice(values: &mut [ValueId], old: ValueId, new: ValueId) {
    for value in values {
        if *value == old {
            *value = new;
        }
    }
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

/// Rejected in-place edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EditError {
    /// The editor was opened on invalid input IR.
    InvalidInput(Diagnostics),
    /// A block is absent or detached.
    InvalidBlock { block: super::BlockId },
    /// An instruction is absent or detached.
    InvalidInstruction { instruction: InstId },
    /// A value is absent.
    InvalidValue { value: ValueId },
    /// Replacement value types differ.
    TypeMismatch {
        old: super::CoreType,
        new: super::CoreType,
    },
    /// A replacement value is defined in detached code.
    DetachedDefinition { value: ValueId },
    /// A replacement does not dominate one reachable use.
    DoesNotDominate {
        value: ValueId,
        use_block: super::BlockId,
    },
    /// Generic editing requires a pure operation.
    NotPure { instruction: InstId },
    /// A result still has attached uses.
    ValueStillUsed { value: ValueId },
    /// Instruction result arities differ.
    ResultCount { old: usize, replacement: usize },
    /// A candidate terminator failed verification; the old terminator was restored.
    InvalidTerminator(Diagnostics),
    /// An ostensibly safe edit unexpectedly produced invalid IR.
    InvalidResult(Diagnostics),
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for EditError {}
