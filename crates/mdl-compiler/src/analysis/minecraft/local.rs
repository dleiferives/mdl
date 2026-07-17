use crate::ir::minecraft::{
    CallableRef, Cardinality, CommandKind, Condition, ExecuteModifierKind, InternalCallableRef,
    MinecraftProgram, ReturnCommand,
};

use super::{
    CommandOutcome, CommandStepCost, CommandStepCounts, CountBound, CountUpperKind,
    FunctionLocalSummary, InternalCallRole, InternalCallSite, NoFiniteBoundReason,
    ReturnValueClass, UnknownCostReason,
};

pub(super) fn summarize_functions(program: &MinecraftProgram) -> Option<Vec<FunctionLocalSummary>> {
    program
        .functions()
        .map(|(function, data)| {
            let steps = data
                .body()
                .commands()
                .map(|(_, command)| classify_step(command.kind()))
                .collect::<Option<Vec<_>>>()?;
            Some(FunctionLocalSummary::new(function, steps, vec![]))
        })
        .collect()
}

fn classify_step(command: &CommandKind) -> Option<CommandStepCost> {
    match command {
        CommandKind::Score(_) | CommandKind::Data(_) => Some(simple_step(
            CommandStepCounts::new(1, 0, 0, 1),
            vec![CommandOutcome::Continue],
        )),
        CommandKind::Say(_) => Some(simple_step(
            CommandStepCounts::new(1, 0, 0, 0),
            vec![CommandOutcome::Continue],
        )),
        CommandKind::Teleport(_) => Some(simple_step(
            CommandStepCounts::new(1, 0, 0, 0),
            vec![
                CommandOutcome::Continue,
                CommandOutcome::NoResult,
                CommandOutcome::Fail,
            ],
        )),
        CommandKind::Raw(_) => Some(unknown_step(
            UnknownCostReason::RawCommand,
            vec![
                CommandOutcome::Continue,
                CommandOutcome::Return(ReturnValueClass::UnknownInteger),
                CommandOutcome::NoResult,
                CommandOutcome::Fail,
            ],
        )),
        CommandKind::Function(call) => Some(classify_call(call.target())),
        CommandKind::Return(ReturnCommand::Value(value)) => Some(simple_step(
            CommandStepCounts::default(),
            vec![CommandOutcome::Return(if *value == 0 {
                ReturnValueClass::Zero
            } else {
                ReturnValueClass::NonZero
            })],
        )),
        CommandKind::Return(ReturnCommand::Fail) => Some(simple_step(
            CommandStepCounts::default(),
            vec![CommandOutcome::Fail],
        )),
        CommandKind::Return(ReturnCommand::Run(nested)) => classify_return_run(nested.kind()),
        CommandKind::Execute(execute) => classify_execute(execute),
    }
}

/// Classifies one already-constructed command with the same local algebra used by
/// whole-target analysis. Lowering uses this only to reconcile closed recipe output;
/// candidate selection never runs whole-program analysis.
pub(crate) fn classify_constructed_command(command: &CommandKind) -> Option<CommandStepCost> {
    classify_step(command)
}

fn simple_step(counts: CommandStepCounts, outcomes: Vec<CommandOutcome>) -> CommandStepCost {
    CommandStepCost::new(counts, CountBound::exact(0), outcomes, vec![], None)
}

fn unknown_step(reason: UnknownCostReason, outcomes: Vec<CommandOutcome>) -> CommandStepCost {
    CommandStepCost::new(
        CommandStepCounts::default(),
        CountBound::unknown(0, reason),
        outcomes,
        vec![],
        Some(reason),
    )
}

fn classify_call(target: &CallableRef) -> CommandStepCost {
    let (counts, edge, unknown) = match target {
        CallableRef::Internal(InternalCallableRef::Function(target)) => (
            CommandStepCounts::new(1, 0, 1, 0),
            Some(InternalCallSite::Function {
                target: *target,
                role: InternalCallRole::Ordinary,
            }),
            None,
        ),
        CallableRef::Internal(InternalCallableRef::Tag(target)) => (
            CommandStepCounts::default(),
            Some(InternalCallSite::Tag {
                target: *target,
                role: InternalCallRole::Ordinary,
            }),
            None,
        ),
        CallableRef::External(crate::ir::minecraft::ExternalCallableRef::Function(_)) => (
            CommandStepCounts::new(1, 0, 0, 0),
            None,
            Some(UnknownCostReason::ExternalFunction),
        ),
        CallableRef::External(crate::ir::minecraft::ExternalCallableRef::Tag(_)) => (
            CommandStepCounts::default(),
            None,
            Some(UnknownCostReason::ExternalFunctionTag),
        ),
    };
    CommandStepCost::new(
        counts,
        unknown.map_or(CountBound::exact(0), |reason| {
            CountBound::unknown(0, reason)
        }),
        vec![CommandOutcome::Continue, CommandOutcome::NoResult],
        edge.into_iter().collect(),
        unknown,
    )
}

