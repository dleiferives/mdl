use std::collections::{HashMap, HashSet};

use crate::entity::EntityId;
use crate::ir::minecraft::{
    ExternalCallableRef, ExternalTagRequirement, FunctionTagEntryKind, FunctionTagId,
    InternalCallableRef, McFunctionId, MinecraftProgram,
};

use super::{CommandOutcome, CostRegionId, FunctionLocalSummary, InternalCallSite};

#[derive(Debug)]
pub(super) struct ExecutionGraph {
    pub(super) outgoing: Vec<Vec<McFunctionId>>,
    pub(super) components: Vec<GraphComponent>,
    pub(super) function_regions: Vec<CostRegionId>,
    pub(super) retained_edges: usize,
    pub(super) tag_expansion_entries: usize,
    pub(super) tag_expansions: Vec<TagExpansion>,
}

#[derive(Debug)]
pub(super) struct GraphComponent {
    pub(super) id: CostRegionId,
    pub(super) functions: Vec<McFunctionId>,
    pub(super) cyclic: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum GraphBuildError {
    InvalidFunctionId,
    InvalidTagId,
    CyclicTag,
    EntityLimit,
    AnalysisLimit,
}

#[derive(Clone, Debug, Default)]
pub(super) struct TagExpansion {
    pub(super) functions: Vec<McFunctionId>,
    pub(super) root_entries: Vec<TagRootEntry>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) enum TagRootEntry {
    Function(McFunctionId),
    External {
        target: ExternalCallableRef,
        requirement: ExternalTagRequirement,
    },
}

impl ExecutionGraph {
    pub(super) fn build(
        program: &MinecraftProgram,
        locals: &[FunctionLocalSummary],
        additional_budget: usize,
    ) -> Result<Self, GraphBuildError> {
        let function_count = program.functions().len();
        if locals.len() != function_count {
            return Err(GraphBuildError::EntityLimit);
        }
        let tag_count = program.function_tags().len();
        let mut budget = additional_budget;
        let tag_expansions = expand_tags(program, function_count, tag_count, &mut budget)?;
        let tag_expansion_entries =
            tag_expansions.iter().try_fold(0usize, |total, expansion| {
                total
                    .checked_add(expansion.root_entries.len())
                    .ok_or(GraphBuildError::EntityLimit)
            })?;
        let mut outgoing = vec![vec![]; function_count];
        let mut retained_edges = 0usize;
        for (function, _) in program.functions() {
            let index =
                index(function, function_count).ok_or(GraphBuildError::InvalidFunctionId)?;
            let mut seen = HashSet::new();
            let local = locals.get(index).ok_or(GraphBuildError::EntityLimit)?;
            let mut reachable = true;
            for step in local.steps() {
                if !reachable {
                    break;
                }
                for call in step.internal_call_sites() {
                    match *call {
                        InternalCallSite::Function { target, .. } => {
                            push_unique_charged(
                                target,
                                &mut seen,
                                &mut outgoing[index],
                                &mut budget,
                            )?;
                        }
                        InternalCallSite::Tag { target, .. } => {
                            let tag = tag_expansions
                                .get(
                                    usize::try_from(target.index())
                                        .map_err(|_| GraphBuildError::InvalidTagId)?,
                                )
                                .ok_or(GraphBuildError::InvalidTagId)?;
                            for function in &tag.functions {
                                push_unique_charged(
                                    *function,
                                    &mut seen,
                                    &mut outgoing[index],
                                    &mut budget,
                                )?;
                            }
                        }
                    }
                }
                reachable = step.outcomes().iter().any(|outcome| {
                    matches!(outcome, CommandOutcome::Continue | CommandOutcome::NoResult)
                });
            }
            retained_edges = retained_edges
                .checked_add(outgoing[index].len())
                .ok_or(GraphBuildError::EntityLimit)?;
        }

        let (components, function_regions) = condense(&outgoing)?;
        Ok(Self {
            outgoing,
            components,
            function_regions,
            retained_edges,
            tag_expansion_entries,
            tag_expansions,
        })
    }
}

fn expand_tags(
    program: &MinecraftProgram,
    function_count: usize,
    tag_count: usize,
    budget: &mut usize,
) -> Result<Vec<TagExpansion>, GraphBuildError> {
    let mut dependencies = vec![vec![]; tag_count];
    for (tag, data) in program.function_tags() {
        let tag_index = index(tag, tag_count).ok_or(GraphBuildError::InvalidTagId)?;
        for entry in data.entries() {
            if let FunctionTagEntryKind::Internal(InternalCallableRef::Tag(dependency)) =
                entry.kind()
            {
                if index(*dependency, tag_count).is_none() {
                    return Err(GraphBuildError::InvalidTagId);
                }
                dependencies[tag_index].push(*dependency);
            }
        }
    }
    let order = tag_postorder(&dependencies)?;
    let mut expansions = vec![TagExpansion::default(); tag_count];
    for tag in order {
        let tag_index = index(tag, tag_count).ok_or(GraphBuildError::InvalidTagId)?;
        let data = program
            .function_tag(tag)
            .ok_or(GraphBuildError::InvalidTagId)?;
        let mut seen_functions = HashSet::new();
        let mut seen_external = HashMap::new();
        let mut expansion = TagExpansion::default();
        for entry in data.entries() {
            match entry.kind() {
                FunctionTagEntryKind::Internal(InternalCallableRef::Function(function)) => {
                    if index(*function, function_count).is_none() {
                        return Err(GraphBuildError::InvalidFunctionId);
                    }
                    let entry = TagRootEntry::Function(*function);
                    if seen_functions.insert(*function) {
                        charge(budget)?;
                        expansion.functions.push(*function);
                        expansion.root_entries.push(entry);
                    }
                }
                FunctionTagEntryKind::Internal(InternalCallableRef::Tag(nested)) => {
                    let nested_index =
                        index(*nested, tag_count).ok_or(GraphBuildError::InvalidTagId)?;
                    let nested = &expansions[nested_index];
                    for entry in &nested.root_entries {
                        match entry {
                            TagRootEntry::Function(function) => {
                                if seen_functions.insert(*function) {
                                    charge(budget)?;
                                    expansion.functions.push(*function);
                                    expansion.root_entries.push(entry.clone());
                                }
                            }
                            TagRootEntry::External {
                                target,
                                requirement,
                            } => push_external_root(
                                target,
                                *requirement,
                                &mut seen_external,
                                &mut expansion.root_entries,
                                budget,
                            )?,
                        }
                    }
                }
                FunctionTagEntryKind::External {
                    target,
                    requirement,
                } => {
                    push_external_root(
                        target,
                        *requirement,
                        &mut seen_external,
                        &mut expansion.root_entries,
                        budget,
                    )?;
                }
            }
        }
        expansions[tag_index] = expansion;
    }
    Ok(expansions)
}

fn push_external_root(
    target: &ExternalCallableRef,
    requirement: ExternalTagRequirement,
    seen: &mut HashMap<ExternalCallableRef, usize>,
    output: &mut Vec<TagRootEntry>,
    budget: &mut usize,
) -> Result<(), GraphBuildError> {
    if let Some(index) = seen.get(target).copied() {
        if requirement == ExternalTagRequirement::Required {
            let Some(TagRootEntry::External { requirement, .. }) = output.get_mut(index) else {
                unreachable!("external tag-entry index always names an external root")
            };
            *requirement = ExternalTagRequirement::Required;
        }
        return Ok(());
    }
    charge(budget)?;
    seen.insert(target.clone(), output.len());
    output.push(TagRootEntry::External {
        target: target.clone(),
        requirement,
    });
    Ok(())
}

fn tag_postorder(
    dependencies: &[Vec<FunctionTagId>],
) -> Result<Vec<FunctionTagId>, GraphBuildError> {
    let mut state = vec![0u8; dependencies.len()];
    let mut order = Vec::with_capacity(dependencies.len());
    for root_index in 0..dependencies.len() {
        if state[root_index] != 0 {
            continue;
        }
        state[root_index] = 1;
        let root = FunctionTagId::from_index(
            u32::try_from(root_index).map_err(|_| GraphBuildError::EntityLimit)?,
        );
        let mut stack = vec![(root, 0usize)];
        while let Some((tag, next)) = stack.last_mut() {
            let tag_index = index(*tag, dependencies.len()).ok_or(GraphBuildError::InvalidTagId)?;
            if *next < dependencies[tag_index].len() {
                let dependency = dependencies[tag_index][*next];
                *next += 1;
                let dependency_index =
                    index(dependency, dependencies.len()).ok_or(GraphBuildError::InvalidTagId)?;
                match state[dependency_index] {
                    0 => {
                        state[dependency_index] = 1;
                        stack.push((dependency, 0));
                    }
                    1 => return Err(GraphBuildError::CyclicTag),
                    _ => {}
                }
            } else {
                state[tag_index] = 2;
                order.push(*tag);
                stack.pop();
            }
        }
    }
    Ok(order)
}

fn condense(
    outgoing: &[Vec<McFunctionId>],
) -> Result<(Vec<GraphComponent>, Vec<CostRegionId>), GraphBuildError> {
    let mut incoming = vec![vec![]; outgoing.len()];
    for (caller_index, callees) in outgoing.iter().enumerate() {
        let caller = function_id(caller_index)?;
        for callee in callees {
            let callee_index =
                index(*callee, outgoing.len()).ok_or(GraphBuildError::InvalidFunctionId)?;
            incoming[callee_index].push(caller);
        }
    }
    let finish_order = finish_order(outgoing)?;
    let mut assigned = vec![false; outgoing.len()];
    let mut components = vec![];
    let mut function_regions = vec![CostRegionId::from_index(0); outgoing.len()];
    for root in finish_order.into_iter().rev() {
        let root_index = index(root, outgoing.len()).ok_or(GraphBuildError::InvalidFunctionId)?;
        if assigned[root_index] {
            continue;
        }
        assigned[root_index] = true;
        let mut functions = vec![];
        let mut stack = vec![(root, 0usize)];
        while let Some((function, next)) = stack.last_mut() {
            let function_index =
                index(*function, outgoing.len()).ok_or(GraphBuildError::InvalidFunctionId)?;
            if *next < incoming[function_index].len() {
                let caller = incoming[function_index][*next];
                *next += 1;
                let caller_index =
                    index(caller, outgoing.len()).ok_or(GraphBuildError::InvalidFunctionId)?;
                if !assigned[caller_index] {
                    assigned[caller_index] = true;
                    stack.push((caller, 0));
                }
            } else {
                functions.push(*function);
                stack.pop();
            }
        }
        functions.sort_unstable();
        let id = CostRegionId::from_index(
            u32::try_from(components.len()).map_err(|_| GraphBuildError::EntityLimit)?,
        );
        for function in &functions {
            let function_index =
                index(*function, outgoing.len()).ok_or(GraphBuildError::InvalidFunctionId)?;
            function_regions[function_index] = id;
        }
        let cyclic = functions.len() > 1
            || functions.first().is_some_and(|function| {
                index(*function, outgoing.len())
                    .is_some_and(|function_index| outgoing[function_index].contains(function))
            });
        components.push(GraphComponent {
            id,
            functions,
            cyclic,
        });
    }
    Ok((components, function_regions))
}

fn finish_order(outgoing: &[Vec<McFunctionId>]) -> Result<Vec<McFunctionId>, GraphBuildError> {
    let mut visited = vec![false; outgoing.len()];
    let mut order = Vec::with_capacity(outgoing.len());
    for root_index in 0..outgoing.len() {
        if visited[root_index] {
            continue;
        }
        visited[root_index] = true;
        let root = function_id(root_index)?;
        let mut stack = vec![(root, 0usize)];
        while let Some((function, next)) = stack.last_mut() {
            let function_index =
                index(*function, outgoing.len()).ok_or(GraphBuildError::InvalidFunctionId)?;
            if *next < outgoing[function_index].len() {
                let callee = outgoing[function_index][*next];
                *next += 1;
                let callee_index =
                    index(callee, outgoing.len()).ok_or(GraphBuildError::InvalidFunctionId)?;
                if !visited[callee_index] {
                    visited[callee_index] = true;
                    stack.push((callee, 0));
                }
            } else {
                order.push(*function);
                stack.pop();
            }
        }
    }
    Ok(order)
}

fn push_unique_charged(
    function: McFunctionId,
    seen: &mut HashSet<McFunctionId>,
    output: &mut Vec<McFunctionId>,
    budget: &mut usize,
) -> Result<(), GraphBuildError> {
    if seen.insert(function) {
        charge(budget)?;
        output.push(function);
    }
    Ok(())
}

fn charge(budget: &mut usize) -> Result<(), GraphBuildError> {
    *budget = budget
        .checked_sub(1)
        .ok_or(GraphBuildError::AnalysisLimit)?;
    Ok(())
}

fn function_id(index: usize) -> Result<McFunctionId, GraphBuildError> {
    Ok(McFunctionId::from_index(
        u32::try_from(index).map_err(|_| GraphBuildError::EntityLimit)?,
    ))
}

fn index<I: EntityId>(id: I, len: usize) -> Option<usize> {
    usize::try_from(id.index())
        .ok()
        .filter(|index| *index < len)
}

#[cfg(test)]
mod tests {
    use crate::entity::EntityId;
    use crate::ir::minecraft::{
        CommandKind, CommandNode, ExternalCallableRef, ExternalTagRequirement, FunctionCall,
        FunctionResourceId, FunctionTagEntry, FunctionTagMerge, FunctionTagResourceId,
        InternalCallableRef, MinecraftProgramBuilder,
    };
    use crate::source::OriginId;
    use crate::target::JavaEditionTarget;

