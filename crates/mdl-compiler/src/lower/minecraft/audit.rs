use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::{EntityId, EntityVec};
use crate::ir::core::{CoreOp, CoreProgram, FunctionId, TerminatorKind};

use super::analysis::{CallSite, SemanticInventory};

/// One reachable call-graph edge with its actionable Core site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CallGraphEdge {
    caller: FunctionId,
    callee: FunctionId,
    site: CallSite,
}

/// Disposable dense reachable-call adjacency in both directions.
struct CallGraph {
    forward: EntityVec<FunctionId, Box<[CallGraphEdge]>>,
    reverse: EntityVec<FunctionId, Box<[CallGraphEdge]>>,
}

impl CallGraph {
    fn new(core: &CoreProgram, inventory: &SemanticInventory) -> Result<Self, Diagnostic> {
        let mut forward = vec![Vec::new(); core.len()];
        let mut reverse = vec![Vec::new(); core.len()];
        for (caller, declaration) in core.functions() {
            let emitted = inventory.function(caller).ok_or_else(|| {
                Diagnostic::new(
                    "lower.invalid-analysis",
                    format!("semantic inventory is missing {caller:?}"),
                    declaration.origin(),
                )
            })?;
            for site in emitted.call_sites().iter().copied() {
                let edge = CallGraphEdge {
                    caller,
                    callee: site.callee(),
                    site,
                };
                slot_mut(&mut forward, caller)
                    .ok_or_else(|| {
                        invalid_edge_diagnostic(edge, "caller is outside the function table")
                    })?
                    .push(edge);
                slot_mut(&mut reverse, edge.callee)
                    .ok_or_else(|| {
                        invalid_edge_diagnostic(edge, "callee is outside the function table")
                    })?
                    .push(edge);
            }
        }
        Ok(Self {
            forward: EntityVec::from_constrained_values(
                forward.into_iter().map(Vec::into_boxed_slice).collect(),
            ),
            reverse: EntityVec::from_constrained_values(
                reverse.into_iter().map(Vec::into_boxed_slice).collect(),
            ),
        })
    }

    fn outgoing(&self, function: FunctionId) -> &[CallGraphEdge] {
        self.forward.get(function).map_or(&[], AsRef::as_ref)
    }

    fn incoming(&self, function: FunctionId) -> &[CallGraphEdge] {
        self.reverse.get(function).map_or(&[], AsRef::as_ref)
    }
}

/// Audits target vocabulary and the fixed-slot non-recursive call ABI.
///
/// The call graph and iterative SCC state are disposable proof scratch: successful
/// auditing returns only `()` so no physical planning phase can accidentally treat
/// an empty token as retained legality evidence.
pub(crate) fn audit_legality(
    core: &CoreProgram,
    inventory: &SemanticInventory,
) -> Result<(), Diagnostics> {
    let call_graph = CallGraph::new(core, inventory).map_err(|finding| {
        Diagnostics::from_findings(vec![finding])
            .expect("one invariant finding always forms diagnostics")
    })?;
    let mut findings = audit_reachable_vocabulary(core, inventory);
    findings.extend(recursion_findings(core, &call_graph));
    drop(call_graph);
    match Diagnostics::from_findings(findings) {
        Some(diagnostics) => Err(diagnostics),
        None => Ok(()),
    }
}

fn audit_reachable_vocabulary(
    core: &CoreProgram,
    inventory: &SemanticInventory,
) -> Vec<Diagnostic> {
    let mut findings = Vec::new();
    for (function, declaration) in core.functions() {
        let Some(body) = declaration.body() else {
            continue;
        };
        let Some(emitted) = inventory.function(function) else {
            continue;
        };
        for instruction in emitted.reachable_instructions().iter().copied() {
            let Some(instruction_data) = body.instruction(instruction) else {
                continue;
            };
            match instruction_data.op() {
                CoreOp::BoolConstant(_)
                | CoreOp::I32Constant(_)
                | CoreOp::I32AddWrapping
                | CoreOp::I32AddOverflowing
                | CoreOp::I32Compare(_)
                | CoreOp::BoolNot
                | CoreOp::Call(_) => {}
            }
        }
        for block in emitted.reachable_blocks().iter().copied() {
            let Some(data) = body.block(block) else {
                continue;
            };
            let Some(terminator) = data.terminator() else {
                continue;
            };
            match terminator.kind() {
                TerminatorKind::Jump(_)
                | TerminatorKind::Branch { .. }
                | TerminatorKind::Return(_) => {}
                TerminatorKind::Unreachable => findings.push(Diagnostic::new(
                    "lower.reachable-unreachable",
                    format!("entry-reachable {function:?} {block:?} cannot be lowered"),
                    terminator.origin(),
                )),
            }
        }
    }
    findings
}

