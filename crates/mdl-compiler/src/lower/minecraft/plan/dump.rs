use std::fmt::Write;

use crate::entity::EntityId;
use crate::ir::core::FunctionId;
use crate::lower::minecraft::MinecraftOptimizationLevel;

use super::{EdgeTransfer, FunctionLayout, InstructionPlan, LoweringPlan, ScalarResultPlacement};

impl LoweringPlan {
    pub(crate) fn dump(&self) -> String {
        let mut output = String::new();
        writeln!(output, "minecraft-plan {:?}", self.target).unwrap();
        if self.optimization_level != MinecraftOptimizationLevel::None {
            writeln!(output, "optimization-level {:?}", self.optimization_level).unwrap();
        }
        writeln!(output, "namespace {}", self.namespace).unwrap();
        writeln!(output, "objective {}", self.pack_abi.register_objective).unwrap();
        writeln!(output, "sentinel {:?}", self.pack_abi.init_sentinel).unwrap();
        for (home, data) in self.homes.iter() {
            writeln!(
                output,
                "home {} {} {:?}",
                home.index(),
                data.holder,
                data.role
            )
            .unwrap();
        }
        for (planned, data) in self.target_functions.iter() {
            writeln!(
                output,
                "target-function {} {} {:?} {:?}",
                planned.index(),
                data.resource,
                data.role,
                data.origin
            )
            .unwrap();
        }
        for (function, layout) in self.functions.iter() {
            dump_function(&mut output, self, function, layout);
        }
        output
    }
}

fn dump_function(
    output: &mut String,
    plan: &LoweringPlan,
    function: FunctionId,
    layout: &FunctionLayout,
) {
    writeln!(
        output,
        "function {} name={:?} params={:?} results={:?}",
        function.index(),
        layout.diagnostic_name_hint,
        layout.parameter_types,
        layout.result_types
    )
    .unwrap();
    writeln!(
        output,
        "  abi entry={} params={:?} results={:?} temp={:?}",
        layout.abi.entry_block.index(),
        layout.abi.parameters,
        layout.abi.results,
        layout.parallel_copy_temp
    )
    .unwrap();
    for (block, planned) in layout.block_functions.iter().enumerate() {
        if let Some(planned) = planned {
            writeln!(output, "  block {block} -> {}", planned.index()).unwrap();
        }
    }
    for (value, home) in layout.value_homes.iter().enumerate() {
        if let Some(home) = home {
            writeln!(output, "  value {value} -> {}", home.index()).unwrap();
        }
    }
    if plan.optimization_level != MinecraftOptimizationLevel::None {
        dump_baseline_function_details(output, plan, layout);
    }
    for (block, transfer) in layout.edge_transfers.iter().enumerate() {
        let Some(transfer) = transfer else { continue };
        match transfer {
            EdgeTransfer::Jump { steps } => {
                writeln!(output, "  edge {block} jump").unwrap();
                dump_steps(output, steps);
            }
            EdgeTransfer::Branch {
                then_edge,
                else_edge,
            } => {
                writeln!(output, "  edge {block} then helper={:?}", then_edge.helper).unwrap();
                dump_steps(output, &then_edge.steps);
                writeln!(output, "  edge {block} else helper={:?}", else_edge.helper).unwrap();
                dump_steps(output, &else_edge.steps);
            }
        }
    }
}

fn dump_baseline_function_details(
    output: &mut String,
    plan: &LoweringPlan,
    layout: &FunctionLayout,
) {
    for home in &layout.edge_temporaries {
        let data = plan
            .homes
            .get(*home)
            .expect("verified plan dump has every edge temporary home");
        if let Some(ty) = plan.home_type(*home) {
            writeln!(
                output,
                "  edge-temporary home={} holder={} type={ty:?}",
                home.index(),
                data.holder
            )
            .unwrap();
        } else {
            writeln!(
                output,
                "  edge-temporary home={} holder={} type=untyped",
                home.index(),
                data.holder
            )
            .unwrap();
        }
    }
    for (instruction, instruction_plan) in layout.instruction_plans.iter().enumerate() {
        if let Some(instruction_plan) = instruction_plan {
            dump_instruction(output, instruction, instruction_plan);
        }
    }
}

