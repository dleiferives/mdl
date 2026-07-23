//! Deterministic behavior inference over checked HIR.

use std::collections::{BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;

use super::context::transfer_run_requirements;
use super::hir::{
    HirBlock, HirCall, HirExpression, HirExpressionKind, HirExternalOp, HirExternalSemantic,
    HirFunction, HirIf, HirRun, HirRunModifier, HirStatement, HirStatementKind, SourceExternalOpId,
    SourceFunctionId, SourceRunId,
};
use crate::ir::semantic::{
    AmbientContextRequirements, ForkBound, FunctionBehavior, ObservableEffect, TransitiveWork,
    WorldEffect, minecraft_descriptor,
};

/// A malformed checked-HIR inventory or reference encountered during inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BehaviorInferenceError {
    NonDenseFunction {
        index: usize,
        actual: SourceFunctionId,
    },
    NonDenseExternalOperation {
        index: usize,
        actual: SourceExternalOpId,
    },
    InvalidCallTarget {
        caller: SourceFunctionId,
        callee: SourceFunctionId,
    },
    InvalidExternalOperation {
        function: SourceFunctionId,
        operation: SourceExternalOpId,
    },
    InvalidRunContextTransfer {
        function: SourceFunctionId,
        run: SourceRunId,
    },
    IdentitySpaceExhausted,
}

impl fmt::Display for BehaviorInferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonDenseFunction { index, actual } => write!(
                formatter,
                "function at behavior index {index} has non-dense identity {actual:?}"
            ),
            Self::NonDenseExternalOperation { index, actual } => write!(
                formatter,
                "external operation at behavior index {index} has non-dense identity {actual:?}"
            ),
            Self::InvalidCallTarget { caller, callee } => write!(
                formatter,
                "function {} has a reachable call to invalid function {}",
                caller.index(),
                callee.index()
            ),
            Self::InvalidExternalOperation {
                function,
                operation,
            } => write!(
                formatter,
                "function {} reaches invalid external operation {}",
                function.index(),
                operation.index()
            ),
            Self::InvalidRunContextTransfer { function, run } => write!(
                formatter,
                "function {} has incompatible context requirements in run scope {}",
                function.index(),
                run.index()
            ),
            Self::IdentitySpaceExhausted => {
                formatter.write_str("behavior-inference identity space exhausted")
            }
        }
    }
}

impl Error for BehaviorInferenceError {}

/// Infers one closed behavior summary per function in dense declaration order.
pub(super) fn infer_function_behaviors(
    functions: &[HirFunction],
    external_ops: &[HirExternalOp],
) -> Result<Box<[FunctionBehavior]>, BehaviorInferenceError> {
    validate_dense_inventories(functions, external_ops)?;
    let graph = build_call_graph(functions)?;
    let decomposition = decompose_call_graph(&graph.calls, &graph.callers);
    let mut summaries = vec![FunctionBehavior::NONE; functions.len()];
    let mut queued = vec![false; functions.len()];

    // Kosaraju discovers caller SCCs before their callee SCCs. Evaluating that
    // condensation order backwards finalizes every external callee before its
    // callers and confines fixed-point iteration to genuinely recursive SCCs.
    for component_index in (0..decomposition.components.len()).rev() {
        let component = &decomposition.components[component_index];
        let mut pending = VecDeque::with_capacity(component.members.len());
        for &function_index in &component.members {
            if component.recursive {
                summaries[function_index] = summaries[function_index].join(RECURSIVE_WORK);
            }
            pending.push_back(function_index);
            queued[function_index] = true;
        }

        while let Some(function_index) = pending.pop_front() {
            queued[function_index] = false;
            let function = &functions[function_index];
            let evaluator = BehaviorEvaluator {
                function: function.id,
                summaries: &summaries,
                external_ops,
            };
            let (evaluated, _) = evaluator.block(&function.body)?;
            let next = summaries[function_index].join(evaluated);
            if next == summaries[function_index] {
                continue;
            }
            summaries[function_index] = next;
            for &caller in &graph.callers[function_index] {
                if decomposition.component_of[caller] == component_index && !queued[caller] {
                    pending.push_back(caller);
                    queued[caller] = true;
                }
            }
        }
    }

    Ok(summaries.into_boxed_slice())
}

