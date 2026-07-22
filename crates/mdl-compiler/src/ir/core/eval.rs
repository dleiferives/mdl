//! Bounded target-independent execution of verified Core programs.
//!
//! This evaluator defines executable reference semantics for the closed pure Core
//! subset. It deliberately does not lower operations, inspect physical
//! realizations, or emulate Minecraft external behavior.

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::diagnostic::Diagnostics;
use crate::entity::EntityVec;
use crate::source::SourceContext;

use super::{
    BlockId, BlockTarget, CoreOp, CoreProgram, CoreType, ExternalOpId, FunctionId, I32Predicate,
    InstId, TerminatorKind, ValueId, verify_program,
};

/// One runtime value in the currently executable Core subset.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CoreValue {
    /// A logical truth value.
    Bool(bool),
    /// A signed 32-bit integer.
    I32(i32),
    /// An immutable ordered `i32` list.
    ListI32(Arc<[i32]>),
    /// An immutable Java-compatible string, represented as UTF-16 code units.
    String(Arc<[u16]>),
}

impl CoreValue {
    /// Returns the Core type of this runtime value.
    #[must_use]
    pub const fn ty(&self) -> CoreType {
        match self {
            Self::Bool(_) => CoreType::Bool,
            Self::I32(_) => CoreType::I32,
            Self::ListI32(_) => CoreType::ListI32,
            Self::String(_) => CoreType::String,
        }
    }

    /// Constructs an immutable evaluator list value.
    #[must_use]
    pub fn list_i32(values: impl Into<Box<[i32]>>) -> Self {
        Self::ListI32(Arc::from(values.into()))
    }

    /// Returns list elements when this is an `i32` list.
    #[must_use]
    pub fn as_list_i32(&self) -> Option<&[i32]> {
        match self {
            Self::ListI32(values) => Some(values),
            Self::Bool(_) | Self::I32(_) | Self::String(_) => None,
        }
    }

    /// Constructs an evaluator string from Unicode text using Java UTF-16 units.
    #[must_use]
    pub fn string(value: &str) -> Self {
        Self::String(Arc::from(
            value.encode_utf16().collect::<Vec<_>>().into_boxed_slice(),
        ))
    }

    /// Returns UTF-16 code units when this is a string.
    #[must_use]
    pub fn as_string_units(&self) -> Option<&[u16]> {
        match self {
            Self::String(units) => Some(units),
            Self::Bool(_) | Self::I32(_) | Self::ListI32(_) => None,
        }
    }
}

impl fmt::Display for CoreValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::I32(value) => write!(formatter, "{value}"),
            Self::ListI32(values) => write!(formatter, "{values:?}"),
            Self::String(units) => write!(formatter, "{:?}", String::from_utf16_lossy(units)),
        }
    }
}

/// Explicit resource bounds for one Core evaluation.
///
/// A step is one executed Core instruction or terminator. It is an evaluator
/// resource unit, not a prediction of Minecraft command work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    clippy::struct_field_names,
    reason = "the repeated max prefix makes each independent evaluator ceiling explicit"
)]
pub struct CoreEvaluationLimits {
    max_steps: usize,
    max_call_depth: usize,
    max_value_nodes: usize,
}

impl CoreEvaluationLimits {
    /// Creates explicit evaluator bounds.
    ///
    /// Both explicit values may be zero. A zero step limit prevents the first
    /// instruction or terminator from executing. A zero call-depth limit prevents
    /// entry into even the root function. The value-node limit starts at its
    /// documented default and can be changed with [`Self::with_max_value_nodes`].
    #[must_use]
    pub const fn new(max_steps: usize, max_call_depth: usize) -> Self {
        Self {
            max_steps,
            max_call_depth,
            max_value_nodes: 1_000_000,
        }
    }

    /// Overrides the maximum number of scalar/container nodes created during one
    /// evaluation. A list or string contributes one container node plus one node
    /// per element or UTF-16 unit.
    #[must_use]
    pub const fn with_max_value_nodes(mut self, max_value_nodes: usize) -> Self {
        self.max_value_nodes = max_value_nodes;
        self
    }

    /// Returns the maximum executed instruction/terminator count.
    #[must_use]
    pub const fn max_steps(self) -> usize {
        self.max_steps
    }

    /// Returns the maximum number of simultaneously active function frames.
    #[must_use]
    pub const fn max_call_depth(self) -> usize {
        self.max_call_depth
    }

    /// Returns the maximum number of evaluator values and container elements that
    /// may be created during one invocation.
    #[must_use]
    pub const fn max_value_nodes(self) -> usize {
        self.max_value_nodes
    }
}

