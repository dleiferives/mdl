use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::entity::EntityId;
use crate::ir::minecraft::{
    CallableRef, Cardinality, CommandKind, Condition, ExecuteModifierKind, FunctionTagId,
    InternalCallableRef, McFunctionId, MinecraftProgram, ReturnCommand,
};

use super::cost::FunctionOutcomeMetrics;
use super::graph::{ExecutionGraph, TagRootEntry};
use super::{
    AnalysisArithmeticCaps, CommandOutcome, CostRegionSummary, CountBound, CountUpperKind,
    FunctionLocalSummary, FunctionOutcomeCost, NoFiniteBoundReason, ReturnValueClass,
    UnknownCostReason,
};

const FUNCTION_EXIT_COUNT: usize = 4;
const COMMAND_RESULT_COUNT: usize = 4;
const RETURN_EXIT_COUNT: usize = 3;
const CONTEXT_CLASS_COUNT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MetricSet {
    pub(super) sequence: CountBound,
    pub(super) execute: CountBound,
    pub(super) calls: CountBound,
    pub(super) score_nbt: CountBound,
    pub(super) max_chain: CountBound,
    /// Whether at least one active-region call can execute on this alternative.
    ///
    /// The solver needs only reachability here, not another dynamic count bound.
    /// Keeping that distinction explicit also prevents recursive scratch from
    /// masquerading as a reportable invocation metric.
    recursive_reachable: bool,
    recursive_sequence: Option<CountBound>,
    recursive_execute: Option<CountBound>,
    recursive_function_invocations: Option<CountBound>,
    recursive_score_nbt: Option<CountBound>,
    recursive_max_chain: Option<CountBound>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FunctionFlow {
    exits: [Option<MetricSet>; FUNCTION_EXIT_COUNT],
    divergence: Option<MetricSet>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CommandFlow {
    continues: [Option<MetricSet>; COMMAND_RESULT_COUNT],
    returns: [Option<MetricSet>; RETURN_EXIT_COUNT],
    divergence: Option<MetricSet>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FunctionExit {
    NoResult = 0,
    Zero = 1,
    NonZero = 2,
    Failure = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandResult {
    NoResult = 0,
    Zero = 1,
    NonZero = 2,
    Failure = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReturnExit {
    Zero = 0,
    NonZero = 1,
    Failure = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ContextClass {
    Zero = 0,
    One = 1,
    Many = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContextFlow {
    alternatives: [Option<MetricSet>; CONTEXT_CLASS_COUNT],
    divergence: Option<MetricSet>,
}

pub(super) struct SolvedCosts {
    pub(super) regions: Vec<CostRegionSummary>,
    pub(super) functions: Vec<MetricSet>,
    pub(super) updates: usize,
    pub(super) command_transfer_visits: usize,
    pub(super) complete: bool,
}

struct SolverContext<'a> {
    program: &'a MinecraftProgram,
    graph: &'a ExecutionGraph,
    solved: &'a [Option<FunctionFlow>],
    active_region: Option<usize>,
    recursive_exit_masks: Option<&'a HashMap<McFunctionId, [bool; FUNCTION_EXIT_COUNT]>>,
    reached_active_calls: Option<&'a RefCell<HashSet<McFunctionId>>>,
    divergent_active_calls: Option<&'a HashSet<McFunctionId>>,
    recursive_targets: Option<&'a HashSet<McFunctionId>>,
    regional_solved: Option<&'a HashMap<McFunctionId, FunctionFlow>>,
    local_only: bool,
    command_transfer_visits: &'a Cell<usize>,
    caps: AnalysisArithmeticCaps,
}

struct FeasibleRegion {
    masks: HashMap<McFunctionId, [bool; FUNCTION_EXIT_COUNT]>,
    components: Vec<FeasibleComponent>,
    updates: usize,
}

struct FeasibleComponent {
    functions: Vec<McFunctionId>,
    cyclic: bool,
}

enum FeasibilityResult {
    Complete(FeasibleRegion),
    Limit { updates: usize },
}

#[allow(
    clippy::too_many_arguments,
    reason = "one region solve borrows explicit graph, solution, cap, budget, and visit-counter inputs"
)]
fn solve_exit_feasibility(
    functions: &[McFunctionId],
    region_index: usize,
    program: &MinecraftProgram,
    graph: &ExecutionGraph,
    solved: &[Option<FunctionFlow>],
    caps: AnalysisArithmeticCaps,
    work_budget: usize,
    command_transfer_visits: &Cell<usize>,
) -> Option<FeasibilityResult> {
    let mut masks = functions
        .iter()
        .copied()
        .map(|function| (function, [false; FUNCTION_EXIT_COUNT]))
        .collect::<HashMap<_, _>>();
    let members = functions.iter().copied().collect::<HashSet<_>>();
    let mut callers = functions
        .iter()
        .copied()
        .map(|function| (function, vec![]))
        .collect::<HashMap<_, _>>();
    for caller in functions {
        let index = usize::try_from(caller.index()).ok()?;
        for callee in graph.outgoing.get(index)? {
            if members.contains(callee) {
                callers.get_mut(callee)?.push(*caller);
            }
        }
    }
    for entries in callers.values_mut() {
        entries.sort_unstable();
        entries.dedup();
    }
    let mut feasible_edges = functions
        .iter()
        .copied()
        .map(|function| (function, HashSet::new()))
        .collect::<HashMap<_, _>>();
    let mut queue = functions.iter().copied().collect::<VecDeque<_>>();
    let mut queued = members;
    let mut updates = 0usize;
    while let Some(function) = queue.pop_front() {
        queued.remove(&function);
        if updates >= work_budget {
            return Some(FeasibilityResult::Limit { updates });
        }
        updates = updates.checked_add(1)?;
        let reached = RefCell::new(HashSet::new());
        let context = SolverContext {
            program,
            graph,
            solved,
            active_region: Some(region_index),
            recursive_exit_masks: Some(&masks),
            reached_active_calls: Some(&reached),
            divergent_active_calls: None,
            recursive_targets: None,
            regional_solved: None,
            local_only: false,
            command_transfer_visits,
            caps,
        };
        let flow = evaluate_function(function, &context)?;
        feasible_edges
            .get_mut(&function)?
            .extend(reached.into_inner());
        let mut changed = false;
        for (exit, reachable) in flow.exits.iter().map(Option::is_some).enumerate() {
            if reachable && !masks.get(&function)?[exit] {
                if updates >= work_budget {
                    return Some(FeasibilityResult::Limit { updates });
                }
                masks.get_mut(&function)?[exit] = true;
                updates = updates.checked_add(1)?;
                changed = true;
            }
        }
        if changed {
            for caller in callers.get(&function)? {
                if queued.insert(*caller) {
                    queue.push_back(*caller);
                }
            }
        }
    }
    let feasible_edges = feasible_edges
        .into_iter()
        .map(|(function, edges)| {
            let mut edges = edges.into_iter().collect::<Vec<_>>();
            edges.sort_unstable();
            (function, edges)
        })
        .collect();
    let components = feasible_components(functions, &feasible_edges)?;
    Some(FeasibilityResult::Complete(FeasibleRegion {
        masks,
        components,
        updates,
    }))
}

fn feasible_components(
    functions: &[McFunctionId],
    global_edges: &HashMap<McFunctionId, Vec<McFunctionId>>,
) -> Option<Vec<FeasibleComponent>> {
    let indices = functions
        .iter()
        .enumerate()
        .map(|(index, function)| (*function, index))
        .collect::<HashMap<_, _>>();
    let mut edges = vec![vec![]; functions.len()];
    let mut incoming = vec![vec![]; functions.len()];
    for (source, function) in functions.iter().enumerate() {
        for target in global_edges.get(function)? {
            let target = *indices.get(target)?;
            edges[source].push(target);
            incoming[target].push(source);
        }
    }
    let order = finish_order_indices(&edges)?;
    let mut assigned = vec![false; functions.len()];
    let mut component_for = vec![usize::MAX; functions.len()];
    let mut components = vec![];
    for root in order.into_iter().rev() {
        if assigned[root] {
            continue;
        }
        let mut members = vec![];
        assigned[root] = true;
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            members.push(node);
            for predecessor in &incoming[node] {
                if !assigned[*predecessor] {
                    assigned[*predecessor] = true;
                    stack.push(*predecessor);
                }
            }
        }
        members.sort_unstable();
        let cyclic = members.len() > 1
            || members
                .first()
                .is_some_and(|member| edges[*member].contains(member));
        let component = components.len();
        for member in &members {
            component_for[*member] = component;
        }
        components.push(FeasibleComponent {
            functions: members
                .into_iter()
                .map(|member| functions[member])
                .collect(),
            cyclic,
        });
    }

    let mut component_edges = vec![vec![]; components.len()];
    for (source, targets) in edges.iter().enumerate() {
        let source_component = *component_for.get(source)?;
        for target in targets {
            let target_component = *component_for.get(*target)?;
            if source_component != target_component {
                component_edges[source_component].push(target_component);
            }
        }
    }
    for targets in &mut component_edges {
        targets.sort_unstable();
        targets.dedup();
    }
    let component_order = region_finish_order(&component_edges)?;
    let mut ordered = Vec::with_capacity(components.len());
    let mut components = components.into_iter().map(Some).collect::<Vec<_>>();
    for component in component_order {
        ordered.push(components.get_mut(component)?.take()?);
    }
    Some(ordered)
}

fn finish_order_indices(edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    let mut visited = vec![false; edges.len()];
    let mut order = Vec::with_capacity(edges.len());
    for root in 0..edges.len() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut stack = vec![(root, 0usize)];
        while let Some((node, next)) = stack.last_mut() {
            if *next < edges[*node].len() {
                let target = edges[*node][*next];
                *next += 1;
                if !*visited.get(target)? {
                    visited[target] = true;
                    stack.push((target, 0));
                }
            } else {
                order.push(*node);
                stack.pop();
            }
        }
    }
    Some(order)
}

#[allow(
    clippy::too_many_lines,
    reason = "the solver keeps deterministic region preflight, evaluation, and all-or-nothing commit in one orchestration loop"
)]
pub(super) fn solve(
    program: &MinecraftProgram,
    graph: &ExecutionGraph,
    locals: &[FunctionLocalSummary],
    caps: AnalysisArithmeticCaps,
    update_limit: usize,
) -> Option<SolvedCosts> {
    if locals.len() != graph.outgoing.len() || program.functions().len() != locals.len() {
        return None;
    }
    let region_edges = region_edges(graph)?;
    let order = region_finish_order(&region_edges)?;
    let mut function_flows = vec![None; locals.len()];
    let mut region_metrics = vec![None; graph.components.len()];
    let mut updates = 0usize;
    let command_transfer_visits = Cell::new(0usize);

    for region_index in order {
        if updates >= update_limit {
            break;
        }
        let component = graph.components.get(region_index)?;
        let feasibility = if component.cyclic {
            let remaining = update_limit.checked_sub(updates)?;
            let Some(feasibility_budget) = remaining.checked_sub(component.functions.len()) else {
                break;
            };
            match solve_exit_feasibility(
                component.functions.as_slice(),
                region_index,
                program,
                graph,
                &function_flows,
                caps,
                feasibility_budget,
                &command_transfer_visits,
            )? {
                FeasibilityResult::Complete(feasibility) => Some(feasibility),
                FeasibilityResult::Limit {
                    updates: feasibility_updates,
                } => {
                    updates = updates.checked_add(feasibility_updates)?;
                    break;
                }
            }
        } else {
            None
        };
        let weighted_updates = match &feasibility {
            Some(feasibility) => feasible_weighted_updates(&feasibility.components)?,
            None => component.functions.len(),
        };
        let region_updates = feasibility
            .as_ref()
            .map_or(0, |value| value.updates)
            .checked_add(weighted_updates)?;
        if updates.checked_add(region_updates)? > update_limit {
            break;
        }
        let mut entries = Vec::with_capacity(component.functions.len());
        if let Some(feasibility) = &feasibility {
            let mut regional = HashMap::with_capacity(component.functions.len());
            for feasible_component in &feasibility.components {
                solve_feasible_component(
                    feasible_component,
                    region_index,
                    program,
                    graph,
                    &function_flows,
                    &feasibility.masks,
                    &mut regional,
                    caps,
                    &command_transfer_visits,
                )?;
            }
            for function in &component.functions {
                entries.push((*function, *regional.get(function)?));
            }
        } else {
            let context = SolverContext {
                program,
                graph,
                solved: &function_flows,
                active_region: None,
                recursive_exit_masks: None,
                reached_active_calls: None,
                divergent_active_calls: None,
                recursive_targets: None,
                regional_solved: None,
                local_only: false,
                command_transfer_visits: &command_transfer_visits,
                caps,
            };
            for function in &component.functions {
                entries.push((*function, evaluate_function(*function, &context)?));
            }
        }
        let mut merged = None;
        for (function, flow) in entries {
            let metrics = flow.merged();
            merged = Some(match merged {
                Some(current) => merge_metrics(current, metrics),
                None => metrics,
            });
            let index = usize::try_from(function.index()).ok()?;
            *function_flows.get_mut(index)? = Some(flow);
        }
        region_metrics[region_index] = merged;
        updates = updates.checked_add(region_updates)?;
    }

    finish_solved_costs(
        graph,
        &function_flows,
        &region_metrics,
        updates,
        command_transfer_visits.get(),
    )
}

