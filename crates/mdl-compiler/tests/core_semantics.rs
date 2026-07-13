use std::collections::HashMap;

use mdl_compiler::ir::core::{
    BlockId, BlockTarget, CoreOp, CoreProgram, CoreType, FunctionBuilder, FunctionId, I32Predicate,
    InstId, Terminator, TerminatorKind, ValueId, verify_program,
};
use mdl_compiler::opt::core::{
    CoreOptimizationLevel, CoreOptimizationOptions, CorePipelineStep, StatisticCount, optimize_core,
};
use mdl_compiler::source::{OriginId, SourceContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TestValue {
    Bool(bool),
    I32(i32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CallEffect {
    caller: FunctionId,
    instruction: InstId,
    callee: FunctionId,
    arguments: Vec<TestValue>,
}

#[derive(Debug, Eq, PartialEq)]
struct Evaluation {
    results: Vec<TestValue>,
    call_effects: Vec<CallEffect>,
    steps: usize,
}

#[derive(Debug, Eq, PartialEq)]
enum EvaluationError {
    StepLimit { completed: usize },
    Malformed,
    ReachedUnreachable,
}

struct PendingCall {
    results: Vec<ValueId>,
}

struct Frame {
    function: FunctionId,
    block: BlockId,
    next_instruction: usize,
    values: HashMap<ValueId, TestValue>,
    pending_call: Option<PendingCall>,
}

fn evaluate(
    program: &CoreProgram,
    function: FunctionId,
    arguments: &[TestValue],
    step_limit: usize,
) -> Result<Evaluation, EvaluationError> {
    let mut frames = vec![new_frame(program, function, arguments)?];
    let mut call_effects = vec![];
    let mut steps = 0_usize;

    loop {
        let frame = frames.last_mut().ok_or(EvaluationError::Malformed)?;
        let body = program
            .function(frame.function)
            .and_then(|declaration| declaration.body())
            .ok_or(EvaluationError::Malformed)?;
        let block = body.block(frame.block).ok_or(EvaluationError::Malformed)?;

        if let Some(instruction) = block.instructions().get(frame.next_instruction).copied() {
            charge_step(&mut steps, step_limit)?;
            let data = body
                .instruction(instruction)
                .ok_or(EvaluationError::Malformed)?;
            let op = data.op().clone();
            let operands = data
                .operands()
                .iter()
                .map(|value| read_value(frame, *value))
                .collect::<Result<Vec<_>, _>>()?;
            let results = data.results().to_vec();
            frame.next_instruction += 1;

            if let CoreOp::Call(callee) = op {
                if frame.pending_call.is_some() {
                    return Err(EvaluationError::Malformed);
                }
                frame.pending_call = Some(PendingCall { results });
                call_effects.push(CallEffect {
                    caller: frame.function,
                    instruction,
                    callee,
                    arguments: operands.clone(),
                });
                frames.push(new_frame(program, callee, &operands)?);
                continue;
            }

            let produced = evaluate_scalar(&op, &operands)?;
            if produced.len() != results.len() {
                return Err(EvaluationError::Malformed);
            }
            for (result, value) in results.into_iter().zip(produced) {
                frame.values.insert(result, value);
            }
            continue;
        }

        charge_step(&mut steps, step_limit)?;
        let terminator = block
            .terminator()
            .ok_or(EvaluationError::Malformed)?
            .kind()
            .clone();
        match terminator {
            TerminatorKind::Jump(target) => enter_target(program, frame, &target)?,
            TerminatorKind::Branch {
                condition,
                then_target,
                else_target,
            } => {
                let TestValue::Bool(condition) = read_value(frame, condition)? else {
                    return Err(EvaluationError::Malformed);
                };
                let target = if condition {
                    &then_target
                } else {
                    &else_target
                };
                enter_target(program, frame, target)?;
            }
            TerminatorKind::Return(values) => {
                let results = values
                    .iter()
                    .map(|value| read_value(frame, *value))
                    .collect::<Result<Vec<_>, _>>()?;
                frames.pop();
                let Some(caller) = frames.last_mut() else {
                    return Ok(Evaluation {
                        results,
                        call_effects,
                        steps,
                    });
                };
                let pending = caller
                    .pending_call
                    .take()
                    .ok_or(EvaluationError::Malformed)?;
                if pending.results.len() != results.len() {
                    return Err(EvaluationError::Malformed);
                }
                for (result, value) in pending.results.into_iter().zip(results) {
                    caller.values.insert(result, value);
                }
            }
            TerminatorKind::Unreachable => return Err(EvaluationError::ReachedUnreachable),
        }
    }
}

fn new_frame(
    program: &CoreProgram,
    function: FunctionId,
    arguments: &[TestValue],
) -> Result<Frame, EvaluationError> {
    let declaration = program
        .function(function)
        .ok_or(EvaluationError::Malformed)?;
    let body = declaration.body().ok_or(EvaluationError::Malformed)?;
    let entry = body.entry();
    let parameters = body
        .block(entry)
        .ok_or(EvaluationError::Malformed)?
        .parameters();
    if parameters.len() != arguments.len() {
        return Err(EvaluationError::Malformed);
    }
    let values = parameters
        .iter()
        .zip(arguments)
        .map(|(parameter, argument)| (parameter.value(), *argument))
        .collect();
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
) -> Result<(), EvaluationError> {
    let body = program
        .function(frame.function)
        .and_then(|declaration| declaration.body())
        .ok_or(EvaluationError::Malformed)?;
    let parameters = body
        .block(target.block())
        .ok_or(EvaluationError::Malformed)?
        .parameters();
    if parameters.len() != target.arguments().len() {
        return Err(EvaluationError::Malformed);
    }
    let arguments = target
        .arguments()
        .iter()
        .map(|value| read_value(frame, *value))
        .collect::<Result<Vec<_>, _>>()?;
    for (parameter, argument) in parameters.iter().zip(arguments) {
        frame.values.insert(parameter.value(), argument);
    }
    frame.block = target.block();
    frame.next_instruction = 0;
    Ok(())
}

fn read_value(frame: &Frame, value: ValueId) -> Result<TestValue, EvaluationError> {
    frame
        .values
        .get(&value)
        .copied()
        .ok_or(EvaluationError::Malformed)
}

fn charge_step(completed: &mut usize, limit: usize) -> Result<(), EvaluationError> {
    if *completed == limit {
        return Err(EvaluationError::StepLimit {
            completed: *completed,
        });
    }
    *completed += 1;
    Ok(())
}

fn evaluate_scalar(op: &CoreOp, operands: &[TestValue]) -> Result<Vec<TestValue>, EvaluationError> {
    let malformed = || EvaluationError::Malformed;
    match (op, operands) {
        (CoreOp::BoolConstant(value), []) => Ok(vec![TestValue::Bool(*value)]),
        (CoreOp::I32Constant(value), []) => Ok(vec![TestValue::I32(*value)]),
        (CoreOp::I32AddWrapping, [TestValue::I32(left), TestValue::I32(right)]) => {
            Ok(vec![TestValue::I32(left.wrapping_add(*right))])
        }
        (CoreOp::I32AddOverflowing, [TestValue::I32(left), TestValue::I32(right)]) => {
            let (sum, overflowed) = left.overflowing_add(*right);
            Ok(vec![TestValue::I32(sum), TestValue::Bool(overflowed)])
        }
        (CoreOp::I32Compare(predicate), [TestValue::I32(left), TestValue::I32(right)]) => {
            Ok(vec![TestValue::Bool(match predicate {
                I32Predicate::Eq => left == right,
                I32Predicate::Ne => left != right,
                I32Predicate::SignedLt => left < right,
                I32Predicate::SignedLe => left <= right,
                I32Predicate::SignedGt => left > right,
                I32Predicate::SignedGe => left >= right,
            })])
        }
        (CoreOp::BoolNot, [TestValue::Bool(value)]) => Ok(vec![TestValue::Bool(!value)]),
        _ => Err(malformed()),
    }
}

fn parameter(builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
    builder.body().block(block).unwrap().parameters()[index].value()
}

fn observable_call_program(
    sources: &SourceContext,
) -> (CoreProgram, FunctionId, FunctionId, InstId) {
    let mut program = CoreProgram::new();
    let increment_function = program
        .declare_function(
            Some("increment"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let choose_function = program
        .declare_function(
            Some("choose"),
            vec![CoreType::I32, CoreType::Bool],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();

    let mut builder = FunctionBuilder::new(&program, sources, increment_function).unwrap();
    let entry = builder.entry_block();
    let input = parameter(&builder, entry, 0);
    let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
    let result = builder
        .i32_add_wrapping(input, one, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(increment_function, builder.finish().unwrap())
        .unwrap();

    let mut builder = FunctionBuilder::new(&program, sources, choose_function).unwrap();
    let entry = builder.entry_block();
    let input = parameter(&builder, entry, 0);
    let condition = parameter(&builder, entry, 1);
    let call_block = builder.create_block(OriginId::UNKNOWN).unwrap();
    let passthrough_block = builder.create_block(OriginId::UNKNOWN).unwrap();
    let join = builder.create_block(OriginId::UNKNOWN).unwrap();
    let joined = builder
        .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(call_block, vec![]),
                else_target: BlockTarget::new(passthrough_block, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(call_block).unwrap();
    let call_result = builder
        .call(increment_function, vec![input], OriginId::UNKNOWN)
        .unwrap()[0];
    let call_instruction = match builder.body().value(call_result).unwrap().definition() {
        mdl_compiler::ir::core::ValueDef::InstResult { instruction, .. } => instruction,
        mdl_compiler::ir::core::ValueDef::BlockParam { .. } => unreachable!(),
    };
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![call_result])),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(passthrough_block).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![input])),
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
    program
        .define_function(choose_function, builder.finish().unwrap())
        .unwrap();
    (
        program,
        increment_function,
        choose_function,
        call_instruction,
    )
}

#[test]
fn evaluator_observes_results_and_ordered_call_effects() {
    let sources = SourceContext::new();
    let (program, increment_function, choose_function, call_instruction) =
        observable_call_program(&sources);
    verify_program(&program, &sources).unwrap();

    let taken_evaluation = evaluate(
        &program,
        choose_function,
        &[TestValue::I32(41), TestValue::Bool(true)],
        100,
    )
    .unwrap();
    assert_eq!(taken_evaluation.results, vec![TestValue::I32(42)]);
    assert_eq!(
        taken_evaluation.call_effects,
        vec![CallEffect {
            caller: choose_function,
            instruction: call_instruction,
            callee: increment_function,
            arguments: vec![TestValue::I32(41)],
        }]
    );

    let bypassed = evaluate(
        &program,
        choose_function,
        &[TestValue::I32(41), TestValue::Bool(false)],
        100,
    )
    .unwrap();
    assert_eq!(bypassed.results, vec![TestValue::I32(41)]);
    assert!(bypassed.call_effects.is_empty());
}

#[test]
fn evaluator_handles_overflow_and_stops_loops_at_the_exact_step_limit() {
    let sources = SourceContext::new();
    let mut program = CoreProgram::new();
    let arithmetic = program
        .declare_function(
            Some("overflow"),
            vec![],
            vec![CoreType::I32, CoreType::Bool],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let looping = program
        .declare_function(Some("loop"), vec![], vec![], OriginId::UNKNOWN)
        .unwrap();

    let mut builder = FunctionBuilder::new(&program, &sources, arithmetic).unwrap();
    let maximum = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
    let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
    let (sum, overflowed) = builder
        .i32_add_overflowing(maximum, one, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![sum, overflowed]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(arithmetic, builder.finish().unwrap())
        .unwrap();

    let mut builder = FunctionBuilder::new(&program, &sources, looping).unwrap();
    let cycle = builder.create_block(OriginId::UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(cycle, vec![])),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    builder.switch_to_block(cycle).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(cycle, vec![])),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(looping, builder.finish().unwrap())
        .unwrap();
    verify_program(&program, &sources).unwrap();

    assert_eq!(
        evaluate(&program, arithmetic, &[], 10).unwrap().results,
        vec![TestValue::I32(i32::MIN), TestValue::Bool(true)]
    );
    assert_eq!(
        evaluate(&program, looping, &[], 7),
        Err(EvaluationError::StepLimit { completed: 7 })
    );
}

fn optimize_for_semantic_comparison(
    program: CoreProgram,
    sources: &SourceContext,
    level: CoreOptimizationLevel,
) -> CoreProgram {
    optimize_core(program, sources, &CoreOptimizationOptions::new(level))
        .unwrap()
        .into_parts()
        .0
}

fn assert_same_observation(
    reference: &CoreProgram,
    baseline: &CoreProgram,
    function: FunctionId,
    arguments: &[TestValue],
    step_limit: usize,
    context: &str,
) {
    let reference = evaluate(reference, function, arguments, step_limit)
        .unwrap_or_else(|error| panic!("reference evaluator failed: {context}: {error:?}"));
    let baseline = evaluate(baseline, function, arguments, step_limit)
        .unwrap_or_else(|error| panic!("baseline evaluator failed: {context}: {error:?}"));
    assert_eq!(
        baseline.results, reference.results,
        "result mismatch: {context}"
    );
    assert_eq!(
        baseline.call_effects, reference.call_effects,
        "ordered call-effect mismatch: {context}"
    );
}

fn define_increment_function(
    program: &mut CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
) {
    let mut builder = FunctionBuilder::new(program, sources, function).unwrap();
    let entry = builder.entry_block();
    let input = parameter(&builder, entry, 0);
    let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
    let incremented = builder
        .i32_add_wrapping(input, one, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![incremented]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(function, builder.finish().unwrap())
        .unwrap();
}

#[derive(Clone, Copy)]
struct NestedCallSites {
    leaf_function: FunctionId,
    middle_function: FunctionId,
    root_to_middle: InstId,
    middle_first_leaf: InstId,
    middle_second_leaf: InstId,
    root_after_leaf: InstId,
    root_bypass_leaf: InstId,
}

fn instruction_defining(builder: &FunctionBuilder<'_>, value: ValueId) -> InstId {
    match builder.body().value(value).unwrap().definition() {
        mdl_compiler::ir::core::ValueDef::InstResult { instruction, .. } => instruction,
        mdl_compiler::ir::core::ValueDef::BlockParam { .. } => {
            panic!("expected an instruction result")
        }
    }
}

fn define_nested_middle(
    program: &mut CoreProgram,
    sources: &SourceContext,
    middle_function: FunctionId,
    leaf_function: FunctionId,
) -> (InstId, InstId) {
    let mut builder = FunctionBuilder::new(program, sources, middle_function).unwrap();
    let middle_entry = builder.entry_block();
    let middle_input = parameter(&builder, middle_entry, 0);
    let first_leaf_result = builder
        .call(leaf_function, vec![middle_input], OriginId::UNKNOWN)
        .unwrap()[0];
    let middle_first_leaf = instruction_defining(&builder, first_leaf_result);
    let second_leaf_result = builder
        .call(leaf_function, vec![first_leaf_result], OriginId::UNKNOWN)
        .unwrap()[0];
    let middle_second_leaf = instruction_defining(&builder, second_leaf_result);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![second_leaf_result]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(middle_function, builder.finish().unwrap())
        .unwrap();
    (middle_first_leaf, middle_second_leaf)
}

fn define_conditional_call_root(
    program: &mut CoreProgram,
    sources: &SourceContext,
    root_function: FunctionId,
    middle_function: FunctionId,
    leaf_function: FunctionId,
) -> (InstId, InstId, InstId) {
    let mut builder = FunctionBuilder::new(program, sources, root_function).unwrap();
    let root_entry = builder.entry_block();
    let root_input = parameter(&builder, root_entry, 0);
    let condition = parameter(&builder, root_entry, 1);
    let nested_path = builder.create_block(OriginId::UNKNOWN).unwrap();
    let bypass_path = builder.create_block(OriginId::UNKNOWN).unwrap();
    let join = builder.create_block(OriginId::UNKNOWN).unwrap();
    let joined = builder
        .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(nested_path, vec![]),
                else_target: BlockTarget::new(bypass_path, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();

    builder.switch_to_block(nested_path).unwrap();
    let middle_result = builder
        .call(middle_function, vec![root_input], OriginId::UNKNOWN)
        .unwrap()[0];
    let root_to_middle = instruction_defining(&builder, middle_result);
    let final_leaf_result = builder
        .call(leaf_function, vec![middle_result], OriginId::UNKNOWN)
        .unwrap()[0];
    let root_after_leaf = instruction_defining(&builder, final_leaf_result);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![final_leaf_result])),
            OriginId::UNKNOWN,
        ))
        .unwrap();

    builder.switch_to_block(bypass_path).unwrap();
    let bypass_result = builder
        .call(leaf_function, vec![root_input], OriginId::UNKNOWN)
        .unwrap()[0];
    let root_bypass_leaf = instruction_defining(&builder, bypass_result);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![bypass_result])),
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
    program
        .define_function(root_function, builder.finish().unwrap())
        .unwrap();
    (root_to_middle, root_after_leaf, root_bypass_leaf)
}

fn conditional_nested_call_program(
    sources: &SourceContext,
) -> (CoreProgram, FunctionId, NestedCallSites) {
    let mut program = CoreProgram::new();
    let leaf_function = program
        .declare_function(
            Some("leaf_increment"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let middle_function = program
        .declare_function(
            Some("middle_increment_twice"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let root_function = program
        .declare_function(
            Some("conditional_nested_calls"),
            vec![CoreType::I32, CoreType::Bool],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    define_increment_function(&mut program, sources, leaf_function);
    let (middle_first_leaf, middle_second_leaf) =
        define_nested_middle(&mut program, sources, middle_function, leaf_function);
    let (root_to_middle, root_after_leaf, root_bypass_leaf) = define_conditional_call_root(
        &mut program,
        sources,
        root_function,
        middle_function,
        leaf_function,
    );

    (
        program,
        root_function,
        NestedCallSites {
            leaf_function,
            middle_function,
            root_to_middle,
            middle_first_leaf,
            middle_second_leaf,
            root_after_leaf,
            root_bypass_leaf,
        },
    )
}

#[test]
fn baseline_preserves_conditional_and_ordered_nested_call_effects() {
    let sources = SourceContext::new();
    let (program, root_function, sites) = conditional_nested_call_program(&sources);
    verify_program(&program, &sources).unwrap();
    let reference =
        optimize_for_semantic_comparison(program.clone(), &sources, CoreOptimizationLevel::None);
    let baseline =
        optimize_for_semantic_comparison(program, &sources, CoreOptimizationLevel::Baseline);

    let bypassed = evaluate(
        &reference,
        root_function,
        &[TestValue::I32(40), TestValue::Bool(false)],
        1_000,
    )
    .unwrap();
    assert_eq!(bypassed.results, vec![TestValue::I32(41)]);
    assert_eq!(
        bypassed.call_effects,
        vec![CallEffect {
            caller: root_function,
            instruction: sites.root_bypass_leaf,
            callee: sites.leaf_function,
            arguments: vec![TestValue::I32(40)],
        }]
    );

    let nested = evaluate(
        &reference,
        root_function,
        &[TestValue::I32(40), TestValue::Bool(true)],
        1_000,
    )
    .unwrap();
    assert_eq!(nested.results, vec![TestValue::I32(43)]);
    assert_eq!(
        nested.call_effects,
        vec![
            CallEffect {
                caller: root_function,
                instruction: sites.root_to_middle,
                callee: sites.middle_function,
                arguments: vec![TestValue::I32(40)],
            },
            CallEffect {
                caller: sites.middle_function,
                instruction: sites.middle_first_leaf,
                callee: sites.leaf_function,
                arguments: vec![TestValue::I32(40)],
            },
            CallEffect {
                caller: sites.middle_function,
                instruction: sites.middle_second_leaf,
                callee: sites.leaf_function,
                arguments: vec![TestValue::I32(41)],
            },
            CallEffect {
                caller: root_function,
                instruction: sites.root_after_leaf,
                callee: sites.leaf_function,
                arguments: vec![TestValue::I32(42)],
            },
        ]
    );

    for condition in [false, true] {
        assert_same_observation(
            &reference,
            &baseline,
            root_function,
            &[TestValue::I32(40), TestValue::Bool(condition)],
            1_000,
            &format!("conditional nested calls condition={condition}"),
        );
    }
}

fn define_complete_pipeline_function(
    program: &mut CoreProgram,
    sources: &SourceContext,
    pipeline_function: FunctionId,
    increment_function: FunctionId,
) {
    let mut builder = FunctionBuilder::new(program, sources, pipeline_function).unwrap();
    let entry = builder.entry_block();
    let input = parameter(&builder, entry, 0);
    let call_result = builder
        .call(increment_function, vec![input], OriginId::UNKNOWN)
        .unwrap()[0];

    // Initial canonicalization removes this identity but cannot erase/reorder the
    // preceding call. SCCP proves the overflowing result and branch condition.
    let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
    let normalized = builder
        .i32_add_wrapping(call_result, zero, OriginId::UNKNOWN)
        .unwrap();
    let maximum = builder.i32_constant(i32::MAX, OriginId::UNKNOWN).unwrap();
    let one = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
    let (_wrapped, overflowed) = builder
        .i32_add_overflowing(maximum, one, OriginId::UNKNOWN)
        .unwrap();
    let selected = builder.create_block(OriginId::UNKNOWN).unwrap();
    let rejected = builder.create_block(OriginId::UNKNOWN).unwrap();
    let tail = builder.create_block(OriginId::UNKNOWN).unwrap();
    let tail_value = builder
        .append_block_parameter(tail, CoreType::I32, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: overflowed,
                then_target: BlockTarget::new(selected, vec![]),
                else_target: BlockTarget::new(rejected, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .unwrap();

    builder.switch_to_block(selected).unwrap();
    let dead = builder.i32_constant(99, OriginId::UNKNOWN).unwrap();
    let _dead_chain = builder
        .i32_add_wrapping(dead, dead, OriginId::UNKNOWN)
        .unwrap();
    let first = builder
        .i32_add_wrapping(normalized, one, OriginId::UNKNOWN)
        .unwrap();
    let duplicate = builder
        .i32_add_wrapping(normalized, one, OriginId::UNKNOWN)
        .unwrap();
    let combined = builder
        .i32_add_wrapping(first, duplicate, OriginId::UNKNOWN)
        .unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(tail, vec![combined])),
            OriginId::UNKNOWN,
        ))
        .unwrap();

    builder.switch_to_block(rejected).unwrap();
    let impossible = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(tail, vec![impossible])),
            OriginId::UNKNOWN,
        ))
        .unwrap();

    builder.switch_to_block(tail).unwrap();
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![tail_value]),
            OriginId::UNKNOWN,
        ))
        .unwrap();
    program
        .define_function(pipeline_function, builder.finish().unwrap())
        .unwrap();
}

