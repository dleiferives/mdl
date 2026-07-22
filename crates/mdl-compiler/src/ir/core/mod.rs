//! A small typed, ordered SSA control-flow graph.
//!
//! Core expresses language-independent computation before Minecraft-specific
//! storage and command lowering. Block parameters represent function parameters,
//! joins, and loop-carried values. Instruction order is semantically significant
//! for operations with unknown effects.

mod ambient;
mod analysis;
mod builder;
mod edit;
mod entity_nbt;
mod eval;
mod external;
mod minecraft;
mod operand;
mod print;
mod query;
mod run_scope;
mod verify;

use std::error::Error;
use std::fmt;

use crate::entity::{EntityLimitError, EntityVec, entity_id};
use crate::source::OriginId;

pub use ambient::{CoreAmbientAnalysis, CoreAmbientAnalysisError};
pub use analysis::{
    ControlFlowGraph, Dominance, DominatorTree, PlacementIndex, Reachability, UseIndex, UseSite,
};
pub use builder::{BuildError, FunctionBuilder};
pub use edit::{EditError, FunctionEditor, ValueReplacement};
pub(crate) use edit::{
    JumpFusionApplicationStatistics, JumpFusionFactStatistics, JumpFusionPreparation,
};
pub use entity_nbt::{EntityNbtPathSegment, EntityNbtReadDecl, EntityNbtReceiver};
pub use eval::{
    CoreCallEvent, CoreEvaluation, CoreEvaluationError, CoreEvaluationLimits, CoreEvaluator,
    CoreValue,
};
pub use external::{
    ExternalOpDecl, ExternalSemanticBinding, TargetFragment, UnsafeCommandFragmentError,
    UnsafeMinecraftCommandFragment,
};
pub use minecraft::{
    MinecraftOperationAttributes, MinecraftOperationDecl, MinecraftOperationOrigins,
};
pub use operand::Operand;
pub use print::{CanonicalPrinter, DebugDumper, PrintError};
pub use query::{EntityQueryDecl, EntityQueryStep};
pub use run_scope::{CoreContextStep, CoreExecutionContext, RunModifierInstance, RunScopeDecl};
#[cfg(test)]
pub(crate) use verify::{reset_verifier_counters, verifier_counters};
pub use verify::{verify_function, verify_program};

pub use crate::diagnostic::{Diagnostic, Diagnostics};

entity_id!(
    /// Identity of a function within one Core program.
    pub struct FunctionId;
);
entity_id!(
    /// Identity of a linked external-operation declaration within one Core program.
    pub struct ExternalOpId;
);
entity_id!(
    /// Identity of an immutable target fragment within one Core program.
    pub struct TargetFragmentId;
);
entity_id!(
    /// Identity of an immutable target-independent semantic entity query.
    pub struct EntityQueryId;
);
entity_id!(
    /// Identity of one program-owned structured contextual invocation.
    pub struct RunScopeId;
);
entity_id!(
    /// Identity of one program-owned typed Minecraft operation.
    pub struct MinecraftOperationId;
);
entity_id!(
    /// Identity of one program-owned schema-typed entity-NBT path read.
    pub struct EntityNbtReadId;
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
    /// An immutable ordered sequence of signed 32-bit integers.
    ListI32,
    /// An immutable Java-compatible UTF-16 string value.
    String,
}

/// Whether a Core function is an implementation detail or a supported datapack entry.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CoreFunctionLinkage {
    /// Calls are known to originate inside this Core program.
    Internal,
    /// The generated entry may be invoked by an unknown external caller.
    DatapackExport,
}

impl fmt::Display for CoreFunctionLinkage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal => formatter.write_str("internal"),
            Self::DatapackExport => formatter.write_str("export"),
        }
    }
}

/// A closed, typed constant representable directly in Core.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TypedCoreConstant {
    /// A Boolean constant.
    Bool(bool),
    /// A signed 32-bit integer constant.
    I32(i32),
}

impl TypedCoreConstant {
    /// Returns the Core type produced by this constant.
    #[must_use]
    pub const fn ty(self) -> CoreType {
        match self {
            Self::Bool(_) => CoreType::Bool,
            Self::I32(_) => CoreType::I32,
        }
    }

