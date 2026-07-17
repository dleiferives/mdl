use crate::entity::EntityId;
use crate::ir::minecraft::{McFunctionId, MinecraftProgram};
use crate::source::SourceContext;

use super::cost::RootExecutionBounds;
use super::graph::{ExecutionGraph, GraphBuildError, TagRootEntry};
use super::local::summarize_functions;
use super::report::{TargetExecutionAnalysisStatsInput, TargetExecutionReportContext};
use super::solve::{MetricSet, add_root_invocation, replace_local_outcomes, solve};
use super::{
    CommandLimitAssumptions, CountBound, ResolvedTargetExecutionRoot, RootEntryRegion,
    RootExecutionSummary, TargetExecutionAnalysisCompletion, TargetExecutionAnalysisFailure,
    TargetExecutionAnalysisLimitKind, TargetExecutionAnalysisLimits, TargetExecutionAnalysisPhase,
    TargetExecutionAnalysisStats, TargetExecutionCensus, TargetExecutionCostReport,
    TargetExecutionRoot, UnknownCostReason,
};

/// Verifies and explicitly analyzes one immutable structured target.
///
/// Configured work-limit exhaustion returns a successful conservative report. A
/// malformed target or an internal checked-construction contradiction returns no
/// partial report and never mutates the target.
///
/// # Errors
///
/// Returns a phase-aware failure when target verification or required checked
/// construction cannot complete.
pub fn analyze_target_execution(
    program: &MinecraftProgram,
    sources: &SourceContext,
    roots: &[TargetExecutionRoot],
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
) -> Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure> {
    crate::ir::minecraft::verify_program(program, sources)
        .map_err(TargetExecutionAnalysisFailure::verification)?;
    analyze_verified_target_execution(program, roots, assumptions, limits)
}

/// Analyzes a target already verified by the trusted lowering boundary.
pub(crate) fn analyze_verified_target_execution(
    program: &MinecraftProgram,
    roots: &[TargetExecutionRoot],
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
) -> Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure> {
    validate_analysis_caps(assumptions, limits)?;
    let census = TargetExecutionCensus::from_program(program).ok_or_else(|| {
        TargetExecutionAnalysisFailure::internal(
            TargetExecutionAnalysisPhase::Construction,
            "target-cost.census-overflow",
            "structured target census exceeded the host index domain",
        )
    })?;
    let base_work = graph_base_entities(census).ok_or_else(|| {
        TargetExecutionAnalysisFailure::internal(
            TargetExecutionAnalysisPhase::Construction,
            "target-cost.graph-size-overflow",
            "target-cost graph inventory exceeded the host index domain",
        )
    })?;
    let mut locals = summarize_functions(program).ok_or_else(|| {
        TargetExecutionAnalysisFailure::internal(
            TargetExecutionAnalysisPhase::Construction,
            "target-cost.local-overflow",
            "local target-cost construction exceeded checked arithmetic",
        )
    })?;
    if base_work > limits.graph_entities() {
        for local in &mut locals {
            local.mark_analysis_limit();
        }
        return early_graph_limit_report(
            program,
            roots,
            assumptions,
            limits,
            census,
            base_work,
            locals,
        );
    }
    let graph = match ExecutionGraph::build(program, &locals, limits.graph_entities() - base_work) {
        Ok(graph) => graph,
        Err(GraphBuildError::AnalysisLimit) => {
            for local in &mut locals {
                local.mark_analysis_limit();
            }
            return early_graph_limit_report(
                program,
                roots,
                assumptions,
                limits,
                census,
                limits.graph_entities().saturating_add(1),
                locals,
            );
        }
        Err(error) => return Err(graph_failure(error)),
    };
    finish_verified_analysis(program, roots, assumptions, limits, census, locals, &graph)
}

fn finish_verified_analysis(
    program: &MinecraftProgram,
    roots: &[TargetExecutionRoot],
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
    census: TargetExecutionCensus,
    mut locals: Vec<super::FunctionLocalSummary>,
    graph: &ExecutionGraph,
) -> Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure> {
    let local_command_transfer_visits =
        replace_local_outcomes(program, graph, &mut locals, limits.arithmetic()).ok_or_else(
            || {
                TargetExecutionAnalysisFailure::internal(
                    TargetExecutionAnalysisPhase::Invariant,
                    "target-cost.local-transfer-invariant",
                    "outcome-sensitive local transfer construction encountered inconsistent state",
                )
            },
        )?;
    let graph_work = graph_entity_count(census, graph)?;
    if graph_work > limits.graph_entities() {
        return Err(TargetExecutionAnalysisFailure::internal(
            TargetExecutionAnalysisPhase::Invariant,
            "target-cost.graph-budget-invariant",
            "completed target-cost graph exceeded its checked construction budget",
        ));
    }
    let solved = solve(
        program,
        graph,
        &locals,
        limits.arithmetic(),
        limits.solver_updates(),
    )
    .ok_or_else(|| {
        TargetExecutionAnalysisFailure::internal(
            TargetExecutionAnalysisPhase::Invariant,
            "target-cost.solve-invariant",
            "target-cost region solver encountered inconsistent checked state",
        )
    })?;
    let completion = if solved.complete {
        TargetExecutionAnalysisCompletion::Complete
    } else {
        TargetExecutionAnalysisCompletion::Incomplete(
            TargetExecutionAnalysisLimitKind::SolverUpdates,
        )
    };
    let root_summaries = resolve_roots(
        program,
        graph,
        &solved.functions,
        roots,
        assumptions,
        limits,
    )?;
    let stats = TargetExecutionAnalysisStats::new(TargetExecutionAnalysisStatsInput {
        graph_entities: graph_work,
        graph_edges: graph.retained_edges,
        tag_expansion_entries: graph.tag_expansion_entries,
        solver_updates: solved.updates,
        command_transfer_visits: local_command_transfer_visits
            .checked_add(solved.command_transfer_visits)
            .ok_or_else(|| {
                TargetExecutionAnalysisFailure::internal(
                    TargetExecutionAnalysisPhase::Construction,
                    "target-cost.command-transfer-visit-overflow",
                    "target-cost command-transfer visit count exceeded the host index domain",
                )
            })?,
        retained_functions: locals.len(),
        retained_regions: solved.regions.len(),
        retained_roots: root_summaries.len(),
    });
    Ok(TargetExecutionCostReport::new(
        TargetExecutionReportContext::new(program.target(), assumptions, limits, completion),
        census,
        stats,
        locals,
        solved.regions,
        root_summaries,
    ))
}

fn graph_entity_count(
    census: TargetExecutionCensus,
    graph: &ExecutionGraph,
) -> Result<usize, TargetExecutionAnalysisFailure> {
    census
        .command_nodes()
        .checked_add(census.functions())
        .and_then(|value| value.checked_add(census.function_tags()))
        .and_then(|value| value.checked_add(census.function_tag_entries()))
        .and_then(|value| value.checked_add(graph.retained_edges))
        .and_then(|value| value.checked_add(graph.tag_expansion_entries))
        .ok_or_else(|| {
            TargetExecutionAnalysisFailure::internal(
                TargetExecutionAnalysisPhase::Construction,
                "target-cost.graph-size-overflow",
                "target-cost graph inventory exceeded the host index domain",
            )
        })
}

fn validate_analysis_caps(
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
) -> Result<(), TargetExecutionAnalysisFailure> {
    if limits.arithmetic().sequence() <= u64::from(assumptions.max_command_sequence_length())
        || limits.arithmetic().forks() <= u64::from(assumptions.max_command_forks())
    {
        Err(TargetExecutionAnalysisFailure::internal(
            TargetExecutionAnalysisPhase::Configuration,
            "target-cost.inconsistent-caps",
            "analysis arithmetic caps must exceed the supplied hard-limit assumptions",
        ))
    } else {
        Ok(())
    }
}

fn graph_base_entities(census: TargetExecutionCensus) -> Option<usize> {
    census
        .command_nodes()
        .checked_add(census.functions())?
        .checked_add(census.function_tags())?
        .checked_add(census.function_tag_entries())
}