fn complete_pipeline_program(sources: &SourceContext) -> (CoreProgram, FunctionId) {
    let mut program = CoreProgram::new();
    let increment_function = program
        .declare_function(
            Some("increment"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    let pipeline_function = program
        .declare_function(
            Some("pipeline"),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .unwrap();
    define_increment_function(&mut program, sources, increment_function);
    define_complete_pipeline_function(&mut program, sources, pipeline_function, increment_function);
    (program, pipeline_function)
}

#[test]
fn baseline_preserves_results_and_ordered_calls_across_the_complete_pipeline() {
    let sources = SourceContext::new();
    let (program, pipeline_function) = complete_pipeline_program(&sources);
    verify_program(&program, &sources).unwrap();

    let reference =
        optimize_for_semantic_comparison(program.clone(), &sources, CoreOptimizationLevel::None);
    let baseline_output = optimize_core(
        program,
        &sources,
        &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
    )
    .unwrap();
    for step in [
        CorePipelineStep::CanonicalizeInitial,
        CorePipelineStep::Sccp,
        CorePipelineStep::DceAfterSccp,
        CorePipelineStep::Fusion,
        CorePipelineStep::Cse,
    ] {
        let summary = baseline_output
            .report()
            .steps()
            .iter()
            .find(|summary| summary.step() == step)
            .unwrap();
        assert_eq!(
            summary.functions_changed(),
            StatisticCount::Exact(1),
            "hand fixture must exercise {step}"
        );
    }
    let baseline = baseline_output.into_parts().0;
    for input in [i32::MIN, -1, 0, 41, i32::MAX] {
        assert_same_observation(
            &reference,
            &baseline,
            pipeline_function,
            &[TestValue::I32(input)],
            1_000,
            &format!("complete-pipeline input={input}"),
        );
    }
}

const GENERATED_CFG_VERSION: u32 = 2;
const GENERATED_SEEDS: [u64; 8] = [0, 1, 2, 3, 5, 8, 13, 21];
const SCALAR_BOUNDARY_INPUT_DOMAIN: &str = "i32=[i32::MIN,-1,0,1,i32::MAX]";
const BOOLEAN_DIAMOND_INPUT_DOMAIN: &str = "i32=[i32::MIN,-1,0,1,i32::MAX];bool=[false,true]";
const NO_ARGUMENT_INPUT_DOMAIN: &str = "arguments=[]";
const TERMINATING_DIAMOND_SHAPE: &str = "terminating-diamond";
const STRAIGHT_LINE_SHAPE: &str = "straight-line";
const ASYMMETRIC_DIAMOND_SHAPE: &str = "asymmetric-diamond";
const TERMINATING_LOOP_SHAPE: &str = "terminating-loop";
const MULTI_RESULT_CALL_SHAPE: &str = "multi-result-call";

#[derive(Clone, Copy)]
struct GeneratedFixtureContext {
    seed: u64,
    shape: &'static str,
    replay_input_domain: &'static str,
}

impl GeneratedFixtureContext {
    const fn new(seed: u64, shape: &'static str, replay_input_domain: &'static str) -> Self {
        Self {
            seed,
            shape,
            replay_input_domain,
        }
    }

    #[track_caller]
    fn fail(self, detail: impl std::fmt::Display) -> ! {
        panic!(
            "generated Core fixture construction failed: version={GENERATED_CFG_VERSION} \
             seed={} shape={} replay-input-domain={}: {detail}",
            self.seed, self.shape, self.replay_input_domain
        )
    }

    #[track_caller]
    fn fail_case(self, detail: impl std::fmt::Display) -> ! {
        panic!(
            "generated Core case failed: version={GENERATED_CFG_VERSION} seed={} shape={} \
             replay-input-domain={}: {detail}",
            self.seed, self.shape, self.replay_input_domain
        )
    }

    #[track_caller]
    fn parameter(self, builder: &FunctionBuilder<'_>, block: BlockId, index: usize) -> ValueId {
        let Some(parameter) = builder
            .body()
            .block(block)
            .and_then(|data| data.parameters().get(index))
        else {
            self.fail(format_args!(
                "missing parameter {index} from generated block {block:?}"
            ));
        };
        parameter.value()
    }

    #[track_caller]
    fn two_results(self, results: &[ValueId]) -> (ValueId, ValueId) {
        match results {
            [first, second] => (*first, *second),
            _ => self.fail(format_args!(
                "expected two generated call results, found {}",
                results.len()
            )),
        }
    }
}

#[track_caller]
fn run_generated_case<T>(context: GeneratedFixtureContext, case: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(case)).unwrap_or_else(|payload| {
        if let Some(message) = payload.downcast_ref::<String>() {
            context.fail_case(message);
        }
        if let Some(message) = payload.downcast_ref::<&str>() {
            context.fail_case(message);
        }
        context.fail_case("non-string panic payload");
    })
}

trait GeneratedConstructionResult<T> {
    fn generated(self, context: GeneratedFixtureContext) -> T;
}

impl<T, E: std::fmt::Display> GeneratedConstructionResult<T> for Result<T, E> {
    #[track_caller]
    fn generated(self, context: GeneratedFixtureContext) -> T {
        self.unwrap_or_else(|error| context.fail(error))
    }
}

fn generated_i32(seed: u64, lane: u64) -> i32 {
    let mut state = seed ^ lane.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    state ^= state >> 29;
    (state >> 32) as i32
}

fn generated_nonzero_i32(seed: u64, lane: u64) -> i32 {
    match generated_i32(seed, lane) {
        0 => 1,
        value => value,
    }
}

fn generated_terminating_diamond(seed: u64, sources: &SourceContext) -> (CoreProgram, FunctionId) {
    let context = GeneratedFixtureContext::new(
        seed,
        TERMINATING_DIAMOND_SHAPE,
        BOOLEAN_DIAMOND_INPUT_DOMAIN,
    );
    let then_constant = generated_i32(seed, 0);
    let else_constant = generated_i32(seed, 1);

    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some(format!("generated_v{GENERATED_CFG_VERSION}_{seed}")),
            vec![CoreType::I32, CoreType::Bool],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .generated(context);
    let mut builder = FunctionBuilder::new(&program, sources, function).generated(context);
    let entry = builder.entry_block();
    let input = context.parameter(&builder, entry, 0);
    let condition = context.parameter(&builder, entry, 1);
    let then_block = builder.create_block(OriginId::UNKNOWN).generated(context);
    let else_block = builder.create_block(OriginId::UNKNOWN).generated(context);
    let join = builder.create_block(OriginId::UNKNOWN).generated(context);
    let joined = builder
        .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(then_block, vec![]),
                else_target: BlockTarget::new(else_block, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(then_block).generated(context);
    let then_value = builder
        .i32_constant(then_constant, OriginId::UNKNOWN)
        .generated(context);
    let then_result = builder
        .i32_add_wrapping(input, then_value, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![then_result])),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(else_block).generated(context);
    let else_value = builder
        .i32_constant(else_constant, OriginId::UNKNOWN)
        .generated(context);
    let else_result = builder
        .i32_add_wrapping(input, else_value, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![else_result])),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(join).generated(context);
    let zero = builder
        .i32_constant(0, OriginId::UNKNOWN)
        .generated(context);
    let normalized = builder
        .i32_add_wrapping(joined, zero, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![normalized]),
            OriginId::UNKNOWN,
        ))
        .generated(context);
    let body = builder.finish().generated(context);
    program.define_function(function, body).generated(context);
    (program, function)
}