fn feasible_weighted_updates(components: &[FeasibleComponent]) -> Option<usize> {
    components.iter().try_fold(0usize, |total, component| {
        let functions = component.functions.len();
        total.checked_add(functions)
    })
}

fn finish_solved_costs(
    graph: &ExecutionGraph,
    function_flows: &[Option<FunctionFlow>],
    region_metrics: &[Option<MetricSet>],
    updates: usize,
    command_transfer_visits: usize,
) -> Option<SolvedCosts> {
    let incomplete = MetricSet::unknown(UnknownCostReason::AnalysisLimit);
    let functions = function_flows
        .iter()
        .map(|flow| flow.map_or(incomplete, |flow| flow.merged()))
        .collect::<Vec<_>>();
    let mut regions = Vec::with_capacity(graph.components.len());
    for component in &graph.components {
        let index = usize::try_from(component.id.index()).ok()?;
        let metrics = region_metrics
            .get(index)
            .copied()
            .flatten()
            .unwrap_or(incomplete);
        regions.push(CostRegionSummary::new(
            component.id,
            component.functions.clone(),
            component.cyclic,
            metrics,
        ));
    }
    Some(SolvedCosts {
        regions,
        functions,
        updates,
        command_transfer_visits,
        complete: function_flows.iter().all(Option::is_some),
    })
}

#[allow(clippy::too_many_arguments)]
fn solve_feasible_component(
    component: &FeasibleComponent,
    region_index: usize,
    program: &MinecraftProgram,
    graph: &ExecutionGraph,
    solved: &[Option<FunctionFlow>],
    exit_masks: &HashMap<McFunctionId, [bool; FUNCTION_EXIT_COUNT]>,
    regional: &mut HashMap<McFunctionId, FunctionFlow>,
    caps: AnalysisArithmeticCaps,
    command_transfer_visits: &Cell<usize>,
) -> Option<()> {
    let recursive_targets = if component.cyclic {
        component.functions.iter().copied().collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };
    let context = SolverContext {
        program,
        graph,
        solved,
        active_region: Some(region_index),
        recursive_exit_masks: Some(exit_masks),
        reached_active_calls: None,
        divergent_active_calls: Some(&recursive_targets),
        recursive_targets: Some(&recursive_targets),
        regional_solved: Some(regional),
        local_only: false,
        command_transfer_visits,
        caps,
    };
    let mut flows = component
        .functions
        .iter()
        .map(|function| Some((*function, evaluate_function(*function, &context)?)))
        .collect::<Option<Vec<_>>>()?;
    if component.cyclic {
        let effects = cycle_metric_effects(flows.iter().map(|(_, flow)| flow));
        let envelope = component_envelope(flows.iter().map(|(_, flow)| flow), caps)?;
        for (_, flow) in &mut flows {
            apply_cycle_bounds(flow, effects, envelope, caps)?;
        }
    }
    for (_, flow) in &mut flows {
        flow.clear_recursive_scratch();
    }
    for (function, flow) in flows {
        regional.insert(function, flow);
    }
    Some(())
}

pub(super) fn replace_local_outcomes(
    program: &MinecraftProgram,
    graph: &ExecutionGraph,
    locals: &mut [FunctionLocalSummary],
    caps: AnalysisArithmeticCaps,
) -> Option<usize> {
    if locals.len() != program.functions().len() || locals.len() != graph.outgoing.len() {
        return None;
    }
    let empty = vec![None; locals.len()];
    let command_transfer_visits = Cell::new(0usize);
    let context = SolverContext {
        program,
        graph,
        solved: &empty,
        active_region: None,
        recursive_exit_masks: None,
        reached_active_calls: None,
        divergent_active_calls: None,
        recursive_targets: None,
        regional_solved: None,
        local_only: true,
        command_transfer_visits: &command_transfer_visits,
        caps,
    };
    for (function, _) in program.functions() {
        let index = usize::try_from(function.index()).ok()?;
        let data = program.function(function)?;
        let step_outcomes = data
            .body()
            .commands()
            .map(|(_, command)| command_outcome_costs(&evaluate_command(command.kind(), &context)?))
            .collect::<Option<Vec<_>>>()?;
        let local = locals.get_mut(index)?;
        if local.steps().len() != step_outcomes.len() {
            return None;
        }
        for (step, outcomes) in local.steps_mut().iter_mut().zip(step_outcomes) {
            step.replace_outcome_costs(outcomes);
        }
        let flow = evaluate_function(function, &context)?;
        let mut exits = vec![];
        for (exit, metrics) in flow.exits.into_iter().enumerate() {
            let Some(metrics) = metrics else {
                continue;
            };
            let outcome = match exit {
                0 => CommandOutcome::NoResult,
                1 => CommandOutcome::Return(ReturnValueClass::Zero),
                2 => CommandOutcome::Return(ReturnValueClass::NonZero),
                3 => CommandOutcome::Fail,
                _ => return None,
            };
            exits.push(FunctionOutcomeCost::new(
                outcome,
                outcome_metrics(metrics),
                metric_unknown_reason(metrics),
            ));
        }
        local.replace_exits(exits);
    }
    Some(command_transfer_visits.get())
}