/// One ordered internal-call observation made during Core evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreCallEvent {
    caller: FunctionId,
    instruction: InstId,
    callee: FunctionId,
    arguments: Box<[CoreValue]>,
}

impl CoreCallEvent {
    /// Creates one expected or recorded call event.
    #[must_use]
    pub fn new(
        calling_function: FunctionId,
        instruction: InstId,
        target_function: FunctionId,
        arguments: impl Into<Box<[CoreValue]>>,
    ) -> Self {
        Self {
            caller: calling_function,
            instruction,
            callee: target_function,
            arguments: arguments.into(),
        }
    }

    /// Returns the active caller.
    #[must_use]
    pub const fn caller(&self) -> FunctionId {
        self.caller
    }

    /// Returns the exact call instruction occurrence.
    #[must_use]
    pub const fn instruction(&self) -> InstId {
        self.instruction
    }

    /// Returns the invoked function.
    #[must_use]
    pub const fn callee(&self) -> FunctionId {
        self.callee
    }

    /// Returns evaluated call arguments in signature order.
    #[must_use]
    pub fn arguments(&self) -> &[CoreValue] {
        &self.arguments
    }
}

/// Complete successful observation of one Core invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreEvaluation {
    results: Box<[CoreValue]>,
    call_events: Box<[CoreCallEvent]>,
    steps: usize,
    peak_call_depth: usize,
}

impl CoreEvaluation {
    /// Returns function results in signature order.
    #[must_use]
    pub fn results(&self) -> &[CoreValue] {
        &self.results
    }

    /// Returns internal calls in exact execution order.
    #[must_use]
    pub fn call_events(&self) -> &[CoreCallEvent] {
        &self.call_events
    }

    /// Returns the number of executed Core instructions and terminators.
    #[must_use]
    pub const fn steps(&self) -> usize {
        self.steps
    }

    /// Returns the maximum number of simultaneously active function frames.
    #[must_use]
    pub const fn peak_call_depth(&self) -> usize {
        self.peak_call_depth
    }
}

/// Deterministic failure to evaluate a Core invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreEvaluationError {
    /// The requested function ID is outside this program.
    InvalidFunction {
        /// Requested identity.
        function: FunctionId,
    },
    /// The requested function has no body.
    UndefinedFunction {
        /// Undefined identity.
        function: FunctionId,
    },
    /// Invocation arity disagrees with the declared signature.
    ArgumentCountMismatch {
        /// Invoked function.
        function: FunctionId,
        /// Declared parameter count.
        expected: usize,
        /// Supplied argument count.
        actual: usize,
    },
    /// One invocation argument has the wrong type.
    ArgumentTypeMismatch {
        /// Invoked function.
        function: FunctionId,
        /// Zero-based parameter index.
        index: usize,
        /// Declared parameter type.
        expected: CoreType,
        /// Supplied value type.
        actual: CoreType,
    },
    /// The next instruction or terminator would exceed the evaluator step limit.
    StepLimitExceeded {
        /// Successfully completed steps.
        completed: usize,
        /// Configured maximum.
        limit: usize,
    },
    /// Entering a function would exceed the active-frame limit.
    CallDepthLimitExceeded {
        /// Function whose frame could not be entered.
        function: FunctionId,
        /// Active frame count before the attempted entry.
        active: usize,
        /// Configured maximum.
        limit: usize,
    },
    /// Producing the next value would exceed the evaluator allocation-work cap.
    ValueNodeLimitExceeded {
        /// Nodes successfully created before the attempted value.
        created: usize,
        /// Nodes required by the attempted value production.
        requested: usize,
        /// Configured maximum.
        limit: usize,
    },
    /// Target-specific external behavior has no Core reference execution.
    UnsupportedExternal {
        /// Function containing the occurrence.
        function: FunctionId,
        /// Exact instruction occurrence.
        instruction: InstId,
        /// External declaration being invoked.
        operation: ExternalOpId,
    },
    /// Execution reached a terminator asserting that control is impossible.
    ReachedUnreachable {
        /// Active function.
        function: FunctionId,
        /// Active block.
        block: BlockId,
    },
    /// A program that should have been verified violated an evaluator invariant.
    InvalidProgramState {
        /// Best available active function.
        function: Option<FunctionId>,
        /// Stable invariant description.
        detail: &'static str,
    },
}