fn generated_straight_line(seed: u64, sources: &SourceContext) -> (CoreProgram, FunctionId) {
    let context =
        GeneratedFixtureContext::new(seed, STRAIGHT_LINE_SHAPE, SCALAR_BOUNDARY_INPUT_DOMAIN);
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some(format!(
                "generated_v{GENERATED_CFG_VERSION}_straight_{seed}"
            )),
            vec![CoreType::I32],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .generated(context);
    let mut builder = FunctionBuilder::new(&program, sources, function).generated(context);
    let entry = builder.entry_block();
    let input = context.parameter(&builder, entry, 0);
    let literal = builder
        .i32_constant(generated_nonzero_i32(seed, 2), OriginId::UNKNOWN)
        .generated(context);
    let representative = builder
        .i32_add_wrapping(input, literal, OriginId::UNKNOWN)
        .generated(context);
    let duplicate = builder
        .i32_add_wrapping(literal, input, OriginId::UNKNOWN)
        .generated(context);
    let _dead = builder
        .i32_add_wrapping(representative, input, OriginId::UNKNOWN)
        .generated(context);
    let result = builder
        .i32_add_wrapping(representative, duplicate, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![result]),
            OriginId::UNKNOWN,
        ))
        .generated(context);
    let body = builder.finish().generated(context);
    program.define_function(function, body).generated(context);
    (program, function)
}

