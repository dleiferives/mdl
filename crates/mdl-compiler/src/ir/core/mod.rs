//! A small typed, ordered SSA control-flow graph.
//!
//! Core expresses language-independent computation before Minecraft-specific
//! storage and command lowering. Block parameters represent function parameters,
//! joins, and loop-carried values. Instruction order is semantically significant
//! for operations with unknown effects.

mod analysis;
mod builder;
mod edit;
mod pass;
mod print;
mod verify;

use std::error::Error;
use std::fmt;

use crate::entity::{EntityLimitError, EntityVec, entity_id};
use crate::source::OriginId;

pub use analysis::{
    ControlFlowGraph, Dominance, DominatorTree, PlacementIndex, Reachability, UseIndex, UseSite,
};
pub use builder::{BuildError, FunctionBuilder};
pub use edit::{EditError, FunctionEditor};
pub use pass::{FailureBundle, FunctionPass, PassError, PassRunner};
pub use print::{CanonicalPrinter, DebugDumper, PrintError};
pub use verify::{Diagnostic, Diagnostics, verify_function, verify_program};

entity_id!(
    /// Identity of a function within one Core program.
    pub struct FunctionId;
);
entity_id!(
    /// Identity of a block within one Core function body.
    pub struct BlockId;
);
entity_id!(
    /// Identity of an instruction within one Core function body.
    pub struct InstId;
);
entity_id!(
    /// Identity of an SSA value within one Core function body.
    pub struct ValueId;
);

/// A value type understood by Core.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CoreType {
    /// A logical truth value.
    Bool,
    /// A signed 32-bit integer with operation-defined overflow semantics.
    I32,
}

impl fmt::Display for CoreType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bool => "bool",
            Self::I32 => "i32",
        })
    }
}

/// Signed comparison performed by [`CoreOp::I32Compare`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum I32Predicate {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Signed less than.
    SignedLt,
    /// Signed less than or equal.
    SignedLe,
    /// Signed greater than.
    SignedGt,
    /// Signed greater than or equal.
    SignedGe,
}

impl I32Predicate {
    pub(crate) const fn mnemonic(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Ne => "ne",
            Self::SignedLt => "slt",
            Self::SignedLe => "sle",
            Self::SignedGt => "sgt",
            Self::SignedGe => "sge",
        }
    }
}

/// Coarse observable-effect classification used by generic Core transforms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectClass {
    /// The operation has no observable effects.
    Pure,
    /// The operation may perform arbitrary observable effects.
    Unknown,
}

/// Whether evaluating an operation outside its original control dependence is safe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Speculation {
    /// Evaluation is total and always safe to speculate.
    Always,
    /// Evaluation must remain in its original control dependence.
    Never,
}

/// One closed Core operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreOp {
    /// Produces one Boolean constant.
    BoolConstant(bool),
    /// Produces one signed 32-bit integer constant.
    I32Constant(i32),
    /// Returns the low 32 bits of two's-complement addition.
    I32AddWrapping,
    /// Returns a wrapping sum and whether signed `i32` addition overflowed.
    I32AddOverflowing,
    /// Compares two signed `i32` values using an explicit predicate.
    I32Compare(I32Predicate),
    /// Negates a Boolean value.
    BoolNot,
    /// Calls one internal Core function.
    Call(FunctionId),
}