impl fmt::Display for CoreEvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFunction { function } => {
                write!(formatter, "invalid function {function:?}")
            }
            Self::UndefinedFunction { function } => {
                write!(formatter, "function {function:?} has no definition")
            }
            Self::ArgumentCountMismatch {
                function,
                expected,
                actual,
            } => write!(
                formatter,
                "function {function:?} expects {expected} arguments, received {actual}"
            ),
            Self::ArgumentTypeMismatch {
                function,
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "argument {index} to function {function:?} has type {actual}, expected {expected}"
            ),
            Self::StepLimitExceeded { completed, limit } => write!(
                formatter,
                "Core evaluation reached its step limit {limit} after {completed} steps"
            ),
            Self::CallDepthLimitExceeded {
                function,
                active,
                limit,
            } => write!(
                formatter,
                "entering function {function:?} at active depth {active} exceeds call-depth limit {limit}"
            ),
            Self::ValueNodeLimitExceeded {
                created,
                requested,
                limit,
            } => write!(
                formatter,
                "Core evaluation created {created} value nodes; the next {requested} exceed limit {limit}"
            ),
            Self::UnsupportedExternal {
                function,
                instruction,
                operation,
            } => write!(
                formatter,
                "function {function:?} instruction {instruction:?} invokes unsupported external operation {operation:?}"
            ),
            Self::ReachedUnreachable { function, block } => write!(
                formatter,
                "Core evaluation reached unreachable in function {function:?} block {block:?}"
            ),
            Self::InvalidProgramState { function, detail } => {
                write!(
                    formatter,
                    "invalid verified Core state in {function:?}: {detail}"
                )
            }
        }
    }
}

impl Error for CoreEvaluationError {}

/// Reusable evaluator over one immutable Core program.
pub struct CoreEvaluator<'program> {
    program: &'program CoreProgram,
    limits: CoreEvaluationLimits,
}

impl<'program> CoreEvaluator<'program> {
    /// Verifies a program and creates an evaluator over it.
    ///
    /// # Errors
    ///
    /// Returns complete Core verifier diagnostics when the program is malformed.
    pub fn checked(
        program: &'program CoreProgram,
        sources: &SourceContext,
        limits: CoreEvaluationLimits,
    ) -> Result<Self, Diagnostics> {
        verify_program(program, sources)?;
        Ok(Self::for_verified_program(program, limits))
    }

    /// Creates an evaluator for a program already accepted by [`verify_program`].
    ///
    /// Prefer [`Self::checked`] at an untrusted boundary. This constructor avoids
    /// repeated verification in differential/parameterized tests. Evaluation still
    /// reports an invariant failure instead of relying on unchecked memory access if
    /// the contract is violated.
    #[must_use]
    pub const fn for_verified_program(
        program: &'program CoreProgram,
        limits: CoreEvaluationLimits,
    ) -> Self {
        Self { program, limits }
    }

    /// Executes one internal function with typed arguments.
    ///
    /// # Errors
    ///
    /// Returns a structured invocation, resource-limit, unsupported-external, or
    /// invalid-program-state failure.
    pub fn evaluate(
        &self,
        function: FunctionId,
        arguments: &[CoreValue],
    ) -> Result<CoreEvaluation, CoreEvaluationError> {
        let declaration = self
            .program
            .function(function)
            .ok_or(CoreEvaluationError::InvalidFunction { function })?;
        validate_arguments(function, declaration.parameters(), arguments)?;
        if self.limits.max_call_depth == 0 {
            return Err(CoreEvaluationError::CallDepthLimitExceeded {
                function,
                active: 0,
                limit: 0,
            });
        }

        let mut value_nodes_created = 0;
        charge_value_nodes(
            &mut value_nodes_created,
            arguments,
            self.limits.max_value_nodes,
        )?;
        let mut state = EvaluationState {
            frames: vec![new_frame(self.program, function, arguments)?],
            call_events: vec![],
            steps: 0,
            peak_call_depth: 1,
            value_nodes_created,
        };

        loop {
            match self.next_action(&state)? {
                EvaluationAction::Instruction(instruction) => {
                    self.execute_instruction(&mut state, instruction)?;
                }
                EvaluationAction::Terminator(terminator) => {
                    if let Some(evaluation) = self.execute_terminator(&mut state, terminator)? {
                        return Ok(evaluation);
                    }
                }
            }
        }
    }