fn generated_asymmetric_diamond(seed: u64, sources: &SourceContext) -> (CoreProgram, FunctionId) {
    let context =
        GeneratedFixtureContext::new(seed, ASYMMETRIC_DIAMOND_SHAPE, BOOLEAN_DIAMOND_INPUT_DOMAIN);
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some(format!(
                "generated_v{GENERATED_CFG_VERSION}_asymmetric_diamond_{seed}"
            )),
            vec![CoreType::I32, CoreType::Bool],
            vec![CoreType::I32, CoreType::Bool],
            OriginId::UNKNOWN,
        )
        .generated(context);
    let mut builder = FunctionBuilder::new(&program, sources, function).generated(context);
    let entry = builder.entry_block();
    let input = context.parameter(&builder, entry, 0);
    let condition = context.parameter(&builder, entry, 1);
    let direct_arm = builder.create_block(OriginId::UNKNOWN).generated(context);
    let extended_arm = builder.create_block(OriginId::UNKNOWN).generated(context);
    let arm_bridge = builder.create_block(OriginId::UNKNOWN).generated(context);
    let bridge_value = builder
        .append_block_parameter(arm_bridge, CoreType::I32, OriginId::UNKNOWN)
        .generated(context);
    let bridge_flag = builder
        .append_block_parameter(arm_bridge, CoreType::Bool, OriginId::UNKNOWN)
        .generated(context);
    let join = builder.create_block(OriginId::UNKNOWN).generated(context);
    let joined_value = builder
        .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
        .generated(context);
    let joined_flag = builder
        .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition,
                then_target: BlockTarget::new(direct_arm, vec![]),
                else_target: BlockTarget::new(extended_arm, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(direct_arm).generated(context);
    let direct_literal = builder
        .i32_constant(generated_nonzero_i32(seed, 3), OriginId::UNKNOWN)
        .generated(context);
    let (direct_value, direct_flag) = builder
        .i32_add_overflowing(input, direct_literal, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![direct_value, direct_flag])),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(extended_arm).generated(context);
    let extended_literal = builder
        .i32_constant(generated_nonzero_i32(seed, 4), OriginId::UNKNOWN)
        .generated(context);
    let extended_value = builder
        .i32_add_wrapping(input, extended_literal, OriginId::UNKNOWN)
        .generated(context);
    let extended_flag = builder
        .bool_constant(false, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                arm_bridge,
                vec![extended_value, extended_flag],
            )),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(arm_bridge).generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, vec![bridge_value, bridge_flag])),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(join).generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![joined_value, joined_flag]),
            OriginId::UNKNOWN,
        ))
        .generated(context);
    let body = builder.finish().generated(context);
    program.define_function(function, body).generated(context);
    (program, function)
}