    pub(crate) const fn op(self) -> CoreOp {
        match self {
            Self::Bool(value) => CoreOp::BoolConstant(value),
            Self::I32(value) => CoreOp::I32Constant(value),
        }
    }
}

impl fmt::Display for CoreType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Bool => "bool",
            Self::I32 => "i32",
            Self::ListI32 => "list<i32>",
            Self::String => "string",
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

/// A validated inclusive signed `i32` interval carried as a Core attribute.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct I32ClosedRange {
    min: i32,
    max: i32,
}

impl I32ClosedRange {
    #[must_use]
    pub const fn new(min: i32, max: i32) -> Option<Self> {
        if min <= max {
            Some(Self { min, max })
        } else {
            None
        }
    }
    #[must_use]
    pub const fn min(self) -> i32 {
        self.min
    }
    #[must_use]
    pub const fn max(self) -> i32 {
        self.max
    }
    #[must_use]
    pub const fn contains(self, value: i32) -> bool {
        value >= self.min && value <= self.max
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

/// Whether equal structural instances of an operation produce equal results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultEquivalence {
    /// Operation payload, operands, and complete result types determine its results.
    Structural,
    /// Structural identity is insufficient to prove equal results.
    Opaque,
}

/// Whether a Core operation permits canonical operand reordering.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum OperandSymmetry {
    /// Operand positions have distinct semantics or there are fewer than two.
    Ordered,
    /// Exactly two operands may be exchanged without changing semantics.
    CommutativePair,
}

/// Semantic role of one non-SSA function-reference occurrence.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FunctionReferenceKind {
    /// An ordinary `CoreOp::Call` occurrence.
    DirectCall,
    /// A function owned by one ordered run modifier.
    RunScopeModifier {
        /// Zero-based modifier position in the run scope.
        modifier_index: usize,
    },
    /// The terminal outlined body of a structured run scope.
    RunScopeBody,
}

/// One function edge with its own provenance and semantic role.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FunctionReference {
    function: FunctionId,
    origin: OriginId,
    kind: FunctionReferenceKind,
}

impl FunctionReference {
    pub(crate) const fn new(
        function: FunctionId,
        origin: OriginId,
        kind: FunctionReferenceKind,
    ) -> Self {
        Self {
            function,
            origin,
            kind,
        }
    }

    /// Returns the referenced function.
    #[must_use]
    pub const fn function(self) -> FunctionId {
        self.function
    }

    /// Returns this exact edge occurrence's provenance.
    #[must_use]
    pub const fn origin(self) -> OriginId {
        self.origin
    }

    /// Returns the edge's semantic ownership.
    #[must_use]
    pub const fn kind(self) -> FunctionReferenceKind {
        self.kind
    }
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
    /// Returns the low 32 bits of two's-complement subtraction.
    I32SubWrapping,
    /// Returns a wrapping sum and whether signed `i32` addition overflowed.
    I32AddOverflowing,
    /// Compares two signed `i32` values using an explicit predicate.
    I32Compare(I32Predicate),
    /// Tests membership in one static closed signed interval.
    I32InClosedRange(I32ClosedRange),
    /// Negates a Boolean value.
    BoolNot,
    /// Produces the empty immutable `i32` list.
    ListI32Empty,
    /// Returns the number of elements in an `i32` list.
    ListI32Length,
    /// Appends one value to an immutable `i32` list.
    ListI32Push,
    /// Returns the last value, or zero when the list is empty.
    ListI32LastOrZero,
    /// Returns the list without its last value; empty remains empty.
    ListI32WithoutLast,
    /// Produces one immutable runtime string constant.
    StringConstant(Box<str>),
    /// Returns the number of Java UTF-16 code units in a string.
    StringLength,
    /// Tests whether a string's final UTF-16 unit is one selected ASCII byte.
    StringEndsWithAscii(u8),
    /// Returns the string without its final UTF-16 code unit; empty remains empty.
    StringWithoutLastUnit,
    /// Calls one internal Core function.
    Call(FunctionId),
    /// Invokes one closed program-owned external declaration.
    External(ExternalOpId),
}