fn classify_return_run(nested: &CommandKind) -> Option<CommandStepCost> {
    let exact_native_result = nested.contract().native_outcome().exact_result();
    let nested = classify_step(nested)?;
    let internal_calls = nested
        .internal_call_sites()
        .iter()
        .copied()
        .map(|edge| match edge {
            InternalCallSite::Function {
                target,
                role: InternalCallRole::Ordinary,
            } => InternalCallSite::Function {
                target,
                role: InternalCallRole::ReturnRun,
            },
            InternalCallSite::Tag {
                target,
                role: InternalCallRole::Ordinary,
            } => InternalCallSite::Tag {
                target,
                role: InternalCallRole::ReturnRun,
            },
            edge => edge,
        })
        .collect();
    let mut outcomes = vec![];
    for outcome in nested.outcomes() {
        match outcome {
            CommandOutcome::Continue => {
                if let Some(result) = exact_native_result {
                    outcomes.push(CommandOutcome::Return(if result == 0 {
                        ReturnValueClass::Zero
                    } else {
                        ReturnValueClass::NonZero
                    }));
                } else {
                    outcomes.push(CommandOutcome::Return(ReturnValueClass::UnknownInteger));
                    outcomes.push(CommandOutcome::Fail);
                }
            }
            CommandOutcome::Return(value) => outcomes.push(CommandOutcome::Return(*value)),
            CommandOutcome::NoResult | CommandOutcome::Fail => {
                outcomes.push(CommandOutcome::Fail);
            }
        }
    }
    outcomes.sort_unstable();
    outcomes.dedup();
    Some(CommandStepCost::new(
        nested.counts(),
        nested.maximum_chain_expansion(),
        outcomes,
        internal_calls,
        nested.intrinsic_unknown_reason(),
    ))
}

fn classify_execute(execute: &crate::ir::minecraft::ExecuteCommand) -> Option<CommandStepCost> {
    let nested = classify_step(execute.run().kind())?;
    let stage_count = u64::try_from(execute.modifiers().len()).ok()?;
    let mut sequence_stages = stage_count;
    let mut counts;
    let mut calls = vec![];
    let mut condition_calls = 0u64;
    let mut filters = false;
    let mut active_contexts = CountBound::exact(1);
    let mut checked_peak = CountBound::exact(0);
    for modifier in execute.modifiers().as_slice() {
        match modifier.kind() {
            ExecuteModifierKind::As(selector) | ExecuteModifierKind::At(selector) => {
                filters = true;
                active_contexts =
                    apply_selector_cardinality(active_contexts, selector.cardinality());
                checked_peak = maximum_bound(checked_peak, active_contexts);
            }
            ExecuteModifierKind::If(condition) | ExecuteModifierKind::Unless(condition) => {
                filters = true;
                active_contexts = allow_empty(active_contexts);
                if let Condition::Function(target) = condition {
                    sequence_stages = sequence_stages.checked_sub(1)?;
                    condition_calls = condition_calls.checked_add(1)?;
                    calls.push(InternalCallSite::Function {
                        target: *target,
                        role: InternalCallRole::Condition,
                    });
                } else {
                    checked_peak = maximum_bound(checked_peak, active_contexts);
                }
            }
            ExecuteModifierKind::In(_)
            | ExecuteModifierKind::Positioned(_)
            | ExecuteModifierKind::Rotated(_)
            | ExecuteModifierKind::Anchored(_)
            | ExecuteModifierKind::Align(_)
            | ExecuteModifierKind::Store(_, _) => {
                checked_peak = maximum_bound(checked_peak, active_contexts);
            }
        }
    }
    counts =
        nested
            .counts()
            .checked_add(CommandStepCounts::new(sequence_stages, stage_count, 0, 0))?;
    calls.extend_from_slice(nested.internal_call_sites());
    counts = counts.checked_add(CommandStepCounts::new(
        condition_calls,
        0,
        condition_calls,
        0,
    ))?;
    let expansion = maximum_bound(checked_peak, nested.maximum_chain_expansion());
    let mut outcomes = nested.outcomes().to_vec();
    if filters && !outcomes.contains(&CommandOutcome::NoResult) {
        outcomes.push(CommandOutcome::NoResult);
    }
    Some(CommandStepCost::new(
        counts,
        expansion,
        outcomes,
        calls,
        nested.intrinsic_unknown_reason(),
    ))
}