    fn next_action(
        &self,
        state: &EvaluationState,
    ) -> Result<EvaluationAction, CoreEvaluationError> {
        let frame = state
            .frames
            .last()
            .ok_or(invalid(None, "empty frame stack"))?;
        let body = self
            .program
            .function(frame.function)
            .and_then(super::Function::body)
            .ok_or_else(|| invalid(Some(frame.function), "active function has no body"))?;
        let block = body
            .block(frame.block)
            .ok_or_else(|| invalid(Some(frame.function), "active block is invalid"))?;
        if let Some(instruction) = block.instructions().get(frame.next_instruction).copied() {
            return Ok(EvaluationAction::Instruction(instruction));
        }
        let terminator = block
            .terminator()
            .ok_or_else(|| invalid(Some(frame.function), "active block has no terminator"))?
            .kind()
            .clone();
        Ok(EvaluationAction::Terminator(terminator))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "instruction dispatch intentionally keeps every Core subset opcode exhaustive"
    )]
    fn execute_instruction(
        &self,
        state: &mut EvaluationState,
        instruction: InstId,
    ) -> Result<(), CoreEvaluationError> {
        charge_step(&mut state.steps, self.limits)?;
        let active_call_depth = state.frames.len();
        let frame = state
            .frames
            .last_mut()
            .ok_or(invalid(None, "empty frame stack"))?;
        let body = self
            .program
            .function(frame.function)
            .and_then(super::Function::body)
            .ok_or_else(|| invalid(Some(frame.function), "active function has no body"))?;
        let data = body
            .instruction(instruction)
            .ok_or_else(|| invalid(Some(frame.function), "attached instruction is invalid"))?;
        let op = data.op().clone();
        let operands = data
            .operands()
            .iter()
            .map(|value| read_value(frame, *value))
            .collect::<Result<Vec<_>, _>>()?;
        let results = data.results().to_vec();
        frame.next_instruction += 1;

        match op {
            CoreOp::Call(target_function) => {
                if frame.pending_call.is_some() {
                    return Err(invalid(
                        Some(frame.function),
                        "caller already has a pending call",
                    ));
                }
                if active_call_depth == self.limits.max_call_depth {
                    return Err(CoreEvaluationError::CallDepthLimitExceeded {
                        function: target_function,
                        active: active_call_depth,
                        limit: self.limits.max_call_depth,
                    });
                }
                let calling_function = frame.function;
                frame.pending_call = Some(PendingCall { results });
                state.call_events.push(CoreCallEvent::new(
                    calling_function,
                    instruction,
                    target_function,
                    operands.clone(),
                ));
                state
                    .frames
                    .push(new_frame(self.program, target_function, &operands)?);
                state.peak_call_depth = state.peak_call_depth.max(state.frames.len());
            }
            CoreOp::External(operation) => {
                return Err(CoreEvaluationError::UnsupportedExternal {
                    function: frame.function,
                    instruction,
                    operation,
                });
            }
            scalar => {
                let produced = evaluate_scalar(&scalar, &operands).ok_or_else(|| {
                    invalid(
                        Some(frame.function),
                        "instruction does not match scalar reference semantics",
                    )
                })?;
                if produced.len() != results.len() {
                    return Err(invalid(
                        Some(frame.function),
                        "scalar result count disagrees with instruction",
                    ));
                }
                charge_value_nodes(
                    &mut state.value_nodes_created,
                    &produced,
                    self.limits.max_value_nodes,
                )?;
                for (result, value) in results.into_iter().zip(produced) {
                    write_value(body, frame, result, value)?;
                }
            }
        }
        Ok(())
    }

    fn execute_terminator(
        &self,
        state: &mut EvaluationState,
        terminator: TerminatorKind,
    ) -> Result<Option<CoreEvaluation>, CoreEvaluationError> {
        charge_step(&mut state.steps, self.limits)?;
        let frame = state
            .frames
            .last_mut()
            .ok_or(invalid(None, "empty frame stack"))?;
        match terminator {
            TerminatorKind::Jump(target) => enter_target(self.program, frame, &target)?,
            TerminatorKind::Branch {
                condition,
                then_target,
                else_target,
            } => {
                let CoreValue::Bool(condition) = read_value(frame, condition)? else {
                    return Err(invalid(
                        Some(frame.function),
                        "branch condition is not Boolean",
                    ));
                };
                let target = if condition {
                    &then_target
                } else {
                    &else_target
                };
                enter_target(self.program, frame, target)?;
            }
            TerminatorKind::Return(values) => {
                let results = values
                    .iter()
                    .map(|value| read_value(frame, *value))
                    .collect::<Result<Vec<_>, _>>()?;
                state.frames.pop();
                let Some(calling_frame) = state.frames.last_mut() else {
                    return Ok(Some(CoreEvaluation {
                        results: results.into_boxed_slice(),
                        call_events: std::mem::take(&mut state.call_events).into_boxed_slice(),
                        steps: state.steps,
                        peak_call_depth: state.peak_call_depth,
                    }));
                };
                write_call_results(self.program, calling_frame, results)?;
            }
            TerminatorKind::Unreachable => {
                return Err(CoreEvaluationError::ReachedUnreachable {
                    function: frame.function,
                    block: frame.block,
                });
            }
        }
        Ok(None)
    }
}