impl CoreOp {
    /// Returns the canonical operation name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::BoolConstant(_) => "core.bool.constant",
            Self::I32Constant(_) => "core.i32.constant",
            Self::I32AddWrapping => "core.i32.add.wrapping",
            Self::I32SubWrapping => "core.i32.sub.wrapping",
            Self::I32AddOverflowing => "core.i32.add.overflowing",
            Self::I32Compare(_) => "core.i32.compare",
            Self::I32InClosedRange(_) => "core.i32.in_closed_range",
            Self::BoolNot => "core.bool.not",
            Self::ListI32Empty => "core.list.i32.empty",
            Self::ListI32Length => "core.list.i32.length",
            Self::ListI32Push => "core.list.i32.push",
            Self::ListI32LastOrZero => "core.list.i32.last_or_zero",
            Self::ListI32WithoutLast => "core.list.i32.without_last",
            Self::StringConstant(_) => "core.string.constant",
            Self::StringLength => "core.string.length",
            Self::StringEndsWithAscii(_) => "core.string.ends_with_ascii",
            Self::StringWithoutLastUnit => "core.string.without_last_unit",
            Self::Call(_) => "core.call",
            Self::External(_) => "core.external",
        }
    }

    /// Returns the operation's conservative effect class.
    #[must_use]
    pub const fn effects(&self) -> EffectClass {
        match self {
            Self::Call(_) | Self::External(_) => EffectClass::Unknown,
            Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32AddWrapping
            | Self::I32SubWrapping
            | Self::I32AddOverflowing
            | Self::I32Compare(_)
            | Self::I32InClosedRange(_)
            | Self::BoolNot
            | Self::ListI32Empty
            | Self::ListI32Length
            | Self::ListI32Push
            | Self::ListI32LastOrZero
            | Self::ListI32WithoutLast
            | Self::StringConstant(_)
            | Self::StringLength
            | Self::StringEndsWithAscii(_)
            | Self::StringWithoutLastUnit => EffectClass::Pure,
        }
    }

    /// Returns the operation's speculation permission.
    #[must_use]
    pub const fn speculation(&self) -> Speculation {
        match self {
            Self::Call(_) | Self::External(_) => Speculation::Never,
            Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32AddWrapping
            | Self::I32SubWrapping
            | Self::I32AddOverflowing
            | Self::I32Compare(_)
            | Self::I32InClosedRange(_)
            | Self::BoolNot
            | Self::ListI32Empty
            | Self::ListI32Length
            | Self::ListI32Push
            | Self::ListI32LastOrZero
            | Self::ListI32WithoutLast
            | Self::StringConstant(_)
            | Self::StringLength
            | Self::StringEndsWithAscii(_)
            | Self::StringWithoutLastUnit => Speculation::Always,
        }
    }

    /// Returns whether complete structural equality proves equal results.
    #[must_use]
    pub const fn result_equivalence(&self) -> ResultEquivalence {
        match self {
            Self::Call(_) | Self::External(_) => ResultEquivalence::Opaque,
            Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32AddWrapping
            | Self::I32SubWrapping
            | Self::I32AddOverflowing
            | Self::I32Compare(_)
            | Self::I32InClosedRange(_)
            | Self::BoolNot
            | Self::ListI32Empty
            | Self::ListI32Length
            | Self::ListI32Push
            | Self::ListI32LastOrZero
            | Self::ListI32WithoutLast
            | Self::StringConstant(_)
            | Self::StringLength
            | Self::StringEndsWithAscii(_)
            | Self::StringWithoutLastUnit => ResultEquivalence::Structural,
        }
    }

    /// Returns whether an unused instance can be removed without changing
    /// observable behavior or introducing a new trap/divergence behavior.
    #[must_use]
    pub const fn is_trivially_discardable(&self) -> bool {
        matches!(self.effects(), EffectClass::Pure)
            && matches!(self.speculation(), Speculation::Always)
    }

    /// Returns the operation's exact operand-symmetry contract.
    #[must_use]
    pub const fn operand_symmetry(&self) -> OperandSymmetry {
        match self {
            Self::I32AddWrapping | Self::I32AddOverflowing => OperandSymmetry::CommutativePair,
            Self::I32Compare(I32Predicate::Eq | I32Predicate::Ne) => {
                OperandSymmetry::CommutativePair
            }
            Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32SubWrapping
            | Self::I32Compare(
                I32Predicate::SignedLt
                | I32Predicate::SignedLe
                | I32Predicate::SignedGt
                | I32Predicate::SignedGe,
            )
            | Self::I32InClosedRange(_)
            | Self::BoolNot
            | Self::ListI32Empty
            | Self::ListI32Length
            | Self::ListI32Push
            | Self::ListI32LastOrZero
            | Self::ListI32WithoutLast
            | Self::StringConstant(_)
            | Self::StringLength
            | Self::StringEndsWithAscii(_)
            | Self::StringWithoutLastUnit
            | Self::Call(_)
            | Self::External(_) => OperandSymmetry::Ordered,
        }
    }

    /// Returns a precise human-readable semantic contract.
    #[must_use]
    pub const fn semantics(&self) -> &'static str {
        match self {
            Self::BoolConstant(_) => "the represented Boolean value",
            Self::I32Constant(_) => "the represented signed 32-bit integer",
            Self::I32AddWrapping => "low 32 bits of two's-complement addition",
            Self::I32SubWrapping => "low 32 bits of two's-complement subtraction",
            Self::I32AddOverflowing => {
                "wrapping sum and true exactly when signed i32 addition overflows"
            }
            Self::I32Compare(_) => "the selected explicit signed i32 comparison",
            Self::I32InClosedRange(_) => {
                "true exactly when the operand is within the inclusive signed interval"
            }
            Self::BoolNot => "Boolean logical negation",
            Self::ListI32Empty => "the empty immutable i32 list",
            Self::ListI32Length => "the exact number of list elements",
            Self::ListI32Push => "an immutable list with one value appended",
            Self::ListI32LastOrZero => "the last list value, or zero for an empty list",
            Self::ListI32WithoutLast => "an immutable list without its last element",
            Self::StringConstant(_) => "the represented immutable UTF-16 string",
            Self::StringLength => "the exact number of Java UTF-16 code units",
            Self::StringEndsWithAscii(_) => {
                "true when the final UTF-16 code unit equals the selected ASCII byte"
            }
            Self::StringWithoutLastUnit => "the string without its final UTF-16 code unit",
            Self::Call(_) => "ordered invocation of the declared internal function",
            Self::External(_) => "ordered invocation of a closed external declaration",
        }
    }

    pub(crate) fn signature<'a>(&self, program: &'a CoreProgram) -> Option<OperationSignature<'a>> {
        match self {
            Self::BoolConstant(_) => Some(OperationSignature::fixed(&[], &[CoreType::Bool])),
            Self::I32Constant(_) => Some(OperationSignature::fixed(&[], &[CoreType::I32])),
            Self::I32AddWrapping | Self::I32SubWrapping => Some(OperationSignature::fixed(
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
            Self::I32InClosedRange(_) => Some(OperationSignature::fixed(
                &[CoreType::I32],
                &[CoreType::Bool],
            )),
            Self::BoolNot => Some(OperationSignature::fixed(
                &[CoreType::Bool],
                &[CoreType::Bool],
            )),
            Self::ListI32Empty => Some(OperationSignature::fixed(&[], &[CoreType::ListI32])),
            Self::ListI32Length | Self::ListI32LastOrZero => Some(OperationSignature::fixed(
                &[CoreType::ListI32],
                &[CoreType::I32],
            )),
            Self::ListI32Push => Some(OperationSignature::fixed(
                &[CoreType::ListI32, CoreType::I32],
                &[CoreType::ListI32],
            )),
            Self::ListI32WithoutLast => Some(OperationSignature::fixed(
                &[CoreType::ListI32],
                &[CoreType::ListI32],
            )),
            Self::StringConstant(_) => Some(OperationSignature::fixed(&[], &[CoreType::String])),
            Self::StringLength => Some(OperationSignature::fixed(
                &[CoreType::String],
                &[CoreType::I32],
            )),
            Self::StringEndsWithAscii(_) => Some(OperationSignature::fixed(
                &[CoreType::String],
                &[CoreType::Bool],
            )),
            Self::StringWithoutLastUnit => Some(OperationSignature::fixed(
                &[CoreType::String],
                &[CoreType::String],
            )),
            Self::Call(function) => {
                let declaration = program.function(*function)?;
                Some(OperationSignature {
                    operands: &declaration.parameters,
                    results: &declaration.results,
                })
            }
            Self::External(operation) => {
                let declaration = program.external_op(*operation)?;
                if !declaration.is_well_formed(program) {
                    return None;
                }
                Some(OperationSignature {
                    operands: &declaration.parameters,
                    results: &declaration.results,
                })
            }
        }
    }

    /// Enumerates every function referenced outside the SSA operand list.
    ///
    /// Analyses that consume non-SSA call edges should use this query instead of
    /// matching ordinary and declaration-backed operation payloads independently.
    pub fn function_references<'a>(
        &'a self,
        program: &'a CoreProgram,
        occurrence_origin: OriginId,
    ) -> impl Iterator<Item = FunctionReference> + 'a {
        let direct = match self {
            Self::Call(function) => Some(FunctionReference::new(
                *function,
                occurrence_origin,
                FunctionReferenceKind::DirectCall,
            )),
            Self::External(_)
            | Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32AddWrapping
            | Self::I32SubWrapping
            | Self::I32AddOverflowing
            | Self::I32Compare(_)
            | Self::I32InClosedRange(_)
            | Self::BoolNot
            | Self::ListI32Empty
            | Self::ListI32Length
            | Self::ListI32Push
            | Self::ListI32LastOrZero
            | Self::ListI32WithoutLast
            | Self::StringConstant(_)
            | Self::StringLength
            | Self::StringEndsWithAscii(_)
            | Self::StringWithoutLastUnit => None,
        };
        let external = match self {
            Self::External(operation) => program.external_op(*operation),
            Self::Call(_)
            | Self::BoolConstant(_)
            | Self::I32Constant(_)
            | Self::I32AddWrapping
            | Self::I32SubWrapping
            | Self::I32AddOverflowing
            | Self::I32Compare(_)
            | Self::I32InClosedRange(_)
            | Self::BoolNot
            | Self::ListI32Empty
            | Self::ListI32Length
            | Self::ListI32Push
            | Self::ListI32LastOrZero
            | Self::ListI32WithoutLast
            | Self::StringConstant(_)
            | Self::StringLength
            | Self::StringEndsWithAscii(_)
            | Self::StringWithoutLastUnit => None,
        };
        direct.into_iter().chain(
            external
                .into_iter()
                .flat_map(move |declaration| declaration.function_references(program)),
        )
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
    pub(crate) linkage: CoreFunctionLinkage,
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

    /// Returns whether this function is internal or a supported datapack entry.
    #[must_use]
    pub const fn linkage(&self) -> CoreFunctionLinkage {
        self.linkage
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
    pub(crate) external_ops: EntityVec<ExternalOpId, ExternalOpDecl>,
    pub(crate) target_fragments: EntityVec<TargetFragmentId, TargetFragment>,
    pub(crate) entity_queries: EntityVec<EntityQueryId, EntityQueryDecl>,
    pub(crate) run_scopes: EntityVec<RunScopeId, RunScopeDecl>,
    pub(crate) minecraft_operations: EntityVec<MinecraftOperationId, MinecraftOperationDecl>,
    pub(crate) entity_nbt_reads: EntityVec<EntityNbtReadId, EntityNbtReadDecl>,
}