fn early_graph_limit_report(
    program: &MinecraftProgram,
    roots: &[TargetExecutionRoot],
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
    census: TargetExecutionCensus,
    preflight_work: usize,
    locals: Vec<super::FunctionLocalSummary>,
) -> Result<TargetExecutionCostReport, TargetExecutionAnalysisFailure> {
    let mut summaries = Vec::with_capacity(roots.len());
    for root in roots {
        match root {
            TargetExecutionRoot::Function(function) => {
                if program.function(*function).is_none() {
                    return Err(invalid_root("function", function.index()));
                }
                let metrics = add_root_invocation(
                    MetricSet::unknown(UnknownCostReason::AnalysisLimit),
                    limits.arithmetic(),
                )
                .ok_or_else(|| {
                    TargetExecutionAnalysisFailure::internal(
                        TargetExecutionAnalysisPhase::Construction,
                        "target-cost.root-arithmetic-overflow",
                        "root invocation accounting exceeded checked arithmetic",
                    )
                })?;
                summaries.push(RootExecutionSummary::new(
                    ResolvedTargetExecutionRoot::Function(*function),
                    RootEntryRegion::AnalysisLimit,
                    RootExecutionBounds::new(
                        metrics.sequence,
                        metrics.execute,
                        metrics.calls,
                        metrics.score_nbt,
                        metrics.max_chain,
                    ),
                    assumptions,
                ));
            }
            TargetExecutionRoot::FunctionTag(tag) => {
                if program.function_tag(*tag).is_none() {
                    return Err(invalid_root("function tag", tag.index()));
                }
                let unknown = CountBound::unknown(0, UnknownCostReason::AnalysisLimit);
                summaries.push(RootExecutionSummary::new(
                    ResolvedTargetExecutionRoot::UnresolvedAnalysisLimitFunctionTag { tag: *tag },
                    RootEntryRegion::AnalysisLimit,
                    RootExecutionBounds::repeated(unknown),
                    assumptions,
                ));
            }
        }
    }
    let stats = TargetExecutionAnalysisStats::new(TargetExecutionAnalysisStatsInput {
        graph_entities: preflight_work,
        graph_edges: 0,
        tag_expansion_entries: 0,
        solver_updates: 0,
        command_transfer_visits: 0,
        retained_functions: locals.len(),
        retained_regions: 0,
        retained_roots: summaries.len(),
    });
    Ok(TargetExecutionCostReport::new(
        TargetExecutionReportContext::new(
            program.target(),
            assumptions,
            limits,
            TargetExecutionAnalysisCompletion::Incomplete(
                TargetExecutionAnalysisLimitKind::GraphEntities,
            ),
        ),
        census,
        stats,
        locals,
        vec![],
        summaries,
    ))
}

fn resolve_roots(
    program: &MinecraftProgram,
    graph: &ExecutionGraph,
    functions: &[MetricSet],
    roots: &[TargetExecutionRoot],
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
) -> Result<Vec<RootExecutionSummary>, TargetExecutionAnalysisFailure> {
    let mut output = vec![];
    for root in roots {
        match root {
            TargetExecutionRoot::Function(function) => output.push(internal_root(
                ResolvedTargetExecutionRoot::Function(*function),
                *function,
                graph,
                functions,
                assumptions,
                limits,
            )?),
            TargetExecutionRoot::FunctionTag(tag) => {
                if program.function_tag(*tag).is_none() {
                    return Err(invalid_root("function tag", tag.index()));
                }
                let tag_index = usize::try_from(tag.index())
                    .map_err(|_| invalid_root("function tag", tag.index()))?;
                let expansion = graph
                    .tag_expansions
                    .get(tag_index)
                    .ok_or_else(|| invalid_root("function tag", tag.index()))?;
                for (entry_index, entry) in expansion.root_entries.iter().enumerate() {
                    let entry_index = u32::try_from(entry_index).map_err(|_| {
                        TargetExecutionAnalysisFailure::internal(
                            TargetExecutionAnalysisPhase::Construction,
                            "target-cost.root-count-overflow",
                            "resolved function-tag root count exceeded the identity domain",
                        )
                    })?;
                    match entry {
                        TagRootEntry::Function(function) => output.push(internal_root(
                            ResolvedTargetExecutionRoot::FunctionTagFunction {
                                tag: *tag,
                                entry_index,
                                function: *function,
                            },
                            *function,
                            graph,
                            functions,
                            assumptions,
                            limits,
                        )?),
                        TagRootEntry::External {
                            target,
                            requirement,
                        } => output.push(RootExecutionSummary::new(
                            ResolvedTargetExecutionRoot::UnresolvedExternalTagEntry {
                                tag: *tag,
                                entry_index,
                                target: target.clone(),
                                requirement: *requirement,
                            },
                            RootEntryRegion::ExternalTagEntry,
                            RootExecutionBounds::repeated(CountBound::unknown(
                                0,
                                UnknownCostReason::ExternalTagEntry,
                            )),
                            assumptions,
                        )),
                    }
                }
            }
        }
    }
    Ok(output)
}

fn internal_root(
    root: ResolvedTargetExecutionRoot,
    function: McFunctionId,
    graph: &ExecutionGraph,
    functions: &[MetricSet],
    assumptions: CommandLimitAssumptions,
    limits: TargetExecutionAnalysisLimits,
) -> Result<RootExecutionSummary, TargetExecutionAnalysisFailure> {
    let index = usize::try_from(function.index())
        .ok()
        .filter(|index| *index < functions.len())
        .ok_or_else(|| invalid_root("function", function.index()))?;
    let region = *graph
        .function_regions
        .get(index)
        .ok_or_else(|| invalid_root("function", function.index()))?;
    let metrics = add_root_invocation(functions[index], limits.arithmetic()).ok_or_else(|| {
        TargetExecutionAnalysisFailure::internal(
            TargetExecutionAnalysisPhase::Construction,
            "target-cost.root-arithmetic-overflow",
            "root invocation accounting exceeded checked arithmetic",
        )
    })?;
    Ok(RootExecutionSummary::new(
        root,
        RootEntryRegion::Internal(region),
        RootExecutionBounds::new(
            metrics.sequence,
            metrics.execute,
            metrics.calls,
            metrics.score_nbt,
            metrics.max_chain,
        ),
        assumptions,
    ))
}

fn invalid_root(kind: &str, index: u32) -> TargetExecutionAnalysisFailure {
    TargetExecutionAnalysisFailure::internal(
        TargetExecutionAnalysisPhase::Construction,
        "target-cost.invalid-root",
        format!("requested {kind} root {index} is absent from the verified target"),
    )
}