fn recursion_findings(core: &CoreProgram, graph: &CallGraph) -> Vec<Diagnostic> {
    let mut components = strongly_connected_components(core, graph)
        .into_iter()
        .filter(|component| is_recursive(component, graph))
        .collect::<Vec<_>>();
    components.sort_by_key(|component| component[0]);

    components
        .into_iter()
        .filter_map(|component| {
            let anchor = earliest_internal_edge(&component, graph)?;
            let members = component
                .iter()
                .map(|function| format!("{function:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            Some(Diagnostic::new(
                "lower.recursive-call-abi",
                format!("fixed-slot call ABI cannot lower recursive component [{members}]"),
                anchor.site.origin(),
            ))
        })
        .collect()
}

fn strongly_connected_components(core: &CoreProgram, graph: &CallGraph) -> Vec<Vec<FunctionId>> {
    let mut visited = vec![false; core.len()];
    let mut finish_order = Vec::with_capacity(core.len());
    for root in core.functions.keys() {
        if is_marked(&visited, root) {
            continue;
        }
        mark(&mut visited, root);
        let mut stack = vec![(root, 0_usize)];
        while let Some((function, next_edge)) = stack.last_mut() {
            let outgoing = graph.outgoing(*function);
            if *next_edge < outgoing.len() {
                let callee = outgoing[*next_edge].callee;
                *next_edge += 1;
                if !is_marked(&visited, callee) {
                    mark(&mut visited, callee);
                    stack.push((callee, 0));
                }
            } else {
                finish_order.push(*function);
                stack.pop();
            }
        }
    }

    let mut assigned = vec![false; core.len()];
    let mut components = Vec::new();
    for root in finish_order.into_iter().rev() {
        if is_marked(&assigned, root) {
            continue;
        }
        mark(&mut assigned, root);
        let mut component = Vec::new();
        let mut stack = vec![(root, 0_usize)];
        while let Some((function, next_edge)) = stack.last_mut() {
            let incoming = graph.incoming(*function);
            if *next_edge < incoming.len() {
                let caller = incoming[*next_edge].caller;
                *next_edge += 1;
                if !is_marked(&assigned, caller) {
                    mark(&mut assigned, caller);
                    stack.push((caller, 0));
                }
            } else {
                component.push(*function);
                stack.pop();
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components
}

fn is_recursive(component: &[FunctionId], graph: &CallGraph) -> bool {
    component.len() > 1
        || component.first().is_some_and(|function| {
            graph
                .outgoing(*function)
                .iter()
                .any(|edge| edge.callee == *function)
        })
}

fn earliest_internal_edge(component: &[FunctionId], graph: &CallGraph) -> Option<CallGraphEdge> {
    component.iter().find_map(|caller| {
        graph
            .outgoing(*caller)
            .iter()
            .find(|edge| component.binary_search(&edge.callee).is_ok())
            .copied()
    })
}

fn slot_mut<I: EntityId, T>(values: &mut [T], id: I) -> Option<&mut T> {
    usize::try_from(id.index())
        .ok()
        .and_then(|index| values.get_mut(index))
}

fn is_marked(values: &[bool], function: FunctionId) -> bool {
    usize::try_from(function.index())
        .ok()
        .and_then(|index| values.get(index))
        .copied()
        .unwrap_or(false)
}

fn mark(values: &mut [bool], function: FunctionId) {
    if let Some(value) = usize::try_from(function.index())
        .ok()
        .and_then(|index| values.get_mut(index))
    {
        *value = true;
    }
}

fn invalid_edge_diagnostic(edge: CallGraphEdge, reason: &str) -> Diagnostic {
    Diagnostic::new(
        "lower.invalid-call-graph",
        format!(
            "call {:?} in {:?} from {:?} to {:?} is invalid: {reason}",
            edge.site.instruction(),
            edge.site.block(),
            edge.caller,
            edge.callee
        ),
        edge.site.origin(),
    )
}

#[cfg(test)]
mod tests {
    use super::audit_legality;
    use crate::ir::core::{CoreProgram, FunctionBuilder, FunctionId, Terminator, TerminatorKind};
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::source::{Origin, OriginId, SourceContext};

    #[test]
    fn accepts_forward_and_nested_acyclic_calls() {
        let (program, _) = call_graph_program(&[vec![1], vec![2], vec![]]);
        let inventory = SemanticInventory::new(&program).unwrap();

        assert_eq!(audit_legality(&program, &inventory), Ok(()));
    }

    #[test]
    fn rejects_self_recursion_once() {
        let (program, functions) = call_graph_program(&[vec![0]]);
        let inventory = SemanticInventory::new(&program).unwrap();

        let diagnostics = audit_legality(&program, &inventory).unwrap_err();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics.findings()[0].code(), "lower.recursive-call-abi");
        assert!(
            diagnostics.findings()[0]
                .message()
                .contains(&format!("{:?}", functions[0]))
        );
    }

    #[test]
    fn rejects_mutual_recursion_once() {
        let (program, _) = call_graph_program(&[vec![1], vec![0]]);
        let inventory = SemanticInventory::new(&program).unwrap();

        let diagnostics = audit_legality(&program, &inventory).unwrap_err();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics.findings()[0].code(), "lower.recursive-call-abi");
    }

    #[test]
    fn unreachable_self_call_does_not_create_a_call_graph_edge() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("entry"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let dead = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(dead).unwrap();
        builder.call(function, vec![], OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        let inventory = SemanticInventory::new(&program).unwrap();

        assert_eq!(audit_legality(&program, &inventory), Ok(()));
    }

    #[test]
    fn recursion_uses_the_earliest_in_component_call_origin() {
        let mut sources = SourceContext::new();
        let first_origin = sources.add_origin(Origin::Unknown).unwrap();
        let later_origin = sources.add_origin(Origin::Unknown).unwrap();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("entry"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        builder.call(function, vec![], first_origin).unwrap();
        builder.call(function, vec![], later_origin).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();
        let inventory = SemanticInventory::new(&program).unwrap();

        let diagnostics = audit_legality(&program, &inventory).unwrap_err();

        assert_eq!(diagnostics.findings()[0].origin(), first_origin);
    }

    #[test]
    fn independent_recursive_components_are_sorted_by_lowest_function() {
        let (program, functions) =
            call_graph_program(&[vec![1], vec![0], vec![2], vec![4], vec![3]]);
        let inventory = SemanticInventory::new(&program).unwrap();

        let diagnostics = audit_legality(&program, &inventory).unwrap_err();

        assert_eq!(diagnostics.len(), 3);
        for (finding, first_member) in
            diagnostics
                .findings()
                .iter()
                .zip([functions[0], functions[2], functions[3]])
        {
            assert!(finding.message().contains(&format!("{first_member:?}")));
        }
    }

    #[test]
    fn reachable_unreachable_is_reported_before_recursion() {
        let (mut program, functions) = call_graph_program(&[vec![0], vec![]]);
        let body = program
            .function_mut(functions[1])
            .unwrap()
            .body
            .as_mut()
            .unwrap();
        body.block_mut(body.entry()).unwrap().terminator = Some(Terminator::new(
            TerminatorKind::Unreachable,
            OriginId::UNKNOWN,
        ));
        let inventory = SemanticInventory::new(&program).unwrap();

        let diagnostics = audit_legality(&program, &inventory).unwrap_err();

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(
            diagnostics.findings()[0].code(),
            "lower.reachable-unreachable"
        );
        assert_eq!(diagnostics.findings()[1].code(), "lower.recursive-call-abi");
    }

    #[test]
    fn deep_chain_and_cycle_use_no_host_recursion() {
        const DEPTH: usize = 10_000;
        let mut chain = (0..DEPTH)
            .map(|index| {
                if index + 1 == DEPTH {
                    vec![]
                } else {
                    vec![index + 1]
                }
            })
            .collect::<Vec<_>>();
        let (program, _) = call_graph_program(&chain);
        let inventory = SemanticInventory::new(&program).unwrap();
        assert!(audit_legality(&program, &inventory).is_ok());

        chain[DEPTH - 1].push(0);
        let (program, _) = call_graph_program(&chain);
        let inventory = SemanticInventory::new(&program).unwrap();
        let diagnostics = audit_legality(&program, &inventory).unwrap_err();
        assert_eq!(diagnostics.len(), 1);
    }

    fn call_graph_program(adjacency: &[Vec<usize>]) -> (CoreProgram, Vec<FunctionId>) {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let functions = adjacency
            .iter()
            .enumerate()
            .map(|(index, _)| {
                program
                    .declare_function(Some(format!("f{index}")), vec![], vec![], OriginId::UNKNOWN)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        for (index, callees) in adjacency.iter().enumerate() {
            let function = functions[index];
            let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
            for callee in callees {
                builder
                    .call(functions[*callee], vec![], OriginId::UNKNOWN)
                    .unwrap();
            }
            builder
                .terminate(Terminator::new(
                    TerminatorKind::Return(vec![]),
                    OriginId::UNKNOWN,
                ))
                .unwrap();
            program
                .define_function(function, builder.finish().unwrap())
                .unwrap();
        }
        (program, functions)
    }
}