impl CoreOp {
    /// Returns the canonical operation name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::BoolConstant(_) => "core.bool.constant",
            Self::I32Constant(_) => "core.i32.constant",
            Self::I32AddWrapping => "core.i32.add.wrapping",
            Self::I32AddOverflowing => "core.i32.add.overflowing",
            Self::I32Compare(_) => "core.i32.compare",
            Self::BoolNot => "core.bool.not",
            Self::Call(_) => "core.call",
        }
    }

    /// Returns the operation's conservative effect class.
    #[must_use]
    pub const fn effects(&self) -> EffectClass {
        match self {
            Self::Call(_) => EffectClass::Unknown,
            Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32AddWrapping
            | Self::I32AddOverflowing
            | Self::I32Compare(_)
            | Self::BoolNot => EffectClass::Pure,
        }
    }

    /// Returns the operation's speculation permission.
    #[must_use]
    pub const fn speculation(&self) -> Speculation {
        match self {
            Self::Call(_) => Speculation::Never,
            Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32AddWrapping
            | Self::I32AddOverflowing
            | Self::I32Compare(_)
            | Self::BoolNot => Speculation::Always,
        }
    }

    /// Returns a precise human-readable semantic contract.
    #[must_use]
    pub const fn semantics(&self) -> &'static str {
        match self {
            Self::BoolConstant(_) => "the represented Boolean value",
            Self::I32Constant(_) => "the represented signed 32-bit integer",
            Self::I32AddWrapping => "low 32 bits of two's-complement addition",
            Self::I32AddOverflowing => {
                "wrapping sum and true exactly when signed i32 addition overflows"
            }
            Self::I32Compare(_) => "the selected explicit signed i32 comparison",
            Self::BoolNot => "Boolean logical negation",
            Self::Call(_) => "ordered invocation of the declared internal function",
        }
    }

    pub(crate) fn signature<'a>(&self, program: &'a CoreProgram) -> Option<OperationSignature<'a>> {
        match self {
            Self::BoolConstant(_) => Some(OperationSignature::fixed(&[], &[CoreType::Bool])),
            Self::I32Constant(_) => Some(OperationSignature::fixed(&[], &[CoreType::I32])),
            Self::I32AddWrapping => Some(OperationSignature::fixed(
                &[CoreType::I32, CoreType::I32],
                &[CoreType::I32],
            )),
            Self::I32AddOverflowing => Some(OperationSignature::fixed(
                &[CoreType::I32, CoreType::I32],
                &[CoreType::I32, CoreType::Bool],
            )),
            Self::I32Compare(_) => Some(OperationSignature::fixed(
                &[CoreType::I32, CoreType::I32],
                &[CoreType::Bool],
            )),
            Self::BoolNot => Some(OperationSignature::fixed(
                &[CoreType::Bool],
                &[CoreType::Bool],
            )),
            Self::Call(function) => {
                let declaration = program.function(*function)?;
                Some(OperationSignature {
                    operands: &declaration.parameters,
                    results: &declaration.results,
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct OperationSignature<'a> {
    pub(crate) operands: &'a [CoreType],
    pub(crate) results: &'a [CoreType],
}

impl<'a> OperationSignature<'a> {
    const fn fixed(operands: &'a [CoreType], results: &'a [CoreType]) -> Self {
        Self { operands, results }
    }
}

/// Function declaration and optional definition.
#[derive(Clone, Debug)]
pub struct Function {
    pub(crate) name_hint: Option<Box<str>>,
    pub(crate) parameters: Vec<CoreType>,
    pub(crate) results: Vec<CoreType>,
    pub(crate) origin: OriginId,
    pub(crate) body: Option<FunctionBody>,
}

impl Function {
    /// Returns the optional non-unique diagnostic name.
    #[must_use]
    pub fn name_hint(&self) -> Option<&str> {
        self.name_hint.as_deref()
    }

    /// Returns the declared parameter types.
    #[must_use]
    pub fn parameters(&self) -> &[CoreType] {
        &self.parameters
    }

    /// Returns the declared result types.
    #[must_use]
    pub fn results(&self) -> &[CoreType] {
        &self.results
    }

    /// Returns the declaration's provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Returns the definition when this internal function has been defined.
    #[must_use]
    pub const fn body(&self) -> Option<&FunctionBody> {
        self.body.as_ref()
    }
}

/// A compilation unit containing internal Core functions.
#[derive(Clone, Debug, Default)]
pub struct CoreProgram {
    pub(crate) functions: EntityVec<FunctionId, Function>,
}

impl CoreProgram {
    /// Creates an empty Core program.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            functions: EntityVec::new(),
        }
    }

    /// Returns whether this program has no function declarations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Returns the number of function declarations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Declares an internal function and returns its stable identity.
    ///
    /// Name hints need not be present or unique.
    ///
    /// # Errors
    ///
    /// Returns an error if the function ID space is exhausted.
    pub fn declare_function(
        &mut self,
        name_hint: Option<impl Into<Box<str>>>,
        parameters: Vec<CoreType>,
        results: Vec<CoreType>,
        origin: OriginId,
    ) -> Result<FunctionId, ProgramError> {
        self.functions
            .push(Function {
                name_hint: name_hint.map(Into::into),
                parameters,
                results,
                origin,
                body: None,
            })
            .map_err(|EntityLimitError| ProgramError::EntityLimit)
    }

    /// Installs a function body exactly once.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid function ID or a second definition.
    pub fn define_function(
        &mut self,
        function: FunctionId,
        body: FunctionBody,
    ) -> Result<(), ProgramError> {
        let declaration = self
            .functions
            .get_mut(function)
            .ok_or(ProgramError::InvalidFunction { function })?;
        if declaration.body.is_some() {
            return Err(ProgramError::AlreadyDefined { function });
        }
        declaration.body = Some(body);
        Ok(())
    }

    /// Returns a function declaration, or `None` for an ID outside this program.
    #[must_use]
    pub fn function(&self, function: FunctionId) -> Option<&Function> {
        self.functions.get(function)
    }

    /// Iterates over function declarations in stable ID order.
    #[must_use]
    pub fn functions(&self) -> impl ExactSizeIterator<Item = (FunctionId, &Function)> + '_ {
        self.functions.iter()
    }

    pub(crate) fn function_mut(&mut self, function: FunctionId) -> Option<&mut Function> {
        self.functions.get_mut(function)
    }

    pub(crate) fn take_function_body(&mut self, function: FunctionId) -> Option<FunctionBody> {
        self.function_mut(function)?.body.take()
    }

    pub(crate) fn restore_function_body(&mut self, function: FunctionId, body: FunctionBody) {
        if let Some(declaration) = self.function_mut(function) {
            declaration.body = Some(body);
        }
    }
}