fn generated_terminating_loop(seed: u64, sources: &SourceContext) -> (CoreProgram, FunctionId) {
    let context =
        GeneratedFixtureContext::new(seed, TERMINATING_LOOP_SHAPE, NO_ARGUMENT_INPUT_DOMAIN);
    let mut program = CoreProgram::new();
    let function = program
        .declare_function(
            Some(format!(
                "generated_v{GENERATED_CFG_VERSION}_terminating_loop_{seed}"
            )),
            vec![],
            vec![CoreType::I32],
            OriginId::UNKNOWN,
        )
        .generated(context);
    let mut builder = FunctionBuilder::new(&program, sources, function).generated(context);
    let loop_block = builder.create_block(OriginId::UNKNOWN).generated(context);
    let counter = builder
        .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
        .generated(context);
    let accumulator = builder
        .append_block_parameter(loop_block, CoreType::I32, OriginId::UNKNOWN)
        .generated(context);
    let body_block = builder.create_block(OriginId::UNKNOWN).generated(context);
    let exit_block = builder.create_block(OriginId::UNKNOWN).generated(context);
    let final_value = builder
        .append_block_parameter(exit_block, CoreType::I32, OriginId::UNKNOWN)
        .generated(context);

    let zero = builder
        .i32_constant(0, OriginId::UNKNOWN)
        .generated(context);
    let start = builder
        .i32_constant(generated_i32(seed, 5), OriginId::UNKNOWN)
        .generated(context);
    let iterations = i32::try_from(seed % 7 + 1).generated(context);
    let bound = builder
        .i32_constant(iterations, OriginId::UNKNOWN)
        .generated(context);
    let one = builder
        .i32_constant(1, OriginId::UNKNOWN)
        .generated(context);
    let delta = builder
        .i32_constant(generated_nonzero_i32(seed, 6), OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(loop_block, vec![zero, start])),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(loop_block).generated(context);
    let keep_running = builder
        .i32_compare(I32Predicate::SignedLt, counter, bound, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: keep_running,
                then_target: BlockTarget::new(body_block, vec![]),
                else_target: BlockTarget::new(exit_block, vec![accumulator]),
            },
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(body_block).generated(context);
    let next_counter = builder
        .i32_add_wrapping(counter, one, OriginId::UNKNOWN)
        .generated(context);
    let next_accumulator = builder
        .i32_add_wrapping(accumulator, delta, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(
                loop_block,
                vec![next_counter, next_accumulator],
            )),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(exit_block).generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![final_value]),
            OriginId::UNKNOWN,
        ))
        .generated(context);
    let body = builder.finish().generated(context);
    program.define_function(function, body).generated(context);
    (program, function)
}