enum EvaluationAction {
    Instruction(InstId),
    Terminator(TerminatorKind),
}

struct EvaluationState {
    frames: Vec<Frame>,
    call_events: Vec<CoreCallEvent>,
    steps: usize,
    peak_call_depth: usize,
    value_nodes_created: usize,
}

fn charge_value_nodes(
    created: &mut usize,
    values: &[CoreValue],
    limit: usize,
) -> Result<(), CoreEvaluationError> {
    let requested = values.iter().try_fold(0_usize, |total, value| {
        total.checked_add(match value {
            CoreValue::Bool(_) | CoreValue::I32(_) => 1,
            CoreValue::ListI32(values) => 1_usize.saturating_add(values.len()),
            CoreValue::String(units) => 1_usize.saturating_add(units.len()),
        })
    });
    let requested = requested.unwrap_or(usize::MAX);
    let next = created.checked_add(requested);
    if next.is_none_or(|next| next > limit) {
        return Err(CoreEvaluationError::ValueNodeLimitExceeded {
            created: *created,
            requested,
            limit,
        });
    }
    *created = next.expect("checked finite value-node total");
    Ok(())
}

struct PendingCall {
    results: Vec<ValueId>,
}

struct Frame {
    function: FunctionId,
    block: BlockId,
    next_instruction: usize,
    values: EntityVec<ValueId, Option<CoreValue>>,
    pending_call: Option<PendingCall>,
}

fn validate_arguments(
    function: FunctionId,
    parameters: &[CoreType],
    arguments: &[CoreValue],
) -> Result<(), CoreEvaluationError> {
    if parameters.len() != arguments.len() {
        return Err(CoreEvaluationError::ArgumentCountMismatch {
            function,
            expected: parameters.len(),
            actual: arguments.len(),
        });
    }
    for (index, (expected, argument)) in parameters.iter().zip(arguments).enumerate() {
        if *expected != argument.ty() {
            return Err(CoreEvaluationError::ArgumentTypeMismatch {
                function,
                index,
                expected: *expected,
                actual: argument.ty(),
            });
        }
    }
    Ok(())
}

fn new_frame(
    program: &CoreProgram,
    function: FunctionId,
    arguments: &[CoreValue],
) -> Result<Frame, CoreEvaluationError> {
    let declaration = program
        .function(function)
        .ok_or(CoreEvaluationError::InvalidFunction { function })?;
    validate_arguments(function, declaration.parameters(), arguments)?;
    let body = declaration
        .body()
        .ok_or(CoreEvaluationError::UndefinedFunction { function })?;
    let entry = body.entry();
    let parameters = body
        .block(entry)
        .ok_or_else(|| invalid(Some(function), "entry block is invalid"))?
        .parameters();
    if parameters.len() != arguments.len() {
        return Err(invalid(
            Some(function),
            "entry parameter count disagrees with function signature",
        ));
    }
    let mut values = EntityVec::from_constrained_values(
        body.values()
            .map(|_| None)
            .collect::<Vec<Option<CoreValue>>>(),
    );
    for (parameter, argument) in parameters.iter().zip(arguments) {
        let value = parameter.value();
        let slot = values
            .get_mut(value)
            .ok_or_else(|| invalid(Some(function), "entry parameter value is invalid"))?;
        *slot = Some(argument.clone());
    }
    Ok(Frame {
        function,
        block: entry,
        next_instruction: 0,
        values,
        pending_call: None,
    })
}

fn enter_target(
    program: &CoreProgram,
    frame: &mut Frame,
    target: &BlockTarget,
) -> Result<(), CoreEvaluationError> {
    let body = program
        .function(frame.function)
        .and_then(super::Function::body)
        .ok_or_else(|| invalid(Some(frame.function), "active function has no body"))?;
    let parameters = body
        .block(target.block())
        .ok_or_else(|| invalid(Some(frame.function), "branch target block is invalid"))?
        .parameters();
    if parameters.len() != target.arguments().len() {
        return Err(invalid(
            Some(frame.function),
            "branch argument count disagrees with target parameters",
        ));
    }
    let arguments = target
        .arguments()
        .iter()
        .map(|value| read_value(frame, *value))
        .collect::<Result<Vec<_>, _>>()?;
    for (parameter, argument) in parameters.iter().zip(arguments) {
        write_value(body, frame, parameter.value(), argument)?;
    }
    frame.block = target.block();
    frame.next_instruction = 0;
    Ok(())
}