fn command_outcome_costs(flow: &CommandFlow) -> Option<Vec<FunctionOutcomeCost>> {
    let mut cells: [Option<MetricSet>; 5] = [None; 5];
    for (result, metrics) in flow.continues.iter().copied().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        let outcome = usize::from(result != CommandResult::NoResult as usize);
        merge_cell(&mut cells[outcome], metrics);
    }
    for (result, metrics) in flow.returns.iter().copied().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        merge_cell(&mut cells[result + 2], metrics);
    }
    let mut outcomes = cells
        .into_iter()
        .enumerate()
        .filter_map(|(index, metrics)| metrics.map(|metrics| (index, metrics)))
        .map(|(index, metrics)| {
            let outcome = match index {
                0 => CommandOutcome::NoResult,
                1 => CommandOutcome::Continue,
                2 => CommandOutcome::Return(ReturnValueClass::Zero),
                3 => CommandOutcome::Return(ReturnValueClass::NonZero),
                4 => CommandOutcome::Fail,
                _ => return None,
            };
            Some(FunctionOutcomeCost::new(
                outcome,
                outcome_metrics(metrics),
                metric_unknown_reason(metrics),
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    outcomes.sort_by_key(FunctionOutcomeCost::outcome);
    Some(outcomes)
}

fn outcome_metrics(metrics: MetricSet) -> FunctionOutcomeMetrics {
    FunctionOutcomeMetrics {
        sequence_operations: metrics.sequence,
        execute_stages: metrics.execute,
        internal_function_invocations: metrics.calls,
        score_nbt_command_executions: metrics.score_nbt,
        maximum_chain_expansion: metrics.max_chain,
    }
}

fn metric_unknown_reason(metrics: MetricSet) -> Option<UnknownCostReason> {
    [
        metrics.sequence,
        metrics.execute,
        metrics.calls,
        metrics.score_nbt,
        metrics.max_chain,
    ]
    .into_iter()
    .filter_map(|bound| match bound.upper().kind() {
        CountUpperKind::Unknown(reason) => Some(reason),
        _ => None,
    })
    .min()
}

pub(super) fn add_root_invocation(
    metrics: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<MetricSet> {
    add_metrics(metrics, MetricSet::invocation(), caps)
}

impl MetricSet {
    const fn zero() -> Self {
        Self {
            sequence: CountBound::exact(0),
            execute: CountBound::exact(0),
            calls: CountBound::exact(0),
            score_nbt: CountBound::exact(0),
            max_chain: CountBound::exact(0),
            recursive_reachable: false,
            recursive_sequence: None,
            recursive_execute: None,
            recursive_function_invocations: None,
            recursive_score_nbt: None,
            recursive_max_chain: None,
        }
    }

    const fn command(score_nbt: bool) -> Self {
        Self {
            sequence: CountBound::exact(1),
            execute: CountBound::exact(0),
            calls: CountBound::exact(0),
            score_nbt: CountBound::exact(if score_nbt { 1 } else { 0 }),
            max_chain: CountBound::exact(0),
            recursive_reachable: false,
            recursive_sequence: None,
            recursive_execute: None,
            recursive_function_invocations: None,
            recursive_score_nbt: None,
            recursive_max_chain: None,
        }
    }

    const fn execute_stage(charges_sequence: bool) -> Self {
        Self {
            sequence: CountBound::exact(if charges_sequence { 1 } else { 0 }),
            execute: CountBound::exact(1),
            calls: CountBound::exact(0),
            score_nbt: CountBound::exact(0),
            max_chain: CountBound::exact(0),
            recursive_reachable: false,
            recursive_sequence: None,
            recursive_execute: None,
            recursive_function_invocations: None,
            recursive_score_nbt: None,
            recursive_max_chain: None,
        }
    }

    const fn invocation() -> Self {
        Self {
            sequence: CountBound::exact(1),
            execute: CountBound::exact(0),
            calls: CountBound::exact(1),
            score_nbt: CountBound::exact(0),
            max_chain: CountBound::exact(0),
            recursive_reachable: false,
            recursive_sequence: None,
            recursive_execute: None,
            recursive_function_invocations: None,
            recursive_score_nbt: None,
            recursive_max_chain: None,
        }
    }

    const fn recursive_marker() -> Self {
        Self {
            sequence: CountBound::exact(0),
            execute: CountBound::exact(0),
            calls: CountBound::exact(0),
            score_nbt: CountBound::exact(0),
            max_chain: CountBound::exact(0),
            recursive_reachable: true,
            recursive_sequence: Some(CountBound::exact(0)),
            recursive_execute: Some(CountBound::exact(0)),
            recursive_function_invocations: Some(CountBound::exact(0)),
            recursive_score_nbt: Some(CountBound::exact(0)),
            recursive_max_chain: Some(CountBound::exact(0)),
        }
    }

    pub(super) const fn unknown(reason: UnknownCostReason) -> Self {
        Self {
            sequence: CountBound::unknown(0, reason),
            execute: CountBound::unknown(0, reason),
            calls: CountBound::unknown(0, reason),
            score_nbt: CountBound::unknown(0, reason),
            max_chain: CountBound::unknown(0, reason),
            recursive_reachable: false,
            recursive_sequence: None,
            recursive_execute: None,
            recursive_function_invocations: None,
            recursive_score_nbt: None,
            recursive_max_chain: None,
        }
    }

    const fn raw_unknown() -> Self {
        Self {
            sequence: CountBound::unknown(0, UnknownCostReason::RawCommand),
            execute: CountBound::unknown(0, UnknownCostReason::RawCommand),
            calls: CountBound::unknown(0, UnknownCostReason::RawCommand),
            score_nbt: CountBound::unknown(0, UnknownCostReason::RawCommand),
            max_chain: CountBound::unknown(0, UnknownCostReason::RawCommand),
            recursive_reachable: false,
            recursive_sequence: None,
            recursive_execute: None,
            recursive_function_invocations: None,
            recursive_score_nbt: None,
            recursive_max_chain: None,
        }
    }

    const fn external_function_unknown() -> Self {
        let reason = UnknownCostReason::ExternalFunction;
        Self {
            sequence: CountBound::unknown(1, reason),
            execute: CountBound::unknown(0, reason),
            calls: CountBound::unknown(0, reason),
            score_nbt: CountBound::unknown(0, reason),
            max_chain: CountBound::unknown(0, reason),
            recursive_reachable: false,
            recursive_sequence: None,
            recursive_execute: None,
            recursive_function_invocations: None,
            recursive_score_nbt: None,
            recursive_max_chain: None,
        }
    }

    fn clear_recursive_scratch(&mut self) {
        self.recursive_reachable = false;
        self.recursive_sequence = None;
        self.recursive_execute = None;
        self.recursive_function_invocations = None;
        self.recursive_score_nbt = None;
        self.recursive_max_chain = None;
    }
}

impl FunctionFlow {
    const fn empty() -> Self {
        Self {
            exits: [None; FUNCTION_EXIT_COUNT],
            divergence: None,
        }
    }

    fn merged(&self) -> MetricSet {
        let mut merged = merge_cells(&self.exits);
        if let Some(divergence) = self.divergence {
            merge_cell(&mut merged, divergence);
        }
        merged.unwrap_or_else(MetricSet::zero)
    }

    fn clear_recursive_scratch(&mut self) {
        for metrics in self.exits.iter_mut().flatten() {
            metrics.clear_recursive_scratch();
        }
        if let Some(metrics) = &mut self.divergence {
            metrics.clear_recursive_scratch();
        }
    }
}

impl CommandFlow {
    const fn empty() -> Self {
        Self {
            continues: [None; COMMAND_RESULT_COUNT],
            returns: [None; RETURN_EXIT_COUNT],
            divergence: None,
        }
    }

    fn continuing(result: CommandResult, metrics: MetricSet) -> Self {
        let mut output = Self::empty();
        output.continues[result as usize] = Some(metrics);
        output
    }

    fn returning(result: ReturnExit, metrics: MetricSet) -> Self {
        let mut output = Self::empty();
        output.returns[result as usize] = Some(metrics);
        output
    }

    fn unknown(reason: UnknownCostReason) -> Self {
        let metrics = MetricSet::unknown(reason);
        Self {
            continues: [Some(metrics); COMMAND_RESULT_COUNT],
            returns: [Some(metrics); RETURN_EXIT_COUNT],
            divergence: Some(metrics),
        }
    }

    fn continuing_unknown(reason: UnknownCostReason) -> Self {
        Self {
            continues: [Some(MetricSet::unknown(reason)); COMMAND_RESULT_COUNT],
            returns: [None; RETURN_EXIT_COUNT],
            divergence: Some(MetricSet::unknown(reason)),
        }
    }

    fn add_prefix(self, prefix: MetricSet, caps: AnalysisArithmeticCaps) -> Option<Self> {
        let mut output = Self::empty();
        for (destination, value) in output.continues.iter_mut().zip(self.continues) {
            *destination = match value {
                Some(metrics) => Some(add_metrics(prefix, metrics, caps)?),
                None => None,
            };
        }
        for (destination, value) in output.returns.iter_mut().zip(self.returns) {
            *destination = match value {
                Some(metrics) => Some(add_metrics(prefix, metrics, caps)?),
                None => None,
            };
        }
        output.divergence = match self.divergence {
            Some(metrics) => Some(add_metrics(prefix, metrics, caps)?),
            None => None,
        };
        Some(output)
    }

    fn merge(&mut self, other: Self) {
        merge_cell_arrays(&mut self.continues, other.continues);
        merge_cell_arrays(&mut self.returns, other.returns);
        if let Some(divergence) = other.divergence {
            merge_cell(&mut self.divergence, divergence);
        }
    }
}

impl ContextFlow {
    const fn one() -> Self {
        Self {
            alternatives: [None, Some(MetricSet::zero()), None],
            divergence: None,
        }
    }

    fn add_to_all(&mut self, metrics: MetricSet, caps: AnalysisArithmeticCaps) -> Option<()> {
        for value in self.alternatives.iter_mut().flatten() {
            *value = add_metrics(*value, metrics, caps)?;
        }
        Some(())
    }

    fn insert(&mut self, class: ContextClass, metrics: MetricSet) {
        merge_cell(&mut self.alternatives[class as usize], metrics);
    }
}

fn evaluate_function(function: McFunctionId, context: &SolverContext<'_>) -> Option<FunctionFlow> {
    let data = context.program.function(function)?;
    let mut active = Some(MetricSet::zero());
    let mut output = FunctionFlow::empty();
    for (_, command) in data.body().commands() {
        let Some(prefix) = active else {
            break;
        };
        let command = evaluate_command(command.kind(), context)?;
        if let Some(divergence) = command.divergence {
            merge_cell(
                &mut output.divergence,
                add_metrics(prefix, divergence, context.caps)?,
            );
        }
        active = match merge_cells(&command.continues) {
            Some(metrics) => Some(add_metrics(prefix, metrics, context.caps)?),
            None => None,
        };
        for (index, metrics) in command.returns.into_iter().enumerate() {
            let Some(metrics) = metrics else {
                continue;
            };
            let exit = match index {
                0 => FunctionExit::Zero,
                1 => FunctionExit::NonZero,
                2 => FunctionExit::Failure,
                _ => return None,
            };
            merge_cell(
                &mut output.exits[exit as usize],
                add_metrics(prefix, metrics, context.caps)?,
            );
        }
    }
    if let Some(metrics) = active {
        merge_cell(&mut output.exits[FunctionExit::NoResult as usize], metrics);
    }
    Some(output)
}

fn evaluate_command(command: &CommandKind, context: &SolverContext<'_>) -> Option<CommandFlow> {
    context
        .command_transfer_visits
        .set(context.command_transfer_visits.get().checked_add(1)?);
    match command {
        CommandKind::Score(_) | CommandKind::Data(_) => {
            let metrics = MetricSet::command(true);
            let mut output = CommandFlow::continuing(CommandResult::Zero, metrics);
            output.merge(CommandFlow::continuing(CommandResult::NonZero, metrics));
            output.merge(CommandFlow::continuing(CommandResult::Failure, metrics));
            Some(output)
        }
        CommandKind::Raw(_) => {
            let metrics = MetricSet::raw_unknown();
            let mut output = CommandFlow::unknown(UnknownCostReason::RawCommand);
            for cell in output.continues.iter_mut().flatten() {
                *cell = metrics;
            }
            for cell in output.returns.iter_mut().flatten() {
                *cell = metrics;
            }
            Some(output)
        }
        CommandKind::Function(call) => evaluate_callable(call.target(), false, context),
        CommandKind::Return(ReturnCommand::Value(value)) => Some(CommandFlow::returning(
            if *value == 0 {
                ReturnExit::Zero
            } else {
                ReturnExit::NonZero
            },
            MetricSet::zero(),
        )),
        CommandKind::Return(ReturnCommand::Fail) => Some(CommandFlow::returning(
            ReturnExit::Failure,
            MetricSet::zero(),
        )),
        CommandKind::Return(ReturnCommand::Run(nested)) => {
            evaluate_return_run(nested.kind(), context)
        }
        CommandKind::Execute(execute) => evaluate_execute(execute, false, context),
    }
}

fn evaluate_return_run(command: &CommandKind, context: &SolverContext<'_>) -> Option<CommandFlow> {
    if let CommandKind::Function(call) = command {
        return evaluate_callable(call.target(), true, context);
    }
    if let CommandKind::Execute(execute) = command {
        return evaluate_execute(execute, true, context);
    }
    let command = evaluate_command(command, context)?;
    Some(map_command_to_return(&command))
}

fn map_command_to_return(command: &CommandFlow) -> CommandFlow {
    let mut output = CommandFlow::empty();
    output.divergence = command.divergence;
    for (index, metrics) in command.continues.iter().copied().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        let result = match index {
            0 | 3 => ReturnExit::Failure,
            1 => ReturnExit::Zero,
            2 => ReturnExit::NonZero,
            _ => unreachable!("fixed command-result table"),
        };
        merge_cell(&mut output.returns[result as usize], metrics);
    }
    merge_cell_arrays(&mut output.returns, command.returns);
    output
}

fn evaluate_callable(
    target: &CallableRef,
    as_return: bool,
    context: &SolverContext<'_>,
) -> Option<CommandFlow> {
    match target {
        CallableRef::Internal(InternalCallableRef::Function(function)) => {
            let callee = callee_flow(*function, context)?;
            let flow = map_function_call(&callee, as_return, context.caps)?;
            if is_active_region_target(*function, context)? {
                flow.add_prefix(MetricSet::recursive_marker(), context.caps)
            } else {
                Some(flow)
            }
        }
        CallableRef::Internal(InternalCallableRef::Tag(tag)) => {
            evaluate_tag(*tag, as_return, context)
        }
        CallableRef::External(crate::ir::minecraft::ExternalCallableRef::Function(_)) => {
            Some(if as_return {
                let metrics = MetricSet::external_function_unknown();
                let mut flow = CommandFlow::empty();
                flow.returns = [Some(metrics); RETURN_EXIT_COUNT];
                flow.divergence = Some(metrics);
                flow
            } else {
                let metrics = MetricSet::external_function_unknown();
                let mut flow = CommandFlow::empty();
                flow.continues = [Some(metrics); COMMAND_RESULT_COUNT];
                flow.divergence = Some(metrics);
                flow
            })
        }
        CallableRef::External(crate::ir::minecraft::ExternalCallableRef::Tag(_)) => {
            Some(if as_return {
                let metrics = MetricSet::unknown(UnknownCostReason::ExternalFunctionTag);
                let mut flow = CommandFlow::empty();
                flow.returns = [Some(metrics); RETURN_EXIT_COUNT];
                flow.divergence = Some(metrics);
                flow
            } else {
                CommandFlow::continuing_unknown(UnknownCostReason::ExternalFunctionTag)
            })
        }
    }
}

fn callee_flow(function: McFunctionId, context: &SolverContext<'_>) -> Option<FunctionFlow> {
    if context.local_only {
        return Some(FunctionFlow {
            exits: [Some(MetricSet::zero()); FUNCTION_EXIT_COUNT],
            divergence: None,
        });
    }
    let index = usize::try_from(function.index()).ok()?;
    let region = usize::try_from(context.graph.function_regions.get(index)?.index()).ok()?;
    if context.active_region == Some(region) {
        let is_recursive_target = context
            .recursive_targets
            .is_none_or(|targets| targets.contains(&function));
        if !is_recursive_target {
            if let Some(flow) = context
                .regional_solved
                .and_then(|flows| flows.get(&function))
                .copied()
            {
                return Some(flow);
            }
        }
        if let Some(reached) = context.reached_active_calls {
            reached.borrow_mut().insert(function);
        }
        let mask = context.recursive_exit_masks?.get(&function)?;
        return Some(FunctionFlow {
            exits: std::array::from_fn(|exit| mask[exit].then_some(MetricSet::zero())),
            divergence: context
                .divergent_active_calls
                .map_or(Some(MetricSet::zero()), |calls| {
                    calls.contains(&function).then_some(MetricSet::zero())
                }),
        });
    }
    context.solved.get(index).copied().flatten()
}

fn is_active_region_target(function: McFunctionId, context: &SolverContext<'_>) -> Option<bool> {
    if context.local_only {
        return Some(false);
    }
    if let Some(targets) = context.recursive_targets {
        return Some(targets.contains(&function));
    }
    let index = usize::try_from(function.index()).ok()?;
    let region = usize::try_from(context.graph.function_regions.get(index)?.index()).ok()?;
    Some(context.active_region == Some(region))
}

fn map_function_call(
    callee: &FunctionFlow,
    as_return: bool,
    caps: AnalysisArithmeticCaps,
) -> Option<CommandFlow> {
    let mut output = CommandFlow::empty();
    if let Some(divergence) = callee.divergence {
        output.divergence = Some(add_metrics(MetricSet::invocation(), divergence, caps)?);
    }
    for (index, metrics) in callee.exits.iter().copied().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        let metrics = add_metrics(MetricSet::invocation(), metrics, caps)?;
        if as_return {
            let result = match index {
                0 | 3 => ReturnExit::Failure,
                1 => ReturnExit::Zero,
                2 => ReturnExit::NonZero,
                _ => return None,
            };
            merge_cell(&mut output.returns[result as usize], metrics);
        } else {
            let result = match index {
                0 => CommandResult::NoResult,
                1 => CommandResult::Zero,
                2 => CommandResult::NonZero,
                3 => CommandResult::Failure,
                _ => return None,
            };
            merge_cell(&mut output.continues[result as usize], metrics);
        }
    }
    Some(output)
}

fn evaluate_tag(
    tag: FunctionTagId,
    as_return: bool,
    context: &SolverContext<'_>,
) -> Option<CommandFlow> {
    let index = usize::try_from(tag.index()).ok()?;
    let expansion = context.graph.tag_expansions.get(index)?;
    if as_return {
        evaluate_return_tag(&expansion.root_entries, context)
    } else {
        evaluate_ordinary_tag(&expansion.root_entries, context)
    }
}

fn evaluate_return_tag(
    entries: &[TagRootEntry],
    context: &SolverContext<'_>,
) -> Option<CommandFlow> {
    let mut active = Some(MetricSet::zero());
    let mut output = CommandFlow::empty();
    for entry in entries {
        let Some(prefix) = active else {
            break;
        };
        match entry {
            TagRootEntry::Function(function) => {
                let callee = callee_flow(*function, context)?;
                let recursive = if is_active_region_target(*function, context)? {
                    MetricSet::recursive_marker()
                } else {
                    MetricSet::zero()
                };
                let mut next = None;
                if let Some(divergence) = callee.divergence {
                    let member = add_metrics(MetricSet::invocation(), divergence, context.caps)?;
                    let member = add_metrics(member, recursive, context.caps)?;
                    merge_cell(
                        &mut output.divergence,
                        add_metrics(prefix, member, context.caps)?,
                    );
                }
                for (exit, metrics) in callee.exits.into_iter().enumerate() {
                    let Some(metrics) = metrics else {
                        continue;
                    };
                    let member = add_metrics(MetricSet::invocation(), metrics, context.caps)?;
                    let member = add_metrics(member, recursive, context.caps)?;
                    let metrics = add_metrics(prefix, member, context.caps)?;
                    match exit {
                        0 => merge_cell(&mut next, metrics),
                        1 => merge_cell(&mut output.returns[ReturnExit::Zero as usize], metrics),
                        2 => merge_cell(&mut output.returns[ReturnExit::NonZero as usize], metrics),
                        3 => merge_cell(&mut output.returns[ReturnExit::Failure as usize], metrics),
                        _ => return None,
                    }
                }
                active = next;
            }
            TagRootEntry::External { .. } => {
                let metrics = add_metrics(
                    prefix,
                    MetricSet::unknown(UnknownCostReason::ExternalTagEntry),
                    context.caps,
                )?;
                for cell in &mut output.returns {
                    merge_cell(cell, metrics);
                }
                merge_cell(&mut output.divergence, metrics);
                active = Some(metrics);
            }
        }
    }
    if let Some(metrics) = active {
        merge_cell(&mut output.returns[ReturnExit::Failure as usize], metrics);
    }
    Some(output)
}

fn evaluate_ordinary_tag(
    entries: &[TagRootEntry],
    context: &SolverContext<'_>,
) -> Option<CommandFlow> {
    let mut active = Some(MetricSet::zero());
    let mut output = CommandFlow::empty();
    for entry in entries {
        let Some(prefix) = active else {
            break;
        };
        let member = evaluate_ordinary_tag_entry(entry, prefix, &mut output, context)?;
        active = match member.metrics {
            Some(metrics) => Some(add_metrics(prefix, metrics, context.caps)?),
            None => None,
        };
    }
    if let Some(active) = active {
        output.continues[CommandResult::NoResult as usize] = Some(active);
    }
    Some(output)
}

struct TagEntryContinuation {
    metrics: Option<MetricSet>,
}

impl TagEntryContinuation {
    const DIVERGES: Self = Self { metrics: None };

    const fn continues(metrics: MetricSet) -> Self {
        Self {
            metrics: Some(metrics),
        }
    }
}

fn evaluate_ordinary_tag_entry(
    entry: &TagRootEntry,
    prefix: MetricSet,
    output: &mut CommandFlow,
    context: &SolverContext<'_>,
) -> Option<TagEntryContinuation> {
    match entry {
        TagRootEntry::Function(function) => {
            let callee = callee_flow(*function, context)?;
            let recursive = if is_active_region_target(*function, context)? {
                MetricSet::recursive_marker()
            } else {
                MetricSet::zero()
            };
            if let Some(divergence) = callee.divergence {
                let member = add_metrics(MetricSet::invocation(), divergence, context.caps)?;
                let member = add_metrics(member, recursive, context.caps)?;
                merge_cell(
                    &mut output.divergence,
                    add_metrics(prefix, member, context.caps)?,
                );
            }
            let Some(metrics) = merge_cells(&callee.exits) else {
                return Some(TagEntryContinuation::DIVERGES);
            };
            let metrics = add_metrics(MetricSet::invocation(), metrics, context.caps)?;
            Some(TagEntryContinuation::continues(add_metrics(
                metrics,
                recursive,
                context.caps,
            )?))
        }
        TagRootEntry::External { .. } => {
            let unknown = MetricSet::unknown(UnknownCostReason::ExternalTagEntry);
            merge_cell(
                &mut output.divergence,
                add_metrics(prefix, unknown, context.caps)?,
            );
            Some(TagEntryContinuation::continues(unknown))
        }
    }
}

fn evaluate_execute(
    execute: &crate::ir::minecraft::ExecuteCommand,
    as_return: bool,
    context: &SolverContext<'_>,
) -> Option<CommandFlow> {
    let mut contexts = ContextFlow::one();
    for modifier in execute.modifiers().as_slice() {
        let is_function_condition = matches!(
            modifier.kind(),
            ExecuteModifierKind::If(Condition::Function(_))
                | ExecuteModifierKind::Unless(Condition::Function(_))
        );
        contexts.add_to_all(
            MetricSet::execute_stage(!is_function_condition),
            context.caps,
        )?;
        contexts = match modifier.kind() {
            ExecuteModifierKind::As(selector) | ExecuteModifierKind::At(selector) => {
                apply_selector(&contexts, selector.cardinality())?
            }
            ExecuteModifierKind::If(Condition::Function(function)) => {
                apply_function_condition(&contexts, *function, false, context)?
            }
            ExecuteModifierKind::Unless(Condition::Function(function)) => {
                apply_function_condition(&contexts, *function, true, context)?
            }
            ExecuteModifierKind::If(_) | ExecuteModifierKind::Unless(_) => apply_filter(&contexts),
            ExecuteModifierKind::In(_) | ExecuteModifierKind::Store(_, _) => {
                preserve_contexts(&contexts)
            }
        };
    }

    let mut output = CommandFlow::empty();
    output.divergence = contexts.divergence;
    for (index, prefix) in contexts.alternatives.into_iter().enumerate() {
        let Some(prefix) = prefix else {
            continue;
        };
        match index {
            0 => {
                let flow = if as_return {
                    CommandFlow::returning(ReturnExit::Failure, prefix)
                } else {
                    CommandFlow::continuing(CommandResult::NoResult, prefix)
                };
                output.merge(flow);
            }
            1 => {
                let nested = if as_return {
                    evaluate_return_run(execute.run().kind(), context)?
                } else {
                    evaluate_command(execute.run().kind(), context)?
                };
                output.merge(nested.add_prefix(prefix, context.caps)?);
            }
            2 => {
                let nested = if as_return {
                    // `return run execute` schedules only the first surviving source.
                    evaluate_return_run(execute.run().kind(), context)?
                } else {
                    repeat_many(
                        &evaluate_command(execute.run().kind(), context)?,
                        context.caps,
                    )?
                };
                output.merge(nested.add_prefix(prefix, context.caps)?);
            }
            _ => return None,
        }
    }
    Some(output)
}

fn apply_selector(input: &ContextFlow, cardinality: Cardinality) -> Option<ContextFlow> {
    let mut output = ContextFlow {
        alternatives: [None; CONTEXT_CLASS_COUNT],
        divergence: input.divergence,
    };
    for (index, metrics) in input.alternatives.iter().copied().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        let source = match index {
            0 => ContextClass::Zero,
            1 => ContextClass::One,
            2 => ContextClass::Many,
            _ => return None,
        };
        match source {
            ContextClass::Zero => output.insert(ContextClass::Zero, metrics),
            ContextClass::One => {
                output.insert(ContextClass::Zero, metrics);
                let mut one = metrics;
                one.max_chain = maximum_bound(one.max_chain, CountBound::exact(1));
                output.insert(ContextClass::One, one);
                if cardinality == Cardinality::Unbounded {
                    let mut many = metrics;
                    many.max_chain = maximum_bound(
                        many.max_chain,
                        CountBound::no_finite_bound(2, NoFiniteBoundReason::SelectorCardinality),
                    );
                    output.insert(ContextClass::Many, many);
                }
            }
            ContextClass::Many => {
                output.insert(ContextClass::Zero, metrics);
                output.insert(ContextClass::One, metrics);
                output.insert(ContextClass::Many, metrics);
            }
        }
    }
    Some(output)
}

fn apply_filter(input: &ContextFlow) -> ContextFlow {
    let mut output = ContextFlow {
        alternatives: [None; CONTEXT_CLASS_COUNT],
        divergence: input.divergence,
    };
    for (index, metrics) in input.alternatives.iter().copied().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        match index {
            0 => output.insert(ContextClass::Zero, metrics),
            1 => {
                output.insert(ContextClass::Zero, metrics);
                output.insert(
                    ContextClass::One,
                    with_context_peak(metrics, ContextClass::One),
                );
            }
            2 => {
                output.insert(ContextClass::Zero, metrics);
                output.insert(ContextClass::One, metrics);
                output.insert(ContextClass::Many, metrics);
            }
            _ => unreachable!("fixed context table"),
        }
    }
    output
}