fn generated_multi_result_call(seed: u64, sources: &SourceContext) -> (CoreProgram, FunctionId) {
    let context =
        GeneratedFixtureContext::new(seed, MULTI_RESULT_CALL_SHAPE, SCALAR_BOUNDARY_INPUT_DOMAIN);
    let mut program = CoreProgram::new();
    let helper = program
        .declare_function(
            Some(format!(
                "generated_v{GENERATED_CFG_VERSION}_checked_add_{seed}"
            )),
            vec![CoreType::I32, CoreType::I32],
            vec![CoreType::I32, CoreType::Bool],
            OriginId::UNKNOWN,
        )
        .generated(context);
    let root = program
        .declare_function(
            Some(format!(
                "generated_v{GENERATED_CFG_VERSION}_multi_call_{seed}"
            )),
            vec![CoreType::I32],
            vec![CoreType::I32, CoreType::Bool],
            OriginId::UNKNOWN,
        )
        .generated(context);

    let mut builder = FunctionBuilder::new(&program, sources, helper).generated(context);
    let helper_entry = builder.entry_block();
    let left = context.parameter(&builder, helper_entry, 0);
    let right = context.parameter(&builder, helper_entry, 1);
    let (sum, overflowed) = builder
        .i32_add_overflowing(left, right, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![sum, overflowed]),
            OriginId::UNKNOWN,
        ))
        .generated(context);
    let body = builder.finish().generated(context);
    program.define_function(helper, body).generated(context);

    let mut builder = FunctionBuilder::new(&program, sources, root).generated(context);
    let root_entry = builder.entry_block();
    let input = context.parameter(&builder, root_entry, 0);
    let offset = builder
        .i32_constant(generated_nonzero_i32(seed, 7), OriginId::UNKNOWN)
        .generated(context);
    let first = builder
        .call(helper, vec![input, offset], OriginId::UNKNOWN)
        .generated(context);
    let (first_sum, first_overflowed) = context.two_results(&first);
    let overflow_arm = builder.create_block(OriginId::UNKNOWN).generated(context);
    let ordinary_arm = builder.create_block(OriginId::UNKNOWN).generated(context);
    let join = builder.create_block(OriginId::UNKNOWN).generated(context);
    let joined_sum = builder
        .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
        .generated(context);
    let joined_overflow = builder
        .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Branch {
                condition: first_overflowed,
                then_target: BlockTarget::new(overflow_arm, vec![]),
                else_target: BlockTarget::new(ordinary_arm, vec![]),
            },
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(overflow_arm).generated(context);
    let overflow_results = builder
        .call(helper, vec![first_sum, input], OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, overflow_results)),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(ordinary_arm).generated(context);
    let ordinary_results = builder
        .call(helper, vec![first_sum, offset], OriginId::UNKNOWN)
        .generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Jump(BlockTarget::new(join, ordinary_results)),
            OriginId::UNKNOWN,
        ))
        .generated(context);

    builder.switch_to_block(join).generated(context);
    builder
        .terminate(Terminator::new(
            TerminatorKind::Return(vec![joined_sum, joined_overflow]),
            OriginId::UNKNOWN,
        ))
        .generated(context);
    let body = builder.finish().generated(context);
    program.define_function(root, body).generated(context);
    (program, root)
}

