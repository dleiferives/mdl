//! Body-scoped construction of well-typed Core IR.

use std::error::Error;
use std::fmt;

use super::{
    BlockData, BlockId, BlockParam, CoreOp, CoreProgram, CoreType, Diagnostics, FunctionBody,
    FunctionId, I32Predicate, InstData, InstId, Terminator, ValueData, ValueDef, ValueId,
    verify_function,
};
use crate::entity::{EntityLimitError, EntityVec};
use crate::source::{OriginId, SourceContext};

/// Builds one function body while deriving all operation result types.
pub struct FunctionBuilder<'a> {
    program: &'a CoreProgram,
    sources: &'a SourceContext,
    function: FunctionId,
    body: FunctionBody,
    insertion_block: BlockId,
}

impl<'a> FunctionBuilder<'a> {
    /// Starts a body for a declared function.
    ///
    /// The entry block is created immediately and receives one block parameter for
    /// each declared function parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if the function is absent, already defined, has invalid
    /// provenance, or entity allocation fails.
    pub fn new(
        program: &'a CoreProgram,
        sources: &'a SourceContext,
        function: FunctionId,
    ) -> Result<Self, BuildError> {
        let declaration = program
            .function(function)
            .ok_or(BuildError::InvalidFunction { function })?;
        if declaration.body.is_some() {
            return Err(BuildError::AlreadyDefined { function });
        }
        if sources.origin(declaration.origin).is_none() {
            return Err(BuildError::InvalidOrigin {
                origin: declaration.origin,
            });
        }

        let mut blocks = EntityVec::new();
        let entry = blocks
            .push(BlockData {
                origin: declaration.origin,
                parameters: vec![],
                instructions: vec![],
                terminator: None,
            })
            .map_err(|EntityLimitError| BuildError::EntityLimit)?;
        let mut builder = Self {
            program,
            sources,
            function,
            body: FunctionBody {
                blocks,
                instructions: EntityVec::new(),
                values: EntityVec::new(),
                block_order: vec![entry],
                entry,
            },
            insertion_block: entry,
        };
        for ty in declaration.parameters.iter().copied() {
            builder.append_block_parameter(entry, ty, declaration.origin)?;
        }
        Ok(builder)
    }

    /// Returns the entry block.
    #[must_use]
    pub const fn entry_block(&self) -> BlockId {
        self.body.entry
    }

    /// Returns the current insertion block.
    #[must_use]
    pub const fn insertion_block(&self) -> BlockId {
        self.insertion_block
    }

    /// Returns a read-only view of the body under construction.
    #[must_use]
    pub const fn body(&self) -> &FunctionBody {
        &self.body
    }

    /// Changes the block receiving subsequently inserted operations.
    ///
    /// # Errors
    ///
    /// Returns an error if the block is not attached to this body.
    pub fn switch_to_block(&mut self, block: BlockId) -> Result<(), BuildError> {
        if !self.body.block_order.contains(&block) {
            return Err(BuildError::InvalidBlock { block });
        }
        self.insertion_block = block;
        Ok(())
    }

    /// Allocates and attaches an empty block at the end of layout.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid provenance or exhausted entity IDs.
    pub fn create_block(&mut self, origin: OriginId) -> Result<BlockId, BuildError> {
        self.validate_origin(origin)?;
        let block = self
            .body
            .blocks
            .push(BlockData {
                origin,
                parameters: vec![],
                instructions: vec![],
                terminator: None,
            })
            .map_err(|EntityLimitError| BuildError::EntityLimit)?;
        self.body.block_order.push(block);
        Ok(block)
    }

    /// Appends one typed block parameter and returns its SSA value.
    ///
    /// # Errors
    ///
    /// Returns an error for a detached/invalid block, invalid provenance, or an
    /// exhausted value/index space.
    pub fn append_block_parameter(
        &mut self,
        block: BlockId,
        ty: CoreType,
        origin: OriginId,
    ) -> Result<ValueId, BuildError> {
        self.validate_origin(origin)?;
        if !self.body.block_order.contains(&block) {
            return Err(BuildError::InvalidBlock { block });
        }
        let parameter_index = self
            .body
            .block(block)
            .and_then(|data| u32::try_from(data.parameters.len()).ok())
            .ok_or(BuildError::EntityLimit)?;
        let value = self
            .body
            .values
            .push(ValueData {
                ty,
                definition: ValueDef::BlockParam {
                    block,
                    parameter_index,
                },
            })
            .map_err(|EntityLimitError| BuildError::EntityLimit)?;
        self.body
            .block_mut(block)
            .ok_or(BuildError::InvalidBlock { block })?
            .parameters
            .push(BlockParam { value, origin });
        Ok(value)
    }