fn preserve_contexts(input: &ContextFlow) -> ContextFlow {
    let mut output = *input;
    for (index, metrics) in output.alternatives.iter_mut().enumerate() {
        let Some(value) = metrics else {
            continue;
        };
        let class = match index {
            0 => ContextClass::Zero,
            1 => ContextClass::One,
            2 => ContextClass::Many,
            _ => unreachable!("fixed context table"),
        };
        *value = with_context_peak(*value, class);
    }
    output
}

fn with_context_peak(mut metrics: MetricSet, class: ContextClass) -> MetricSet {
    let peak = match class {
        ContextClass::Zero => CountBound::exact(0),
        ContextClass::One => CountBound::exact(1),
        ContextClass::Many => {
            CountBound::no_finite_bound(2, NoFiniteBoundReason::SelectorCardinality)
        }
    };
    metrics.max_chain = maximum_bound(metrics.max_chain, peak);
    metrics
}

fn apply_function_condition(
    input: &ContextFlow,
    function: McFunctionId,
    unless: bool,
    context: &SolverContext<'_>,
) -> Option<ContextFlow> {
    let mut output = ContextFlow {
        alternatives: [None; CONTEXT_CLASS_COUNT],
        divergence: input.divergence,
    };
    let needs_callee = input.alternatives[ContextClass::One as usize].is_some()
        || input.alternatives[ContextClass::Many as usize].is_some();
    let callee = if needs_callee {
        Some(callee_flow(function, context)?)
    } else {
        None
    };
    let recursive = if needs_callee && is_active_region_target(function, context)? {
        MetricSet::recursive_marker()
    } else {
        MetricSet::zero()
    };
    for (index, prefix) in input.alternatives.iter().copied().enumerate() {
        let Some(prefix) = prefix else {
            continue;
        };
        match index {
            0 => output.insert(ContextClass::Zero, prefix),
            1 => apply_one_function_condition(
                &mut output,
                prefix,
                callee.as_ref()?,
                unless,
                recursive,
                context.caps,
            )?,
            2 => apply_many_function_condition(
                &mut output,
                prefix,
                callee.as_ref()?,
                unless,
                recursive,
                context.caps,
            )?,
            _ => return None,
        }
    }
    Some(output)
}