fn write_call_results(
    program: &CoreProgram,
    calling_frame: &mut Frame,
    results: Vec<CoreValue>,
) -> Result<(), CoreEvaluationError> {
    let pending = calling_frame.pending_call.take().ok_or_else(|| {
        invalid(
            Some(calling_frame.function),
            "return has no pending caller result",
        )
    })?;
    if pending.results.len() != results.len() {
        return Err(invalid(
            Some(calling_frame.function),
            "callee result count disagrees with call instruction",
        ));
    }
    let body = program
        .function(calling_frame.function)
        .and_then(super::Function::body)
        .ok_or_else(|| invalid(Some(calling_frame.function), "caller function has no body"))?;
    for (result, value) in pending.results.into_iter().zip(results) {
        write_value(body, calling_frame, result, value)?;
    }
    Ok(())
}

fn read_value(frame: &Frame, value: ValueId) -> Result<CoreValue, CoreEvaluationError> {
    frame
        .values
        .get(value)
        .and_then(Clone::clone)
        .ok_or_else(|| invalid(Some(frame.function), "SSA value is unavailable"))
}

fn write_value(
    body: &super::FunctionBody,
    frame: &mut Frame,
    value: ValueId,
    runtime: CoreValue,
) -> Result<(), CoreEvaluationError> {
    let expected = body
        .value(value)
        .ok_or_else(|| invalid(Some(frame.function), "result value is invalid"))?
        .ty();
    if expected != runtime.ty() {
        return Err(invalid(
            Some(frame.function),
            "runtime result type disagrees with SSA value type",
        ));
    }
    let slot = frame
        .values
        .get_mut(value)
        .ok_or_else(|| invalid(Some(frame.function), "result slot is invalid"))?;
    *slot = Some(runtime);
    Ok(())
}

fn charge_step(
    completed: &mut usize,
    limits: CoreEvaluationLimits,
) -> Result<(), CoreEvaluationError> {
    if *completed == limits.max_steps {
        return Err(CoreEvaluationError::StepLimitExceeded {
            completed: *completed,
            limit: limits.max_steps,
        });
    }
    *completed += 1;
    Ok(())
}

fn evaluate_scalar(op: &CoreOp, operands: &[CoreValue]) -> Option<Vec<CoreValue>> {
    match (op, operands) {
        (CoreOp::BoolConstant(value), []) => Some(vec![CoreValue::Bool(*value)]),
        (CoreOp::I32Constant(value), []) => Some(vec![CoreValue::I32(*value)]),
        (CoreOp::I32AddWrapping, [CoreValue::I32(left), CoreValue::I32(right)]) => {
            Some(vec![CoreValue::I32(left.wrapping_add(*right))])
        }
        (CoreOp::I32SubWrapping, [CoreValue::I32(left), CoreValue::I32(right)]) => {
            Some(vec![CoreValue::I32(left.wrapping_sub(*right))])
        }
        (CoreOp::I32AddOverflowing, [CoreValue::I32(left), CoreValue::I32(right)]) => {
            let (sum, overflowed) = left.overflowing_add(*right);
            Some(vec![CoreValue::I32(sum), CoreValue::Bool(overflowed)])
        }
        (CoreOp::I32Compare(predicate), [CoreValue::I32(left), CoreValue::I32(right)]) => {
            Some(vec![CoreValue::Bool(match predicate {
                I32Predicate::Eq => left == right,
                I32Predicate::Ne => left != right,
                I32Predicate::SignedLt => left < right,
                I32Predicate::SignedLe => left <= right,
                I32Predicate::SignedGt => left > right,
                I32Predicate::SignedGe => left >= right,
            })])
        }
        (CoreOp::I32InClosedRange(range), [CoreValue::I32(value)]) => {
            Some(vec![CoreValue::Bool(range.contains(*value))])
        }
        (CoreOp::BoolNot, [CoreValue::Bool(value)]) => Some(vec![CoreValue::Bool(!value)]),
        (CoreOp::ListI32Empty, []) => Some(vec![CoreValue::list_i32(Vec::<i32>::new())]),
        (CoreOp::ListI32Length, [CoreValue::ListI32(values)]) => i32::try_from(values.len())
            .ok()
            .map(|length| vec![CoreValue::I32(length)]),
        (CoreOp::ListI32Push, [CoreValue::ListI32(values), CoreValue::I32(value)]) => {
            let mut output = values.to_vec();
            output.push(*value);
            Some(vec![CoreValue::list_i32(output)])
        }
        (CoreOp::ListI32LastOrZero, [CoreValue::ListI32(values)]) => {
            Some(vec![CoreValue::I32(values.last().copied().unwrap_or(0))])
        }
        (CoreOp::ListI32WithoutLast, [CoreValue::ListI32(values)]) => {
            let mut output = values.to_vec();
            output.pop();
            Some(vec![CoreValue::list_i32(output)])
        }
        (CoreOp::StringConstant(value), []) => Some(vec![CoreValue::string(value)]),
        (CoreOp::StringLength, [CoreValue::String(units)]) => i32::try_from(units.len())
            .ok()
            .map(|length| vec![CoreValue::I32(length)]),
        (CoreOp::StringEndsWithAscii(ascii), [CoreValue::String(units)]) => {
            Some(vec![CoreValue::Bool(
                units.last().copied() == Some(u16::from(*ascii)),
            )])
        }
        (CoreOp::StringWithoutLastUnit, [CoreValue::String(units)]) => {
            let mut output = units.to_vec();
            output.pop();
            Some(vec![CoreValue::String(Arc::from(
                output.into_boxed_slice(),
            ))])
        }
        _ => None,
    }
}

