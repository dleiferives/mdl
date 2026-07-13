use crate::diagnostic::Diagnostics;
use crate::ir::core::{BlockId, CoreOp, CoreProgram, FunctionId, InstId, TerminatorKind, ValueId};
use crate::ir::minecraft::MinecraftProgram;
use crate::source::OriginId;

use super::call::lower_planned_call;
use super::construct::{FunctionLoweringCx, TargetConstruction, invariant_diagnostics};
use super::control::{lower_branch, lower_branch_helper, lower_jump, lower_return};
use super::plan::{BranchArm, EdgeTransfer, HomeId, InstructionPlan, LoweringPlan};
use super::scalar::{ScalarLowering, lower_scalar_operation};

/// Constructs every frozen block and helper, returning no partial target on failure.
pub(crate) fn construct_program(
    core: &CoreProgram,
    plan: &LoweringPlan,
) -> Result<MinecraftProgram, Diagnostics> {
    let mut target = TargetConstruction::declare(plan)?;
    target.define_initialization(plan)?;
    for (function, declaration) in core.functions() {
        let body = declaration.body().ok_or_else(|| {
            invariant_diagnostics(
                "planned function definition disappeared",
                declaration.origin(),
            )
        })?;
        lower_blocks(&mut target, function, body, plan)?;
        lower_branch_helpers(&mut target, function, body, plan)?;
    }
    target.finish()
}

fn lower_blocks(
    target: &mut TargetConstruction,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
) -> Result<(), Diagnostics> {
    for block in body.block_order().iter().copied() {
        if let Some(planned) = plan.block_function(function, block) {
            lower_block(target, function, block, body, plan, planned)?;
        }
    }
    Ok(())
}

fn lower_block(
    target: &mut TargetConstruction,
    function: FunctionId,
    block: BlockId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
    planned: super::plan::PlannedFunctionId,
) -> Result<(), Diagnostics> {
    let mut context = target.begin_function(body, plan, planned)?;
    let data = body.block(block).ok_or_else(|| {
        invariant_diagnostics("planned Core block disappeared", OriginId::UNKNOWN)
    })?;
    for instruction in data.instructions().iter().copied() {
        lower_instruction(
            &mut context,
            function,
            instruction,
            body,
            plan,
            data.origin(),
        )?;
    }
    lower_terminator(&mut context, function, block, data, plan)?;
    context.finish();
    Ok(())
}

fn lower_instruction(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    instruction: InstId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
    block_origin: OriginId,
) -> Result<(), Diagnostics> {
    let data = body.instruction(instruction).ok_or_else(|| {
        invariant_diagnostics("planned Core instruction disappeared", block_origin)
    })?;
    let instruction_plan = plan
        .instruction_plan(function, instruction)
        .ok_or_else(|| {
            invariant_diagnostics(
                "materialized Core instruction has no frozen instruction plan",
                data.origin(),
            )
        })?;
    match instruction_plan {
        InstructionPlan::OmittedPure => Ok(()),
        InstructionPlan::Scalar { operands, results } => {
            if matches!(data.op(), CoreOp::Call(_)) {
                return Err(invariant_diagnostics(
                    "call instruction has a scalar physical plan",
                    data.origin(),
                ));
            }
            let result_homes = results
                .iter()
                .copied()
                .map(super::plan::ScalarResultPlacement::home)
                .collect::<Vec<_>>();
            if lower_scalar_operation(context, data.op(), operands, &result_homes, data.origin())?
                == ScalarLowering::Lowered
            {
                Ok(())
            } else {
                Err(invariant_diagnostics(
                    "scalar physical plan was deferred as a call",
                    data.origin(),
                ))
            }
        }
        InstructionPlan::Call {
            arguments,
            result_destinations,
        } => {
            let CoreOp::Call(callee) = data.op() else {
                return Err(invariant_diagnostics(
                    "non-call instruction has a call physical plan",
                    data.origin(),
                ));
            };
            lower_planned_call(
                context,
                *callee,
                arguments,
                result_destinations,
                data.origin(),
            )
        }
    }
}

fn lower_terminator(
    context: &mut FunctionLoweringCx<'_, '_>,
    function: FunctionId,
    block: BlockId,
    data: &crate::ir::core::BlockData,
    plan: &LoweringPlan,
) -> Result<(), Diagnostics> {
    let terminator = data.terminator().ok_or_else(|| {
        invariant_diagnostics("planned Core block lost its terminator", data.origin())
    })?;
    match terminator.kind() {
        TerminatorKind::Jump(destination) => lower_jump(
            context,
            function,
            block,
            destination.block(),
            terminator.origin(),
        ),
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => lower_branch(
            context,
            function,
            block,
            value_home(plan, function, *condition, terminator.origin())?,
            then_target.block(),
            else_target.block(),
            terminator.origin(),
        ),
        TerminatorKind::Return(values) => lower_return(
            context,
            function,
            &value_homes(plan, function, values, terminator.origin())?,
            terminator.origin(),
        ),
        TerminatorKind::Unreachable => Err(invariant_diagnostics(
            "reachable `unreachable` survived the legality audit",
            terminator.origin(),
        )),
    }
}

fn lower_branch_helpers(
    target: &mut TargetConstruction,
    function: FunctionId,
    body: &crate::ir::core::FunctionBody,
    plan: &LoweringPlan,
) -> Result<(), Diagnostics> {
    for block in body.block_order().iter().copied() {
        if plan.block_function(function, block).is_none() {
            continue;
        }
        let terminator = body
            .block(block)
            .and_then(crate::ir::core::BlockData::terminator)
            .ok_or_else(|| {
                invariant_diagnostics("planned Core block lost its terminator", OriginId::UNKNOWN)
            })?;
        let TerminatorKind::Branch {
            then_target,
            else_target,
            ..
        } = terminator.kind()
        else {
            continue;
        };
        let Some(EdgeTransfer::Branch {
            then_edge,
            else_edge,
        }) = plan.edge_transfer(function, block)
        else {
            return Err(invariant_diagnostics(
                "planned branch helper transfer disappeared",
                terminator.origin(),
            ));
        };
        for (arm, edge, destination) in [
            (BranchArm::Then, then_edge, then_target.block()),
            (BranchArm::Else, else_edge, else_target.block()),
        ] {
            let Some(helper) = edge.helper() else {
                continue;
            };
            let mut context = target.begin_function(body, plan, helper)?;
            lower_branch_helper(
                &mut context,
                function,
                block,
                arm,
                destination,
                terminator.origin(),
            )?;
            context.finish();
        }
    }
    Ok(())
}

fn value_homes(
    plan: &LoweringPlan,
    function: FunctionId,
    values: &[ValueId],
    origin: OriginId,
) -> Result<Vec<HomeId>, Diagnostics> {
    values
        .iter()
        .copied()
        .map(|value| value_home(plan, function, value, origin))
        .collect()
}