fn invoked_callee_metrics(
    metrics: MetricSet,
    recursive: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<MetricSet> {
    let metrics = add_metrics(MetricSet::invocation(), metrics, caps)?;
    add_metrics(metrics, recursive, caps)
}

fn apply_one_function_condition(
    output: &mut ContextFlow,
    prefix: MetricSet,
    callee: &FunctionFlow,
    unless: bool,
    recursive: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<()> {
    if let Some(divergence) = callee.divergence {
        let divergence = invoked_callee_metrics(divergence, recursive, caps)?;
        merge_cell(
            &mut output.divergence,
            add_metrics(prefix, divergence, caps)?,
        );
    }
    for (exit, metrics) in callee.exits.into_iter().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        let pass = (exit == FunctionExit::NonZero as usize) ^ unless;
        let metrics = add_metrics(
            prefix,
            invoked_callee_metrics(metrics, recursive, caps)?,
            caps,
        )?;
        let class = if pass {
            ContextClass::One
        } else {
            ContextClass::Zero
        };
        // `execute if|unless function` is a CustomModifierExecutor in Java 26.2.
        // It preserves/filter contexts for later stages but bypasses BuildContexts'
        // ordinary fork-limit check, so its own surviving context is not a checked
        // redirect expansion.
        output.insert(class, metrics);
    }
    Some(())
}

fn apply_many_function_condition(
    output: &mut ContextFlow,
    prefix: MetricSet,
    callee: &FunctionFlow,
    unless: bool,
    recursive: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<()> {
    if let Some(divergence) = callee.divergence {
        let divergence = invoked_callee_metrics(divergence, recursive, caps)?;
        let divergence = scale_metrics_zero_or_many(divergence, caps)?;
        merge_cell(
            &mut output.divergence,
            add_metrics(prefix, divergence, caps)?,
        );
    }
    let mut pass_possible = false;
    let mut fail_possible = false;
    let mut one_call = None;
    for (exit, metrics) in callee.exits.into_iter().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        let pass = (exit == FunctionExit::NonZero as usize) ^ unless;
        pass_possible |= pass;
        fail_possible |= !pass;
        merge_cell(
            &mut one_call,
            invoked_callee_metrics(metrics, recursive, caps)?,
        );
    }
    let Some(one_call) = one_call else {
        return Some(());
    };
    let metrics = add_metrics(prefix, scale_metrics_many(one_call, caps)?, caps)?;
    match (pass_possible, fail_possible) {
        (true, false) => output.insert(ContextClass::Many, metrics),
        (false, true) => output.insert(ContextClass::Zero, metrics),
        (true, true) => {
            output.insert(ContextClass::Zero, metrics);
            output.insert(ContextClass::One, metrics);
            output.insert(ContextClass::Many, metrics);
        }
        (false, false) => unreachable!("a completed predicate exit has one result"),
    }
    Some(())
}

fn repeat_many(flow: &CommandFlow, caps: AnalysisArithmeticCaps) -> Option<CommandFlow> {
    let mut output = CommandFlow::empty();
    for (index, metrics) in flow.continues.iter().copied().enumerate() {
        let Some(metrics) = metrics else {
            continue;
        };
        merge_cell(
            &mut output.continues[index],
            scale_metrics_many(metrics, caps)?,
        );
    }
    let continuation = merge_cells(&flow.continues);
    if let Some(divergence) = flow.divergence {
        merge_cell(&mut output.divergence, divergence);
        if let Some(continuation) = continuation {
            merge_cell(
                &mut output.divergence,
                add_metrics(
                    scale_metrics_zero_or_many(continuation, caps)?,
                    divergence,
                    caps,
                )?,
            );
        }
    }
    for (index, terminal) in flow.returns.iter().copied().enumerate() {
        let Some(terminal) = terminal else {
            continue;
        };
        merge_cell(&mut output.returns[index], terminal);
        if let Some(continuation) = continuation {
            let prefix = scale_metrics_zero_or_many(continuation, caps)?;
            merge_cell(
                &mut output.returns[index],
                add_metrics(prefix, terminal, caps)?,
            );
        }
    }
    output.continues[CommandResult::NoResult as usize] = merge_cells(&output.continues);
    for index in 1..COMMAND_RESULT_COUNT {
        output.continues[index] = None;
    }
    Some(output)
}

#[derive(Clone, Copy)]
struct CycleMetricEffects {
    sequence: CycleMetricEffect,
    execute: CycleMetricEffect,
    function_invocations: CycleMetricEffect,
    score_nbt: CycleMetricEffect,
    max_chain: CountBound,
}

#[derive(Clone, Copy)]
enum CycleMetricEffect {
    Zero,
    Positive,
    NoFinite(NoFiniteBoundReason),
    Unknown(UnknownCostReason),
}

fn cycle_metric_effects<'a>(flows: impl Iterator<Item = &'a FunctionFlow>) -> CycleMetricEffects {
    let mut effects = CycleMetricEffects {
        sequence: CycleMetricEffect::Zero,
        execute: CycleMetricEffect::Zero,
        function_invocations: CycleMetricEffect::Zero,
        score_nbt: CycleMetricEffect::Zero,
        max_chain: CountBound::exact(0),
    };
    for flow in flows {
        for metrics in flow.exits.iter().flatten() {
            if !metrics.recursive_reachable {
                continue;
            }
            effects.sequence = merge_cycle_effect(
                effects.sequence,
                cycle_effect(metrics.recursive_sequence.unwrap_or(CountBound::exact(0))),
            );
            effects.execute = merge_cycle_effect(
                effects.execute,
                cycle_effect(metrics.recursive_execute.unwrap_or(CountBound::exact(0))),
            );
            effects.function_invocations = merge_cycle_effect(
                effects.function_invocations,
                cycle_effect(
                    metrics
                        .recursive_function_invocations
                        .unwrap_or(CountBound::exact(0)),
                ),
            );
            effects.score_nbt = merge_cycle_effect(
                effects.score_nbt,
                cycle_effect(metrics.recursive_score_nbt.unwrap_or(CountBound::exact(0))),
            );
            effects.max_chain = maximum_bound(
                effects.max_chain,
                metrics.recursive_max_chain.unwrap_or(CountBound::exact(0)),
            );
        }
        if let Some(metrics) = &flow.divergence {
            if metrics.recursive_reachable {
                effects.sequence = merge_cycle_effect(
                    effects.sequence,
                    cycle_effect(metrics.recursive_sequence.unwrap_or(CountBound::exact(0))),
                );
                effects.execute = merge_cycle_effect(
                    effects.execute,
                    cycle_effect(metrics.recursive_execute.unwrap_or(CountBound::exact(0))),
                );
                effects.function_invocations = merge_cycle_effect(
                    effects.function_invocations,
                    cycle_effect(
                        metrics
                            .recursive_function_invocations
                            .unwrap_or(CountBound::exact(0)),
                    ),
                );
                effects.score_nbt = merge_cycle_effect(
                    effects.score_nbt,
                    cycle_effect(metrics.recursive_score_nbt.unwrap_or(CountBound::exact(0))),
                );
                effects.max_chain = maximum_bound(
                    effects.max_chain,
                    metrics.recursive_max_chain.unwrap_or(CountBound::exact(0)),
                );
            }
        }
    }
    effects
}