    /// Inserts a typed operation and returns all derived result values.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid callee, operand count/type, detached value,
    /// terminated insertion block, invalid provenance, or exhausted entity IDs.
    pub fn insert(
        &mut self,
        op: CoreOp,
        operands: Vec<ValueId>,
        origin: OriginId,
    ) -> Result<Vec<ValueId>, BuildError> {
        self.validate_origin(origin)?;
        let signature = op.signature(self.program).ok_or(match op {
            CoreOp::Call(function) => BuildError::InvalidFunction { function },
            _ => BuildError::InvalidOperationContract,
        })?;
        if signature.operands.len() != operands.len() {
            return Err(BuildError::OperandCount {
                expected: signature.operands.len(),
                actual: operands.len(),
            });
        }
        for (index, (operand, expected)) in operands
            .iter()
            .copied()
            .zip(signature.operands.iter().copied())
            .enumerate()
        {
            let actual = self
                .body
                .value(operand)
                .ok_or(BuildError::InvalidValue { value: operand })?
                .ty;
            if actual != expected {
                return Err(BuildError::OperandType {
                    index,
                    expected,
                    actual,
                });
            }
        }
        let block = self
            .body
            .block(self.insertion_block)
            .ok_or(BuildError::InvalidBlock {
                block: self.insertion_block,
            })?;
        if block.terminator.is_some() {
            return Err(BuildError::BlockAlreadyTerminated {
                block: self.insertion_block,
            });
        }

        let instruction = self
            .body
            .instructions
            .push(InstData {
                op,
                operands,
                results: vec![],
                origin,
            })
            .map_err(|EntityLimitError| BuildError::EntityLimit)?;
        let mut results = Vec::with_capacity(signature.results.len());
        for (index, ty) in signature.results.iter().copied().enumerate() {
            let result_index = u32::try_from(index).map_err(|_| BuildError::EntityLimit)?;
            let value = self
                .body
                .values
                .push(ValueData {
                    ty,
                    definition: ValueDef::InstResult {
                        instruction,
                        result_index,
                    },
                })
                .map_err(|EntityLimitError| BuildError::EntityLimit)?;
            results.push(value);
        }
        self.body
            .instruction_mut(instruction)
            .ok_or(BuildError::InvalidInstruction { instruction })?
            .results
            .clone_from(&results);
        self.body
            .block_mut(self.insertion_block)
            .ok_or(BuildError::InvalidBlock {
                block: self.insertion_block,
            })?
            .instructions
            .push(instruction);
        Ok(results)
    }

    /// Inserts a Boolean constant.
    ///
    /// # Errors
    ///
    /// Returns any structural insertion error.
    pub fn bool_constant(&mut self, value: bool, origin: OriginId) -> Result<ValueId, BuildError> {
        self.one_result(CoreOp::BoolConstant(value), vec![], origin)
    }

    /// Inserts an `i32` constant.
    ///
    /// # Errors
    ///
    /// Returns any structural insertion error.
    pub fn i32_constant(&mut self, value: i32, origin: OriginId) -> Result<ValueId, BuildError> {
        self.one_result(CoreOp::I32Constant(value), vec![], origin)
    }

    /// Inserts wrapping signed addition.
    ///
    /// # Errors
    ///
    /// Returns any structural insertion error.
    pub fn i32_add_wrapping(
        &mut self,
        left: ValueId,
        right: ValueId,
        origin: OriginId,
    ) -> Result<ValueId, BuildError> {
        self.one_result(CoreOp::I32AddWrapping, vec![left, right], origin)
    }

    /// Inserts signed overflowing addition, returning the wrapping sum and overflow
    /// flag as distinct SSA values.
    ///
    /// # Errors
    ///
    /// Returns any structural insertion error.
    pub fn i32_add_overflowing(
        &mut self,
        left: ValueId,
        right: ValueId,
        origin: OriginId,
    ) -> Result<(ValueId, ValueId), BuildError> {
        let results = self.insert(CoreOp::I32AddOverflowing, vec![left, right], origin)?;
        match results.as_slice() {
            [sum, overflowed] => Ok((*sum, *overflowed)),
            _ => Err(BuildError::InvalidOperationContract),
        }
    }