fn scalar_boundary_arguments() -> Vec<Vec<TestValue>> {
    [i32::MIN, -1, 0, 1, i32::MAX]
        .into_iter()
        .map(|value| vec![TestValue::I32(value)])
        .collect()
}

fn asymmetric_diamond_arguments() -> Vec<Vec<TestValue>> {
    let mut arguments = vec![];
    for condition in [false, true] {
        for input in [i32::MIN, -1, 0, 1, i32::MAX] {
            arguments.push(vec![TestValue::I32(input), TestValue::Bool(condition)]);
        }
    }
    arguments
}

struct GeneratedDifferentialCase {
    context: GeneratedFixtureContext,
    program: CoreProgram,
    function: FunctionId,
    arguments: Vec<Vec<TestValue>>,
    expected_call_effects: usize,
    expected_distinct_call_sites: usize,
}

fn generated_case(
    context: GeneratedFixtureContext,
    sources: &SourceContext,
    build: fn(u64, &SourceContext) -> (CoreProgram, FunctionId),
    arguments: Vec<Vec<TestValue>>,
    expected_call_effects: usize,
    expected_distinct_call_sites: usize,
) -> GeneratedDifferentialCase {
    let (program, function) = run_generated_case(context, || build(context.seed, sources));
    GeneratedDifferentialCase {
        context,
        program,
        function,
        arguments,
        expected_call_effects,
        expected_distinct_call_sites,
    }
}