const fn invalid(function: Option<FunctionId>, detail: &'static str) -> CoreEvaluationError {
    CoreEvaluationError::InvalidProgramState { function, detail }
}

#[cfg(test)]
mod tests {
    use super::{CoreEvaluationError, CoreEvaluationLimits, CoreEvaluator, CoreValue};
    use crate::ir::core::{
        BlockTarget, CoreProgram, CoreType, ExternalSemanticBinding, FunctionBuilder, FunctionId,
        TargetFragment, Terminator, TerminatorKind,
    };
    use crate::source::{OriginId, SourceContext};

    const ORIGIN: OriginId = OriginId::UNKNOWN;

    fn parameter(
        builder: &FunctionBuilder<'_>,
        block: super::BlockId,
        index: usize,
    ) -> super::ValueId {
        builder.body().block(block).unwrap().parameters()[index].value()
    }

    fn identity_program() -> (SourceContext, CoreProgram, FunctionId) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("identity"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                ORIGIN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let input = parameter(&builder, builder.entry_block(), 0);
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![input]), ORIGIN))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        (sources, program, function)
    }

    #[test]
    fn checked_constructor_rejects_undefined_programs() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        program
            .declare_function(
                Some("undefined"),
                Vec::<CoreType>::new(),
                Vec::<CoreType>::new(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let error = CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(1, 1))
            .err()
            .expect("undefined program unexpectedly accepted");
        assert!(error.contains_code("core.undefined-function"));
    }

    #[test]
    fn runtime_values_retain_exact_core_types() {
        assert_eq!(CoreValue::Bool(false).ty(), CoreType::Bool);
        assert_eq!(CoreValue::I32(0).ty(), CoreType::I32);
    }

    #[test]
    fn invocation_rejects_wrong_argument_count_and_type() {
        let (sources, program, function) = identity_program();
        let evaluator =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(1, 1)).unwrap();

        assert_eq!(
            evaluator.evaluate(function, &[]),
            Err(CoreEvaluationError::ArgumentCountMismatch {
                function,
                expected: 1,
                actual: 0,
            })
        );
        assert_eq!(
            evaluator.evaluate(function, &[CoreValue::Bool(false)]),
            Err(CoreEvaluationError::ArgumentTypeMismatch {
                function,
                index: 0,
                expected: CoreType::I32,
                actual: CoreType::Bool,
            })
        );
    }

    #[test]
    fn step_limit_is_an_exact_instruction_and_terminator_budget() {
        let (sources, program, function) = identity_program();
        let exhausted =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(0, 1)).unwrap();
        assert_eq!(
            exhausted.evaluate(function, &[CoreValue::I32(7)]),
            Err(CoreEvaluationError::StepLimitExceeded {
                completed: 0,
                limit: 0,
            })
        );

        let exact =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(1, 1)).unwrap();
        let evaluation = exact.evaluate(function, &[CoreValue::I32(7)]).unwrap();
        assert_eq!(evaluation.results(), [CoreValue::I32(7)]);
        assert_eq!(evaluation.steps(), 1);
        assert_eq!(evaluation.peak_call_depth(), 1);
    }

    #[test]
    fn direct_recursion_is_bounded_by_active_call_depth() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("recurse"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                ORIGIN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let input = parameter(&builder, builder.entry_block(), 0);
        let result = builder.call(function, vec![input], ORIGIN).unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                ORIGIN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let evaluator =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(100, 3)).unwrap();
        assert_eq!(
            evaluator.evaluate(function, &[CoreValue::I32(9)]),
            Err(CoreEvaluationError::CallDepthLimitExceeded {
                function,
                active: 3,
                limit: 3,
            })
        );
    }

    #[test]
    fn mutual_recursion_uses_the_same_depth_contract() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let first = program
            .declare_function(Some("first"), vec![], vec![], ORIGIN)
            .unwrap();
        let second = program
            .declare_function(Some("second"), vec![], vec![], ORIGIN)
            .unwrap();
        for (function, callee) in [(first, second), (second, first)] {
            let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
            builder.call(callee, vec![], ORIGIN).unwrap();
            builder
                .terminate(Terminator::new(TerminatorKind::Return(vec![]), ORIGIN))
                .unwrap();
            program
                .define_function(function, builder.finish().unwrap())
                .unwrap();
        }

        let evaluator =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(100, 4)).unwrap();
        assert_eq!(
            evaluator.evaluate(first, &[]),
            Err(CoreEvaluationError::CallDepthLimitExceeded {
                function: first,
                active: 4,
                limit: 4,
            })
        );
    }

    #[test]
    fn zero_depth_rejects_root_entry() {
        let (sources, program, function) = identity_program();
        let evaluator =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(1, 0)).unwrap();
        assert_eq!(
            evaluator.evaluate(function, &[CoreValue::I32(0)]),
            Err(CoreEvaluationError::CallDepthLimitExceeded {
                function,
                active: 0,
                limit: 0,
            })
        );
    }

    #[test]
    fn external_operations_are_rejected_at_the_exact_occurrence() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let fragment = program
            .declare_target_fragment(
                TargetFragment::unsafe_minecraft_command("say evaluator boundary").unwrap(),
            )
            .unwrap();
        let operation = program
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                ORIGIN,
            )
            .unwrap();
        let function = program
            .declare_function(Some("external"), vec![], vec![], ORIGIN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let block = builder.entry_block();
        builder.external(operation, vec![], ORIGIN).unwrap();
        let instruction = *builder
            .body()
            .block(block)
            .unwrap()
            .instructions()
            .last()
            .unwrap();
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![]), ORIGIN))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let evaluator =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(10, 1)).unwrap();
        assert_eq!(
            evaluator.evaluate(function, &[]),
            Err(CoreEvaluationError::UnsupportedExternal {
                function,
                instruction,
                operation,
            })
        );
    }

    #[test]
    fn unreachable_is_a_distinct_execution_failure() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("impossible"), vec![], vec![], ORIGIN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let block = builder.entry_block();
        builder
            .terminate(Terminator::new(TerminatorKind::Unreachable, ORIGIN))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let evaluator =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(1, 1)).unwrap();
        assert_eq!(
            evaluator.evaluate(function, &[]),
            Err(CoreEvaluationError::ReachedUnreachable { function, block })
        );
    }

    #[test]
    fn deep_cfg_execution_is_deterministic_and_iterative() {
        const BLOCKS: usize = 2_048;

        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("deep_cfg"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                ORIGIN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let mut value = parameter(&builder, builder.entry_block(), 0);
        for _ in 0..BLOCKS {
            let next = builder.create_block(ORIGIN).unwrap();
            let next_value = builder
                .append_block_parameter(next, CoreType::I32, ORIGIN)
                .unwrap();
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Jump(BlockTarget::new(next, vec![value])),
                    ORIGIN,
                ))
                .unwrap();
            builder.switch_to_block(next).unwrap();
            value = next_value;
        }
        builder
            .terminate(Terminator::new(TerminatorKind::Return(vec![value]), ORIGIN))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let evaluator =
            CoreEvaluator::checked(&program, &sources, CoreEvaluationLimits::new(BLOCKS + 1, 1))
                .unwrap();
        let first = evaluator
            .evaluate(function, &[CoreValue::I32(123)])
            .unwrap();
        let second = evaluator
            .evaluate(function, &[CoreValue::I32(123)])
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.results(), [CoreValue::I32(123)]);
        assert_eq!(first.steps(), BLOCKS + 1);
    }
}