const FINITE_WORK: FunctionBehavior = FunctionBehavior::new(
    AmbientContextRequirements::NONE,
    WorldEffect::None,
    ObservableEffect::None,
    ForkBound::None,
    TransitiveWork::Finite,
    false,
);

const RECURSIVE_WORK: FunctionBehavior = FunctionBehavior::new(
    AmbientContextRequirements::NONE,
    WorldEffect::None,
    ObservableEffect::None,
    ForkBound::None,
    TransitiveWork::NoFiniteUpperBound,
    false,
);

const UNSAFE_UNKNOWN: FunctionBehavior = FunctionBehavior::new(
    AmbientContextRequirements::UNKNOWN,
    WorldEffect::Unknown,
    ObservableEffect::Unknown,
    ForkBound::Unknown,
    TransitiveWork::Unknown,
    true,
);

fn validate_dense_inventories(
    functions: &[HirFunction],
    external_ops: &[HirExternalOp],
) -> Result<(), BehaviorInferenceError> {
    for (index, function) in functions.iter().enumerate() {
        let expected = SourceFunctionId::from_index(index)
            .ok_or(BehaviorInferenceError::IdentitySpaceExhausted)?;
        if function.id != expected {
            return Err(BehaviorInferenceError::NonDenseFunction {
                index,
                actual: function.id,
            });
        }
    }
    for (index, operation) in external_ops.iter().enumerate() {
        let expected = SourceExternalOpId::from_index(index)
            .ok_or(BehaviorInferenceError::IdentitySpaceExhausted)?;
        if operation.id != expected {
            return Err(BehaviorInferenceError::NonDenseExternalOperation {
                index,
                actual: operation.id,
            });
        }
    }
    Ok(())
}

struct CallGraph {
    calls: Vec<Vec<usize>>,
    callers: Vec<Vec<usize>>,
}

fn build_call_graph(functions: &[HirFunction]) -> Result<CallGraph, BehaviorInferenceError> {
    let mut calls = vec![vec![]; functions.len()];
    let mut callers = vec![vec![]; functions.len()];
    for (caller_index, function) in functions.iter().enumerate() {
        let mut referenced = BTreeSet::new();
        collect_block_calls(&function.body, &mut referenced);
        for callee in referenced {
            let Some(callee_index) = callee
                .as_usize()
                .filter(|callee_index| *callee_index < functions.len())
            else {
                return Err(BehaviorInferenceError::InvalidCallTarget {
                    caller: function.id,
                    callee,
                });
            };
            calls[caller_index].push(callee_index);
            callers[callee_index].push(caller_index);
        }
    }
    Ok(CallGraph { calls, callers })
}

fn collect_block_calls(block: &HirBlock, calls: &mut BTreeSet<SourceFunctionId>) -> bool {
    for statement in &block.statements {
        if !collect_statement_calls(statement, calls) {
            return false;
        }
    }
    true
}

fn collect_statement_calls(
    statement: &HirStatement,
    calls: &mut BTreeSet<SourceFunctionId>,
) -> bool {
    match &statement.kind {
        HirStatementKind::Declaration { initializer, .. } => {
            if let Some(initializer) = initializer {
                collect_expression_calls(initializer, calls);
            }
            true
        }
        HirStatementKind::Assignment { value, .. } => {
            collect_expression_calls(value, calls);
            true
        }
        HirStatementKind::Call(call) => {
            collect_call_calls(call, calls);
            true
        }
        HirStatementKind::External(_) => true,
        HirStatementKind::If(conditional) => collect_if_calls(conditional, calls),
        HirStatementKind::Switch(switch) => {
            collect_expression_calls(&switch.scrutinee, calls);
            switch
                .arms
                .iter()
                .any(|arm| collect_block_calls(&arm.body, calls))
        }
        HirStatementKind::While(statement) => {
            collect_expression_calls(&statement.condition, calls);
            collect_block_calls(&statement.body, calls);
            true
        }
        HirStatementKind::Break | HirStatementKind::Continue => false,
        HirStatementKind::Destructure { operand, .. } => {
            collect_expression_calls(operand, calls);
            true
        }
        HirStatementKind::Run(run) => {
            collect_block_calls(&run.body, calls);
            true
        }
        HirStatementKind::Return(value) => {
            if let Some(value) = value {
                collect_expression_calls(value, calls);
            }
            false
        }
    }
}