fn assert_generated_differential(case: GeneratedDifferentialCase, sources: &SourceContext) {
    let GeneratedDifferentialCase {
        context: fixture_context,
        program,
        function,
        arguments,
        expected_call_effects,
        expected_distinct_call_sites,
    } = case;
    let seed = fixture_context.seed;
    let shape = fixture_context.shape;
    run_generated_case(fixture_context, || {
        verify_program(&program, sources).unwrap_or_else(|error| {
            panic!(
                "generated Core invalid: version={GENERATED_CFG_VERSION} shape={shape} seed={seed}: {error}"
            )
        });
        let reference =
            optimize_for_semantic_comparison(program.clone(), sources, CoreOptimizationLevel::None);
        let baseline =
            optimize_for_semantic_comparison(program, sources, CoreOptimizationLevel::Baseline);
        let mut observed_call_sites = vec![];
        for input in &arguments {
            let context = format!(
                "generated version={GENERATED_CFG_VERSION} shape={shape} seed={seed} input={input:?}"
            );
            let reference_evaluation = evaluate(&reference, function, input, 10_000)
                .unwrap_or_else(|error| panic!("{context}: reference evaluator failed: {error:?}"));
            assert_eq!(
                reference_evaluation.call_effects.len(),
                expected_call_effects,
                "fixture did not exercise its intended call surface: {context}"
            );
            observed_call_sites.extend(
                reference_evaluation
                    .call_effects
                    .iter()
                    .map(|effect| effect.instruction),
            );
            assert_same_observation(&reference, &baseline, function, input, 10_000, &context);
        }
        observed_call_sites.sort_unstable();
        observed_call_sites.dedup();
        assert_eq!(
            observed_call_sites.len(),
            expected_distinct_call_sites,
            "generated call-site coverage mismatch: version={GENERATED_CFG_VERSION} shape={shape} seed={seed}"
        );
    });
}

#[test]
fn generated_terminating_cfgs_match_none_results_and_effect_order() {
    let sources = SourceContext::new();
    for seed in 0_u64..64 {
        let fixture_context = GeneratedFixtureContext::new(
            seed,
            TERMINATING_DIAMOND_SHAPE,
            BOOLEAN_DIAMOND_INPUT_DOMAIN,
        );
        run_generated_case(fixture_context, || {
            let (program, function) = generated_terminating_diamond(seed, &sources);
            verify_program(&program, &sources).unwrap_or_else(|error| {
                panic!(
                    "generated Core invalid: version={GENERATED_CFG_VERSION} \
                     shape={TERMINATING_DIAMOND_SHAPE} seed={seed}: {error}"
                )
            });
            let reference = optimize_for_semantic_comparison(
                program.clone(),
                &sources,
                CoreOptimizationLevel::None,
            );
            let baseline = optimize_for_semantic_comparison(
                program,
                &sources,
                CoreOptimizationLevel::Baseline,
            );
            for condition in [false, true] {
                for input in [i32::MIN, -1, 0, 1, i32::MAX] {
                    assert_same_observation(
                        &reference,
                        &baseline,
                        function,
                        &[TestValue::I32(input), TestValue::Bool(condition)],
                        1_000,
                        &format!(
                            "generated version={GENERATED_CFG_VERSION} \
                             shape={TERMINATING_DIAMOND_SHAPE} seed={seed} \
                             input={input} condition={condition}"
                        ),
                    );
                }
            }
        });
    }
}

#[test]
fn generated_distinct_cfg_families_match_none_results_and_effect_order() {
    let sources = SourceContext::new();
    for seed in GENERATED_SEEDS {
        let cases = [
            generated_case(
                GeneratedFixtureContext::new(
                    seed,
                    STRAIGHT_LINE_SHAPE,
                    SCALAR_BOUNDARY_INPUT_DOMAIN,
                ),
                &sources,
                generated_straight_line,
                scalar_boundary_arguments(),
                0,
                0,
            ),
            generated_case(
                GeneratedFixtureContext::new(
                    seed,
                    ASYMMETRIC_DIAMOND_SHAPE,
                    BOOLEAN_DIAMOND_INPUT_DOMAIN,
                ),
                &sources,
                generated_asymmetric_diamond,
                asymmetric_diamond_arguments(),
                0,
                0,
            ),
            generated_case(
                GeneratedFixtureContext::new(
                    seed,
                    TERMINATING_LOOP_SHAPE,
                    NO_ARGUMENT_INPUT_DOMAIN,
                ),
                &sources,
                generated_terminating_loop,
                vec![vec![]],
                0,
                0,
            ),
            generated_case(
                GeneratedFixtureContext::new(
                    seed,
                    MULTI_RESULT_CALL_SHAPE,
                    SCALAR_BOUNDARY_INPUT_DOMAIN,
                ),
                &sources,
                generated_multi_result_call,
                scalar_boundary_arguments(),
                2,
                3,
            ),
        ];
        for case in cases {
            assert_generated_differential(case, &sources);
        }
    }
}