fn graph_failure(error: GraphBuildError) -> TargetExecutionAnalysisFailure {
    TargetExecutionAnalysisFailure::internal(
        TargetExecutionAnalysisPhase::Invariant,
        "target-cost.graph-invariant",
        format!("target-cost graph construction failed: {error:?}"),
    )
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityId;
    use crate::ir::minecraft::{
        AtMostOneSelector, CommandKind, CommandNode, Condition, DimensionId, EntitySelector,
        ExecuteCommand, ExecuteModifier, ExecuteModifierKind, ExecuteModifiers, FakeScoreHolder,
        FunctionCall, FunctionResourceId, FunctionTagEntry, FunctionTagMerge,
        FunctionTagResourceId, InternalCallableRef, MinecraftProgramBuilder, ObjectiveName,
        ReturnCommand, SayCommand, SayMessage, ScoreCommand, ScoreHolders, ScoreSelection,
        UnboundedSelector, UnsafeRawCommand,
    };
    use crate::source::OriginId;
    use crate::target::JavaEditionTarget;

    use super::*;
    use crate::analysis::minecraft::{
        AnalysisArithmeticCaps, CommandLimitStatus, CommandOutcome, CountUpperKind,
        NoFiniteBoundReason, ReturnValueClass, TargetExecutionAnalysisPhase,
    };

    fn assumptions() -> CommandLimitAssumptions {
        CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2)
    }

    fn limits(graph_entities: usize, solver_updates: usize) -> TargetExecutionAnalysisLimits {
        TargetExecutionAnalysisLimits::new(
            AnalysisArithmeticCaps::minimum_for(assumptions()),
            graph_entities,
            solver_updates,
        )
    }

    fn score() -> CommandNode {
        CommandNode::new(
            CommandKind::Score(ScoreCommand::PlayersSet {
                target: ScoreSelection::new(
                    ScoreHolders::from(FakeScoreHolder::new("#value").unwrap()),
                    ObjectiveName::new("mdl.reg").unwrap(),
                ),
                value: 1,
            }),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    fn returned() -> CommandNode {
        CommandNode::new(
            CommandKind::Return(ReturnCommand::Value(1)),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    fn say(message: &str) -> CommandNode {
        CommandNode::new(
            CommandKind::Say(SayCommand::new(SayMessage::new(message).unwrap())),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    fn call(target: impl Into<crate::ir::minecraft::CallableRef>) -> CommandNode {
        CommandNode::new(
            CommandKind::Function(FunctionCall::new(target.into())),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    fn execute(modifier: ExecuteModifierKind, run: CommandNode) -> CommandNode {
        CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(ExecuteModifier::new(modifier, OriginId::UNKNOWN), vec![]),
                run,
            )),
            OriginId::UNKNOWN,
        )
        .unwrap()
    }

    #[test]
    fn structured_say_cost_and_exact_native_result_reach_global_solver() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:say_entry").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let predicate = builder
            .declare_function(
                FunctionResourceId::parse("mdl:say_predicate").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(entry).unwrap();
        body.push(execute(
            ExecuteModifierKind::If(Condition::Function(predicate)),
            score(),
        ))
        .unwrap();
        body.push(score()).unwrap();
        body.finish();
        let mut body = builder.begin_function(predicate).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(say("hello"))),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(root.sequence_operations(), CountBound::exact(5));
        assert_eq!(root.score_nbt_command_executions(), CountBound::exact(2));
        assert_eq!(report.census().say_commands(), 1);
        assert_eq!(report.census().raw_commands(), 0);
        assert_eq!(
            report.functions()[usize::try_from(predicate.index()).unwrap()].steps()[0].outcomes(),
            &[CommandOutcome::Return(ReturnValueClass::NonZero)]
        );
    }

    #[derive(Clone, Copy, Debug)]
    enum AnalysisScaleShape {
        Deep,
        DeepManyRoot,
        Wide,
        Cycle,
    }

    fn analysis_scale_fixture(
        shape: AnalysisScaleShape,
        count: usize,
    ) -> (MinecraftProgram, Vec<TargetExecutionRoot>) {
        assert!(count >= 2);
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let functions = (0..count)
            .map(|index| {
                builder
                    .declare_function(
                        FunctionResourceId::parse(&format!("mdl:scale_{index}")).unwrap(),
                        OriginId::UNKNOWN,
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        match shape {
            AnalysisScaleShape::Deep | AnalysisScaleShape::DeepManyRoot => {
                for pair in functions.windows(2) {
                    let mut body = builder.begin_function(pair[0]).unwrap();
                    body.push(call(InternalCallableRef::Function(pair[1])))
                        .unwrap();
                    body.finish();
                }
                builder
                    .begin_function(*functions.last().unwrap())
                    .unwrap()
                    .finish();
            }
            AnalysisScaleShape::Wide => {
                let mut body = builder.begin_function(functions[0]).unwrap();
                for function in functions.iter().copied().skip(1) {
                    body.push(call(InternalCallableRef::Function(function)))
                        .unwrap();
                }
                body.finish();
                for function in functions.iter().copied().skip(1) {
                    builder.begin_function(function).unwrap().finish();
                }
            }
            AnalysisScaleShape::Cycle => {
                for (index, function) in functions.iter().copied().enumerate() {
                    let target = functions[(index + 1) % functions.len()];
                    let mut body = builder.begin_function(function).unwrap();
                    body.push(call(InternalCallableRef::Function(target)))
                        .unwrap();
                    body.finish();
                }
            }
        }
        let roots = if matches!(shape, AnalysisScaleShape::DeepManyRoot) {
            functions
                .iter()
                .copied()
                .map(TargetExecutionRoot::Function)
                .collect()
        } else {
            vec![TargetExecutionRoot::Function(functions[0])]
        };
        (builder.finish().unwrap(), roots)
    }

    #[test]
    fn straight_line_call_chain_has_exact_root_accounting() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:caller").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let leaf = builder
            .declare_function(
                FunctionResourceId::parse("mdl:callee").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(entry).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Function(FunctionCall::new(
                    InternalCallableRef::Function(leaf).into(),
                )),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.push(returned()).unwrap();
        body.finish();
        let mut body = builder.begin_function(leaf).unwrap();
        body.push(score()).unwrap();
        body.push(returned()).unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(root.sequence_operations().lower(), 3);
        assert_eq!(
            root.sequence_operations().upper().kind(),
            CountUpperKind::Finite(3)
        );
        assert_eq!(
            root.internal_function_invocations().upper().kind(),
            CountUpperKind::Finite(2)
        );
        assert_eq!(
            root.score_nbt_command_executions().upper().kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(report.stats().retained_functions(), 2);
        assert_eq!(report.stats().retained_roots(), 1);
    }

    #[test]
    fn unconditional_return_excludes_unreachable_raw_and_recursive_suffixes() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:early").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(entry).unwrap();
        body.push(returned()).unwrap();
        body.push(call(InternalCallableRef::Function(entry)))
            .unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Raw(UnsafeRawCommand::new("say unreachable").unwrap()),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(
            root.sequence_operations().upper().kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(
            root.internal_function_invocations().upper().kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(
            report.functions()[0].steps()[1].internal_call_sites(),
            &[crate::analysis::minecraft::InternalCallSite::Function {
                target: entry,
                role: crate::analysis::minecraft::InternalCallRole::Ordinary,
            }]
        );
        assert_eq!(report.stats().graph_edges(), 0);
        assert!(!report.regions()[0].is_cyclic());
    }

    #[test]
    fn typed_site_occurrences_project_to_one_structural_graph_edge() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry_function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:site_projection_caller").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let target_function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:site_projection_callee").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:site_projection_tag").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let mut entries = builder.begin_function_tag(tag).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(target_function),
            OriginId::UNKNOWN,
        ));
        entries.finish();

        let mut body = builder.begin_function(entry_function).unwrap();
        body.push(call(InternalCallableRef::Function(target_function)))
            .unwrap();
        body.push(execute(
            ExecuteModifierKind::If(Condition::Function(target_function)),
            score(),
        ))
        .unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(call(InternalCallableRef::Tag(tag)))),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let mut body = builder.begin_function(target_function).unwrap();
        body.push(returned()).unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry_function)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let sites = report.functions()[usize::try_from(entry_function.index()).unwrap()]
            .steps()
            .iter()
            .flat_map(|step| step.internal_call_sites().iter().copied())
            .collect::<Vec<_>>();
        assert_eq!(
            sites,
            vec![
                crate::analysis::minecraft::InternalCallSite::Function {
                    target: target_function,
                    role: crate::analysis::minecraft::InternalCallRole::Ordinary,
                },
                crate::analysis::minecraft::InternalCallSite::Function {
                    target: target_function,
                    role: crate::analysis::minecraft::InternalCallRole::Condition,
                },
                crate::analysis::minecraft::InternalCallSite::Tag {
                    target: tag,
                    role: crate::analysis::minecraft::InternalCallRole::ReturnRun,
                },
            ]
        );
        assert_eq!(report.stats().graph_edges(), 1);
        let repeated = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry_function)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        assert_eq!(report, repeated);
        let dump = report.dump();
        assert_eq!(dump, repeated.dump());
        let step_lines = dump
            .lines()
            .filter(|line| line.trim_start().starts_with("step "))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            step_lines,
            "  step 0 direct=sequence:1/execute:0/calls:1/score-nbt:0 max-chain=CountBound { lower: 0, upper: CountUpper(Finite(0)) } outcomes=[Continue, NoResult] call-sites=[Function { target: McFunctionId(1), role: Ordinary }] unknown=None\n  step 1 direct=sequence:2/execute:1/calls:1/score-nbt:1 max-chain=CountBound { lower: 0, upper: CountUpper(Finite(0)) } outcomes=[Continue, NoResult] call-sites=[Function { target: McFunctionId(1), role: Condition }] unknown=None\n  step 2 direct=sequence:0/execute:0/calls:0/score-nbt:0 max-chain=CountBound { lower: 0, upper: CountUpper(Finite(0)) } outcomes=[Return(UnknownInteger), Fail] call-sites=[Tag { target: FunctionTagId(0), role: ReturnRun }] unknown=None\n  step 0 direct=sequence:0/execute:0/calls:0/score-nbt:0 max-chain=CountBound { lower: 0, upper: CountUpper(Finite(0)) } outcomes=[Return(NonZero)] call-sites=[] unknown=None"
        );
    }

    #[test]
    fn execute_context_ranges_charge_fixed_stages_and_repeat_only_the_body() {
        for (selector, sequence, score_upper, expected_upper) in [
            (
                AtMostOneSelector::SelfExecutor.into(),
                CountUpperKind::Finite(4),
                CountUpperKind::Finite(2),
                false,
            ),
            (
                UnboundedSelector::AllEntities.into(),
                CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality),
                CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality),
                true,
            ),
        ] {
            let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
            let entry = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:fork").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let mut body = builder.begin_function(entry).unwrap();
            body.push(execute(ExecuteModifierKind::As(selector), score()))
                .unwrap();
            body.push(score()).unwrap();
            body.finish();
            let program = builder.finish().unwrap();
            let report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &[TargetExecutionRoot::Function(entry)],
                assumptions(),
                limits(1_000, 1_000),
            )
            .unwrap();
            let root = &report.roots()[0];
            assert_eq!(root.sequence_operations().lower(), 3);
            assert_eq!(root.sequence_operations().upper().kind(), sequence);
            assert_eq!(root.score_nbt_command_executions().lower(), 1);
            assert_eq!(
                root.score_nbt_command_executions().upper().kind(),
                score_upper
            );
            assert_eq!(
                matches!(
                    root.maximum_chain_expansion().upper().kind(),
                    CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality)
                ),
                expected_upper
            );
            if !expected_upper {
                assert_eq!(root.maximum_chain_expansion().lower(), 0);
                let step = &report.functions()[0].steps()[0];
                let outcomes = step.outcome_costs();
                assert_eq!(
                    step.outcomes(),
                    outcomes
                        .iter()
                        .map(crate::analysis::minecraft::FunctionOutcomeCost::outcome)
                        .collect::<Vec<_>>()
                );
                let zero_context = outcomes
                    .iter()
                    .find(|outcome| {
                        outcome.outcome() == crate::analysis::minecraft::CommandOutcome::NoResult
                    })
                    .unwrap();
                let one_context = outcomes
                    .iter()
                    .find(|outcome| {
                        outcome.outcome() == crate::analysis::minecraft::CommandOutcome::Continue
                    })
                    .unwrap();
                assert_eq!(
                    zero_context.sequence_operations().upper().kind(),
                    CountUpperKind::Finite(1)
                );
                assert_eq!(
                    one_context.sequence_operations().upper().kind(),
                    CountUpperKind::Finite(2)
                );
            }
        }
    }

    #[test]
    fn bounded_execute_contexts_keep_finite_body_cost_and_prefix_peak() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:bounded_fork").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let selector = EntitySelector::armor_stands(vec![], Some(2))
            .unwrap()
            .into();
        let mut body = builder.begin_function(entry).unwrap();
        body.push(execute(ExecuteModifierKind::As(selector), score()))
            .unwrap();
        body.push(score()).unwrap();
        body.finish();

        let report = analyze_target_execution(
            &builder.finish().unwrap(),
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(
            root.sequence_operations().upper().kind(),
            CountUpperKind::Finite(5)
        );
        assert_eq!(
            root.score_nbt_command_executions().upper().kind(),
            CountUpperKind::Finite(3)
        );
        assert_eq!(root.maximum_chain_expansion().lower(), 0);
        assert_eq!(
            root.maximum_chain_expansion().upper().kind(),
            CountUpperKind::Finite(2)
        );
        assert_eq!(
            report.functions()[usize::try_from(entry.index()).unwrap()].steps()[0]
                .maximum_chain_expansion()
                .upper()
                .kind(),
            CountUpperKind::Finite(2)
        );
    }

    #[test]
    fn unbounded_execute_call_keeps_structure_separate_from_runtime_multiplicity() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry_function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:selector_caller").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let worker_function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:selector_callee").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut body = builder.begin_function(entry_function).unwrap();
        body.push(execute(
            ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
            call(InternalCallableRef::Function(worker_function)),
        ))
        .unwrap();
        body.finish();
        builder.begin_function(worker_function).unwrap().finish();

        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry_function)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();

        let caller_summary = &report.functions()[usize::try_from(entry_function.index()).unwrap()];
        let step = &caller_summary.steps()[0];
        assert_eq!(step.internal_call_sites().len(), 1);
        assert_eq!(step.counts().internal_function_invocations(), 1);
        assert!(step.outcome_costs().iter().any(|outcome| {
            outcome.internal_function_invocations().upper().kind()
                == CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality)
        }));
        assert_eq!(report.stats().graph_edges(), 1);
        assert_eq!(report.roots()[0].internal_function_invocations().lower(), 1);
        assert_eq!(
            report.roots()[0]
                .internal_function_invocations()
                .upper()
                .kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality)
        );
    }

    #[test]
    fn function_conditions_use_callee_outcomes_without_charging_a_generic_stage() {
        for (returned_value, unless, expected_sequence, expected_scores) in [
            (0, false, 3, 1),
            (1, false, 4, 2),
            (0, true, 4, 2),
            (1, true, 3, 1),
        ] {
            let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
            let entry = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:guard").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let predicate = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:predicate").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let condition = Condition::Function(predicate);
            let modifier = if unless {
                ExecuteModifierKind::Unless(condition)
            } else {
                ExecuteModifierKind::If(condition)
            };
            let mut body = builder.begin_function(entry).unwrap();
            body.push(execute(modifier, score())).unwrap();
            body.push(score()).unwrap();
            body.finish();
            let mut body = builder.begin_function(predicate).unwrap();
            body.push(
                CommandNode::new(
                    CommandKind::Return(ReturnCommand::Value(returned_value)),
                    OriginId::UNKNOWN,
                )
                .unwrap(),
            )
            .unwrap();
            body.finish();
            let program = builder.finish().unwrap();
            let report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &[TargetExecutionRoot::Function(entry)],
                assumptions(),
                limits(1_000, 1_000),
            )
            .unwrap();
            let root = &report.roots()[0];
            assert_eq!(
                root.sequence_operations().upper().kind(),
                CountUpperKind::Finite(expected_sequence)
            );
            assert_eq!(
                root.score_nbt_command_executions().upper().kind(),
                CountUpperKind::Finite(expected_scores)
            );
            assert_eq!(
                root.execute_stages().upper().kind(),
                CountUpperKind::Finite(1)
            );
            assert_eq!(
                root.maximum_chain_expansion().upper().kind(),
                CountUpperKind::Finite(0),
                "a custom function condition is not an ordinary fork-checked redirect"
            );
            assert_eq!(
                report.functions()[usize::try_from(entry.index()).unwrap()].steps()[0]
                    .maximum_chain_expansion()
                    .upper()
                    .kind(),
                CountUpperKind::Finite(0),
                "syntax-local cost must use the same checked-redirect definition"
            );

            let fork_one = CommandLimitAssumptions::new(65_536, 1).unwrap();
            let fork_one_report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &[TargetExecutionRoot::Function(entry)],
                fork_one,
                TargetExecutionAnalysisLimits::new(
                    AnalysisArithmeticCaps::minimum_for(fork_one),
                    1_000,
                    1_000,
                ),
            )
            .unwrap();
            assert_eq!(
                fork_one_report.roots()[0].fork_limit_status(),
                CommandLimitStatus::ProvenWithin
            );
        }
    }

    #[test]
    fn mixed_custom_and_ordinary_execute_modifiers_retain_the_checked_peak() {
        for custom_first in [true, false] {
            let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
            let entry = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:mixed_guard").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let predicate = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:mixed_predicate").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let custom = ExecuteModifier::new(
                ExecuteModifierKind::If(Condition::Function(predicate)),
                OriginId::UNKNOWN,
            );
            let checked = ExecuteModifier::new(
                ExecuteModifierKind::In(DimensionId::parse("minecraft:overworld").unwrap()),
                OriginId::UNKNOWN,
            );
            let (first, rest) = if custom_first {
                (custom, vec![checked])
            } else {
                (checked, vec![custom])
            };
            let command = CommandNode::new(
                CommandKind::Execute(ExecuteCommand::new(
                    ExecuteModifiers::new(first, rest),
                    score(),
                )),
                OriginId::UNKNOWN,
            )
            .unwrap();
            let mut body = builder.begin_function(entry).unwrap();
            body.push(command).unwrap();
            body.finish();
            let mut body = builder.begin_function(predicate).unwrap();
            body.push(returned()).unwrap();
            body.finish();
            let program = builder.finish().unwrap();
            let fork_one = CommandLimitAssumptions::new(65_536, 1).unwrap();
            let report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &[TargetExecutionRoot::Function(entry)],
                fork_one,
                TargetExecutionAnalysisLimits::new(
                    AnalysisArithmeticCaps::minimum_for(fork_one),
                    1_000,
                    1_000,
                ),
            )
            .unwrap();
            let root = &report.roots()[0];
            assert_eq!(
                root.maximum_chain_expansion().upper().kind(),
                CountUpperKind::Finite(1)
            );
            assert_eq!(
                root.fork_limit_status(),
                CommandLimitStatus::ProvenExceeds,
                "the ordinary modifier remains fork-checked in either order"
            );
        }
    }

    #[test]
    fn no_result_function_condition_is_false_and_unless_is_true() {
        for (unless, expected_sequence, expected_scores) in [(false, 3, 1), (true, 4, 2)] {
            let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
            let entry = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:no_result_guard").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let predicate = builder
                .declare_function(
                    FunctionResourceId::parse("mdl:empty_predicate").unwrap(),
                    OriginId::UNKNOWN,
                )
                .unwrap();
            let condition = Condition::Function(predicate);
            let modifier = if unless {
                ExecuteModifierKind::Unless(condition)
            } else {
                ExecuteModifierKind::If(condition)
            };
            let mut body = builder.begin_function(entry).unwrap();
            body.push(execute(modifier, score())).unwrap();
            body.push(score()).unwrap();
            body.finish();
            builder.begin_function(predicate).unwrap().finish();
            let program = builder.finish().unwrap();
            let report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &[TargetExecutionRoot::Function(entry)],
                assumptions(),
                limits(1_000, 1_000),
            )
            .unwrap();
            assert_eq!(
                report.roots()[0].sequence_operations().upper().kind(),
                CountUpperKind::Finite(expected_sequence)
            );
            assert_eq!(
                report.roots()[0]
                    .score_nbt_command_executions()
                    .upper()
                    .kind(),
                CountUpperKind::Finite(expected_scores)
            );
        }
    }

    #[test]
    fn zero_context_before_unbounded_selector_does_not_invent_a_fork() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:zero_before_fork").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let predicate = builder
            .declare_function(
                FunctionResourceId::parse("mdl:false_before_fork").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(
                        ExecuteModifierKind::If(Condition::Function(predicate)),
                        OriginId::UNKNOWN,
                    ),
                    vec![ExecuteModifier::new(
                        ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
                        OriginId::UNKNOWN,
                    )],
                ),
                score(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let mut body = builder.begin_function(entry).unwrap();
        body.push(command).unwrap();
        body.finish();
        let mut body = builder.begin_function(predicate).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::Value(0)),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(
            root.sequence_operations().upper().kind(),
            CountUpperKind::Finite(3)
        );
        assert_eq!(
            root.score_nbt_command_executions().upper().kind(),
            CountUpperKind::Finite(0)
        );
        assert_eq!(
            root.maximum_chain_expansion().upper().kind(),
            CountUpperKind::Finite(0)
        );

        let zero_forks = CommandLimitAssumptions::new(65_536, 0).unwrap();
        let zero_fork_report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            zero_forks,
            TargetExecutionAnalysisLimits::new(
                AnalysisArithmeticCaps::minimum_for(zero_forks),
                1_000,
                1_000,
            ),
        )
        .unwrap();
        assert_eq!(
            zero_fork_report.roots()[0].fork_limit_status(),
            CommandLimitStatus::MayExceed,
            "a solved zero-output execute path can still encounter the fork-zero guard"
        );
    }

    #[test]
    fn return_run_propagates_function_results_and_stops_ordered_tags() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let ordinary = builder
            .declare_function(
                FunctionResourceId::parse("mdl:ordinary_tag").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let returning = builder
            .declare_function(
                FunctionResourceId::parse("mdl:return_tag").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let first = builder
            .declare_function(
                FunctionResourceId::parse("mdl:first").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let second = builder
            .declare_function(
                FunctionResourceId::parse("mdl:second").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:ordered").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let mut body = builder.begin_function(ordinary).unwrap();
        body.push(call(InternalCallableRef::Tag(tag))).unwrap();
        body.finish();
        let mut body = builder.begin_function(returning).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(call(InternalCallableRef::Tag(tag)))),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.push(score()).unwrap();
        body.finish();
        let mut body = builder.begin_function(first).unwrap();
        body.push(returned()).unwrap();
        body.finish();
        let mut body = builder.begin_function(second).unwrap();
        body.push(score()).unwrap();
        body.finish();
        let mut entries = builder.begin_function_tag(tag).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(first),
            OriginId::UNKNOWN,
        ));
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(second),
            OriginId::UNKNOWN,
        ));
        entries.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[
                TargetExecutionRoot::Function(ordinary),
                TargetExecutionRoot::Function(returning),
            ],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        assert_eq!(
            report.roots()[0].sequence_operations().upper().kind(),
            CountUpperKind::Finite(4)
        );
        assert_eq!(
            report.roots()[0]
                .score_nbt_command_executions()
                .upper()
                .kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(
            report.roots()[1].sequence_operations().upper().kind(),
            CountUpperKind::Finite(2)
        );
        assert_eq!(
            report.roots()[1]
                .score_nbt_command_executions()
                .upper()
                .kind(),
            CountUpperKind::Finite(0)
        );
    }

    #[test]
    fn return_run_unbounded_execute_runs_at_most_the_first_body_context() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:return_first_context").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let nested = execute(
            ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
            score(),
        );
        let mut body = builder.begin_function(entry).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(nested)),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(root.sequence_operations().lower(), 2);
        assert_eq!(
            root.sequence_operations().upper().kind(),
            CountUpperKind::Finite(3)
        );
        assert_eq!(root.score_nbt_command_executions().lower(), 0);
        assert_eq!(
            root.score_nbt_command_executions().upper().kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(
            root.maximum_chain_expansion().upper().kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality)
        );
    }

    #[test]
    fn scale_counters_are_linear_for_wide_deep_cyclic_and_many_root_programs() {
        const COUNT: usize = 2_048;
        for shape in [
            AnalysisScaleShape::Deep,
            AnalysisScaleShape::DeepManyRoot,
            AnalysisScaleShape::Wide,
            AnalysisScaleShape::Cycle,
        ] {
            let (program, roots) = analysis_scale_fixture(shape, COUNT);
            let cyclic = matches!(shape, AnalysisScaleShape::Cycle);
            let graph_entities = if cyclic { 3 * COUNT } else { 3 * COUNT - 2 };
            let solver_updates = if cyclic { 2 * COUNT } else { COUNT };
            let report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &roots,
                assumptions(),
                limits(graph_entities, solver_updates),
            )
            .unwrap();
            assert_eq!(
                report.completion(),
                TargetExecutionAnalysisCompletion::Complete,
                "shape={shape:?}"
            );
            let census = report.census();
            assert_eq!(census.functions(), COUNT, "shape={shape:?}");
            assert_eq!(
                census.top_level_commands(),
                if cyclic { COUNT } else { COUNT - 1 },
                "shape={shape:?}"
            );
            assert_eq!(
                census.command_nodes(),
                census.top_level_commands(),
                "shape={shape:?}"
            );
            assert_eq!(
                census.function_calls(),
                census.top_level_commands(),
                "shape={shape:?}"
            );
            let stats = report.stats();
            assert_eq!(stats.graph_entities(), graph_entities, "shape={shape:?}");
            assert_eq!(
                stats.graph_edges(),
                if cyclic { COUNT } else { COUNT - 1 },
                "shape={shape:?}"
            );
            assert_eq!(stats.solver_updates(), solver_updates, "shape={shape:?}");
            assert_eq!(
                stats.command_transfer_visits(),
                if cyclic { 4 * COUNT } else { 3 * (COUNT - 1) },
                "shape={shape:?}"
            );
            assert_eq!(stats.retained_functions(), COUNT, "shape={shape:?}");
            assert_eq!(
                stats.retained_regions(),
                if cyclic { 1 } else { COUNT },
                "shape={shape:?}"
            );
            assert_eq!(
                stats.retained_roots(),
                if matches!(shape, AnalysisScaleShape::DeepManyRoot) {
                    COUNT
                } else {
                    1
                },
                "shape={shape:?}"
            );
        }
    }

    #[test]
    #[ignore = "non-gating geometric target-cost analysis benchmark; run with --release"]
    fn reports_geometric_target_cost_scaling_without_a_timing_threshold() {
        use std::time::Instant;

        for shape in [
            AnalysisScaleShape::Deep,
            AnalysisScaleShape::DeepManyRoot,
            AnalysisScaleShape::Wide,
            AnalysisScaleShape::Cycle,
        ] {
            for count in [1_000, 2_000, 4_000, 8_000, 16_000] {
                let (program, roots) = analysis_scale_fixture(shape, count);
                let started = Instant::now();
                let report = analyze_target_execution(
                    &program,
                    &SourceContext::new(),
                    &roots,
                    assumptions(),
                    limits(usize::MAX, usize::MAX),
                )
                .unwrap();
                eprintln!(
                    "target-cost shape={shape:?} functions={count} elapsed={:?} graph-entities={} solver-updates={} command-transfer-visits={} roots={}",
                    started.elapsed(),
                    report.stats().graph_entities(),
                    report.stats().solver_updates(),
                    report.stats().command_transfer_visits(),
                    report.stats().retained_roots(),
                );
            }
        }
    }

    #[test]
    fn long_guard_sequence_uses_bounded_outcome_state() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:many_guards").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(entry).unwrap();
        for _ in 0..100 {
            body.push(execute(
                ExecuteModifierKind::If(Condition::ScoreMatches(
                    crate::ir::minecraft::ScoreRef::new(
                        FakeScoreHolder::new("#guard").unwrap().into(),
                        ObjectiveName::new("mdl.reg").unwrap(),
                    ),
                    crate::ir::minecraft::ScoreRange::exact(1),
                )),
                score(),
            ))
            .unwrap();
        }
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(10_000, 10_000),
        )
        .unwrap();
        assert_eq!(report.functions()[0].exits().len(), 1);
        assert_eq!(
            report.roots()[0]
                .score_nbt_command_executions()
                .upper()
                .kind(),
            CountUpperKind::Finite(100)
        );
    }

    #[test]
    fn positive_recursive_metrics_and_zero_score_metric_stay_distinct() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:loop").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Function(FunctionCall::new(
                    InternalCallableRef::Function(function).into(),
                )),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(function)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(
            root.sequence_operations().upper().kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
        );
        assert_eq!(
            root.score_nbt_command_executions().upper().kind(),
            CountUpperKind::Finite(0)
        );
    }

    #[test]
    fn positive_cycle_weight_propagates_across_every_member_of_a_feasible_scc() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let scored = builder
            .declare_function(
                FunctionResourceId::parse("mdl:scored_cycle_member").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let forwarding = builder
            .declare_function(
                FunctionResourceId::parse("mdl:forwarding_cycle_member").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(scored).unwrap();
        body.push(score()).unwrap();
        body.push(call(InternalCallableRef::Function(forwarding)))
            .unwrap();
        body.finish();
        let mut body = builder.begin_function(forwarding).unwrap();
        body.push(call(InternalCallableRef::Function(scored)))
            .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[
                TargetExecutionRoot::Function(scored),
                TargetExecutionRoot::Function(forwarding),
            ],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        for root in report.roots() {
            assert_eq!(
                root.score_nbt_command_executions().upper().kind(),
                CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
            );
        }
    }

    #[test]
    fn unknown_and_fork_effects_propagate_across_feasible_scc_members() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let raw_member = builder
            .declare_function(
                FunctionResourceId::parse("mdl:raw_cycle_member").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let raw_forwarder = builder
            .declare_function(
                FunctionResourceId::parse("mdl:raw_cycle_forwarder").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let fork_member = builder
            .declare_function(
                FunctionResourceId::parse("mdl:fork_cycle_member").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let fork_forwarder = builder
            .declare_function(
                FunctionResourceId::parse("mdl:fork_cycle_forwarder").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut body = builder.begin_function(raw_member).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Raw(UnsafeRawCommand::new("say unknown cycle effect").unwrap()),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.push(call(InternalCallableRef::Function(raw_forwarder)))
            .unwrap();
        body.finish();
        let mut body = builder.begin_function(raw_forwarder).unwrap();
        body.push(call(InternalCallableRef::Function(raw_member)))
            .unwrap();
        body.finish();

        let mut body = builder.begin_function(fork_member).unwrap();
        body.push(execute(
            ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
            call(InternalCallableRef::Function(fork_forwarder)),
        ))
        .unwrap();
        body.finish();
        let mut body = builder.begin_function(fork_forwarder).unwrap();
        body.push(call(InternalCallableRef::Function(fork_member)))
            .unwrap();
        body.finish();

        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[
                TargetExecutionRoot::Function(raw_forwarder),
                TargetExecutionRoot::Function(fork_forwarder),
            ],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        for metric in [
            report.roots()[0].sequence_operations(),
            report.roots()[0].internal_function_invocations(),
        ] {
            assert_eq!(
                metric.upper().kind(),
                CountUpperKind::Unknown(UnknownCostReason::RawCommand)
            );
        }
        assert_eq!(
            report.roots()[1].maximum_chain_expansion().upper().kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::SelectorCardinality)
        );
    }

    #[test]
    fn terminating_base_cost_propagates_through_a_recursive_scc() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let branching = builder
            .declare_function(
                FunctionResourceId::parse("mdl:recursive_base_branch").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let forwarding = builder
            .declare_function(
                FunctionResourceId::parse("mdl:recursive_base_forwarder").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let condition = Condition::ScoreMatches(
            crate::ir::minecraft::ScoreRef::new(
                FakeScoreHolder::new("#branch").unwrap().into(),
                ObjectiveName::new("mdl.reg").unwrap(),
            ),
            crate::ir::minecraft::ScoreRange::exact(1),
        );
        let recursive_return = CommandNode::new(
            CommandKind::Return(ReturnCommand::run(call(InternalCallableRef::Function(
                forwarding,
            )))),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let mut body = builder.begin_function(branching).unwrap();
        body.push(execute(
            ExecuteModifierKind::If(condition),
            recursive_return,
        ))
        .unwrap();
        body.push(score()).unwrap();
        body.push(returned()).unwrap();
        body.finish();
        let mut body = builder.begin_function(forwarding).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(call(InternalCallableRef::Function(
                    branching,
                )))),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(forwarding)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let score = report.roots()[0].score_nbt_command_executions();
        assert_eq!(score.lower(), 0);
        assert_eq!(score.upper().kind(), CountUpperKind::Finite(1));
        assert_eq!(
            report.roots()[0].sequence_operations().upper().kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
        );
    }

    #[test]
    fn zero_context_function_condition_does_not_create_a_feasible_call_edge() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:zero_context_entry").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let guarded = builder
            .declare_function(
                FunctionResourceId::parse("mdl:zero_context_guarded").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let false_predicate = builder
            .declare_function(
                FunctionResourceId::parse("mdl:zero_context_false").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(entry).unwrap();
        body.push(call(InternalCallableRef::Function(guarded)))
            .unwrap();
        body.finish();
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(
                        ExecuteModifierKind::If(Condition::Function(false_predicate)),
                        OriginId::UNKNOWN,
                    ),
                    vec![ExecuteModifier::new(
                        ExecuteModifierKind::If(Condition::Function(entry)),
                        OriginId::UNKNOWN,
                    )],
                ),
                score(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let mut body = builder.begin_function(guarded).unwrap();
        body.push(command).unwrap();
        body.push(score()).unwrap();
        body.finish();
        let mut body = builder.begin_function(false_predicate).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::Value(0)),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(entry)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        assert_eq!(
            report.roots()[0]
                .score_nbt_command_executions()
                .upper()
                .kind(),
            CountUpperKind::Finite(1)
        );
    }

    #[test]
    fn divergent_many_context_predicate_and_tag_member_are_valid_flows() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let predicate = builder
            .declare_function(
                FunctionResourceId::parse("mdl:divergent_predicate").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let conditioned = builder
            .declare_function(
                FunctionResourceId::parse("mdl:divergent_condition_entry").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let tagged = builder
            .declare_function(
                FunctionResourceId::parse("mdl:divergent_tag_entry").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let unreachable_member = builder
            .declare_function(
                FunctionResourceId::parse("mdl:unreachable_tag_member").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:divergent_tag").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let mut body = builder.begin_function(predicate).unwrap();
        body.push(call(InternalCallableRef::Function(predicate)))
            .unwrap();
        body.finish();
        let command = CommandNode::new(
            CommandKind::Execute(ExecuteCommand::new(
                ExecuteModifiers::new(
                    ExecuteModifier::new(
                        ExecuteModifierKind::As(UnboundedSelector::AllEntities.into()),
                        OriginId::UNKNOWN,
                    ),
                    vec![ExecuteModifier::new(
                        ExecuteModifierKind::If(Condition::Function(predicate)),
                        OriginId::UNKNOWN,
                    )],
                ),
                score(),
            )),
            OriginId::UNKNOWN,
        )
        .unwrap();
        let mut body = builder.begin_function(conditioned).unwrap();
        body.push(command).unwrap();
        body.finish();
        let mut body = builder.begin_function(tagged).unwrap();
        body.push(call(InternalCallableRef::Tag(tag))).unwrap();
        body.finish();
        let mut body = builder.begin_function(unreachable_member).unwrap();
        body.push(score()).unwrap();
        body.finish();
        let mut entries = builder.begin_function_tag(tag).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(predicate),
            OriginId::UNKNOWN,
        ));
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(unreachable_member),
            OriginId::UNKNOWN,
        ));
        entries.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[
                TargetExecutionRoot::Function(conditioned),
                TargetExecutionRoot::Function(tagged),
            ],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        assert_eq!(
            report.completion(),
            TargetExecutionAnalysisCompletion::Complete
        );
        for root in report.roots() {
            assert_eq!(
                root.score_nbt_command_executions().upper().kind(),
                CountUpperKind::Finite(0)
            );
        }
    }

    #[test]
    fn infeasible_recursive_edge_stays_finite_but_repeated_leaf_work_is_positive() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let guarded = builder
            .declare_function(
                FunctionResourceId::parse("mdl:guarded_recursion").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let predicate = builder
            .declare_function(
                FunctionResourceId::parse("mdl:false").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let repeated = builder
            .declare_function(
                FunctionResourceId::parse("mdl:repeated").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let leaf = builder
            .declare_function(
                FunctionResourceId::parse("mdl:leaf_score").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();

        let mut body = builder.begin_function(guarded).unwrap();
        body.push(execute(
            ExecuteModifierKind::If(Condition::Function(predicate)),
            call(InternalCallableRef::Function(guarded)),
        ))
        .unwrap();
        body.push(score()).unwrap();
        body.finish();
        let mut body = builder.begin_function(predicate).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::Value(0)),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let mut body = builder.begin_function(repeated).unwrap();
        body.push(call(InternalCallableRef::Function(leaf)))
            .unwrap();
        body.push(call(InternalCallableRef::Function(repeated)))
            .unwrap();
        body.finish();
        let mut body = builder.begin_function(leaf).unwrap();
        body.push(score()).unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[
                TargetExecutionRoot::Function(guarded),
                TargetExecutionRoot::Function(repeated),
            ],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        assert_eq!(
            report.roots()[0].sequence_operations().upper().kind(),
            CountUpperKind::Finite(3)
        );
        assert_eq!(
            report.roots()[0]
                .score_nbt_command_executions()
                .upper()
                .kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(
            report.roots()[1]
                .score_nbt_command_executions()
                .upper()
                .kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
        );
    }

    #[test]
    fn feasible_edges_split_an_acyclic_subgraph_out_of_a_syntactic_scc() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let a = builder
            .declare_function(
                FunctionResourceId::parse("mdl:syntactic_a").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let b = builder
            .declare_function(
                FunctionResourceId::parse("mdl:syntactic_b").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let false_predicate = builder
            .declare_function(
                FunctionResourceId::parse("mdl:syntactic_false").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(a).unwrap();
        body.push(call(InternalCallableRef::Function(b))).unwrap();
        body.finish();
        let mut body = builder.begin_function(b).unwrap();
        body.push(execute(
            ExecuteModifierKind::If(Condition::Function(false_predicate)),
            call(InternalCallableRef::Function(a)),
        ))
        .unwrap();
        body.finish();
        let mut body = builder.begin_function(false_predicate).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::Value(0)),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(a)],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        let root = &report.roots()[0];
        assert_eq!(
            root.sequence_operations().upper().kind(),
            CountUpperKind::Finite(3)
        );
        assert_eq!(
            root.internal_function_invocations().upper().kind(),
            CountUpperKind::Finite(3)
        );
        let syntactic_cycle = report
            .regions()
            .iter()
            .find(|region| region.is_cyclic())
            .unwrap();
        assert_eq!(
            syntactic_cycle.sequence_operations().upper().kind(),
            CountUpperKind::Finite(2)
        );
    }

    #[test]
    fn nonreturning_recursion_distinguishes_prefix_from_unreachable_suffix_work() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let score_before = builder
            .declare_function(
                FunctionResourceId::parse("mdl:score_before_recursion").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let score_after = builder
            .declare_function(
                FunctionResourceId::parse("mdl:score_after_recursion").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let caller = builder
            .declare_function(
                FunctionResourceId::parse("mdl:nonreturning_caller").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(score_before).unwrap();
        body.push(score()).unwrap();
        body.push(call(InternalCallableRef::Function(score_before)))
            .unwrap();
        body.finish();
        let mut body = builder.begin_function(score_after).unwrap();
        body.push(call(InternalCallableRef::Function(score_after)))
            .unwrap();
        body.push(score()).unwrap();
        body.finish();
        let mut body = builder.begin_function(caller).unwrap();
        body.push(call(InternalCallableRef::Function(score_after)))
            .unwrap();
        body.push(score()).unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[
                TargetExecutionRoot::Function(score_before),
                TargetExecutionRoot::Function(score_after),
                TargetExecutionRoot::Function(caller),
            ],
            assumptions(),
            limits(1_000, 1_000),
        )
        .unwrap();
        assert_eq!(
            report.roots()[0]
                .score_nbt_command_executions()
                .upper()
                .kind(),
            CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
        );
        for root in &report.roots()[1..] {
            assert_eq!(
                root.score_nbt_command_executions().upper().kind(),
                CountUpperKind::Finite(0)
            );
            assert_eq!(
                root.sequence_operations().upper().kind(),
                CountUpperKind::NoFiniteBoundProven(NoFiniteBoundReason::PositiveCycle)
            );
        }
    }

    #[test]
    fn arithmetic_caps_stop_doubling_without_losing_a_cheap_alternative() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let functions = (0..20)
            .map(|index| {
                builder
                    .declare_function(
                        FunctionResourceId::parse(&format!("mdl:double_{index}")).unwrap(),
                        OriginId::UNKNOWN,
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for pair in functions.windows(2) {
            let mut body = builder.begin_function(pair[0]).unwrap();
            body.push(call(InternalCallableRef::Function(pair[1])))
                .unwrap();
            body.push(call(InternalCallableRef::Function(pair[1])))
                .unwrap();
            body.finish();
        }
        let mut body = builder.begin_function(*functions.last().unwrap()).unwrap();
        body.push(score()).unwrap();
        body.finish();
        let optional = builder
            .declare_function(
                FunctionResourceId::parse("mdl:optional_high").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(optional).unwrap();
        body.push(execute(
            ExecuteModifierKind::As(AtMostOneSelector::SelfExecutor.into()),
            call(InternalCallableRef::Function(functions[0])),
        ))
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[
                TargetExecutionRoot::Function(functions[0]),
                TargetExecutionRoot::Function(optional),
            ],
            assumptions(),
            limits(10_000, 10_000),
        )
        .unwrap();
        assert_eq!(
            report.roots()[0].sequence_operations().upper().kind(),
            CountUpperKind::AboveAnalysisCap
        );
        assert_eq!(
            report.roots()[1].sequence_operations().upper().kind(),
            CountUpperKind::AboveAnalysisCap
        );
        assert_eq!(report.roots()[1].sequence_operations().lower(), 2);
    }

    #[test]
    fn raw_behavior_and_analysis_limits_have_distinct_unknown_reasons() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:raw").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Raw(UnsafeRawCommand::new("say unknown").unwrap()),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();
        for (limits, reason, completion) in [
            (
                limits(1_000, 1_000),
                UnknownCostReason::RawCommand,
                TargetExecutionAnalysisCompletion::Complete,
            ),
            (
                limits(0, 1_000),
                UnknownCostReason::AnalysisLimit,
                TargetExecutionAnalysisCompletion::Incomplete(
                    TargetExecutionAnalysisLimitKind::GraphEntities,
                ),
            ),
            (
                limits(1_000, 0),
                UnknownCostReason::AnalysisLimit,
                TargetExecutionAnalysisCompletion::Incomplete(
                    TargetExecutionAnalysisLimitKind::SolverUpdates,
                ),
            ),
        ] {
            let report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &[TargetExecutionRoot::Function(function)],
                assumptions(),
                limits,
            )
            .unwrap();
            assert_eq!(
                report.roots()[0].sequence_operations().upper().kind(),
                CountUpperKind::Unknown(reason)
            );
            assert_eq!(report.completion(), completion);
            assert_eq!(report.limits(), limits);
        }
    }

    #[test]
    fn graph_limit_preserves_sites_but_marks_all_dynamic_outcomes_unknown() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:graph_limited_site").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(call(InternalCallableRef::Function(function)))
            .unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(function)],
            assumptions(),
            limits(0, 1_000),
        )
        .unwrap();
        let local = &report.functions()[0];
        assert_eq!(local.steps()[0].counts().internal_function_invocations(), 1);
        assert_eq!(
            local.steps()[0].internal_call_sites(),
            &[crate::analysis::minecraft::InternalCallSite::Function {
                target: function,
                role: crate::analysis::minecraft::InternalCallRole::Ordinary,
            }]
        );
        assert_eq!(local.steps()[0].intrinsic_unknown_reason(), None);
        assert_eq!(local.steps()[0].outcome_costs().len(), 2);
        assert_eq!(local.exits().len(), 1);
        for outcome in local.steps()[0].outcome_costs().iter().chain(local.exits()) {
            for bound in [
                outcome.sequence_operations(),
                outcome.execute_stages(),
                outcome.internal_function_invocations(),
                outcome.score_nbt_command_executions(),
                outcome.maximum_chain_expansion(),
            ] {
                assert_eq!(
                    bound.upper().kind(),
                    CountUpperKind::Unknown(UnknownCostReason::AnalysisLimit)
                );
            }
            assert_eq!(
                outcome.unknown_reason(),
                Some(UnknownCostReason::AnalysisLimit)
            );
        }
        assert!(report.regions().is_empty());
        assert_eq!(report.stats().graph_edges(), 0);
        assert_eq!(report.stats().graph_entities(), 2);
        let repeated = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(function)],
            assumptions(),
            limits(0, 1_000),
        )
        .unwrap();
        assert_eq!(report, repeated);
        assert_eq!(report.dump(), repeated.dump());
    }

    #[test]
    fn solver_limit_keeps_complete_local_outcomes_exact() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:solver_limited_local").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut body = builder.begin_function(function).unwrap();
        body.push(score()).unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(function)],
            assumptions(),
            limits(1_000, 0),
        )
        .unwrap();
        assert_eq!(
            report.functions()[0].steps()[0].outcome_costs()[0]
                .sequence_operations()
                .upper()
                .kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(
            report.functions()[0].exits()[0]
                .sequence_operations()
                .upper()
                .kind(),
            CountUpperKind::Finite(1)
        );
        assert_eq!(
            report.roots()[0].sequence_operations().upper().kind(),
            CountUpperKind::Unknown(UnknownCostReason::AnalysisLimit)
        );
    }

    #[test]
    fn cyclic_feasibility_work_is_counted_before_solver_fallback() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let branching = builder
            .declare_function(
                FunctionResourceId::parse("mdl:limited_feasibility_branch").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let forwarding = builder
            .declare_function(
                FunctionResourceId::parse("mdl:limited_feasibility_forwarder").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let condition = Condition::ScoreMatches(
            crate::ir::minecraft::ScoreRef::new(
                FakeScoreHolder::new("#limited_branch").unwrap().into(),
                ObjectiveName::new("mdl.reg").unwrap(),
            ),
            crate::ir::minecraft::ScoreRange::exact(1),
        );
        let mut body = builder.begin_function(branching).unwrap();
        body.push(execute(
            ExecuteModifierKind::If(condition),
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(call(InternalCallableRef::Function(
                    forwarding,
                )))),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        ))
        .unwrap();
        body.push(returned()).unwrap();
        body.finish();
        let mut body = builder.begin_function(forwarding).unwrap();
        body.push(
            CommandNode::new(
                CommandKind::Return(ReturnCommand::run(call(InternalCallableRef::Function(
                    branching,
                )))),
                OriginId::UNKNOWN,
            )
            .unwrap(),
        )
        .unwrap();
        body.finish();
        let program = builder.finish().unwrap();

        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::Function(branching)],
            assumptions(),
            limits(1_000, 3),
        )
        .unwrap();
        assert_eq!(
            report.completion(),
            TargetExecutionAnalysisCompletion::Incomplete(
                TargetExecutionAnalysisLimitKind::SolverUpdates
            )
        );
        assert_eq!(report.stats().solver_updates(), 1);
        assert_eq!(
            report.roots()[0].sequence_operations().upper().kind(),
            CountUpperKind::Unknown(UnknownCostReason::AnalysisLimit)
        );
    }

    #[test]
    fn graph_preflight_limit_returns_one_typed_unresolved_tag_root() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:limited_member").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        builder.begin_function(function).unwrap().finish();
        let tag = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:limited_tag").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let mut entries = builder.begin_function_tag(tag).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(function),
            OriginId::UNKNOWN,
        ));
        entries.finish();
        let program = builder.finish().unwrap();
        let report = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[TargetExecutionRoot::FunctionTag(tag)],
            assumptions(),
            limits(0, 1_000),
        )
        .unwrap();
        assert_eq!(report.functions().len(), 1);
        assert_eq!(report.regions().len(), 0);
        assert_eq!(report.roots().len(), 1);
        assert_eq!(
            report.roots()[0].root(),
            &ResolvedTargetExecutionRoot::UnresolvedAnalysisLimitFunctionTag { tag }
        );
        assert_eq!(
            report.roots()[0].entry_region(),
            RootEntryRegion::AnalysisLimit
        );
        assert_eq!(
            report.roots()[0].sequence_operations().upper().kind(),
            CountUpperKind::Unknown(UnknownCostReason::AnalysisLimit)
        );
    }

    #[test]
    fn independent_tags_complete_at_their_exact_linear_graph_budget() {
        const COUNT: usize = 100;
        const GRAPH_ENTITIES: usize = COUNT * 4;
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let functions = (0..COUNT)
            .map(|index| {
                let function = builder
                    .declare_function(
                        FunctionResourceId::parse(&format!("mdl:budget_function_{index}")).unwrap(),
                        OriginId::UNKNOWN,
                    )
                    .unwrap();
                builder.begin_function(function).unwrap().finish();
                function
            })
            .collect::<Vec<_>>();
        for (index, function) in functions.iter().copied().enumerate() {
            let tag = builder
                .declare_function_tag(
                    FunctionTagResourceId::parse(&format!("mdl:budget_tag_{index}")).unwrap(),
                    OriginId::UNKNOWN,
                    FunctionTagMerge::Append,
                )
                .unwrap();
            let mut entries = builder.begin_function_tag(tag).unwrap();
            entries.push(FunctionTagEntry::internal(
                InternalCallableRef::Function(function),
                OriginId::UNKNOWN,
            ));
            entries.finish();
        }
        let program = builder.finish().unwrap();
        for (graph_limit, completion) in [
            (GRAPH_ENTITIES, TargetExecutionAnalysisCompletion::Complete),
            (
                GRAPH_ENTITIES - 1,
                TargetExecutionAnalysisCompletion::Incomplete(
                    TargetExecutionAnalysisLimitKind::GraphEntities,
                ),
            ),
        ] {
            let report = analyze_target_execution(
                &program,
                &SourceContext::new(),
                &[TargetExecutionRoot::Function(functions[0])],
                assumptions(),
                limits(graph_limit, 1_000),
            )
            .unwrap();
            assert_eq!(report.completion(), completion);
            assert_eq!(report.functions().len(), COUNT);
            if graph_limit == GRAPH_ENTITIES {
                assert_eq!(report.stats().graph_entities(), GRAPH_ENTITIES);
                assert_eq!(report.stats().tag_expansion_entries(), COUNT);
            }
        }
    }

    #[test]
    fn arithmetic_caps_built_for_smaller_assumptions_are_configuration_failure() {
        let program = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2)
            .finish()
            .unwrap();
        let smaller = CommandLimitAssumptions::new(1, 1).unwrap();
        let requested = CommandLimitAssumptions::new(10, 10).unwrap();
        let mismatched_limits = TargetExecutionAnalysisLimits::new(
            AnalysisArithmeticCaps::minimum_for(smaller),
            10,
            10,
        );
        let failure = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[],
            requested,
            mismatched_limits,
        )
        .unwrap_err();
        assert_eq!(failure.phase(), TargetExecutionAnalysisPhase::Configuration);
        assert!(
            failure
                .diagnostics()
                .contains_code("target-cost.inconsistent-caps")
        );
    }

    #[test]
    fn invalid_target_fails_verification_without_a_report() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let function = builder
            .declare_function(
                FunctionResourceId::parse("mdl:undefined").unwrap(),
                OriginId::from_index(99),
            )
            .unwrap();
        builder.begin_function(function).unwrap().finish();
        let program = builder.finish().unwrap();
        let failure = analyze_target_execution(
            &program,
            &SourceContext::new(),
            &[],
            assumptions(),
            limits(10, 10),
        )
        .unwrap_err();
        assert_eq!(failure.phase(), TargetExecutionAnalysisPhase::Verification);
    }
}