fn collect_if_calls(conditional: &HirIf, calls: &mut BTreeSet<SourceFunctionId>) -> bool {
    let mut any_arm_continues = false;
    for arm in &conditional.arms {
        collect_expression_calls(&arm.condition, calls);
        any_arm_continues |= collect_block_calls(&arm.body, calls);
    }
    conditional
        .else_body
        .as_ref()
        .is_none_or(|else_body| collect_block_calls(else_body, calls) || any_arm_continues)
}

fn collect_call_calls(call: &HirCall, calls: &mut BTreeSet<SourceFunctionId>) {
    for argument in &call.arguments {
        collect_expression_calls(argument, calls);
    }
    calls.insert(call.callee);
}

fn collect_expression_calls(expression: &HirExpression, calls: &mut BTreeSet<SourceFunctionId>) {
    match &expression.kind {
        HirExpressionKind::Call(call) => collect_call_calls(call, calls),
        HirExpressionKind::Switch(switch) => {
            collect_expression_calls(&switch.scrutinee, calls);
            for arm in &switch.arms {
                collect_expression_calls(&arm.body, calls);
            }
        }
        HirExpressionKind::StructConstruct { fields, .. }
        | HirExpressionKind::AnonymousStructConstruct { fields, .. } => {
            for field in fields {
                collect_expression_calls(&field.value, calls);
            }
        }
        HirExpressionKind::StructProject { aggregate, .. }
        | HirExpressionKind::AnonymousStructProject { aggregate, .. }
        | HirExpressionKind::Index { aggregate, .. } => {
            collect_expression_calls(aggregate, calls);
        }
        HirExpressionKind::ListI32 { operands, .. }
        | HirExpressionKind::String { operands, .. } => {
            for operand in operands {
                collect_expression_calls(operand, calls);
            }
        }
        HirExpressionKind::Not(operand) => collect_expression_calls(operand, calls),
        HirExpressionKind::WrappingArithmetic { left, right, .. }
        | HirExpressionKind::Compare { left, right, .. } => {
            collect_expression_calls(left, calls);
            collect_expression_calls(right, calls);
        }
        HirExpressionKind::External(_)
        | HirExpressionKind::Bool(_)
        | HirExpressionKind::Int32(_)
        | HirExpressionKind::EnumVariant { .. }
        | HirExpressionKind::Local(_) => {}
    }
}

#[derive(Debug, Eq, PartialEq)]
struct CallComponent {
    members: Vec<usize>,
    recursive: bool,
}

#[derive(Debug, Eq, PartialEq)]
struct CallGraphDecomposition {
    /// SCCs in caller-before-callee condensation order.
    components: Vec<CallComponent>,
    component_of: Vec<usize>,
}

/// Builds a deterministic SCC condensation using iterative Kosaraju traversal.
fn decompose_call_graph(calls: &[Vec<usize>], callers: &[Vec<usize>]) -> CallGraphDecomposition {
    let mut visited = vec![false; calls.len()];
    let mut finish_order = Vec::with_capacity(calls.len());
    for root in 0..calls.len() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut stack = vec![(root, 0_usize)];
        while let Some((node, next_edge)) = stack.last_mut() {
            if let Some(&successor) = calls[*node].get(*next_edge) {
                *next_edge += 1;
                if !visited[successor] {
                    visited[successor] = true;
                    stack.push((successor, 0));
                }
            } else {
                let (finished, _) = stack.pop().expect("the DFS stack is nonempty");
                finish_order.push(finished);
            }
        }
    }

    let mut assigned = vec![false; calls.len()];
    let mut components = vec![];
    let mut component_of = vec![usize::MAX; calls.len()];
    for &root in finish_order.iter().rev() {
        if assigned[root] {
            continue;
        }
        assigned[root] = true;
        let mut component = vec![];
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            component.push(node);
            for &predecessor in &callers[node] {
                if !assigned[predecessor] {
                    assigned[predecessor] = true;
                    stack.push(predecessor);
                }
            }
        }
        component.sort_unstable();
        let recursive = component.len() > 1
            || component
                .first()
                .is_some_and(|node| calls[*node].binary_search(node).is_ok());
        let component_index = components.len();
        for &node in &component {
            component_of[node] = component_index;
        }
        components.push(CallComponent {
            members: component,
            recursive,
        });
    }
    CallGraphDecomposition {
        components,
        component_of,
    }
}