/// Function declaration or definition failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramError {
    /// The function ID space is exhausted.
    EntityLimit,
    /// No declaration with this function ID exists.
    InvalidFunction {
        /// Invalid function identity.
        function: FunctionId,
    },
    /// The declaration already has a body.
    AlreadyDefined {
        /// Already-defined function identity.
        function: FunctionId,
    },
}

impl fmt::Display for ProgramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntityLimit => formatter.write_str("function ID space is exhausted"),
            Self::InvalidFunction { function } => {
                write!(formatter, "invalid function {function:?}")
            }
            Self::AlreadyDefined { function } => {
                write!(formatter, "function {function:?} is already defined")
            }
        }
    }
}

impl Error for ProgramError {}

/// One block parameter and its definition provenance.
#[derive(Clone, Debug)]
pub struct BlockParam {
    pub(crate) value: ValueId,
    pub(crate) origin: OriginId,
}

impl BlockParam {
    /// Returns the SSA value defined by this parameter.
    #[must_use]
    pub const fn value(&self) -> ValueId {
        self.value
    }

    /// Returns the parameter's provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// One attached or detached basic block.
#[derive(Clone, Debug)]
pub struct BlockData {
    pub(crate) origin: OriginId,
    pub(crate) parameters: Vec<BlockParam>,
    pub(crate) instructions: Vec<InstId>,
    pub(crate) terminator: Option<Terminator>,
}

impl BlockData {
    /// Returns the block's provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Returns the block parameters in definition order.
    #[must_use]
    pub fn parameters(&self) -> &[BlockParam] {
        &self.parameters
    }

    /// Returns attached instruction IDs in execution order.
    #[must_use]
    pub fn instructions(&self) -> &[InstId] {
        &self.instructions
    }