fn dump_instruction(output: &mut String, instruction: usize, plan: &InstructionPlan) {
    match plan {
        InstructionPlan::OmittedPure => {
            writeln!(output, "  instruction {instruction} omitted-pure").unwrap();
        }
        InstructionPlan::Scalar { operands, results } => {
            writeln!(output, "  instruction {instruction} scalar").unwrap();
            for (operand_index, home) in operands.iter().enumerate() {
                writeln!(output, "    operand {operand_index} home={}", home.index()).unwrap();
            }
            for result in results {
                match *result {
                    ScalarResultPlacement::Semantic {
                        result_index,
                        value,
                        home,
                    } => {
                        writeln!(
                            output,
                            "    result {result_index} semantic value={} home={}",
                            value.index(),
                            home.index()
                        )
                        .unwrap();
                    }
                    ScalarResultPlacement::RecipeTemporary { result_index, home } => {
                        writeln!(
                            output,
                            "    result {result_index} recipe-temporary home={}",
                            home.index()
                        )
                        .unwrap();
                    }
                }
            }
        }
        InstructionPlan::Call {
            arguments,
            result_destinations,
        } => {
            writeln!(output, "  instruction {instruction} call").unwrap();
            for (argument_index, home) in arguments.iter().enumerate() {
                writeln!(
                    output,
                    "    argument {argument_index} home={}",
                    home.index()
                )
                .unwrap();
            }
            for (slot, destination) in result_destinations.iter().enumerate() {
                if let Some(destination) = destination {
                    writeln!(
                        output,
                        "    result-slot {slot} semantic result={} value={} home={}",
                        destination.result_index(),
                        destination.value().index(),
                        destination.home().index()
                    )
                    .unwrap();
                } else {
                    writeln!(output, "    result-slot {slot} omitted").unwrap();
                }
            }
        }
    }
}

fn dump_steps(output: &mut String, steps: &[super::MoveStep]) {
    for step in steps {
        writeln!(
            output,
            "    move {} <- {}",
            step.destination.index(),
            step.source.index()
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use crate::analysis::minecraft::CommandLimitAssumptions;
    use crate::entity::{EntityId, EntityVec};
    use crate::ir::minecraft::{ObjectiveName, PackNamespace};
    use crate::lower::minecraft::{LoweringOptions, MinecraftOptimizationLevel};
    use crate::target::JavaEditionTarget;

    use super::super::{LoweringPlan, PackAbi, PlannedFunctionId};

    #[test]
    fn empty_none_dump_retains_the_legacy_text_exactly() {
        let plan = empty_plan(MinecraftOptimizationLevel::None);
        let expected = format!(
            concat!(
                "minecraft-plan V26_2\n",
                "namespace mdl\n",
                "objective mdl.reg\n",
                "sentinel {:?}\n",
            ),
            plan.pack_abi.init_sentinel
        );
        assert_eq!(plan.dump(), expected);
    }

    #[test]
    fn baseline_dump_names_the_selected_policy() {
        let plan = empty_plan(MinecraftOptimizationLevel::Baseline);
        assert!(
            plan.dump()
                .starts_with("minecraft-plan V26_2\noptimization-level Baseline\n")
        );
    }

    fn empty_plan(optimization_level: MinecraftOptimizationLevel) -> LoweringPlan {
        let target = JavaEditionTarget::V26_2;
        let options = LoweringOptions::new(
            target,
            PackNamespace::new("mdl").unwrap(),
            ObjectiveName::new("mdl.reg").unwrap(),
        )
        .unwrap()
        .with_optimization_level(optimization_level);
        LoweringPlan {
            optimization_level,
            target,
            command_limit_assumptions: CommandLimitAssumptions::for_target(target),
            namespace: options.namespace().clone(),
            pack_abi: PackAbi {
                register_objective: options.register_objective().clone(),
                init_sentinel: options.generated_names().init_sentinel(),
            },
            load: PlannedFunctionId::from_index(0),
            init_try_create: PlannedFunctionId::from_index(1),
            homes: EntityVec::new(),
            target_functions: EntityVec::new(),
            functions: EntityVec::new(),
        }
    }
}