fn apply_selector_cardinality(active: CountBound, cardinality: Cardinality) -> CountBound {
    let Some(selector_maximum) = cardinality.maximum() else {
        return CountBound::no_finite_bound(0, NoFiniteBoundReason::SelectorCardinality);
    };
    match active.upper().kind() {
        CountUpperKind::Finite(active_maximum) => {
            match active_maximum.checked_mul(u64::from(selector_maximum)) {
                Some(maximum) => CountBound::finite(0, maximum)
                    .expect("selector multiplication keeps zero below its finite upper bound"),
                None => CountBound::saturated_above_analysis_cap(0),
            }
        }
        CountUpperKind::AboveAnalysisCap => CountBound::saturated_above_analysis_cap(0),
        CountUpperKind::NoFiniteBoundProven(reason) => CountBound::no_finite_bound(0, reason),
        CountUpperKind::Unknown(reason) => CountBound::unknown(0, reason),
    }
}

fn allow_empty(bound: CountBound) -> CountBound {
    match bound.upper().kind() {
        CountUpperKind::Finite(upper) => {
            CountBound::finite(0, upper).expect("zero remains below a finite context upper bound")
        }
        CountUpperKind::AboveAnalysisCap => CountBound::saturated_above_analysis_cap(0),
        CountUpperKind::NoFiniteBoundProven(reason) => CountBound::no_finite_bound(0, reason),
        CountUpperKind::Unknown(reason) => CountBound::unknown(0, reason),
    }
}