    use super::{ExecutionGraph, TagRootEntry};

    #[test]
    #[allow(clippy::too_many_lines)]
    fn expands_nested_tags_once_and_condenses_recursive_functions() {
        let mut builder = MinecraftProgramBuilder::new(JavaEditionTarget::V26_2);
        let entry = builder
            .declare_function(
                FunctionResourceId::parse("mdl:entry").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let left = builder
            .declare_function(
                FunctionResourceId::parse("mdl:left").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let right = builder
            .declare_function(
                FunctionResourceId::parse("mdl:right").unwrap(),
                OriginId::UNKNOWN,
            )
            .unwrap();
        let inner = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:inner").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let outer = builder
            .declare_function_tag(
                FunctionTagResourceId::parse("mdl:outer").unwrap(),
                OriginId::UNKNOWN,
                FunctionTagMerge::Append,
            )
            .unwrap();
        let call = |target| {
            CommandNode::new(
                CommandKind::Function(FunctionCall::new(target)),
                OriginId::UNKNOWN,
            )
            .unwrap()
        };
        let mut body = builder.begin_function(entry).unwrap();
        body.push(call(InternalCallableRef::Tag(outer).into()))
            .unwrap();
        body.finish();
        let mut body = builder.begin_function(left).unwrap();
        body.push(call(InternalCallableRef::Function(right).into()))
            .unwrap();
        body.finish();
        let mut body = builder.begin_function(right).unwrap();
        body.push(call(InternalCallableRef::Function(left).into()))
            .unwrap();
        body.finish();
        let mut entries = builder.begin_function_tag(inner).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(left),
            OriginId::UNKNOWN,
        ));
        entries.push(FunctionTagEntry::external(
            ExternalCallableRef::Function(FunctionResourceId::parse("other:f").unwrap()),
            ExternalTagRequirement::Required,
            OriginId::UNKNOWN,
        ));
        entries.finish();
        let mut entries = builder.begin_function_tag(outer).unwrap();
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Tag(inner),
            OriginId::UNKNOWN,
        ));
        entries.push(FunctionTagEntry::internal(
            InternalCallableRef::Function(right),
            OriginId::UNKNOWN,
        ));
        entries.push(FunctionTagEntry::external(
            ExternalCallableRef::Function(FunctionResourceId::parse("other:f").unwrap()),
            ExternalTagRequirement::Optional,
            OriginId::UNKNOWN,
        ));
        entries.finish();

        let program = builder.finish().unwrap();
        let locals = crate::analysis::minecraft::local::summarize_functions(&program).unwrap();
        let graph = ExecutionGraph::build(&program, &locals, usize::MAX).unwrap();
        assert_eq!(graph.outgoing[0], [left, right]);
        assert_eq!(graph.retained_edges, 4);
        assert_eq!(graph.tag_expansion_entries, 5);
        assert_eq!(
            graph.tag_expansions[usize::try_from(outer.index()).unwrap()]
                .root_entries
                .len(),
            3
        );
        assert!(matches!(
            &graph.tag_expansions[usize::try_from(outer.index()).unwrap()].root_entries[1],
            TagRootEntry::External {
                target: ExternalCallableRef::Function(resource),
                requirement: ExternalTagRequirement::Required,
            } if resource == &FunctionResourceId::parse("other:f").unwrap()
        ));
        assert_eq!(graph.components.len(), 2);
        let recursive = graph
            .components
            .iter()
            .find(|component| component.functions.len() == 2)
            .unwrap();
        assert_eq!(recursive.functions, [left, right]);
        assert!(recursive.cyclic);
        assert_eq!(
            graph.function_regions[usize::try_from(left.index()).unwrap()],
            recursive.id
        );
    }
}