struct BehaviorEvaluator<'a> {
    function: SourceFunctionId,
    summaries: &'a [FunctionBehavior],
    external_ops: &'a [HirExternalOp],
}

impl BehaviorEvaluator<'_> {
    fn block(&self, block: &HirBlock) -> Result<(FunctionBehavior, bool), BehaviorInferenceError> {
        let mut behavior = FunctionBehavior::NONE;
        for statement in &block.statements {
            let (statement_behavior, continues) = self.statement(statement)?;
            behavior = behavior.join(statement_behavior);
            if !continues {
                return Ok((behavior, false));
            }
        }
        Ok((behavior, true))
    }

    fn statement(
        &self,
        statement: &HirStatement,
    ) -> Result<(FunctionBehavior, bool), BehaviorInferenceError> {
        let result = match &statement.kind {
            HirStatementKind::Declaration { initializer, .. } => (
                initializer
                    .as_ref()
                    .map(|value| self.expression(value).map(|value| value.join(FINITE_WORK)))
                    .transpose()?
                    .unwrap_or(FunctionBehavior::NONE),
                true,
            ),
            HirStatementKind::Assignment { value, .. } => {
                (self.expression(value)?.join(FINITE_WORK), true)
            }
            HirStatementKind::Call(call) => (self.call(call)?, true),
            HirStatementKind::External(operation) => (self.external(*operation)?, true),
            HirStatementKind::If(conditional) => self.conditional(conditional)?,
            HirStatementKind::Switch(switch) => {
                let mut behavior = self.expression(&switch.scrutinee)?.join(FINITE_WORK);
                let mut continues = false;
                for arm in &switch.arms {
                    let (arm_behavior, arm_continues) = self.block(&arm.body)?;
                    behavior = behavior.join(arm_behavior);
                    continues |= arm_continues;
                }
                (behavior, continues)
            }
            HirStatementKind::While(statement) => {
                let condition = self.expression(&statement.condition)?;
                let (body, _) = self.block(&statement.body)?;
                (condition.join(body).join(RECURSIVE_WORK), true)
            }
            HirStatementKind::Break | HirStatementKind::Continue => (FunctionBehavior::NONE, false),
            HirStatementKind::Destructure { operand, .. } => {
                (self.expression(operand)?.join(FINITE_WORK), true)
            }
            HirStatementKind::Run(run) => (self.run(run)?, true),
            HirStatementKind::Return(value) => (
                value
                    .as_ref()
                    .map(|value| self.expression(value).map(|value| value.join(FINITE_WORK)))
                    .transpose()?
                    .unwrap_or(FunctionBehavior::NONE),
                false,
            ),
        };
        Ok(result)
    }

    fn conditional(
        &self,
        conditional: &HirIf,
    ) -> Result<(FunctionBehavior, bool), BehaviorInferenceError> {
        let mut behavior = FINITE_WORK;
        let mut any_arm_continues = false;
        for arm in &conditional.arms {
            behavior = behavior.join(self.expression(&arm.condition)?);
            let (body, continues) = self.block(&arm.body)?;
            behavior = behavior.join(body);
            any_arm_continues |= continues;
        }
        let continues = if let Some(else_body) = &conditional.else_body {
            let (else_behavior, else_continues) = self.block(else_body)?;
            behavior = behavior.join(else_behavior);
            any_arm_continues || else_continues
        } else {
            true
        };
        Ok((behavior, continues))
    }

    fn run(&self, run: &HirRun) -> Result<FunctionBehavior, BehaviorInferenceError> {
        let (body, _) = self.block(&run.body)?;
        let requirements =
            transfer_run_requirements(body.required_ambient_context(), &run.modifiers).map_err(
                |_| BehaviorInferenceError::InvalidRunContextTransfer {
                    function: self.function,
                    run: run.id,
                },
            )?;
        let mut prefix_fork = ForkBound::None;
        let mut effect = body.world_effect();
        for modifier in &run.modifiers {
            match modifier {
                HirRunModifier::As { query, .. } | HirRunModifier::At { query, .. } => {
                    prefix_fork = prefix_fork.multiply(query_fork_bound(query.semantic.maximum()));
                    effect = effect.join(WorldEffect::Read);
                }
                HirRunModifier::AtExecutor { .. }
                | HirRunModifier::Positioned { .. }
                | HirRunModifier::Rotated { .. }
                | HirRunModifier::In { .. }
                | HirRunModifier::Anchored { .. }
                | HirRunModifier::Align { .. } => {}
            }
        }
        Ok(FunctionBehavior::new(
            requirements,
            effect,
            body.observable_effect(),
            prefix_fork.join(body.fork_bound()),
            contextual_work(prefix_fork, body.transitive_work()),
            body.contains_unsafe_unknown(),
        ))
    }

    fn external(
        &self,
        operation: SourceExternalOpId,
    ) -> Result<FunctionBehavior, BehaviorInferenceError> {
        let Some(operation) = operation
            .as_usize()
            .and_then(|index| self.external_ops.get(index))
        else {
            return Err(BehaviorInferenceError::InvalidExternalOperation {
                function: self.function,
                operation,
            });
        };
        match &operation.semantic {
            HirExternalSemantic::UnsafeMinecraftCommand { .. } => Ok(UNSAFE_UNKNOWN),
            HirExternalSemantic::MinecraftOperation {
                key,
                receiver_kind,
                attributes,
                ..
            } => {
                let descriptor = minecraft_descriptor(*key);
                let requirements = match attributes {
                    super::hir::HirMinecraftOperationAttributes::Say { .. }
                    | super::hir::HirMinecraftOperationAttributes::MoveBy { .. } => {
                        crate::ir::semantic::AmbientContextRequirements::NONE.with_executor(
                            crate::ir::semantic::ContextRequirement::Required(*receiver_kind),
                        )
                    }
                    super::hir::HirMinecraftOperationAttributes::Teleport { position, .. } => {
                        let mut requirements =
                            crate::ir::semantic::AmbientContextRequirements::NONE
                                .with_executor(crate::ir::semantic::ContextRequirement::Required(
                                    *receiver_kind,
                                ))
                                .with_dimension(crate::ir::semantic::ContextRequirement::Required(
                                    (),
                                ));
                        match position {
                            crate::ir::semantic::PositionSpec::World(position)
                                if position.reads_position() =>
                            {
                                requirements = requirements.with_position(
                                    crate::ir::semantic::ContextRequirement::Required(()),
                                );
                            }
                            crate::ir::semantic::PositionSpec::Local(_) => {
                                requirements = requirements
                                    .with_position(
                                        crate::ir::semantic::ContextRequirement::Required(()),
                                    )
                                    .with_rotation(
                                        crate::ir::semantic::ContextRequirement::Required(()),
                                    )
                                    .with_anchor(
                                        crate::ir::semantic::ContextRequirement::Required(()),
                                    );
                            }
                            crate::ir::semantic::PositionSpec::World(_) => {}
                        }
                        requirements
                    }
                };
                Ok(FunctionBehavior::new(
                    requirements,
                    descriptor.world_effect(),
                    descriptor.observable_effect(),
                    descriptor.fork_behavior(),
                    descriptor.work_behavior(),
                    false,
                ))
            }
            HirExternalSemantic::EntityNbtRead { receiver, .. } => {
                // Same real-world behavior as the retired ReadMainHandWrittenBookLiteralPage
                // verb it generalizes: a pure, finite, non-forking NBT read. An entity
                // receiver requires the current executor; a block receiver (BE-1) is
                // self-contained in its position and needs no ambient context at all.
                let requirements = match receiver {
                    super::hir::HirEntityNbtReceiver::Entity { kind, .. } => {
                        AmbientContextRequirements::NONE
                            .with_executor(crate::ir::semantic::ContextRequirement::Required(*kind))
                    }
                    super::hir::HirEntityNbtReceiver::Block { .. } => {
                        AmbientContextRequirements::NONE
                    }
                };
                Ok(FunctionBehavior::new(
                    requirements,
                    WorldEffect::Read,
                    ObservableEffect::None,
                    ForkBound::None,
                    TransitiveWork::Finite,
                    false,
                ))
            }
            HirExternalSemantic::EntityNbtWrite { .. } => {
                // A whole-slot write (PS-16, BE-2): lowers unconditionally to
                // `item replace`, a pure, finite, non-forking world mutation.
                // Always a block receiver, self-contained in its position —
                // no ambient context, mirroring `EntityNbtRead`'s own `Block`
                // case.
                Ok(FunctionBehavior::new(
                    AmbientContextRequirements::NONE,
                    WorldEffect::Write,
                    ObservableEffect::None,
                    ForkBound::None,
                    TransitiveWork::Finite,
                    false,
                ))
            }
        }
    }

    fn expression(
        &self,
        expression: &HirExpression,
    ) -> Result<FunctionBehavior, BehaviorInferenceError> {
        match &expression.kind {
            HirExpressionKind::Bool(_)
            | HirExpressionKind::Int32(_)
            | HirExpressionKind::EnumVariant { .. } => Ok(FINITE_WORK),
            HirExpressionKind::Local(_) => Ok(FunctionBehavior::NONE),
            HirExpressionKind::Call(call) => self.call(call),
            HirExpressionKind::External(operation) => self.external(*operation),
            HirExpressionKind::StructConstruct { fields, .. }
            | HirExpressionKind::AnonymousStructConstruct { fields, .. } => {
                let mut behavior = FINITE_WORK;
                for field in fields {
                    behavior = behavior.join(self.expression(&field.value)?);
                }
                Ok(behavior)
            }
            HirExpressionKind::StructProject { aggregate, .. }
            | HirExpressionKind::AnonymousStructProject { aggregate, .. }
            | HirExpressionKind::Index { aggregate, .. } => {
                Ok(self.expression(aggregate)?.join(FINITE_WORK))
            }
            HirExpressionKind::ListI32 { operands, .. }
            | HirExpressionKind::String { operands, .. } => {
                let mut behavior = FINITE_WORK;
                for operand in operands {
                    behavior = behavior.join(self.expression(operand)?);
                }
                Ok(behavior)
            }
            HirExpressionKind::Not(operand) => Ok(self.expression(operand)?.join(FINITE_WORK)),
            HirExpressionKind::WrappingArithmetic { left, right, .. }
            | HirExpressionKind::Compare { left, right, .. } => Ok(self
                .expression(left)?
                .join(self.expression(right)?)
                .join(FINITE_WORK)),
            HirExpressionKind::Switch(switch) => {
                let mut behavior = self.expression(&switch.scrutinee)?.join(FINITE_WORK);
                for arm in &switch.arms {
                    behavior = behavior.join(self.expression(&arm.body)?);
                }
                Ok(behavior)
            }
        }
    }

    fn call(&self, call: &HirCall) -> Result<FunctionBehavior, BehaviorInferenceError> {
        let mut behavior = FINITE_WORK;
        for argument in &call.arguments {
            behavior = behavior.join(self.expression(argument)?);
        }
        let Some(callee) = call
            .callee
            .as_usize()
            .and_then(|index| self.summaries.get(index))
        else {
            return Err(BehaviorInferenceError::InvalidCallTarget {
                caller: self.function,
                callee: call.callee,
            });
        };
        Ok(behavior.join(*callee))
    }
}