    /// Returns the block terminator when present.
    #[must_use]
    pub const fn terminator(&self) -> Option<&Terminator> {
        self.terminator.as_ref()
    }
}

/// One operation instance.
#[derive(Clone, Debug)]
pub struct InstData {
    pub(crate) op: CoreOp,
    pub(crate) operands: Vec<ValueId>,
    pub(crate) results: Vec<ValueId>,
    pub(crate) origin: OriginId,
}

impl InstData {
    /// Returns the operation.
    #[must_use]
    pub const fn op(&self) -> &CoreOp {
        &self.op
    }

    /// Returns SSA operands in contract order.
    #[must_use]
    pub fn operands(&self) -> &[ValueId] {
        &self.operands
    }

    /// Returns SSA results in contract order.
    #[must_use]
    pub fn results(&self) -> &[ValueId] {
        &self.results
    }

    /// Returns the instruction's provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// One SSA value and its single definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValueData {
    pub(crate) ty: CoreType,
    pub(crate) definition: ValueDef,
}

impl ValueData {
    /// Returns the immutable value type.
    #[must_use]
    pub const fn ty(self) -> CoreType {
        self.ty
    }

    /// Returns the value's unique definition.
    #[must_use]
    pub const fn definition(self) -> ValueDef {
        self.definition
    }
}

/// Unique definition of an SSA value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValueDef {
    /// A basic-block parameter.
    BlockParam {
        /// Defining block.
        block: BlockId,
        /// Zero-based index in the block parameter list.
        parameter_index: u32,
    },
    /// An instruction result.
    InstResult {
        /// Defining instruction.
        instruction: InstId,
        /// Zero-based index in the instruction result list.
        result_index: u32,
    },
}

/// A successor block plus SSA arguments for its parameters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockTarget {
    pub(crate) block: BlockId,
    pub(crate) arguments: Vec<ValueId>,
}

impl BlockTarget {
    /// Creates one control-flow edge target.
    #[must_use]
    pub fn new(block: BlockId, arguments: Vec<ValueId>) -> Self {
        Self { block, arguments }
    }

    /// Returns the target block.
    #[must_use]
    pub const fn block(&self) -> BlockId {
        self.block
    }

    /// Returns edge arguments in destination-parameter order.
    #[must_use]
    pub fn arguments(&self) -> &[ValueId] {
        &self.arguments
    }
}

/// A block's structural terminator and provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terminator {
    pub(crate) kind: TerminatorKind,
    pub(crate) origin: OriginId,
}

impl Terminator {
    /// Creates a terminator.
    #[must_use]
    pub const fn new(kind: TerminatorKind, origin: OriginId) -> Self {
        Self { kind, origin }
    }

    /// Returns the structural terminator kind.
    #[must_use]
    pub const fn kind(&self) -> &TerminatorKind {
        &self.kind
    }

    /// Returns the terminator's provenance.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    /// Iterates over successor targets in deterministic semantic order.
    #[must_use]
    pub fn successors(&self) -> impl ExactSizeIterator<Item = &BlockTarget> {
        let targets = match &self.kind {
            TerminatorKind::Jump(target) => vec![target],
            TerminatorKind::Branch {
                then_target,
                else_target,
                ..
            } => vec![then_target, else_target],
            TerminatorKind::Return(_) | TerminatorKind::Unreachable => vec![],
        };
        targets.into_iter()
    }
}

/// Closed structural control flow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminatorKind {
    /// Unconditional transfer.
    Jump(BlockTarget),
    /// Conditional transfer to exactly one of two targets.
    Branch {
        /// Boolean branch condition.
        condition: ValueId,
        /// Target selected when the condition is true.
        then_target: BlockTarget,
        /// Target selected when the condition is false.
        else_target: BlockTarget,
    },
    /// Returns values from the current function.
    Return(Vec<ValueId>),
    /// Declares that control cannot continue from this block.
    Unreachable,
}

impl TerminatorKind {
    pub(crate) fn for_each_successor(&self, mut visit: impl FnMut(&BlockTarget)) {
        match self {
            Self::Jump(target) => visit(target),
            Self::Branch {
                then_target,
                else_target,
                ..
            } => {
                visit(then_target);
                visit(else_target);
            }
            Self::Return(_) | Self::Unreachable => {}
        }
    }
}