fn value_home(
    plan: &LoweringPlan,
    function: FunctionId,
    value: ValueId,
    origin: OriginId,
) -> Result<HomeId, Diagnostics> {
    plan.value_home(function, value).ok_or_else(|| {
        invariant_diagnostics("reachable Core value has no planned score home", origin)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::construct_program;
    use crate::ir::core::{
        BlockId, BlockTarget, CoreProgram, CoreType, FunctionBuilder, FunctionId, I32Predicate,
        Terminator, TerminatorKind,
    };
    use crate::ir::minecraft::{
        CallableRef, CommandKind, CommandNode, Condition, ExecuteModifierKind, InternalCallableRef,
        McFunctionId, MinecraftDebugDumper, MinecraftProgram, ObjectiveName, PackNamespace,
        ReturnCommand, ScoreCommand, ScoreComparison, ScoreHolders, ScoreOperation, ScoreRangeKind,
        ScoreRef, ScoreSelection, SingleScoreHolder, render_function, verify_program,
    };
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::lower::minecraft::audit::audit_legality;
    use crate::lower::minecraft::plan::{LoweringPlan, PlanBuilder};
    use crate::lower::minecraft::{
        LoweredFunction, LoweringOptions, LoweringOutput, MinecraftOptimizationLevel, RegisterSlot,
        lower_to_minecraft,
    };
    use crate::opt::core::{
        CoreOptimizationLevel, CoreOptimizationOptions, CorePassStatistics, CorePipelineStep,
        StatisticCount, optimize_core,
    };
    use crate::source::{OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the loop fixture mirrors the canonical Core proof and checks its target result"
    )]
    fn sum_down_lowers_and_executes_with_ordinary_loop_copies() {
        let sources = SourceContext::new();
        let (core, function) = sum_down_program(&sources);
        let (plan, target) = lower_fixture(&core, &sources);
        let rendered = render_all(&target);
        assert!(!rendered.contains("#f0t0"));
        assert!(rendered.contains("mdl:__mdl/f0/e1_0"));

        let mut executor = TestExecutor::new(&target, 10_000);
        set_abi_parameters(&mut executor, &plan, function, &[4, 10]);
        let outcome = executor.run(entry_target(&target, function));
        assert!(outcome.success);
        assert_eq!(outcome.value, 1);
        assert_eq!(result_values(&executor, &plan, function), vec![20]);
    }

    #[test]
    fn baseline_executes_loop_carried_wrapping_left_reuse_with_fewer_commands() {
        let sources = SourceContext::new();
        let (core, function, parameter, sum) = wrapping_reuse_program(&sources, false);

        let none = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::None),
        )
        .unwrap();
        let baseline = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
        )
        .unwrap();
        let none_rendered = render_all(none.program());
        let baseline_rendered = render_all(baseline.program());
        let baseline_report = baseline.dump_lowering();
        assert!(accepted_coalescing_merges(&baseline_report) > 0);
        assert_eq!(
            dumped_value_home(&baseline_report, function, parameter),
            dumped_value_home(&baseline_report, function, sum),
            "the executed wrapping add must reuse its left operand's exact home"
        );
        assert!(
            baseline_rendered
                .matches("scoreboard players operation")
                .count()
                < none_rendered
                    .matches("scoreboard players operation")
                    .count(),
            "None:\n{none_rendered}\nBaseline:\n{baseline_rendered}"
        );
        assert!(!has_self_assignment(&baseline_rendered));

        let observation = execute_lowered(&baseline, function, &[], &[], "wrapping-left-reuse");
        assert_eq!(observation.results, vec![3]);
        assert!(observation.effect_calls.is_empty());
    }

    #[test]
    fn baseline_executes_same_value_x_plus_x_in_its_coalesced_home() {
        let sources = SourceContext::new();
        let (core, function, parameter, sum) = wrapping_reuse_program(&sources, true);
        let baseline = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
        )
        .unwrap();
        let rendered = render_all(baseline.program());
        let report = baseline.dump_lowering();
        assert!(accepted_coalescing_merges(&report) > 0);
        assert_eq!(
            dumped_value_home(&report, function, parameter),
            dumped_value_home(&report, function, sum)
        );
        assert!(!has_self_assignment(&rendered));
        assert!(has_self_addition(&rendered));

        let observation = execute_lowered(&baseline, function, &[], &[], "same-value-x-plus-x");
        assert_eq!(observation.results, vec![2]);
        assert!(observation.effect_calls.is_empty());
    }

    #[test]
    fn baseline_executes_a_call_result_reusing_its_dead_caller_argument() {
        let sources = SourceContext::new();
        let (core, calling_function, argument, result) = call_result_reuse_program(&sources);
        let none = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::None),
        )
        .unwrap();
        let baseline = lower_to_minecraft(
            &core,
            &sources,
            &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
        )
        .unwrap();
        let none_rendered = render_all(none.program());
        let baseline_rendered = render_all(baseline.program());
        let baseline_report = baseline.dump_lowering();
        assert!(accepted_coalescing_merges(&baseline_report) > 0);
        assert_eq!(
            dumped_value_home(&baseline_report, calling_function, argument),
            dumped_value_home(&baseline_report, calling_function, result),
            "the executed call result must reuse its exact dead argument home"
        );
        assert!(
            baseline_rendered
                .matches("scoreboard players operation")
                .count()
                < none_rendered
                    .matches("scoreboard players operation")
                    .count()
        );

        let observation = execute_lowered(
            &baseline,
            calling_function,
            &[],
            &[],
            "call-result-dead-argument-reuse",
        );
        assert_eq!(observation.results, vec![5]);
        assert!(observation.effect_calls.is_empty());
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "the finite swap loop must expose the cyclic backedge in one complete fixture"
    )]
    fn finite_countdown_swap_lowers_and_executes_through_edge_temporary() {
        let sources = SourceContext::new();
        let (core, function, backedge) = countdown_swap_program(&sources);
        let (plan, target) = lower_fixture(&core, &sources);
        let rendered_backedge = render_resource(&target, &function_resource(function, backedge));
        assert_eq!(rendered_backedge.matches("#f0t0").count(), 2);
        assert!(
            rendered_backedge
                .contains("scoreboard players operation #f0v5 mdl.reg = #f0t0 mdl.reg")
        );

        let mut executor = TestExecutor::new(&target, 10_000);
        set_abi_parameters(&mut executor, &plan, function, &[3, 10, 20]);
        let outcome = executor.run(entry_target(&target, function));
        assert!(outcome.success);
        assert_eq!(outcome.value, 1);
        assert_eq!(result_values(&executor, &plan, function), vec![20, 10]);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one cross-layer fixture proves optimization, lowering, target results, and effect order"
    )]
    fn baseline_core_optimization_preserves_lowered_target_results_and_call_order() {
        let sources = SourceContext::new();
        let (core, fixture) = target_differential_program(&sources);

        let reference = optimize_core(
            core.clone(),
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::None),
        )
        .unwrap();
        let baseline = optimize_core(
            core.clone(),
            &sources,
            &CoreOptimizationOptions::new(CoreOptimizationLevel::Baseline),
        )
        .unwrap();
        crate::ir::core::verify_program(reference.program(), &sources).unwrap();
        crate::ir::core::verify_program(baseline.program(), &sources).unwrap();

        let reference_body = reference
            .program()
            .function(fixture.entry)
            .unwrap()
            .body()
            .unwrap();
        let baseline_body = baseline
            .program()
            .function(fixture.entry)
            .unwrap()
            .body()
            .unwrap();
        assert_eq!(reference_body.block_counts().attached, 5);
        assert_eq!(baseline_body.block_counts().attached, 4);
        assert_eq!(reference_body.instruction_counts().attached, 9);
        assert_eq!(baseline_body.instruction_counts().attached, 8);

        let fusion = baseline
            .report()
            .steps()
            .iter()
            .find(|summary| summary.step() == CorePipelineStep::Fusion)
            .unwrap();
        let CorePassStatistics::Fusion(fusion) = fusion.statistics() else {
            panic!("fusion step exposed non-fusion statistics");
        };
        assert!(matches!(fusion.blocks_consumed, StatisticCount::Exact(1..)));
        let cse = baseline
            .report()
            .steps()
            .iter()
            .find(|summary| summary.step() == CorePipelineStep::Cse)
            .unwrap();
        let CorePassStatistics::Cse(cse) = cse.statistics() else {
            panic!("CSE step exposed non-CSE statistics");
        };
        assert!(matches!(
            cse.instructions_erased,
            StatisticCount::Exact(1..)
        ));

        let direct_reference = lower_to_minecraft(&core, &sources, &options()).unwrap();
        let reference_target =
            lower_to_minecraft(reference.program(), &sources, &options()).unwrap();
        let baseline_target = lower_to_minecraft(baseline.program(), &sources, &options()).unwrap();
        assert_eq!(
            MinecraftDebugDumper::program(direct_reference.program()),
            MinecraftDebugDumper::program(reference_target.program()),
            "Core optimization level `None` must retain the Stage 4 target exactly"
        );
        verify_program(reference_target.program(), &sources).unwrap();
        verify_program(baseline_target.program(), &sources).unwrap();

        for input in [i32::MIN, -1, 0, i32::MAX] {
            let reference_observation = execute_lowered(
                &reference_target,
                fixture.entry,
                &[input],
                &[fixture.increment, fixture.decrement],
                &format!("fixed target differential input={input}"),
            );
            let baseline_observation = execute_lowered(
                &baseline_target,
                fixture.entry,
                &[input],
                &[fixture.increment, fixture.decrement],
                &format!("fixed target differential input={input}"),
            );
            assert_eq!(
                baseline_observation, reference_observation,
                "lowered target differential failed for input {input}"
            );
            assert_eq!(reference_observation.results, vec![input.wrapping_add(5)]);
            let expected_calls = if input < 0 {
                vec![fixture.decrement, fixture.increment]
            } else {
                vec![fixture.increment, fixture.decrement]
            };
            assert_eq!(reference_observation.effect_calls, expected_calls);
        }
    }

    #[test]
    fn generated_typed_cfgs_match_their_core_models_under_both_lowering_policies() {
        const GENERATOR_VERSION: u32 = 1;
        const SEEDS: [u64; 4] = [0, 1, 0x5eed_5eed, u64::MAX];

        let sources = SourceContext::new();
        for seed in SEEDS {
            let suite = generated_target_suite(&sources, GENERATOR_VERSION, seed);
            let suite_context = format!(
                "generator_version={GENERATOR_VERSION} shape=all seed={seed:#018x} inputs=all"
            );
            crate::ir::core::verify_program(&suite.core, &sources).unwrap_or_else(|diagnostics| {
                panic!("{suite_context}: generated Core verification failed: {diagnostics:?}")
            });
            let none = lower_to_minecraft(
                &suite.core,
                &sources,
                &options().with_optimization_level(MinecraftOptimizationLevel::None),
            )
            .unwrap_or_else(|diagnostics| {
                panic!("{suite_context}: Minecraft None lowering failed: {diagnostics:?}")
            });
            let baseline = lower_to_minecraft(
                &suite.core,
                &sources,
                &options().with_optimization_level(MinecraftOptimizationLevel::Baseline),
            )
            .unwrap_or_else(|diagnostics| {
                panic!("{suite_context}: Minecraft Baseline lowering failed: {diagnostics:?}")
            });
            verify_program(none.program(), &sources).unwrap_or_else(|diagnostics| {
                panic!(
                    "{suite_context}: Minecraft None target verification failed: {diagnostics:?}"
                )
            });
            verify_program(baseline.program(), &sources).unwrap_or_else(|diagnostics| {
                panic!(
                    "{suite_context}: Minecraft Baseline target verification failed: {diagnostics:?}"
                )
            });
            assert!(
                accepted_coalescing_merges(&baseline.dump_lowering()) > 0,
                "{suite_context}: generated Baseline suite must exercise a real coalescing decision"
            );

            for case in &suite.cases {
                for inputs in case.inputs.iter().copied() {
                    let context = format!(
                        "generator_version={GENERATOR_VERSION} shape={} seed={seed:#018x} inputs={inputs:?}",
                        case.shape.name()
                    );
                    let expected = case.oracle.observe(inputs);
                    let none_observation = execute_lowered(
                        &none,
                        case.entry,
                        &inputs,
                        &case.observable_functions,
                        &context,
                    );
                    let baseline_observation = execute_lowered(
                        &baseline,
                        case.entry,
                        &inputs,
                        &case.observable_functions,
                        &context,
                    );
                    assert_eq!(
                        none_observation.results, expected.results,
                        "{context}: Minecraft None results disagree with the Core fixture model"
                    );
                    assert_eq!(
                        none_observation.effect_calls, expected.effect_calls,
                        "{context}: Minecraft None effect order disagrees with the Core fixture model"
                    );
                    assert_eq!(
                        baseline_observation.results, expected.results,
                        "{context}: Minecraft Baseline results disagree with the Core fixture model"
                    );
                    assert_eq!(
                        baseline_observation.effect_calls, expected.effect_calls,
                        "{context}: Minecraft Baseline effect order disagrees with the Core fixture model"
                    );
                    assert_eq!(
                        baseline_observation, none_observation,
                        "{context}: Minecraft lowering policies disagree"
                    );
                }
            }
        }
    }

    fn accepted_coalescing_merges(report: &str) -> u64 {
        report
            .lines()
            .filter_map(|line| {
                line.split_whitespace()
                    .find_map(|field| field.strip_prefix("merges="))
            })
            .filter_map(|merges| merges.parse::<u64>().ok())
            .sum()
    }

    fn wrapping_reuse_program(
        sources: &SourceContext,
        same_operand: bool,
    ) -> (
        CoreProgram,
        FunctionId,
        crate::ir::core::ValueId,
        crate::ir::core::ValueId,
    ) {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("wrapping_reuse"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        let initial = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let increment = builder.i32_constant(2, OriginId::UNKNOWN).unwrap();
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
        let right = if same_operand { parameter } else { increment };
        let sum = builder
            .i32_add_wrapping(parameter, right, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
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

    fn call_result_reuse_program(
        sources: &SourceContext,
    ) -> (
        CoreProgram,
        FunctionId,
        crate::ir::core::ValueId,
        crate::ir::core::ValueId,
    ) {
        let mut core = CoreProgram::new();
        let identity_function = core
            .declare_function(
                Some("identity"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut identity_builder = FunctionBuilder::new(&core, sources, identity_function).unwrap();
        let identity_parameter = parameter(&identity_builder, identity_builder.entry_block(), 0);
        identity_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![identity_parameter]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(identity_function, identity_builder.finish().unwrap())
            .unwrap();

        let calling_function = core
            .declare_function(
                Some("call-result-reuse"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, calling_function).unwrap();
        let initial = builder.i32_constant(5, OriginId::UNKNOWN).unwrap();
        let condition = builder.bool_constant(false, OriginId::UNKNOWN).unwrap();
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let argument = builder
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
            .call(identity_function, vec![argument], OriginId::UNKNOWN)
            .unwrap()[0];
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
        core.define_function(calling_function, builder.finish().unwrap())
            .unwrap();
        (core, calling_function, argument, result)
    }

    fn dumped_value_home(
        report: &str,
        function: FunctionId,
        value: crate::ir::core::ValueId,
    ) -> u64 {
        use crate::entity::EntityId;

        let function_prefix = format!("function {} ", function.index());
        let value_prefix = format!("  value {} -> ", value.index());
        let mut in_function = false;
        for line in report.lines() {
            if line.starts_with("function ") {
                in_function = line.starts_with(&function_prefix);
                continue;
            }
            if in_function {
                if let Some(home) = line.strip_prefix(&value_prefix) {
                    return home.parse().expect("lowering report home is numeric");
                }
            }
        }
        panic!(
            "lowering report omitted function {} value {}",
            function.index(),
            value.index()
        );
    }

    fn has_self_assignment(rendered: &str) -> bool {
        rendered.lines().any(|line| {
            let Some(operation) = line.strip_prefix("scoreboard players operation ") else {
                return false;
            };
            let Some((destination, source)) = operation.split_once(" = ") else {
                return false;
            };
            destination == source
        })
    }

    fn has_self_addition(rendered: &str) -> bool {
        rendered.lines().any(|line| {
            let Some(operation) = line.strip_prefix("scoreboard players operation ") else {
                return false;
            };
            let Some((destination, source)) = operation.split_once(" += ") else {
                return false;
            };
            destination == source
        })
    }

    #[derive(Clone, Copy)]
    struct TargetDifferentialFixture {
        entry: FunctionId,
        increment: FunctionId,
        decrement: FunctionId,
    }

    #[derive(Debug, Eq, PartialEq)]
    struct TargetObservation {
        results: Vec<i32>,
        effect_calls: Vec<FunctionId>,
    }

    #[derive(Clone, Copy, Debug)]
    enum GeneratedShape {
        StraightLine,
        BranchJoin,
        TerminatingLoop,
        MultiResultCalls,
    }

    impl GeneratedShape {
        const fn name(self) -> &'static str {
            match self {
                Self::StraightLine => "straight-line",
                Self::BranchJoin => "branch-join",
                Self::TerminatingLoop => "terminating-loop",
                Self::MultiResultCalls => "multi-result-effectful-calls",
            }
        }
    }

    struct GeneratedTargetSuite {
        core: CoreProgram,
        cases: Vec<GeneratedTargetCase>,
    }

    struct GeneratedTargetCase {
        shape: GeneratedShape,
        entry: FunctionId,
        observable_functions: Box<[FunctionId]>,
        inputs: Box<[[i32; 2]]>,
        oracle: GeneratedOracle,
    }

    #[derive(Clone, Copy)]
    enum GeneratedOracle {
        StraightLine {
            left_offset: i32,
            right_offset: i32,
            predicate: I32Predicate,
            negate: bool,
        },
        BranchJoin {
            condition: I32Predicate,
            then_swap: bool,
            else_swap: bool,
            then_offset: i32,
            else_offset: i32,
            then_predicate: I32Predicate,
            else_predicate: I32Predicate,
            then_negate: bool,
            else_negate: bool,
        },
        TerminatingLoop {
            iterations: i32,
            delta: i32,
            rotate: bool,
        },
        MultiResultCalls {
            pair: FunctionId,
            first: FunctionId,
            second: FunctionId,
            first_offset: i32,
            second_offset: i32,
            negate_condition: bool,
        },
    }

    impl GeneratedOracle {
        fn observe(self, [left, right]: [i32; 2]) -> TargetObservation {
            let (results, effect_calls) = match self {
                Self::StraightLine {
                    left_offset,
                    right_offset,
                    predicate,
                    negate,
                } => {
                    let left = left.wrapping_add(left_offset);
                    let right = right.wrapping_add(right_offset);
                    let (sum, overflowed) = left.overflowing_add(right);
                    let compared = evaluate_predicate(predicate, left, right) ^ negate;
                    (
                        vec![sum, bool_score(overflowed), bool_score(compared)],
                        vec![],
                    )
                }
                Self::BranchJoin {
                    condition,
                    then_swap,
                    else_swap,
                    then_offset,
                    else_offset,
                    then_predicate,
                    else_predicate,
                    then_negate,
                    else_negate,
                } => {
                    let (swap, offset, predicate, negate) =
                        if evaluate_predicate(condition, left, right) {
                            (then_swap, then_offset, then_predicate, then_negate)
                        } else {
                            (else_swap, else_offset, else_predicate, else_negate)
                        };
                    let (first, second) = if swap { (right, left) } else { (left, right) };
                    (
                        vec![
                            first.wrapping_add(offset),
                            bool_score(evaluate_predicate(predicate, first, second) ^ negate),
                        ],
                        vec![],
                    )
                }
                Self::TerminatingLoop {
                    iterations,
                    delta,
                    rotate,
                } => {
                    let mut count = iterations;
                    let mut accumulator = left;
                    let mut carried = right;
                    while count > 0 {
                        count = count.wrapping_add(-1);
                        let next_accumulator = accumulator.wrapping_add(carried);
                        let next_carried = carried.wrapping_add(delta);
                        (accumulator, carried) = if rotate {
                            (next_carried, next_accumulator)
                        } else {
                            (next_accumulator, next_carried)
                        };
                    }
                    (vec![accumulator, carried], vec![])
                }
                Self::MultiResultCalls {
                    pair,
                    first,
                    second,
                    first_offset,
                    second_offset,
                    negate_condition,
                } => {
                    let (sum, overflowed) = left.overflowing_add(right);
                    let first_then_second = overflowed ^ negate_condition;
                    let (first_call, first_delta, second_call, second_delta) = if first_then_second
                    {
                        (first, first_offset, second, second_offset)
                    } else {
                        (second, second_offset, first, first_offset)
                    };
                    let result = sum.wrapping_add(first_delta).wrapping_add(second_delta);
                    (
                        vec![result, bool_score(overflowed)],
                        vec![pair, first_call, second_call],
                    )
                }
            };
            TargetObservation {
                results,
                effect_calls,
            }
        }
    }

    fn generated_target_suite(
        sources: &SourceContext,
        generator_version: u32,
        seed: u64,
    ) -> GeneratedTargetSuite {
        let mut rng = GeneratedRng::new(generator_version, seed);
        let mut core = CoreProgram::new();
        let cases = vec![
            append_generated_straight_line(&mut core, sources, &mut rng),
            append_generated_branch_join(&mut core, sources, &mut rng),
            append_generated_loop(&mut core, sources, &mut rng),
            append_generated_calls(&mut core, sources, &mut rng),
        ];
        GeneratedTargetSuite { core, cases }
    }

    fn append_generated_straight_line(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_straight_line"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let left_offset = rng.next_i32();
        let right_offset = rng.next_i32();
        let predicate = rng.predicate();
        let negate = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut builder = FunctionBuilder::new(core, sources, entry).unwrap();
        let block = builder.entry_block();
        let left = parameter(&builder, block, 0);
        let right = parameter(&builder, block, 1);
        let left_literal = builder
            .i32_constant(left_offset, OriginId::UNKNOWN)
            .unwrap();
        let right_literal = builder
            .i32_constant(right_offset, OriginId::UNKNOWN)
            .unwrap();
        let adjusted_left = builder
            .i32_add_wrapping(left, left_literal, OriginId::UNKNOWN)
            .unwrap();
        let adjusted_right = builder
            .i32_add_wrapping(right, right_literal, OriginId::UNKNOWN)
            .unwrap();
        let (sum, overflowed) = builder
            .i32_add_overflowing(adjusted_left, adjusted_right, OriginId::UNKNOWN)
            .unwrap();
        let compared = builder
            .i32_compare(predicate, adjusted_left, adjusted_right, OriginId::UNKNOWN)
            .unwrap();
        let compared = maybe_negate(&mut builder, compared, negate);
        let dead = builder
            .i32_constant(rng.next_i32(), OriginId::UNKNOWN)
            .unwrap();
        builder
            .i32_add_wrapping(dead, adjusted_left, OriginId::UNKNOWN)
            .unwrap();
        return_values(&mut builder, vec![sum, overflowed, compared]);
        core.define_function(entry, builder.finish().unwrap())
            .unwrap();

        GeneratedTargetCase {
            shape: GeneratedShape::StraightLine,
            entry,
            observable_functions: Box::new([]),
            inputs,
            oracle: GeneratedOracle::StraightLine {
                left_offset,
                right_offset,
                predicate,
                negate,
            },
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the generated diamond keeps typed edge arguments and each arm's oracle knobs together"
    )]
    fn append_generated_branch_join(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_branch_join"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let condition = rng.predicate();
        let then_swap = rng.next_bool();
        let else_swap = rng.next_bool();
        let then_offset = rng.next_i32();
        let else_offset = rng.next_i32();
        let then_predicate = rng.predicate();
        let else_predicate = rng.predicate();
        let then_negate = rng.next_bool();
        let else_negate = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut builder = FunctionBuilder::new(core, sources, entry).unwrap();
        let entry_block = builder.entry_block();
        let left = parameter(&builder, entry_block, 0);
        let right = parameter(&builder, entry_block, 1);
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let then_first = builder
            .append_block_parameter(then_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let then_second = builder
            .append_block_parameter(then_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let then_dead = builder
            .append_block_parameter(then_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_first = builder
            .append_block_parameter(else_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let else_second = builder
            .append_block_parameter(else_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let else_dead = builder
            .append_block_parameter(else_block, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined_integer = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let joined_boolean = builder
            .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        let branch_condition = builder
            .i32_compare(condition, left, right, OriginId::UNKNOWN)
            .unwrap();
        let dead = builder
            .i32_constant(rng.next_i32(), OriginId::UNKNOWN)
            .unwrap();
        let (then_left, then_right) = if then_swap {
            (right, left)
        } else {
            (left, right)
        };
        let (else_left, else_right) = if else_swap {
            (right, left)
        } else {
            (left, right)
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: branch_condition,
                    then_target: BlockTarget::new(then_block, vec![then_left, then_right, dead]),
                    else_target: BlockTarget::new(else_block, vec![else_left, else_right, dead]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(then_block).unwrap();
        let offset = builder
            .i32_constant(then_offset, OriginId::UNKNOWN)
            .unwrap();
        let result = builder
            .i32_add_wrapping(then_first, offset, OriginId::UNKNOWN)
            .unwrap();
        let compared = builder
            .i32_compare(then_predicate, then_first, then_second, OriginId::UNKNOWN)
            .unwrap();
        let compared = maybe_negate(&mut builder, compared, then_negate);
        builder
            .i32_add_wrapping(then_dead, then_second, OriginId::UNKNOWN)
            .unwrap();
        jump(&mut builder, join, vec![result, compared]);

        builder.switch_to_block(else_block).unwrap();
        let offset = builder
            .i32_constant(else_offset, OriginId::UNKNOWN)
            .unwrap();
        let result = builder
            .i32_add_wrapping(else_first, offset, OriginId::UNKNOWN)
            .unwrap();
        let compared = builder
            .i32_compare(else_predicate, else_first, else_second, OriginId::UNKNOWN)
            .unwrap();
        let compared = maybe_negate(&mut builder, compared, else_negate);
        builder
            .i32_add_wrapping(else_dead, else_second, OriginId::UNKNOWN)
            .unwrap();
        jump(&mut builder, join, vec![result, compared]);

        builder.switch_to_block(join).unwrap();
        return_values(&mut builder, vec![joined_integer, joined_boolean]);
        core.define_function(entry, builder.finish().unwrap())
            .unwrap();

        GeneratedTargetCase {
            shape: GeneratedShape::BranchJoin,
            entry,
            observable_functions: Box::new([]),
            inputs,
            oracle: GeneratedOracle::BranchJoin {
                condition,
                then_swap,
                else_swap,
                then_offset,
                else_offset,
                then_predicate,
                else_predicate,
                then_negate,
                else_negate,
            },
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the generated loop keeps its bounded state transition and dead carried cycle visible"
    )]
    fn append_generated_loop(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_terminating_loop"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let iterations = rng.bounded_loop_iterations();
        let delta = rng.next_i32();
        let rotate = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut builder = FunctionBuilder::new(core, sources, entry).unwrap();
        let entry_block = builder.entry_block();
        let initial_accumulator = parameter(&builder, entry_block, 0);
        let initial_carried = parameter(&builder, entry_block, 1);
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let count = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let accumulator = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let carried = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let dead = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let body = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result_accumulator = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let result_carried = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let initial_count = builder.i32_constant(iterations, OriginId::UNKNOWN).unwrap();
        let initial_dead = builder
            .i32_constant(rng.next_i32(), OriginId::UNKNOWN)
            .unwrap();
        jump(
            &mut builder,
            header,
            vec![
                initial_count,
                initial_accumulator,
                initial_carried,
                initial_dead,
            ],
        );

        builder.switch_to_block(header).unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let done = builder
            .i32_compare(I32Predicate::SignedLe, count, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: done,
                    then_target: BlockTarget::new(exit, vec![accumulator, carried]),
                    else_target: BlockTarget::new(body, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(body).unwrap();
        let minus_one = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
        let delta_value = builder.i32_constant(delta, OriginId::UNKNOWN).unwrap();
        let next_count = builder
            .i32_add_wrapping(count, minus_one, OriginId::UNKNOWN)
            .unwrap();
        let next_accumulator = builder
            .i32_add_wrapping(accumulator, carried, OriginId::UNKNOWN)
            .unwrap();
        let next_carried = builder
            .i32_add_wrapping(carried, delta_value, OriginId::UNKNOWN)
            .unwrap();
        let next_dead = builder
            .i32_add_wrapping(dead, delta_value, OriginId::UNKNOWN)
            .unwrap();
        let (next_accumulator, next_carried) = if rotate {
            (next_carried, next_accumulator)
        } else {
            (next_accumulator, next_carried)
        };
        jump(
            &mut builder,
            header,
            vec![next_count, next_accumulator, next_carried, next_dead],
        );

        builder.switch_to_block(exit).unwrap();
        return_values(&mut builder, vec![result_accumulator, result_carried]);
        core.define_function(entry, builder.finish().unwrap())
            .unwrap();

        GeneratedTargetCase {
            shape: GeneratedShape::TerminatingLoop,
            entry,
            observable_functions: Box::new([]),
            inputs,
            oracle: GeneratedOracle::TerminatingLoop {
                iterations,
                delta,
                rotate,
            },
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the call fixture deliberately keeps its multi-result callee and both effect orders visible"
    )]
    fn append_generated_calls(
        core: &mut CoreProgram,
        sources: &SourceContext,
        rng: &mut GeneratedRng,
    ) -> GeneratedTargetCase {
        let entry = core
            .declare_function(
                Some("generated_effectful_calls"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let pair = core
            .declare_function(
                Some("generated_pair"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::Bool, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let first = core
            .declare_function(
                Some("generated_first_effect"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let second = core
            .declare_function(
                Some("generated_second_effect"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let first_offset = rng.next_i32();
        let second_offset = rng.next_i32();
        let extra_offset = rng.next_i32();
        let negate_condition = rng.next_bool();
        let inputs = generated_inputs(rng);

        let mut pair_builder = FunctionBuilder::new(core, sources, pair).unwrap();
        let pair_entry = pair_builder.entry_block();
        let pair_left = parameter(&pair_builder, pair_entry, 0);
        let pair_right = parameter(&pair_builder, pair_entry, 1);
        let (sum, overflowed) = pair_builder
            .i32_add_overflowing(pair_left, pair_right, OriginId::UNKNOWN)
            .unwrap();
        let extra_literal = pair_builder
            .i32_constant(extra_offset, OriginId::UNKNOWN)
            .unwrap();
        let extra = pair_builder
            .i32_add_wrapping(sum, extra_literal, OriginId::UNKNOWN)
            .unwrap();
        return_values(&mut pair_builder, vec![sum, overflowed, extra]);
        core.define_function(pair, pair_builder.finish().unwrap())
            .unwrap();
        define_wrapping_offset(core, sources, first, first_offset);
        define_wrapping_offset(core, sources, second, second_offset);

        let mut builder = FunctionBuilder::new(core, sources, entry).unwrap();
        let entry_block = builder.entry_block();
        let left = parameter(&builder, entry_block, 0);
        let right = parameter(&builder, entry_block, 1);
        let call_results = builder
            .call(pair, vec![left, right], OriginId::UNKNOWN)
            .unwrap();
        let sum = call_results[0];
        let overflowed = call_results[1];
        let condition = maybe_negate(&mut builder, overflowed, negate_condition);
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined_integer = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let joined_boolean = builder
            .append_block_parameter(join, CoreType::Bool, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(then_block).unwrap();
        let after_first = builder.call(first, vec![sum], OriginId::UNKNOWN).unwrap()[0];
        let after_second = builder
            .call(second, vec![after_first], OriginId::UNKNOWN)
            .unwrap()[0];
        jump(&mut builder, join, vec![after_second, overflowed]);

        builder.switch_to_block(else_block).unwrap();
        let after_second = builder.call(second, vec![sum], OriginId::UNKNOWN).unwrap()[0];
        let after_first = builder
            .call(first, vec![after_second], OriginId::UNKNOWN)
            .unwrap()[0];
        jump(&mut builder, join, vec![after_first, overflowed]);

        builder.switch_to_block(join).unwrap();
        return_values(&mut builder, vec![joined_integer, joined_boolean]);
        core.define_function(entry, builder.finish().unwrap())
            .unwrap();

        GeneratedTargetCase {
            shape: GeneratedShape::MultiResultCalls,
            entry,
            observable_functions: vec![pair, first, second].into_boxed_slice(),
            inputs,
            oracle: GeneratedOracle::MultiResultCalls {
                pair,
                first,
                second,
                first_offset,
                second_offset,
                negate_condition,
            },
        }
    }

    fn generated_inputs(rng: &mut GeneratedRng) -> Box<[[i32; 2]]> {
        vec![
            [0, 0],
            [i32::MAX, 1],
            [i32::MIN, -1],
            [-7, 11],
            [rng.next_i32(), rng.next_i32()],
        ]
        .into_boxed_slice()
    }

    fn maybe_negate(
        builder: &mut FunctionBuilder<'_>,
        value: crate::ir::core::ValueId,
        negate: bool,
    ) -> crate::ir::core::ValueId {
        if negate {
            builder.bool_not(value, OriginId::UNKNOWN).unwrap()
        } else {
            value
        }
    }

    fn jump(
        builder: &mut FunctionBuilder<'_>,
        target: BlockId,
        arguments: Vec<crate::ir::core::ValueId>,
    ) {
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(target, arguments)),
                OriginId::UNKNOWN,
            ))
            .unwrap();
    }

    fn return_values(builder: &mut FunctionBuilder<'_>, values: Vec<crate::ir::core::ValueId>) {
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(values),
                OriginId::UNKNOWN,
            ))
            .unwrap();
    }

    const fn bool_score(value: bool) -> i32 {
        if value { 1 } else { 0 }
    }

    const fn evaluate_predicate(predicate: I32Predicate, left: i32, right: i32) -> bool {
        match predicate {
            I32Predicate::Eq => left == right,
            I32Predicate::Ne => left != right,
            I32Predicate::SignedLt => left < right,
            I32Predicate::SignedLe => left <= right,
            I32Predicate::SignedGt => left > right,
            I32Predicate::SignedGe => left >= right,
        }
    }

    struct GeneratedRng {
        state: u64,
    }

    impl GeneratedRng {
        fn new(generator_version: u32, seed: u64) -> Self {
            Self {
                state: seed ^ u64::from(generator_version).wrapping_mul(0x9e37_79b9_7f4a_7c15),
            }
        }

        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut value = self.state;
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^ (value >> 31)
        }

        fn next_i32(&mut self) -> i32 {
            let bytes = self.next_u64().to_le_bytes();
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }

        fn next_bool(&mut self) -> bool {
            self.next_u64() & 1 != 0
        }

        fn predicate(&mut self) -> I32Predicate {
            const PREDICATES: [I32Predicate; 6] = [
                I32Predicate::Eq,
                I32Predicate::Ne,
                I32Predicate::SignedLt,
                I32Predicate::SignedLe,
                I32Predicate::SignedGt,
                I32Predicate::SignedGe,
            ];
            let index =
                usize::try_from(self.next_u64() % 6).expect("six generated predicates fit usize");
            PREDICATES[index]
        }

        fn bounded_loop_iterations(&mut self) -> i32 {
            match self.next_u64() % 5 {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 3,
                _ => 4,
            }
        }
    }

    fn target_differential_program(
        sources: &SourceContext,
    ) -> (CoreProgram, TargetDifferentialFixture) {
        let mut core = CoreProgram::new();
        let entry = core
            .declare_function(
                Some("target_differential"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let increment = core
            .declare_function(
                Some("observable_increment"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let decrement = core
            .declare_function(
                Some("observable_decrement"),
                vec![CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();

        define_wrapping_offset(&mut core, sources, increment, 1);
        define_wrapping_offset(&mut core, sources, decrement, -1);
        define_target_differential_entry(&mut core, sources, entry, increment, decrement);
        (
            core,
            TargetDifferentialFixture {
                entry,
                increment,
                decrement,
            },
        )
    }

    fn define_target_differential_entry(
        core: &mut CoreProgram,
        sources: &SourceContext,
        entry: FunctionId,
        increment: FunctionId,
        decrement: FunctionId,
    ) {
        let mut builder = FunctionBuilder::new(core, sources, entry).unwrap();
        let entry_block = builder.entry_block();
        let input = parameter(&builder, entry_block, 0);
        let work = builder.create_block(OriginId::UNKNOWN).unwrap();
        let work_input = builder
            .append_block_parameter(work, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(work, vec![input])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(work).unwrap();
        let five = builder.i32_constant(5, OriginId::UNKNOWN).unwrap();
        let first_sum = builder
            .i32_add_wrapping(work_input, five, OriginId::UNKNOWN)
            .unwrap();
        let duplicate_sum = builder
            .i32_add_wrapping(work_input, five, OriginId::UNKNOWN)
            .unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let nonnegative = builder
            .i32_compare(I32Predicate::SignedGe, work_input, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: nonnegative,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(then_block).unwrap();
        let incremented = builder
            .call(increment, vec![first_sum], OriginId::UNKNOWN)
            .unwrap()[0];
        let restored = builder
            .call(decrement, vec![incremented], OriginId::UNKNOWN)
            .unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![restored])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(else_block).unwrap();
        let decremented = builder
            .call(decrement, vec![duplicate_sum], OriginId::UNKNOWN)
            .unwrap()[0];
        let restored = builder
            .call(increment, vec![decremented], OriginId::UNKNOWN)
            .unwrap()[0];
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![restored])),
                OriginId::UNKNOWN,
            ))
            .unwrap();

        builder.switch_to_block(join).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(entry, builder.finish().unwrap())
            .unwrap();
    }

    fn define_wrapping_offset(
        core: &mut CoreProgram,
        sources: &SourceContext,
        function: FunctionId,
        offset: i32,
    ) {
        let mut builder = FunctionBuilder::new(core, sources, function).unwrap();
        let input = parameter(&builder, builder.entry_block(), 0);
        let offset = builder.i32_constant(offset, OriginId::UNKNOWN).unwrap();
        let result = builder
            .i32_add_wrapping(input, offset, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
    }

    fn execute_lowered(
        output: &LoweringOutput,
        entry: FunctionId,
        arguments: &[i32],
        observable_functions: &[FunctionId],
        context: &str,
    ) -> TargetObservation {
        let lowered = output.map().function(entry).unwrap();
        assert_eq!(
            lowered.parameter_homes().len(),
            arguments.len(),
            "{context}: generated entry parameter arity changed during lowering"
        );
        let mut executor = TestExecutor::new(output.program(), 10_000);
        set_public_abi_parameters(&mut executor, lowered, arguments, context);
        let outcome = executor.run(public_entry_target(output.program(), lowered));
        assert!(outcome.success, "{context}: generated target call failed");
        assert_eq!(
            outcome.value, 1,
            "{context}: generated target entry returned a non-success value"
        );
        let observable_targets = observable_functions
            .iter()
            .copied()
            .map(|function| {
                let lowered = output.map().function(function).unwrap();
                (public_entry_target(output.program(), lowered), function)
            })
            .collect::<HashMap<_, _>>();
        let effect_calls = executor
            .called_functions
            .iter()
            .filter_map(|target| observable_targets.get(target).copied())
            .collect();
        let results = lowered
            .result_homes()
            .iter()
            .map(|(_, slot)| executor.score(&register_score(slot)))
            .collect();
        TargetObservation {
            results,
            effect_calls,
        }
    }

    fn set_public_abi_parameters(
        executor: &mut TestExecutor<'_>,
        function: &LoweredFunction,
        values: &[i32],
        context: &str,
    ) {
        for ((ty, slot), value) in function
            .parameter_homes()
            .iter()
            .zip(values.iter().copied())
        {
            assert!(
                *ty != CoreType::Bool || matches!(value, 0 | 1),
                "{context}: generated Boolean ABI input is not canonical"
            );
            executor.scores.insert(register_score(slot), value);
        }
    }

    fn public_entry_target(program: &MinecraftProgram, function: &LoweredFunction) -> McFunctionId {
        program
            .functions()
            .find_map(|(id, data)| (data.resource() == function.entry_resource()).then_some(id))
            .unwrap()
    }

    fn register_score(slot: &RegisterSlot) -> ScoreRef {
        ScoreRef::new(
            SingleScoreHolder::Fake(slot.holder().clone()),
            slot.objective().clone(),
        )
    }

    fn sum_down_program(sources: &SourceContext) -> (CoreProgram, FunctionId) {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("sum_down"),
                vec![CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        let entry = builder.entry_block();
        let initial_counter = parameter(&builder, entry, 0);
        let initial_accumulator = parameter(&builder, entry, 1);
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let counter = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let accumulator = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let body = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    header,
                    vec![initial_counter, initial_accumulator],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let done = builder
            .i32_compare(I32Predicate::SignedLe, counter, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: done,
                    then_target: BlockTarget::new(exit, vec![accumulator]),
                    else_target: BlockTarget::new(body, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(body).unwrap();
        let minus_one = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
        let next_counter = builder
            .i32_add_wrapping(counter, minus_one, OriginId::UNKNOWN)
            .unwrap();
        let next_accumulator = builder
            .i32_add_wrapping(accumulator, counter, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    header,
                    vec![next_counter, next_accumulator],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function)
    }

    fn countdown_swap_program(sources: &SourceContext) -> (CoreProgram, FunctionId, BlockId) {
        let mut core = CoreProgram::new();
        let function = core
            .declare_function(
                Some("countdown_swap"),
                vec![CoreType::I32, CoreType::I32, CoreType::I32],
                vec![CoreType::I32, CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&core, sources, function).unwrap();
        let entry = builder.entry_block();
        let initial_count = parameter(&builder, entry, 0);
        let initial_left = parameter(&builder, entry, 1);
        let initial_right = parameter(&builder, entry, 2);
        let header = builder.create_block(OriginId::UNKNOWN).unwrap();
        let count = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let left = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let right = builder
            .append_block_parameter(header, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let backedge = builder.create_block(OriginId::UNKNOWN).unwrap();
        let exit = builder.create_block(OriginId::UNKNOWN).unwrap();
        let result_left = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        let result_right = builder
            .append_block_parameter(exit, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(
                    header,
                    vec![initial_count, initial_left, initial_right],
                )),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(header).unwrap();
        let zero = builder.i32_constant(0, OriginId::UNKNOWN).unwrap();
        let done = builder
            .i32_compare(I32Predicate::SignedLe, count, zero, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: done,
                    then_target: BlockTarget::new(exit, vec![left, right]),
                    else_target: BlockTarget::new(backedge, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(backedge).unwrap();
        let minus_one = builder.i32_constant(-1, OriginId::UNKNOWN).unwrap();
        let next_count = builder
            .i32_add_wrapping(count, minus_one, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(header, vec![next_count, right, left])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(exit).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![result_left, result_right]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        core.define_function(function, builder.finish().unwrap())
            .unwrap();
        (core, function, backedge)
    }

    fn parameter(
        builder: &FunctionBuilder<'_>,
        block: BlockId,
        index: usize,
    ) -> crate::ir::core::ValueId {
        builder.body().block(block).unwrap().parameters()[index].value()
    }

    fn lower_fixture(
        core: &CoreProgram,
        sources: &SourceContext,
    ) -> (LoweringPlan, MinecraftProgram) {
        crate::ir::core::verify_program(core, sources).unwrap();
        let analyses = SemanticInventory::new(core).unwrap();
        audit_legality(core, &analyses).unwrap();
        let mut builder = PlanBuilder::new(core, options()).unwrap();
        builder
            .physicalize(
                core,
                &analyses,
                &crate::lower::minecraft::demand::RuntimeDemand::optimization_disabled(),
            )
            .unwrap();
        let plan = builder.finish(core, &analyses).unwrap();
        let target = construct_program(core, &plan).unwrap();
        verify_program(&target, sources).unwrap();
        (plan, target)
    }

    fn options() -> LoweringOptions {
        LoweringOptions::new(
            JavaEditionTarget::V26_2,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
    }

    fn render_all(program: &MinecraftProgram) -> String {
        program
            .functions()
            .map(|(_, function)| {
                String::from_utf8(render_function(program, function).unwrap()).unwrap()
            })
            .collect()
    }

    fn render_resource(program: &MinecraftProgram, resource: &str) -> String {
        let (_, function) = program
            .functions()
            .find(|(_, function)| function.resource().to_string() == resource)
            .unwrap();
        String::from_utf8(render_function(program, function).unwrap()).unwrap()
    }

    fn function_resource(function: FunctionId, block: BlockId) -> String {
        use crate::entity::EntityId;
        format!("mdl:__mdl/f{}/b{}", function.index(), block.index())
    }

    fn entry_target(program: &MinecraftProgram, function: FunctionId) -> McFunctionId {
        use crate::entity::EntityId;
        let resource = format!("mdl:__mdl/f{}/b0", function.index());
        program
            .functions()
            .find_map(|(id, data)| (data.resource().to_string() == resource).then_some(id))
            .unwrap()
    }

    fn set_abi_parameters(
        executor: &mut TestExecutor<'_>,
        plan: &LoweringPlan,
        function: FunctionId,
        values: &[i32],
    ) {
        for (home, value) in plan
            .function_parameters(function)
            .unwrap()
            .iter()
            .copied()
            .zip(values.iter().copied())
        {
            executor.scores.insert(plan.score(home).unwrap(), value);
        }
    }

    fn result_values(
        executor: &TestExecutor<'_>,
        plan: &LoweringPlan,
        function: FunctionId,
    ) -> Vec<i32> {
        plan.function_results(function)
            .unwrap()
            .iter()
            .map(|home| executor.score(&plan.score(*home).unwrap()))
            .collect()
    }

    #[derive(Clone, Copy, Debug)]
    struct Outcome {
        value: i32,
        success: bool,
    }

    enum Flow {
        Continue(Outcome),
        Return(Outcome),
    }

    struct TestExecutor<'a> {
        program: &'a MinecraftProgram,
        scores: HashMap<ScoreRef, i32>,
        called_functions: Vec<McFunctionId>,
        remaining_commands: usize,
    }

    impl<'a> TestExecutor<'a> {
        fn new(program: &'a MinecraftProgram, command_budget: usize) -> Self {
            Self {
                program,
                scores: HashMap::new(),
                called_functions: Vec::new(),
                remaining_commands: command_budget,
            }
        }

        fn run(&mut self, function: McFunctionId) -> Outcome {
            let commands = self
                .program
                .function(function)
                .unwrap()
                .body()
                .commands()
                .map(|(_, command)| command.clone())
                .collect::<Vec<_>>();
            let mut last = Outcome {
                value: 0,
                success: true,
            };
            for command in &commands {
                match self.command(command) {
                    Flow::Continue(outcome) => last = outcome,
                    Flow::Return(outcome) => return outcome,
                }
            }
            last
        }

        fn command(&mut self, command: &CommandNode) -> Flow {
            self.remaining_commands = self
                .remaining_commands
                .checked_sub(1)
                .expect("generated loop exceeded the test command budget");
            match command.kind() {
                CommandKind::Score(command) => Flow::Continue(self.score_command(command)),
                CommandKind::Function(call) => {
                    let CallableRef::Internal(InternalCallableRef::Function(target)) =
                        call.target()
                    else {
                        panic!("lowered test program called a non-function target");
                    };
                    self.called_functions.push(*target);
                    Flow::Continue(self.run(*target))
                }
                CommandKind::Return(ReturnCommand::Value(value)) => Flow::Return(Outcome {
                    value: *value,
                    success: true,
                }),
                CommandKind::Return(ReturnCommand::Fail) => Flow::Return(Outcome {
                    value: 0,
                    success: false,
                }),
                CommandKind::Return(ReturnCommand::Run(run)) => {
                    Flow::Return(self.command(run).outcome())
                }
                CommandKind::Execute(execute) => {
                    let selected = execute.modifiers().as_slice().iter().all(|modifier| {
                        match modifier.kind() {
                            ExecuteModifierKind::If(condition) => self.condition(condition),
                            ExecuteModifierKind::Unless(condition) => !self.condition(condition),
                            _ => panic!("loop lowering emitted a context-changing modifier"),
                        }
                    });
                    if selected {
                        self.command(execute.run())
                    } else {
                        Flow::Continue(Outcome {
                            value: 0,
                            success: false,
                        })
                    }
                }
                CommandKind::Data(_) | CommandKind::Raw(_) => {
                    panic!("loop lowering emitted a non-score primitive")
                }
            }
        }

        fn score_command(&mut self, command: &ScoreCommand) -> Outcome {
            match command {
                ScoreCommand::PlayersSet { target, value } => {
                    self.scores.insert(score_ref(target), *value);
                    Outcome {
                        value: *value,
                        success: true,
                    }
                }
                ScoreCommand::PlayersOperation { target, op, source } => {
                    let target = score_ref(target);
                    let source = score_ref(source);
                    let left = self.score(&target);
                    let right = self.score(&source);
                    let value = match op {
                        ScoreOperation::Assign => right,
                        ScoreOperation::Add => left.wrapping_add(right),
                        ScoreOperation::Subtract => left.wrapping_sub(right),
                        _ => panic!("loop lowering emitted an untested score operation"),
                    };
                    self.scores.insert(target, value);
                    Outcome {
                        value,
                        success: true,
                    }
                }
                _ => panic!("loop lowering emitted an untested score command"),
            }
        }

        fn condition(&self, condition: &Condition) -> bool {
            match condition {
                Condition::ScoreMatches(score, range) => {
                    let value = self.score(score);
                    match range.kind() {
                        ScoreRangeKind::Exact(expected) => value == expected,
                        ScoreRangeKind::AtLeast(minimum) => value >= minimum,
                        ScoreRangeKind::AtMost(maximum) => value <= maximum,
                        ScoreRangeKind::Between { min, max } => (min..=max).contains(&value),
                    }
                }
                Condition::ScoreCompare(left, comparison, right) => {
                    let left = self.score(left);
                    let right = self.score(right);
                    match comparison {
                        ScoreComparison::Equal => left == right,
                        ScoreComparison::LessThan => left < right,
                        ScoreComparison::LessOrEqual => left <= right,
                        ScoreComparison::GreaterThan => left > right,
                        ScoreComparison::GreaterOrEqual => left >= right,
                    }
                }
                _ => panic!("loop lowering emitted an untested condition"),
            }
        }

        fn score(&self, score: &ScoreRef) -> i32 {
            *self.scores.get(score).unwrap_or(&0)
        }
    }

    impl Flow {
        const fn outcome(self) -> Outcome {
            match self {
                Self::Continue(outcome) | Self::Return(outcome) => outcome,
            }
        }
    }

    fn score_ref(selection: &ScoreSelection) -> ScoreRef {
        let ScoreHolders::Fake(holder) = selection.holders() else {
            panic!("lowered loop used a selector score");
        };
        ScoreRef::new(
            SingleScoreHolder::Fake(holder.clone()),
            selection.objective().clone(),
        )
    }
}