fn cycle_effect(bound: CountBound) -> CycleMetricEffect {
    match bound.upper().kind() {
        CountUpperKind::Finite(0) => CycleMetricEffect::Zero,
        CountUpperKind::Finite(_) | CountUpperKind::AboveAnalysisCap => CycleMetricEffect::Positive,
        CountUpperKind::NoFiniteBoundProven(reason) => CycleMetricEffect::NoFinite(reason),
        CountUpperKind::Unknown(reason) => CycleMetricEffect::Unknown(reason),
    }
}

fn merge_cycle_effect(left: CycleMetricEffect, right: CycleMetricEffect) -> CycleMetricEffect {
    match (left, right) {
        (CycleMetricEffect::Unknown(left), CycleMetricEffect::Unknown(right)) => {
            CycleMetricEffect::Unknown(left.min(right))
        }
        (CycleMetricEffect::Unknown(reason), _) | (_, CycleMetricEffect::Unknown(reason)) => {
            CycleMetricEffect::Unknown(reason)
        }
        (CycleMetricEffect::NoFinite(left), CycleMetricEffect::NoFinite(right)) => {
            CycleMetricEffect::NoFinite(left.min(right))
        }
        (CycleMetricEffect::NoFinite(reason), _) | (_, CycleMetricEffect::NoFinite(reason)) => {
            CycleMetricEffect::NoFinite(reason)
        }
        (CycleMetricEffect::Positive, _) | (_, CycleMetricEffect::Positive) => {
            CycleMetricEffect::Positive
        }
        (CycleMetricEffect::Zero, CycleMetricEffect::Zero) => CycleMetricEffect::Zero,
    }
}