/// Ordered SSA body of one function.
#[derive(Clone, Debug)]
pub struct FunctionBody {
    pub(crate) blocks: EntityVec<BlockId, BlockData>,
    pub(crate) instructions: EntityVec<InstId, InstData>,
    pub(crate) values: EntityVec<ValueId, ValueData>,
    pub(crate) block_order: Vec<BlockId>,
    pub(crate) entry: BlockId,
}

impl FunctionBody {
    /// Returns the entry block.
    #[must_use]
    pub const fn entry(&self) -> BlockId {
        self.entry
    }

    /// Returns attached blocks in deterministic layout order.
    #[must_use]
    pub fn block_order(&self) -> &[BlockId] {
        &self.block_order
    }

    /// Returns a block, including a detached block, when allocated in this body.
    #[must_use]
    pub fn block(&self, block: BlockId) -> Option<&BlockData> {
        self.blocks.get(block)
    }

    /// Returns an instruction, including a detached instruction, when allocated.
    #[must_use]
    pub fn instruction(&self, instruction: InstId) -> Option<&InstData> {
        self.instructions.get(instruction)
    }

    /// Returns a value, including a detached definition, when allocated.
    #[must_use]
    pub fn value(&self, value: ValueId) -> Option<ValueData> {
        self.values.get(value).copied()
    }

    /// Returns allocated and attached block counts.
    #[must_use]
    pub fn block_counts(&self) -> EntityCounts {
        EntityCounts {
            allocated: self.blocks.len(),
            attached: self.block_order.len(),
        }
    }

    /// Returns allocated and attached instruction counts.
    #[must_use]
    pub fn instruction_counts(&self) -> EntityCounts {
        let attached = self
            .block_order
            .iter()
            .filter_map(|block| self.blocks.get(*block))
            .map(|block| block.instructions.len())
            .sum();
        EntityCounts {
            allocated: self.instructions.len(),
            attached,
        }
    }

    pub(crate) fn block_mut(&mut self, block: BlockId) -> Option<&mut BlockData> {
        self.blocks.get_mut(block)
    }

    pub(crate) fn instruction_mut(&mut self, instruction: InstId) -> Option<&mut InstData> {
        self.instructions.get_mut(instruction)
    }
}

/// Allocated-versus-attached entity totals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EntityCounts {
    /// IDs allocated by the owning body.
    pub allocated: usize,
    /// IDs currently present in executable layout.
    pub attached: usize,
}

#[cfg(test)]
mod tests {
    use super::{CoreOp, CoreProgram, CoreType, EffectClass, OriginId, Speculation};

    #[test]
    fn core_types_are_compact_values() {
        assert_eq!(CoreType::Bool, CoreType::Bool);
        assert_ne!(CoreType::Bool, CoreType::I32);
        assert_eq!(std::mem::size_of::<CoreType>(), 1);
    }

    #[test]
    fn operations_centralize_effect_and_speculation_contracts() {
        assert_eq!(CoreOp::I32AddWrapping.effects(), EffectClass::Pure);
        assert_eq!(CoreOp::I32AddWrapping.speculation(), Speculation::Always);

        let mut program = CoreProgram::new();
        let callee = program
            .declare_function(
                Some("callee"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        assert_eq!(CoreOp::Call(callee).effects(), EffectClass::Unknown);
        assert_eq!(CoreOp::Call(callee).speculation(), Speculation::Never);
    }

    #[test]
    fn function_names_are_non_unique_hints() {
        let mut program = CoreProgram::new();
        let first = program
            .declare_function(Some("same"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let second = program
            .declare_function(Some("same"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();

        assert_ne!(first, second);
        assert_eq!(program.function(first).unwrap().name_hint(), Some("same"));
        assert_eq!(program.function(second).unwrap().name_hint(), Some("same"));
    }
}