fn query_fork_bound(maximum: Option<u32>) -> ForkBound {
    maximum.map_or(ForkBound::NoFiniteUpperBound, |maximum| {
        ForkBound::finite(u64::from(maximum)).unwrap_or(ForkBound::NoFiniteUpperBound)
    })
}

fn contextual_work(prefix: ForkBound, body: TransitiveWork) -> TransitiveWork {
    match (prefix, body) {
        (ForkBound::Unknown, _) | (_, TransitiveWork::Unknown) => TransitiveWork::Unknown,
        (ForkBound::NoFiniteUpperBound, _) | (_, TransitiveWork::NoFiniteUpperBound) => {
            TransitiveWork::NoFiniteUpperBound
        }
        (ForkBound::None, body) => body,
        (ForkBound::Finite(_), TransitiveWork::Zero | TransitiveWork::Finite) => {
            TransitiveWork::Finite
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CallComponent, contextual_work, decompose_call_graph, infer_function_behaviors};
    use crate::frontend::context::{apply_run_modifiers, function_entry_context};
    use crate::frontend::hir::{
        FunctionResult, FunctionVisibility, HirBlock, HirCall, HirEntityQuery, HirEntityQueryStep,
        HirExternalOp, HirExternalSemantic, HirFunction, HirRun, HirRunModifier, HirStatement,
        HirStatementKind, SourceExternalOpId, SourceFunctionId, SourceModuleId, SourceRunId,
    };
    use crate::ir::semantic::{
        EntityKind, ForkBound, ObservableEffect, StaticEntityQuery, TransitiveWork,
    };
    use crate::source::OriginId;
    use std::num::NonZeroU32;

    #[test]
    fn iterative_scc_detection_is_caller_first_and_marks_recursion() {
        let calls = vec![vec![1], vec![0], vec![3], vec![3], vec![]];
        let callers = vec![vec![1], vec![0], vec![], vec![2, 3], vec![]];

        let decomposition = decompose_call_graph(&calls, &callers);
        assert_eq!(decomposition.component_of[0], decomposition.component_of[1]);
        assert!(decomposition.components[decomposition.component_of[0]].recursive);
        assert!(decomposition.components[decomposition.component_of[3]].recursive);
        assert!(!decomposition.components[decomposition.component_of[2]].recursive);
        assert!(!decomposition.components[decomposition.component_of[4]].recursive);
        assert!(decomposition.component_of[2] < decomposition.component_of[3]);
        assert!(decomposition.components.contains(&CallComponent {
            members: vec![0, 1],
            recursive: true,
        }));
    }

    #[test]
    fn contextual_work_preserves_unknown_and_unbounded_forks() {
        assert_eq!(
            contextual_work(ForkBound::None, TransitiveWork::Zero),
            TransitiveWork::Zero
        );
        assert_eq!(
            contextual_work(ForkBound::NoFiniteUpperBound, TransitiveWork::Zero),
            TransitiveWork::NoFiniteUpperBound
        );
        assert_eq!(
            contextual_work(ForkBound::finite(3).unwrap(), TransitiveWork::Unknown),
            TransitiveWork::Unknown
        );
    }

    #[test]
    fn large_chains_wide_fanout_and_cycles_stay_iterative() {
        const FUNCTIONS: usize = 20_000;

        let external = HirExternalOp {
            id: SourceExternalOpId::from_index(0).unwrap(),
            semantic: HirExternalSemantic::UnsafeMinecraftCommand {
                command: "say scale".into(),
                command_origin: OriginId::UNKNOWN,
            },
            origin: OriginId::UNKNOWN,
        };
        let chain = (0..FUNCTIONS)
            .map(|index| {
                let statement = if index + 1 == FUNCTIONS {
                    external_statement(external.id)
                } else {
                    call_statement(SourceFunctionId::from_index(index + 1).unwrap())
                };
                function(index, vec![statement])
            })
            .collect::<Vec<_>>();
        let summaries = infer_function_behaviors(&chain, std::slice::from_ref(&external)).unwrap();
        assert!(summaries[0].contains_unsafe_unknown());
        assert!(summaries[FUNCTIONS - 1].contains_unsafe_unknown());
        assert_eq!(summaries[0].observable_effect(), ObservableEffect::Unknown);
        assert_eq!(
            summaries[FUNCTIONS - 1].observable_effect(),
            ObservableEffect::Unknown
        );
        drop((summaries, chain));

        // Every later leaf contributes a strictly larger fact. A global
        // lowest-ID worklist would re-evaluate the wide root after each leaf
        // and rescan its complete body quadratically. Condensation ordering
        // finalizes all leaves before evaluating the root once.
        let mut wide = std::iter::once(function(0, vec![]))
            .chain((1..FUNCTIONS).map(|index| {
                function(
                    index,
                    vec![run_statement(
                        index - 1,
                        u32::try_from(index).expect("test index fits in u32"),
                    )],
                )
            }))
            .collect::<Vec<_>>();
        wide[0].body.statements = (1..FUNCTIONS)
            .map(|index| call_statement(SourceFunctionId::from_index(index).unwrap()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let summaries = infer_function_behaviors(&wide, &[]).unwrap();
        assert_eq!(summaries[0].transitive_work(), TransitiveWork::Finite);
        assert_eq!(summaries[0].observable_effect(), ObservableEffect::None);
        assert_eq!(
            summaries[0].fork_bound(),
            ForkBound::finite(u64::try_from(FUNCTIONS - 1).unwrap()).unwrap()
        );
        drop((summaries, wide));

        let cycle = (0..FUNCTIONS)
            .map(|index| {
                let callee = SourceFunctionId::from_index((index + 1) % FUNCTIONS).unwrap();
                function(index, vec![call_statement(callee)])
            })
            .collect::<Vec<_>>();
        let summaries = infer_function_behaviors(&cycle, &[]).unwrap();
        assert!(
            summaries
                .iter()
                .all(|summary| summary.transitive_work() == TransitiveWork::NoFiniteUpperBound)
        );
        assert!(
            summaries
                .iter()
                .all(|summary| summary.observable_effect() == ObservableEffect::None)
        );
    }

    fn function(index: usize, statements: Vec<HirStatement>) -> HirFunction {
        HirFunction {
            id: SourceFunctionId::from_index(index).unwrap(),
            module: SourceModuleId::from_index(0).unwrap(),
            visibility: FunctionVisibility::Private,
            visibility_origin: None,
            one_tick: false,
            name_origin: OriginId::UNKNOWN,
            parameter_count: 0,
            result: FunctionResult::Void,
            result_origin: None,
            bindings: Box::new([]),
            body: HirBlock {
                statements: statements.into_boxed_slice(),
                closing_brace_origin: OriginId::UNKNOWN,
                origin: OriginId::UNKNOWN,
            },
            entry_capture: None,
            origin: OriginId::UNKNOWN,
        }
    }

    fn call_statement(callee: SourceFunctionId) -> HirStatement {
        HirStatement {
            kind: HirStatementKind::Call(HirCall {
                callee,
                arguments: Box::new([]),
                origin: OriginId::UNKNOWN,
            }),
            origin: OriginId::UNKNOWN,
        }
    }

    fn external_statement(operation: SourceExternalOpId) -> HirStatement {
        HirStatement {
            kind: HirStatementKind::External(operation),
            origin: OriginId::UNKNOWN,
        }
    }

    fn run_statement(run_index: usize, maximum: u32) -> HirStatement {
        let run = SourceRunId::from_index(run_index).unwrap();
        let maximum = NonZeroU32::new(maximum).unwrap();
        let origin = OriginId::UNKNOWN;
        let modifiers = vec![HirRunModifier::As {
            query: HirEntityQuery {
                semantic: StaticEntityQuery::entities(EntityKind::ArmorStand)
                    .limit(maximum.get())
                    .unwrap(),
                steps: vec![
                    HirEntityQueryStep::Entities {
                        kind: EntityKind::ArmorStand,
                        origin,
                        kind_origin: origin,
                    },
                    HirEntityQueryStep::Limit {
                        maximum,
                        origin,
                        value_origin: origin,
                    },
                ],
            },
            origin,
        }]
        .into_boxed_slice();
        let resulting_context =
            apply_run_modifiers(function_entry_context(), run, &modifiers).unwrap();
        HirStatement {
            kind: HirStatementKind::Run(HirRun {
                id: run,
                modifiers,
                resulting_context,
                capture: None,
                body: HirBlock {
                    statements: Box::new([]),
                    closing_brace_origin: origin,
                    origin,
                },
                origin,
            }),
            origin,
        }
    }
}