fn component_envelope<'a>(
    flows: impl Iterator<Item = &'a FunctionFlow>,
    caps: AnalysisArithmeticCaps,
) -> Option<MetricSet> {
    let mut envelope = None;
    for flow in flows {
        let metrics = flow.merged();
        envelope = Some(match envelope {
            Some(current) => add_metrics(current, metrics, caps)?,
            None => metrics,
        });
    }
    Some(envelope.unwrap_or_else(MetricSet::zero))
}

fn apply_cycle_bounds(
    flow: &mut FunctionFlow,
    effects: CycleMetricEffects,
    envelope: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<()> {
    for metrics in flow.exits.iter_mut().flatten() {
        apply_metric_cycle_bounds(metrics, effects, envelope, caps)?;
    }
    if let Some(metrics) = &mut flow.divergence {
        apply_metric_cycle_bounds(metrics, effects, envelope, caps)?;
    }
    Some(())
}

fn apply_metric_cycle_bounds(
    metrics: &mut MetricSet,
    effects: CycleMetricEffects,
    envelope: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<()> {
    if !metrics.recursive_reachable {
        return Some(());
    }
    let extended = add_metrics(*metrics, envelope, caps)?;
    metrics.sequence = merge_alternative_bound(metrics.sequence, extended.sequence);
    metrics.execute = merge_alternative_bound(metrics.execute, extended.execute);
    metrics.calls = merge_alternative_bound(metrics.calls, extended.calls);
    metrics.score_nbt = merge_alternative_bound(metrics.score_nbt, extended.score_nbt);
    metrics.max_chain = maximum_bound(metrics.max_chain, extended.max_chain);
    metrics.sequence = apply_cycle_effect(metrics.sequence, effects.sequence);
    metrics.execute = apply_cycle_effect(metrics.execute, effects.execute);
    metrics.calls = apply_cycle_effect(metrics.calls, effects.function_invocations);
    metrics.score_nbt = apply_cycle_effect(metrics.score_nbt, effects.score_nbt);
    metrics.max_chain = maximum_bound(metrics.max_chain, effects.max_chain);
    Some(())
}

fn apply_cycle_effect(bound: CountBound, effect: CycleMetricEffect) -> CountBound {
    match effect {
        CycleMetricEffect::Zero => bound,
        CycleMetricEffect::Positive => cycle_bound(bound, true),
        CycleMetricEffect::NoFinite(reason) => match bound.upper().kind() {
            CountUpperKind::Unknown(reason) => CountBound::unknown(bound.lower(), reason),
            _ => CountBound::no_finite_bound(bound.lower(), reason),
        },
        CycleMetricEffect::Unknown(reason) => match bound.upper().kind() {
            CountUpperKind::Unknown(current) => {
                CountBound::unknown(bound.lower(), current.min(reason))
            }
            _ => CountBound::unknown(bound.lower(), reason),
        },
    }
}

fn bound_may_be_positive(bound: CountBound) -> bool {
    match bound.upper().kind() {
        CountUpperKind::Finite(0) => false,
        CountUpperKind::Finite(_)
        | CountUpperKind::AboveAnalysisCap
        | CountUpperKind::NoFiniteBoundProven(_)
        | CountUpperKind::Unknown(_) => true,
    }
}

fn cycle_bound(bound: CountBound, positive: bool) -> CountBound {
    if !positive {
        return bound;
    }
    match bound.upper().kind() {
        CountUpperKind::Unknown(reason) => CountBound::unknown(bound.lower(), reason),
        _ => CountBound::no_finite_bound(bound.lower(), NoFiniteBoundReason::PositiveCycle),
    }
}

fn scale_metrics_many(metrics: MetricSet, caps: AnalysisArithmeticCaps) -> Option<MetricSet> {
    scale_metrics(
        metrics,
        CountBound::no_finite_bound(2, NoFiniteBoundReason::SelectorCardinality),
        caps,
    )
}

fn scale_metrics_zero_or_many(
    metrics: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<MetricSet> {
    scale_metrics(
        metrics,
        CountBound::no_finite_bound(0, NoFiniteBoundReason::SelectorCardinality),
        caps,
    )
}

fn scale_metrics(
    metrics: MetricSet,
    count: CountBound,
    caps: AnalysisArithmeticCaps,
) -> Option<MetricSet> {
    Some(MetricSet {
        sequence: multiply_bound(metrics.sequence, count, caps.sequence())?,
        execute: multiply_bound(metrics.execute, count, caps.sequence())?,
        calls: multiply_bound(metrics.calls, count, caps.sequence())?,
        score_nbt: multiply_bound(metrics.score_nbt, count, caps.sequence())?,
        max_chain: metrics.max_chain,
        recursive_reachable: metrics.recursive_reachable && bound_may_be_positive(count),
        recursive_sequence: match metrics.recursive_sequence {
            Some(bound) => Some(multiply_bound(bound, count, caps.sequence())?),
            None => None,
        },
        recursive_execute: match metrics.recursive_execute {
            Some(bound) => Some(multiply_bound(bound, count, caps.sequence())?),
            None => None,
        },
        recursive_function_invocations: match metrics.recursive_function_invocations {
            Some(bound) => Some(multiply_bound(bound, count, caps.sequence())?),
            None => None,
        },
        recursive_score_nbt: match metrics.recursive_score_nbt {
            Some(bound) => Some(multiply_bound(bound, count, caps.sequence())?),
            None => None,
        },
        recursive_max_chain: metrics.recursive_max_chain,
    })
}

fn add_metrics(
    left: MetricSet,
    right: MetricSet,
    caps: AnalysisArithmeticCaps,
) -> Option<MetricSet> {
    let recursive_sequence = sequential_recursive_bound(
        left.recursive_sequence,
        left.sequence,
        right.recursive_sequence,
        right.sequence,
        caps.sequence(),
    )?
    .into_option();
    let recursive_execute = sequential_recursive_bound(
        left.recursive_execute,
        left.execute,
        right.recursive_execute,
        right.execute,
        caps.sequence(),
    )?
    .into_option();
    let recursive_function_invocations = sequential_recursive_bound(
        left.recursive_function_invocations,
        left.calls,
        right.recursive_function_invocations,
        right.calls,
        caps.sequence(),
    )?
    .into_option();
    let recursive_score_nbt = sequential_recursive_bound(
        left.recursive_score_nbt,
        left.score_nbt,
        right.recursive_score_nbt,
        right.score_nbt,
        caps.sequence(),
    )?
    .into_option();
    let recursive_max_chain = sequential_recursive_maximum(
        left.recursive_max_chain,
        left.max_chain,
        right.recursive_max_chain,
        right.max_chain,
    );
    Some(MetricSet {
        sequence: add_bound(left.sequence, right.sequence, caps.sequence())?,
        execute: add_bound(left.execute, right.execute, caps.sequence())?,
        calls: add_bound(left.calls, right.calls, caps.sequence())?,
        score_nbt: add_bound(left.score_nbt, right.score_nbt, caps.sequence())?,
        max_chain: maximum_bound(left.max_chain, right.max_chain),
        recursive_reachable: left.recursive_reachable || right.recursive_reachable,
        recursive_sequence,
        recursive_execute,
        recursive_function_invocations,
        recursive_score_nbt,
        recursive_max_chain,
    })
}

fn merge_metrics(left: MetricSet, right: MetricSet) -> MetricSet {
    MetricSet {
        sequence: merge_alternative_bound(left.sequence, right.sequence),
        execute: merge_alternative_bound(left.execute, right.execute),
        calls: merge_alternative_bound(left.calls, right.calls),
        score_nbt: merge_alternative_bound(left.score_nbt, right.score_nbt),
        max_chain: merge_alternative_bound(left.max_chain, right.max_chain),
        recursive_reachable: left.recursive_reachable || right.recursive_reachable,
        recursive_sequence: merge_optional_alternatives(
            left.recursive_sequence,
            right.recursive_sequence,
        ),
        recursive_execute: merge_optional_alternatives(
            left.recursive_execute,
            right.recursive_execute,
        ),
        recursive_function_invocations: merge_optional_alternatives(
            left.recursive_function_invocations,
            right.recursive_function_invocations,
        ),
        recursive_score_nbt: merge_optional_alternatives(
            left.recursive_score_nbt,
            right.recursive_score_nbt,
        ),
        recursive_max_chain: merge_optional_maxima(
            left.recursive_max_chain,
            right.recursive_max_chain,
        ),
    }
}

fn sequential_recursive_maximum(
    left_recursive: Option<CountBound>,
    left_total: CountBound,
    right_recursive: Option<CountBound>,
    right_total: CountBound,
) -> Option<CountBound> {
    let from_left = left_recursive.map(|bound| maximum_bound(bound, right_total));
    let from_right = right_recursive.map(|bound| maximum_bound(left_total, bound));
    merge_optional_maxima(from_left, from_right)
}

fn merge_optional_maxima(
    left: Option<CountBound>,
    right: Option<CountBound>,
) -> Option<CountBound> {
    match (left, right) {
        (Some(left), Some(right)) => Some(merge_alternative_bound(left, right)),
        (Some(bound), None) | (None, Some(bound)) => Some(bound),
        (None, None) => None,
    }
}

enum OptionalCountBound {
    Absent,
    Present(CountBound),
}

impl OptionalCountBound {
    const fn into_option(self) -> Option<CountBound> {
        match self {
            Self::Absent => None,
            Self::Present(bound) => Some(bound),
        }
    }
}

fn sequential_recursive_bound(
    left_recursive: Option<CountBound>,
    left_total: CountBound,
    right_recursive: Option<CountBound>,
    right_total: CountBound,
    cap: u64,
) -> Option<OptionalCountBound> {
    let from_left = match left_recursive {
        Some(bound) => Some(add_bound(bound, right_total, cap)?),
        None => None,
    };
    let from_right = match right_recursive {
        Some(bound) => Some(add_bound(left_total, bound, cap)?),
        None => None,
    };
    Some(match merge_optional_alternatives(from_left, from_right) {
        Some(bound) => OptionalCountBound::Present(bound),
        None => OptionalCountBound::Absent,
    })
}

fn merge_optional_alternatives(
    left: Option<CountBound>,
    right: Option<CountBound>,
) -> Option<CountBound> {
    match (left, right) {
        (Some(left), Some(right)) => Some(merge_alternative_bound(left, right)),
        (Some(bound), None) | (None, Some(bound)) => Some(bound),
        (None, None) => None,
    }
}

fn merge_cells(cells: &[Option<MetricSet>]) -> Option<MetricSet> {
    let mut output = None;
    for metrics in cells.iter().flatten() {
        merge_cell(&mut output, *metrics);
    }
    output
}

fn merge_cell(slot: &mut Option<MetricSet>, metrics: MetricSet) {
    *slot = Some(slot.map_or(metrics, |current| merge_metrics(current, metrics)));
}

fn merge_cell_arrays<const N: usize>(
    destination: &mut [Option<MetricSet>; N],
    source: [Option<MetricSet>; N],
) {
    for (destination, source) in destination.iter_mut().zip(source) {
        if let Some(source) = source {
            merge_cell(destination, source);
        }
    }
}

fn add_bound(left: CountBound, right: CountBound, cap: u64) -> Option<CountBound> {
    let lower = capped_add(left.lower(), right.lower(), cap)?;
    match (left.upper().kind(), right.upper().kind()) {
        (CountUpperKind::Unknown(left), CountUpperKind::Unknown(right)) => {
            Some(CountBound::unknown(lower, left.min(right)))
        }
        (CountUpperKind::Unknown(reason), _) | (_, CountUpperKind::Unknown(reason)) => {
            Some(CountBound::unknown(lower, reason))
        }
        (CountUpperKind::NoFiniteBoundProven(left), CountUpperKind::NoFiniteBoundProven(right)) => {
            Some(CountBound::no_finite_bound(lower, left.min(right)))
        }
        (CountUpperKind::NoFiniteBoundProven(reason), _)
        | (_, CountUpperKind::NoFiniteBoundProven(reason)) => {
            Some(CountBound::no_finite_bound(lower, reason))
        }
        (CountUpperKind::AboveAnalysisCap, _) | (_, CountUpperKind::AboveAnalysisCap) => {
            Some(CountBound::saturated_above_analysis_cap(lower))
        }
        (CountUpperKind::Finite(left), CountUpperKind::Finite(right)) => {
            let upper = left.checked_add(right);
            match upper {
                Some(upper) if upper <= cap => CountBound::finite(lower, upper).ok(),
                Some(upper) => CountBound::above_analysis_cap(lower, upper, cap).ok(),
                None => Some(CountBound::saturated_above_analysis_cap(lower)),
            }
        }
    }
}

fn multiply_bound(left: CountBound, right: CountBound, cap: u64) -> Option<CountBound> {
    if is_exact_zero(left) || is_exact_zero(right) {
        return Some(CountBound::exact(0));
    }
    let lower = capped_multiply(left.lower(), right.lower(), cap)?;
    match (left.upper().kind(), right.upper().kind()) {
        (CountUpperKind::Unknown(left), CountUpperKind::Unknown(right)) => {
            Some(CountBound::unknown(lower, left.min(right)))
        }
        (CountUpperKind::Unknown(reason), _) | (_, CountUpperKind::Unknown(reason)) => {
            Some(CountBound::unknown(lower, reason))
        }
        (CountUpperKind::NoFiniteBoundProven(left), CountUpperKind::NoFiniteBoundProven(right)) => {
            Some(CountBound::no_finite_bound(lower, left.min(right)))
        }
        (CountUpperKind::NoFiniteBoundProven(reason), _)
        | (_, CountUpperKind::NoFiniteBoundProven(reason)) => {
            Some(CountBound::no_finite_bound(lower, reason))
        }
        (CountUpperKind::AboveAnalysisCap, _) | (_, CountUpperKind::AboveAnalysisCap) => {
            Some(CountBound::saturated_above_analysis_cap(lower))
        }
        (CountUpperKind::Finite(left), CountUpperKind::Finite(right)) => {
            match left.checked_mul(right) {
                Some(upper) if upper <= cap => CountBound::finite(lower, upper).ok(),
                Some(upper) => CountBound::above_analysis_cap(lower, upper, cap).ok(),
                None => Some(CountBound::saturated_above_analysis_cap(lower)),
            }
        }
    }
}

fn capped_add(left: u64, right: u64, cap: u64) -> Option<u64> {
    if left > cap || right > cap || left > cap.saturating_sub(right) {
        Some(cap.saturating_add(1))
    } else {
        left.checked_add(right)
    }
}

fn capped_multiply(left: u64, right: u64, cap: u64) -> Option<u64> {
    if left == 0 || right == 0 {
        return Some(0);
    }
    if left > cap || right > cap || left > cap / right {
        Some(cap.saturating_add(1))
    } else {
        left.checked_mul(right)
    }
}

fn is_exact_zero(bound: CountBound) -> bool {
    bound.lower() == 0 && bound.upper().kind() == CountUpperKind::Finite(0)
}

fn merge_alternative_bound(left: CountBound, right: CountBound) -> CountBound {
    let lower = left.lower().min(right.lower());
    merge_upper(lower, left.upper().kind(), right.upper().kind())
}

fn maximum_bound(left: CountBound, right: CountBound) -> CountBound {
    let lower = left.lower().max(right.lower());
    merge_upper(lower, left.upper().kind(), right.upper().kind())
}

fn merge_upper(lower: u64, left: CountUpperKind, right: CountUpperKind) -> CountBound {
    match (left, right) {
        (CountUpperKind::Unknown(left), CountUpperKind::Unknown(right)) => {
            CountBound::unknown(lower, left.min(right))
        }
        (CountUpperKind::Unknown(reason), _) | (_, CountUpperKind::Unknown(reason)) => {
            CountBound::unknown(lower, reason)
        }
        (CountUpperKind::NoFiniteBoundProven(left), CountUpperKind::NoFiniteBoundProven(right)) => {
            CountBound::no_finite_bound(lower, left.min(right))
        }
        (CountUpperKind::NoFiniteBoundProven(reason), _)
        | (_, CountUpperKind::NoFiniteBoundProven(reason)) => {
            CountBound::no_finite_bound(lower, reason)
        }
        (CountUpperKind::AboveAnalysisCap, _) | (_, CountUpperKind::AboveAnalysisCap) => {
            CountBound::saturated_above_analysis_cap(lower)
        }
        (CountUpperKind::Finite(left), CountUpperKind::Finite(right)) => {
            CountBound::finite(lower, left.max(right)).expect("merged finite ranges remain ordered")
        }
    }
}

fn region_edges(graph: &ExecutionGraph) -> Option<Vec<Vec<usize>>> {
    let mut output = vec![vec![]; graph.components.len()];
    for (caller, callees) in graph.outgoing.iter().enumerate() {
        let caller_region = usize::try_from(graph.function_regions.get(caller)?.index()).ok()?;
        let mut seen = HashSet::new();
        for callee in callees {
            let callee_index = usize::try_from(callee.index()).ok()?;
            let destination_region =
                usize::try_from(graph.function_regions.get(callee_index)?.index()).ok()?;
            if caller_region != destination_region && seen.insert(destination_region) {
                output[caller_region].push(destination_region);
            }
        }
    }
    Some(output)
}

fn region_finish_order(edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    let mut visited = vec![false; edges.len()];
    let mut output = vec![];
    for root in 0..edges.len() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        let mut stack = vec![(root, 0usize)];
        while let Some((region, next)) = stack.last_mut() {
            if *next < edges[*region].len() {
                let callee = edges[*region][*next];
                *next += 1;
                if !*visited.get(callee)? {
                    visited[callee] = true;
                    stack.push((callee, 0));
                }
            } else {
                output.push(*region);
                stack.pop();
            }
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_arithmetic_cap_still_saturates_without_construction_failure() {
        let sum = add_bound(CountBound::exact(u64::MAX), CountBound::exact(1), u64::MAX).unwrap();
        assert_eq!(sum.lower(), u64::MAX);
        assert_eq!(sum.upper().kind(), CountUpperKind::AboveAnalysisCap);

        let product =
            multiply_bound(CountBound::exact(u64::MAX), CountBound::exact(2), u64::MAX).unwrap();
        assert_eq!(product.lower(), u64::MAX);
        assert_eq!(product.upper().kind(), CountUpperKind::AboveAnalysisCap);
    }
}