    /// Inserts one explicit signed `i32` comparison.
    ///
    /// # Errors
    ///
    /// Returns any structural insertion error.
    pub fn i32_compare(
        &mut self,
        predicate: I32Predicate,
        left: ValueId,
        right: ValueId,
        origin: OriginId,
    ) -> Result<ValueId, BuildError> {
        self.one_result(CoreOp::I32Compare(predicate), vec![left, right], origin)
    }

    /// Inserts Boolean negation.
    ///
    /// # Errors
    ///
    /// Returns any structural insertion error.
    pub fn bool_not(&mut self, value: ValueId, origin: OriginId) -> Result<ValueId, BuildError> {
        self.one_result(CoreOp::BoolNot, vec![value], origin)
    }

    /// Inserts an internal call and returns its declared results.
    ///
    /// The callee may still be undefined, enabling forward and mutually recursive
    /// construction, but it must already be declared.
    ///
    /// # Errors
    ///
    /// Returns any structural insertion error or an invalid callee error.
    pub fn call(
        &mut self,
        function: FunctionId,
        arguments: Vec<ValueId>,
        origin: OriginId,
    ) -> Result<Vec<ValueId>, BuildError> {
        self.insert(CoreOp::Call(function), arguments, origin)
    }

    /// Sets the current block's terminator exactly once.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid provenance or an already-terminated block.
    pub fn terminate(&mut self, terminator: Terminator) -> Result<(), BuildError> {
        self.validate_origin(terminator.origin)?;
        let block = self
            .body
            .block_mut(self.insertion_block)
            .ok_or(BuildError::InvalidBlock {
                block: self.insertion_block,
            })?;
        if block.terminator.is_some() {
            return Err(BuildError::BlockAlreadyTerminated {
                block: self.insertion_block,
            });
        }
        block.terminator = Some(terminator);
        Ok(())
    }

    /// Verifies and returns the finished body.
    ///
    /// # Errors
    ///
    /// Returns all safely detectable verifier diagnostics.
    pub fn finish(self) -> Result<FunctionBody, Diagnostics> {
        verify_function(self.program, self.sources, self.function, &self.body)?;
        Ok(self.body)
    }

    fn one_result(
        &mut self,
        op: CoreOp,
        operands: Vec<ValueId>,
        origin: OriginId,
    ) -> Result<ValueId, BuildError> {
        let results = self.insert(op, operands, origin)?;
        results
            .into_iter()
            .next()
            .ok_or(BuildError::InvalidOperationContract)
    }

    fn validate_origin(&self, origin: OriginId) -> Result<(), BuildError> {
        self.sources
            .origin(origin)
            .map(|_| ())
            .ok_or(BuildError::InvalidOrigin { origin })
    }
}

/// Function construction failure detected before full verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildError {
    /// No declaration with this function ID exists.
    InvalidFunction { function: FunctionId },
    /// The declared function already has a body.
    AlreadyDefined { function: FunctionId },
    /// A block is absent or detached.
    InvalidBlock { block: BlockId },
    /// An instruction is absent.
    InvalidInstruction { instruction: InstId },
    /// A value is absent.
    InvalidValue { value: ValueId },
    /// Provenance is absent from the source context.
    InvalidOrigin { origin: OriginId },
    /// The insertion block already has a terminator.
    BlockAlreadyTerminated { block: BlockId },
    /// Operand arity differs from the operation contract.
    OperandCount { expected: usize, actual: usize },
    /// One operand type differs from the operation contract.
    OperandType {
        index: usize,
        expected: CoreType,
        actual: CoreType,
    },
    /// A built-in operation did not expose its required result contract.
    InvalidOperationContract,
    /// An entity or local index cannot be represented.
    EntityLimit,
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for BuildError {}

#[cfg(test)]
mod tests {
    use super::FunctionBuilder;
    use crate::ir::core::{CoreProgram, CoreType, Terminator, TerminatorKind};
    use crate::source::{OriginId, SourceContext};

    #[test]
    fn derives_result_types_from_operation_contracts() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("add"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let parameter = builder.body.block(entry).unwrap().parameters[0].value;
        let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let sum = builder
            .i32_add_wrapping(parameter, one, OriginId::UNKNOWN)
            .unwrap();
        assert_eq!(builder.body.value(sum).unwrap().ty, CoreType::I32);
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![sum]),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        assert!(builder.finish().is_ok());
    }
}