impl CoreProgram {
    /// Creates an empty Core program.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            functions: EntityVec::new(),
            external_ops: EntityVec::new(),
            target_fragments: EntityVec::new(),
            entity_queries: EntityVec::new(),
            run_scopes: EntityVec::new(),
            minecraft_operations: EntityVec::new(),
            entity_nbt_reads: EntityVec::new(),
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
        self.declare_function_with_linkage(
            name_hint,
            CoreFunctionLinkage::Internal,
            parameters,
            results,
            origin,
        )
    }

    /// Declares a function with explicit target-entry linkage.
    ///
    /// Linkage does not affect whether another Core function may reference the
    /// declaration. Name hints need not be present or unique.
    ///
    /// # Errors
    ///
    /// Returns an error if the function ID space is exhausted.
    pub fn declare_function_with_linkage(
        &mut self,
        name_hint: Option<impl Into<Box<str>>>,
        linkage: CoreFunctionLinkage,
        parameters: Vec<CoreType>,
        results: Vec<CoreType>,
        origin: OriginId,
    ) -> Result<FunctionId, ProgramError> {
        self.functions
            .push(Function {
                name_hint: name_hint.map(Into::into),
                linkage,
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

    /// Iterates supported datapack entries in stable function-ID order.
    pub fn exported_functions(&self) -> impl Iterator<Item = (FunctionId, &Function)> + '_ {
        self.functions()
            .filter(|(_, function)| function.linkage == CoreFunctionLinkage::DatapackExport)
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
    /// A Core entity ID space is exhausted.
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
    /// An external binding names a target fragment outside this program.
    InvalidTargetFragment {
        /// Invalid fragment identity.
        fragment: TargetFragmentId,
    },
    /// An external binding names a semantic entity query outside this program.
    InvalidEntityQuery {
        /// Invalid query identity.
        query: EntityQueryId,
    },
    /// A semantic query declaration's ordered steps do not reproduce its value.
    InvalidEntityQueryDeclaration,
    /// A structured run scope names an incompatible outlined function body.
    InvalidRunScopeBody {
        /// Function that cannot be invoked as a run-scope body.
        function: FunctionId,
    },
    /// A structured run scope contains an invalid typed modifier.
    InvalidRunScopeModifier {
        /// Provenance of the invalid modifier.
        origin: OriginId,
    },
    /// An external binding names a run scope outside this program.
    InvalidRunScope {
        /// Invalid run-scope identity.
        scope: RunScopeId,
    },
    /// A typed Minecraft operation has an invalid key, receiver, or attributes.
    InvalidMinecraftOperation,
    /// An entity-NBT path read has an empty path or an incapable receiver kind.
    InvalidEntityNbtRead,
    /// An external binding names an entity-NBT path read outside this program.
    InvalidEntityNbtReadReference {
        /// Invalid entity-NBT read identity.
        read: EntityNbtReadId,
    },
    /// A closed external binding was paired with an unsupported typed signature.
    InvalidExternalSignature {
        /// Binding whose closed contract was violated.
        binding: ExternalSemanticBinding,
    },
}

impl fmt::Display for ProgramError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntityLimit => formatter.write_str("Core entity ID space is exhausted"),
            Self::InvalidFunction { function } => {
                write!(formatter, "invalid function {function:?}")
            }
            Self::AlreadyDefined { function } => {
                write!(formatter, "function {function:?} is already defined")
            }
            Self::InvalidTargetFragment { fragment } => {
                write!(formatter, "invalid target fragment {fragment:?}")
            }
            Self::InvalidEntityQuery { query } => {
                write!(formatter, "invalid entity query {query:?}")
            }
            Self::InvalidEntityQueryDeclaration => {
                formatter.write_str("invalid semantic entity-query declaration")
            }
            Self::InvalidRunScopeBody { function } => {
                write!(formatter, "invalid run-scope body {function:?}")
            }
            Self::InvalidRunScopeModifier { origin } => {
                write!(formatter, "invalid run-scope modifier at {origin:?}")
            }
            Self::InvalidRunScope { scope } => {
                write!(formatter, "invalid run scope {scope:?}")
            }
            Self::InvalidMinecraftOperation => {
                formatter.write_str("invalid typed Minecraft operation")
            }
            Self::InvalidEntityNbtRead => formatter.write_str("invalid entity-NBT path read"),
            Self::InvalidEntityNbtReadReference { read } => {
                write!(formatter, "invalid entity-NBT path read {read:?}")
            }
            Self::InvalidExternalSignature { binding } => {
                write!(
                    formatter,
                    "invalid signature for external binding {binding:?}"
                )
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

    /// Iterates over every allocated SSA value in stable identity order.
    #[must_use]
    pub fn values(&self) -> impl ExactSizeIterator<Item = (ValueId, ValueData)> + '_ {
        self.values.iter().map(|(value, data)| (value, *data))
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

    /// Returns allocated-versus-attached SSA value counts.
    #[must_use]
    pub fn value_counts(&self) -> EntityCounts {
        let attached = self
            .block_order
            .iter()
            .filter_map(|block| self.blocks.get(*block))
            .map(|block| {
                block.parameters.len()
                    + block
                        .instructions
                        .iter()
                        .filter_map(|instruction| self.instructions.get(*instruction))
                        .map(|instruction| instruction.results.len())
                        .sum::<usize>()
            })
            .sum();
        EntityCounts {
            allocated: self.values.len(),
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
    use super::{
        CoreFunctionLinkage, CoreOp, CoreProgram, CoreType, EffectClass, ExternalSemanticBinding,
        I32Predicate, OperandSymmetry, OriginId, ResultEquivalence, Speculation, TargetFragment,
        TypedCoreConstant,
    };

    #[test]
    fn core_types_are_compact_values() {
        assert_eq!(CoreType::Bool, CoreType::Bool);
        assert_ne!(CoreType::Bool, CoreType::I32);
        assert_eq!(std::mem::size_of::<CoreType>(), 1);
    }

    #[test]
    fn function_linkage_is_explicit_and_exports_are_stable_roots() {
        let mut program = CoreProgram::new();
        let internal = program
            .declare_function(Some("helper"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let exported = program
            .declare_function_with_linkage(
                Some("entry"),
                CoreFunctionLinkage::DatapackExport,
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();

        assert_eq!(
            program.function(internal).unwrap().linkage(),
            CoreFunctionLinkage::Internal
        );
        assert_eq!(
            program
                .exported_functions()
                .map(|(function, _)| function)
                .collect::<Vec<_>>(),
            [exported]
        );
    }

    #[test]
    fn operations_centralize_effect_and_speculation_contracts() {
        assert_eq!(CoreOp::I32AddWrapping.effects(), EffectClass::Pure);
        assert_eq!(CoreOp::I32AddWrapping.speculation(), Speculation::Always);
        assert_eq!(
            CoreOp::I32AddWrapping.result_equivalence(),
            ResultEquivalence::Structural
        );
        assert!(CoreOp::I32AddWrapping.is_trivially_discardable());
        assert_eq!(
            CoreOp::I32AddWrapping.operand_symmetry(),
            OperandSymmetry::CommutativePair
        );
        assert_eq!(
            CoreOp::I32Compare(I32Predicate::Eq).operand_symmetry(),
            OperandSymmetry::CommutativePair
        );
        assert_eq!(
            CoreOp::I32Compare(I32Predicate::SignedLt).operand_symmetry(),
            OperandSymmetry::Ordered
        );
        assert_eq!(TypedCoreConstant::I32(3).ty(), CoreType::I32);

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
        assert_eq!(
            CoreOp::Call(callee).result_equivalence(),
            ResultEquivalence::Opaque
        );
        assert!(!CoreOp::Call(callee).is_trivially_discardable());
        assert_eq!(
            CoreOp::Call(callee)
                .function_references(&program, OriginId::UNKNOWN)
                .map(super::FunctionReference::function)
                .collect::<Vec<_>>(),
            [callee]
        );

        let fragment = program
            .declare_target_fragment(
                TargetFragment::unsafe_minecraft_command("say opaque").unwrap(),
            )
            .unwrap();
        let external = program
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let external = CoreOp::External(external);
        assert_eq!(external.effects(), EffectClass::Unknown);
        assert_eq!(external.speculation(), Speculation::Never);
        assert_eq!(external.result_equivalence(), ResultEquivalence::Opaque);
        assert!(!external.is_trivially_discardable());
        assert!(
            external
                .function_references(&program, OriginId::UNKNOWN)
                .next()
                .is_none()
        );
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