fn maximum_bound(left: CountBound, right: CountBound) -> CountBound {
    let lower = left.lower().max(right.lower());
    match (left.upper().kind(), right.upper().kind()) {
        (CountUpperKind::Unknown(reason), _) | (_, CountUpperKind::Unknown(reason)) => {
            CountBound::unknown(lower, reason)
        }
        (CountUpperKind::NoFiniteBoundProven(reason), _)
        | (_, CountUpperKind::NoFiniteBoundProven(reason)) => {
            CountBound::no_finite_bound(lower, reason)
        }
        (CountUpperKind::AboveAnalysisCap, _) | (_, CountUpperKind::AboveAnalysisCap) => {
            // Only solver arithmetic creates capped bounds; local syntax cannot reach this arm.
            CountBound::unknown(lower, UnknownCostReason::AnalysisLimit)
        }
        (CountUpperKind::Finite(left), CountUpperKind::Finite(right)) => {
            CountBound::finite(lower, left.max(right)).expect("maximum preserves range ordering")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityId;
    use crate::ir::minecraft::{
        CommandKind, CommandNode, Condition, EntitySelector, ExecuteCommand, ExecuteModifier,
        ExecuteModifierKind, ExecuteModifiers, FunctionCall, FunctionResourceId, FunctionTagEntry,
        FunctionTagMerge, FunctionTagResourceId, InternalCallableRef, MinecraftProgramBuilder,
        ReturnCommand, SayCommand, SayMessage, UnboundedSelector, UnsafeRawCommand,
    };
    use crate::source::OriginId;
    use crate::target::JavaEditionTarget;

    use super::*;

    #[test]
    fn local_classifier_multiplies_bounded_selector_prefixes() {
        let as_two = ExecuteModifier::new(
            ExecuteModifierKind::As(
                EntitySelector::armor_stands(vec![], Some(2))
                    .unwrap()
                    .into(),
            ),
            OriginId::UNKNOWN,
        );
        let at_three = ExecuteModifier::new(
            ExecuteModifierKind::At(
                EntitySelector::armor_stands(vec![], Some(3))
                    .unwrap()
                    .into(),
            ),
            OriginId::UNKNOWN,
        );
        let nested = CommandNode::new(
            CommandKind::Return(ReturnCommand::Value(1)),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let execute = ExecuteCommand::new(ExecuteModifiers::new(as_two, vec![at_three]), nested);

        let cost = classify_execute(&execute).unwrap();
        assert_eq!(cost.maximum_chain_expansion().lower(), 0);
        assert_eq!(
            cost.maximum_chain_expansion().upper().kind(),
            CountUpperKind::Finite(6)
        );
    }

    #[test]
    fn say_is_one_known_command_and_return_run_projects_exact_native_result() {
        let say = || CommandKind::Say(SayCommand::new(SayMessage::new("hello").unwrap()));

        let direct = classify_step(&say()).unwrap();
        assert_eq!(direct.counts().sequence_operations(), 1);
        assert_eq!(direct.counts().execute_stages(), 0);
        assert_eq!(direct.counts().internal_function_invocations(), 0);
        assert_eq!(direct.counts().score_nbt_command_executions(), 0);
        assert_eq!(direct.outcomes(), &[CommandOutcome::Continue]);
        assert_eq!(direct.intrinsic_unknown_reason(), None);

        let returned = classify_return_run(&say()).unwrap();
        assert_eq!(
            returned.outcomes(),
            &[CommandOutcome::Return(ReturnValueClass::NonZero)]
        );
    }

    #[test]
    fn local_classifier_preserves_structural_outcomes_without_inventing_exit_costs() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:local").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let returned = CommandNode::new(
            CommandKind::Return(ReturnCommand::Value(1)),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let execute = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(
                        ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
                        OriginId::UNKNOWN,
                    ),
                    vec![],
                ),
                returned,
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let raw = CommandNode::new(
            CommandKind::Raw(UnsafeRawCommand::new("say unknown").unwrap()),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(execute).unwrap();
        body.push(raw).unwrap();
        body.finish();

        let summaries = summarize_functions(&builder.finish().unwrap()).unwrap();
        let summary = &summaries[0];
        assert_eq!(summary.steps().len(), 2);
        assert_eq!(
            summary.steps()[0].maximum_chain_expansion().upper().kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality)
        );
        assert!(summary.exits().is_empty());
        assert!(
            summary.steps()[0]
                .outcomes()
                .contains(&CommandOutcome::Return(ReturnValueClass::NonZero))
        );
        assert!(
            summary.steps()[0]
                .outcomes()
                .contains(&CommandOutcome::NoResult)
        );
        assert!(summary.steps()[1].intrinsic_unknown_reason().is_some());
    }

    #[test]
    fn local_classifier_retains_ordered_duplicate_typed_call_sites() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let caller = builder
            .declare_function(
                FunctionResourceId::parse("mdl:site_caller").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let predicate = builder
            .declare_function(
                FunctionResourceId::parse("mdl:site_predicate").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:site_tag").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let mut entries = builder.begin_function_tag(tag).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(predicate),
            OriginId::UNKNOWN,
        ));
        entries.finish();

        let run = CommandNode::new(
            CommandKind::Function(FunctionCall::new(InternalCallableRef::Tag(tag).into())),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let execute = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(
                        ExecuteModifierKind::If(Condition::Function(predicate)),
                        OriginId::UNKNOWN,
                    ),
                    vec![ExecuteModifier::new(
                        ExecuteModifierKind::Unless(Condition::Function(predicate)),
                        OriginId::UNKNOWN,
                    )],
                ),
                run,
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let mut body = builder.begin_function(caller).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(execute)),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let mut body = builder.begin_function(predicate).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::Value(1)),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();

        let summaries = summarize_functions(&builder.finish().unwrap()).unwrap();
        assert_eq!(
            summaries[usize::try_from(caller.index()).unwrap()].steps()[0].internal_call_sites(),
            &[
                InternalCallSite::Function {
                    target: predicate,
                    role: InternalCallRole::Condition,
                },
                InternalCallSite::Function {
                    target: predicate,
                    role: InternalCallRole::Condition,
                },
                InternalCallSite::Tag {
                    target: tag,
                    role: InternalCallRole::ReturnRun,
                },
            ]
        );
    }
}
