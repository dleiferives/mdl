use std::collections::HashSet;

use crate::analysis::minecraft::CommandLimitAssumptions;
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::{EntityId, EntityVec};
use crate::ir::core::{
    CoreOp, CoreProgram, ExternalSemanticBinding, FunctionId, InstId, RunModifierInstance,
    TargetFragment, TerminatorKind,
};
use crate::ir::minecraft::UnsafeRawCommand;
use crate::target::JavaEditionTarget;

use super::analysis::{CallSite, SemanticInventory};
use super::api::CommandLimitEvidence;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RedirectGuardClass {
    GenericCommandForks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RunPrefixFact {
    modifier_index: usize,
    invocation_bounds: crate::ir::semantic::InvocationBounds,
    origin: crate::source::OriginId,
    guard: RedirectGuardClass,
}

struct ReachableVocabularyAuditOutput<'a> {
    minimum_max_command_forks: &'a mut u64,
    findings: &'a mut Vec<Diagnostic>,
}

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

/// Immutable recursive-SCC classification, separate from invocation multiplicity.
#[derive(Clone, Debug)]
pub(crate) struct ActivationOverlapAnalysis {
    recursive_functions: Box<[bool]>,
    recursive_call_slots: EntityVec<FunctionId, Box<[bool]>>,
}

impl ActivationOverlapAnalysis {
    pub(crate) fn analyze(
        core: &CoreProgram,
        inventory: &SemanticInventory,
    ) -> Result<Self, Diagnostic> {
        let graph = CallGraph::new(core, inventory)?;
        let mut recursive_functions = vec![false; core.len()];
        let mut component_of = vec![None; core.len()];
        for (component_index, component) in strongly_connected_components(core, &graph)
            .into_iter()
            .filter(|component| is_recursive(component, &graph))
            .enumerate()
        {
            for function in component {
                let index = usize::try_from(function.index()).map_err(|_| {
                    Diagnostic::new(
                        "lower.invalid-call-graph",
                        "recursive function identity does not fit the host index domain",
                        crate::source::OriginId::UNKNOWN,
                    )
                })?;
                recursive_functions[index] = true;
                component_of[index] = Some(component_index);
            }
        }
        let mut recursive_call_slots = EntityVec::new();
        for (function, declaration) in core.functions() {
            let body = declaration.body().ok_or_else(|| {
                Diagnostic::new(
                    "lower.invalid-call-graph",
                    format!("{function:?} has no definition"),
                    declaration.origin(),
                )
            })?;
            let mut slots = vec![false; body.instruction_counts().allocated];
            for edge in graph.outgoing(function) {
                let source_component = usize::try_from(edge.caller.index())
                    .ok()
                    .and_then(|index| component_of.get(index))
                    .copied()
                    .flatten();
                let target_component = usize::try_from(edge.callee.index())
                    .ok()
                    .and_then(|index| component_of.get(index))
                    .copied()
                    .flatten();
                if source_component.is_some() && source_component == target_component {
                    let index = usize::try_from(edge.site.instruction().index()).map_err(|_| {
                        invalid_edge_diagnostic(
                            *edge,
                            "instruction does not fit the host index domain",
                        )
                    })?;
                    *slots.get_mut(index).ok_or_else(|| {
                        invalid_edge_diagnostic(*edge, "instruction is outside its caller body")
                    })? = true;
                }
            }
            let inserted = recursive_call_slots
                .push(slots.into_boxed_slice())
                .map_err(|_| {
                    Diagnostic::new(
                        "lower.invalid-call-graph",
                        "recursive call-slot identity space exhausted",
                        declaration.origin(),
                    )
                })?;
            if inserted != function {
                return Err(Diagnostic::new(
                    "lower.invalid-call-graph",
                    "recursive call-slot table is not dense",
                    declaration.origin(),
                ));
            }
        }
        Ok(Self {
            recursive_functions: recursive_functions.into_boxed_slice(),
            recursive_call_slots,
        })
    }

    pub(crate) fn function_is_recursive(&self, function: FunctionId) -> bool {
        usize::try_from(function.index())
            .ok()
            .and_then(|index| self.recursive_functions.get(index))
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn call_is_recursive(&self, function: FunctionId, instruction: InstId) -> bool {
        usize::try_from(instruction.index())
            .ok()
            .and_then(|index| self.recursive_call_slots.get(function)?.get(index))
            .copied()
            .unwrap_or(false)
    }
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
/// The call graph and iterative SCC state are disposable proof scratch. Successful
/// auditing retains only the command-limit inputs and the derived deployment
/// requirement needed by later immutable planning phases.
pub(crate) fn audit_legality(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    target: JavaEditionTarget,
    assumptions: CommandLimitAssumptions,
) -> Result<CommandLimitEvidence, Diagnostics> {
    let call_graph = CallGraph::new(core, inventory).map_err(|finding| {
        Diagnostics::from_findings(vec![finding])
            .expect("one invariant finding always forms diagnostics")
    })?;
    let (findings, minimum_max_command_forks) =
        audit_reachable_vocabulary(core, inventory, target, assumptions);
    drop(call_graph);
    match Diagnostics::from_findings(findings) {
        Some(diagnostics) => Err(diagnostics),
        None => Ok(CommandLimitEvidence::new(
            assumptions,
            CommandLimitAssumptions::for_target(target),
            minimum_max_command_forks,
        )),
    }
}

fn audit_reachable_vocabulary(
    core: &CoreProgram,
    inventory: &SemanticInventory,
    target: JavaEditionTarget,
    assumptions: CommandLimitAssumptions,
) -> (Vec<Diagnostic>, u64) {
    let mut findings = Vec::new();
    let mut audited_external_ops = HashSet::new();
    let mut minimum_max_command_forks = 0_u64;
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
                | CoreOp::I32SubWrapping
                | CoreOp::I32AddOverflowing
                | CoreOp::I32Compare(_)
                | CoreOp::I32InClosedRange(_)
                | CoreOp::BoolNot
                | CoreOp::ListI32Empty
                | CoreOp::ListI32Length
                | CoreOp::ListI32Push
                | CoreOp::ListI32LastOrZero
                | CoreOp::ListI32WithoutLast
                | CoreOp::StringConstant(_)
                | CoreOp::StringLength
                | CoreOp::StringEndsWithAscii(_)
                | CoreOp::StringWithoutLastUnit
                | CoreOp::Call(_)
                | CoreOp::Schedule(..)
                | CoreOp::ScheduleClear(_) => {}
                CoreOp::External(operation) => {
                    if audited_external_ops.insert(*operation) {
                        let mut output = ReachableVocabularyAuditOutput {
                            minimum_max_command_forks: &mut minimum_max_command_forks,
                            findings: &mut findings,
                        };
                        audit_external_operation(
                            core,
                            *operation,
                            target,
                            assumptions,
                            instruction_data.origin(),
                            &mut output,
                        );
                    }
                }
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
    (findings, minimum_max_command_forks)
}

fn audit_external_operation(
    core: &CoreProgram,
    operation: crate::ir::core::ExternalOpId,
    target: JavaEditionTarget,
    assumptions: CommandLimitAssumptions,
    origin: crate::source::OriginId,
    output: &mut ReachableVocabularyAuditOutput<'_>,
) {
    let Some(declaration) = core.external_op(operation) else {
        output.findings.push(Diagnostic::new(
            "lower.invalid-external-operation",
            format!("external operation {operation:?} is absent from the Core inventory"),
            origin,
        ));
        return;
    };
    match declaration.binding() {
        ExternalSemanticBinding::UnsafeTargetFragment(fragment) => {
            let Some(TargetFragment::UnsafeMinecraftCommand(command)) =
                core.target_fragment(fragment)
            else {
                output.findings.push(Diagnostic::new(
                    "lower.invalid-target-fragment",
                    format!(
                        "external operation {operation:?} refers to absent fragment {fragment:?}"
                    ),
                    origin,
                ));
                return;
            };
            if let Err(error) = UnsafeRawCommand::new_for_target(command.as_str(), target) {
                output.findings.push(Diagnostic::new(
                    "lower.invalid-target-fragment",
                    format!("unsafe command for {target:?} cannot be emitted: {error}"),
                    origin,
                ));
            }
        }
        ExternalSemanticBinding::MinecraftRunScope(scope) => {
            audit_run_scope(core, operation, scope, target, assumptions, origin, output);
        }
        ExternalSemanticBinding::MinecraftOperation(semantic) => {
            if core.minecraft_operation(semantic).is_none() {
                output.findings.push(Diagnostic::new(
                    "lower.invalid-minecraft-operation",
                    format!(
                        "external operation {operation:?} refers to an absent typed Minecraft operation"
                    ),
                    origin,
                ));
            }
        }
        ExternalSemanticBinding::EntityNbtRead(read) => {
            if core.entity_nbt_read(read).is_none() {
                output.findings.push(Diagnostic::new(
                    "lower.invalid-entity-nbt-read",
                    format!(
                        "external operation {operation:?} refers to an absent entity-NBT path read"
                    ),
                    origin,
                ));
            }
        }
        ExternalSemanticBinding::EntityNbtWrite(write) => {
            if core.entity_nbt_write(write).is_none() {
                output.findings.push(Diagnostic::new(
                    "lower.invalid-entity-nbt-write",
                    format!(
                        "external operation {operation:?} refers to an absent entity-NBT path write"
                    ),
                    origin,
                ));
            }
        }
    }
}

fn audit_run_scope(
    core: &CoreProgram,
    operation: crate::ir::core::ExternalOpId,
    scope: crate::ir::core::RunScopeId,
    target: JavaEditionTarget,
    assumptions: CommandLimitAssumptions,
    origin: crate::source::OriginId,
    output: &mut ReachableVocabularyAuditOutput<'_>,
) {
    let Some(scope_decl) = core.run_scope(scope) else {
        output.findings.push(Diagnostic::new(
            "lower.invalid-run-scope",
            format!("external operation {operation:?} refers to absent scope {scope:?}"),
            origin,
        ));
        return;
    };
    if core.function(scope_decl.callee()).is_none() {
        output.findings.push(Diagnostic::new(
            "lower.invalid-run-body",
            format!(
                "run scope {scope:?} refers to absent body {:?}",
                scope_decl.callee()
            ),
            origin,
        ));
    }
    let Some(prefixes) = run_prefix_ledger(core, scope_decl) else {
        output.findings.push(Diagnostic::new(
            "lower.invalid-run-scope",
            format!("run scope {scope:?} has an invalid modifier reference"),
            scope_decl.origin(),
        ));
        return;
    };
    if let Some(prefix) = prefixes
        .iter()
        .find(|prefix| prefix.invocation_bounds.upper().is_none())
    {
        output.findings.push(Diagnostic::new(
            "lower.unbounded-command-forks",
            format!(
                "run-scope modifier {} produces unbounded prefix bounds {}; no finite minecraft:max_command_forks assumption can prove this redirect safe",
                prefix.modifier_index,
                prefix.invocation_bounds,
            ),
            prefix.origin,
        ));
    } else if let Some((required, requirement_origin)) = required_max_command_forks(&prefixes) {
        *output.minimum_max_command_forks = (*output.minimum_max_command_forks).max(required);
        if u64::from(assumptions.max_command_forks()) < required {
            output.findings.push(Diagnostic::new(
                "lower.unsupported-command-fork-assumption",
                format!(
                    "run scope {scope:?} requires minecraft:max_command_forks to be at least {required}; configured lowering assumption is {}",
                    assumptions.max_command_forks()
                ),
                requirement_origin,
            ));
        }
    }
    for modifier in scope_decl.modifiers().iter().cloned() {
        let query = match modifier {
            RunModifierInstance::AsEntityQuery { query, .. }
            | RunModifierInstance::AtEntityQuery { query, .. } => query,
            RunModifierInstance::AtExecutor { .. }
            | RunModifierInstance::Positioned { .. }
            | RunModifierInstance::Rotated { .. }
            | RunModifierInstance::In { .. }
            | RunModifierInstance::Anchored { .. }
            | RunModifierInstance::Align { .. } => continue,
        };
        let Some(query) = core.entity_query(query) else {
            continue;
        };
        if let Err(failure) = super::query::lower_entity_query(query) {
            output.findings.push(Diagnostic::new(
                "lower.unsupported-entity-query",
                format!(
                    "entity query cannot be emitted for {target:?}: {}",
                    failure.error
                ),
                failure.origin,
            ));
        }
    }
}

fn run_prefix_ledger(
    core: &CoreProgram,
    scope: &crate::ir::core::RunScopeDecl,
) -> Option<Vec<RunPrefixFact>> {
    let mut contexts = crate::ir::semantic::InvocationBounds::EXACTLY_ONCE;
    let mut prefixes = Vec::with_capacity(scope.modifiers().len());
    for (modifier_index, modifier) in scope.modifiers().iter().cloned().enumerate() {
        let origin = modifier.origin();
        if let RunModifierInstance::AsEntityQuery { query, .. }
        | RunModifierInstance::AtEntityQuery { query, .. } = modifier
        {
            let query = core.entity_query(query)?;
            contexts = contexts.multiply(
                crate::ir::semantic::InvocationBounds::from_query_cardinality(
                    query.semantic().ty().cardinality(),
                ),
            );
        }
        prefixes.push(RunPrefixFact {
            modifier_index,
            invocation_bounds: contexts,
            origin,
            guard: RedirectGuardClass::GenericCommandForks,
        });
    }
    Some(prefixes)
}

fn required_max_command_forks(
    prefixes: &[RunPrefixFact],
) -> Option<(u64, crate::source::OriginId)> {
    let mut required = 0_u64;
    let mut requirement_origin = prefixes
        .first()
        .map_or(crate::source::OriginId::UNKNOWN, |prefix| prefix.origin);
    for prefix in prefixes {
        match prefix.guard {
            RedirectGuardClass::GenericCommandForks => {
                let prefix_required = prefix.invocation_bounds.upper()?.checked_add(1)?;
                if prefix_required > required {
                    required = prefix_required;
                    requirement_origin = prefix.origin;
                }
            }
        }
    }
    Some((required, requirement_origin))
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
            "call {:?} ({:?}) in {:?} from {:?} to {:?} is invalid: {reason}",
            edge.site.instruction(),
            edge.site.reference_kind(),
            edge.site.block(),
            edge.caller,
            edge.callee
        ),
        edge.site.origin(),
    )
}

#[cfg(test)]
mod tests {
    use super::{ActivationOverlapAnalysis, audit_legality as audit_with_target};
    use crate::analysis::minecraft::CommandLimitAssumptions;
    use crate::entity::EntityId;
    use crate::ir::core::{
        CoreProgram, EntityQueryDecl, ExternalSemanticBinding, FunctionBuilder, FunctionId, InstId,
        RunModifierInstance, TargetFragment, Terminator, TerminatorKind,
    };
    use crate::ir::semantic::{EntityKind, StaticEntityQuery};
    use crate::lower::minecraft::analysis::SemanticInventory;
    use crate::source::{Origin, OriginId, SourceContext};
    use crate::target::JavaEditionTarget;

    fn audit_legality(
        core: &CoreProgram,
        inventory: &SemanticInventory,
    ) -> Result<crate::lower::minecraft::CommandLimitEvidence, crate::diagnostic::Diagnostics> {
        audit_with_target(
            core,
            inventory,
            JavaEditionTarget::V26_2,
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2),
        )
    }

    #[test]
    fn accepts_forward_and_nested_acyclic_calls() {
        let (program, _) = call_graph_program(&[vec![1], vec![2], vec![]]);
        let inventory = SemanticInventory::new(&program).unwrap();

        assert!(audit_legality(&program, &inventory).is_ok());
    }

    #[test]
    fn unsafe_target_fragments_are_supported_by_the_selected_target() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let fragment = program
            .declare_target_fragment(
                TargetFragment::unsafe_minecraft_command("say pending").unwrap(),
            )
            .unwrap();
        let external = program
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let function = program
            .declare_function(Some("entry"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        builder
            .external(external, vec![], OriginId::UNKNOWN)
            .unwrap();
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

        assert!(audit_legality(&program, &inventory).is_ok());
    }

    #[test]
    fn ordinary_redirect_requires_a_strictly_larger_fork_assumption() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let body = program
            .declare_function(Some("body"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut body_builder = FunctionBuilder::new(&program, &sources, body).unwrap();
        body_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(body, body_builder.finish().unwrap())
            .unwrap();
        let query = program
            .declare_entity_query(EntityQueryDecl::from_semantic(
                StaticEntityQuery::entities(EntityKind::ArmorStand)
                    .limit(1)
                    .unwrap(),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let scope = program
            .declare_run_scope(
                vec![RunModifierInstance::AsEntityQuery {
                    query,
                    origin: OriginId::UNKNOWN,
                }],
                body,
                OriginId::UNKNOWN,
            )
            .unwrap();
        let external = program
            .declare_external_op(
                ExternalSemanticBinding::MinecraftRunScope(scope),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let entry = program
            .declare_function(Some("entry"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut entry_builder = FunctionBuilder::new(&program, &sources, entry).unwrap();
        entry_builder
            .external(external, vec![], OriginId::UNKNOWN)
            .unwrap();
        entry_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(entry, entry_builder.finish().unwrap())
            .unwrap();
        let inventory = SemanticInventory::new(&program).unwrap();

        let rejected = audit_with_target(
            &program,
            &inventory,
            JavaEditionTarget::V26_2,
            CommandLimitAssumptions::new(100, 1).unwrap(),
        )
        .unwrap_err();
        assert_eq!(rejected.len(), 1);
        assert_eq!(
            rejected.findings()[0].code(),
            "lower.unsupported-command-fork-assumption"
        );
        assert!(rejected.findings()[0].message().contains("at least 2"));

        let evidence = audit_with_target(
            &program,
            &inventory,
            JavaEditionTarget::V26_2,
            CommandLimitAssumptions::new(100, 2).unwrap(),
        )
        .unwrap();
        assert_eq!(evidence.minimum_max_command_forks(), 2);
        assert_eq!(
            evidence.configured_assumptions(),
            CommandLimitAssumptions::new(100, 2).unwrap()
        );
        assert_eq!(
            evidence.target_defaults(),
            CommandLimitAssumptions::for_target(JavaEditionTarget::V26_2)
        );
    }

    #[test]
    fn one_redirect_is_rejected_when_the_fork_limit_is_zero() {
        let mut sources = SourceContext::new();
        let modifier_origin = sources.add_origin(Origin::Unknown).unwrap();
        let program = run_scope_program(&sources, &[(1, modifier_origin)], 1);
        let inventory = SemanticInventory::new(&program).unwrap();

        let diagnostics = audit_with_fork_limit(&program, &inventory, 0).unwrap_err();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics.findings()[0].code(),
            "lower.unsupported-command-fork-assumption"
        );
        assert_eq!(diagnostics.findings()[0].origin(), modifier_origin);
        assert!(diagnostics.findings()[0].message().contains("at least 2"));
    }

    #[test]
    fn zero_modifier_scope_needs_no_command_fork_capacity() {
        let sources = SourceContext::new();
        let program = run_scope_program(&sources, &[], 1);
        let inventory = SemanticInventory::new(&program).unwrap();

        let evidence = audit_with_fork_limit(&program, &inventory, 0).unwrap();
        assert_eq!(evidence.minimum_max_command_forks(), 0);
    }

    #[test]
    fn unreachable_run_scope_does_not_raise_the_derived_minimum() {
        let mut sources = SourceContext::new();
        let modifier_origin = sources.add_origin(Origin::Unknown).unwrap();
        let program = run_scope_program(&sources, &[(1, modifier_origin)], 0);
        let inventory = SemanticInventory::new(&program).unwrap();

        let evidence = audit_with_fork_limit(&program, &inventory, 0).unwrap();

        assert_eq!(evidence.minimum_max_command_forks(), 0);
    }

    #[test]
    fn repeated_at_most_one_redirects_share_the_strict_boundary() {
        let mut sources = SourceContext::new();
        let first_origin = sources.add_origin(Origin::Unknown).unwrap();
        let second_origin = sources.add_origin(Origin::Unknown).unwrap();
        let program = run_scope_program(&sources, &[(1, first_origin), (1, second_origin)], 1);
        let inventory = SemanticInventory::new(&program).unwrap();

        let rejected = audit_with_fork_limit(&program, &inventory, 1).unwrap_err();
        assert_eq!(rejected.len(), 1);
        assert_eq!(
            rejected.findings()[0].code(),
            "lower.unsupported-command-fork-assumption"
        );
        assert_eq!(rejected.findings()[0].origin(), first_origin);
        assert!(rejected.findings()[0].message().contains("at least 2"));

        let evidence = audit_with_fork_limit(&program, &inventory, 2).unwrap();
        assert_eq!(evidence.minimum_max_command_forks(), 2);
    }

    #[test]
    fn bounded_many_context_prefix_uses_the_serial_activation_contract() {
        let mut sources = SourceContext::new();
        let first_origin = sources.add_origin(Origin::Unknown).unwrap();
        let violating_origin = sources.add_origin(Origin::Unknown).unwrap();
        let program = run_scope_program(&sources, &[(1, first_origin), (2, violating_origin)], 1);
        let inventory = SemanticInventory::new(&program).unwrap();

        let evidence = audit_with_fork_limit(
            &program,
            &inventory,
            CommandLimitAssumptions::MAX_CONFIGURED_VALUE,
        )
        .unwrap();

        assert_eq!(evidence.minimum_max_command_forks(), 3);
    }

    #[test]
    fn one_external_declaration_is_audited_once_when_reused() {
        let mut sources = SourceContext::new();
        let modifier_origin = sources.add_origin(Origin::Unknown).unwrap();
        let program = run_scope_program(&sources, &[(2, modifier_origin)], 2);
        let inventory = SemanticInventory::new(&program).unwrap();

        let evidence = audit_with_fork_limit(
            &program,
            &inventory,
            CommandLimitAssumptions::MAX_CONFIGURED_VALUE,
        )
        .unwrap();

        assert_eq!(evidence.minimum_max_command_forks(), 3);
    }

    #[test]
    fn classifies_self_recursion_without_rejecting_legality() {
        let (program, functions) = call_graph_program(&[vec![0]]);
        let inventory = SemanticInventory::new(&program).unwrap();
        let activation = ActivationOverlapAnalysis::analyze(&program, &inventory).unwrap();

        assert!(audit_legality(&program, &inventory).is_ok());
        assert!(activation.function_is_recursive(functions[0]));
        assert!(activation.call_is_recursive(functions[0], InstId::from_index(0)));
    }

    #[test]
    fn classifies_only_mutual_scc_edges_as_recursive() {
        let (program, functions) = call_graph_program(&[vec![1], vec![0], vec![0]]);
        let inventory = SemanticInventory::new(&program).unwrap();
        let activation = ActivationOverlapAnalysis::analyze(&program, &inventory).unwrap();

        assert!(activation.function_is_recursive(functions[0]));
        assert!(activation.function_is_recursive(functions[1]));
        assert!(!activation.function_is_recursive(functions[2]));
        assert!(!activation.call_is_recursive(functions[2], InstId::from_index(0)));
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

        assert!(audit_legality(&program, &inventory).is_ok());
    }

    #[test]
    fn every_internal_self_edge_is_classified() {
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

        let activation = ActivationOverlapAnalysis::analyze(&program, &inventory).unwrap();

        assert!(activation.call_is_recursive(function, InstId::from_index(0)));
        assert!(activation.call_is_recursive(function, InstId::from_index(1)));
    }

    #[test]
    fn independent_recursive_components_are_all_classified() {
        let (program, functions) =
            call_graph_program(&[vec![1], vec![0], vec![2], vec![4], vec![3]]);
        let inventory = SemanticInventory::new(&program).unwrap();

        let activation = ActivationOverlapAnalysis::analyze(&program, &inventory).unwrap();

        assert!(
            functions
                .iter()
                .copied()
                .all(|function| activation.function_is_recursive(function))
        );
    }

    #[test]
    fn reachable_unreachable_remains_a_legality_error_with_recursion_supported() {
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

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics.findings()[0].code(),
            "lower.reachable-unreachable"
        );
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
        assert!(audit_legality(&program, &inventory).is_ok());
        let activation = ActivationOverlapAnalysis::analyze(&program, &inventory).unwrap();
        assert!(activation.function_is_recursive(FunctionId::from_index(0)));
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

    fn audit_with_fork_limit(
        program: &CoreProgram,
        inventory: &SemanticInventory,
        max_command_forks: u32,
    ) -> Result<crate::lower::minecraft::CommandLimitEvidence, crate::diagnostic::Diagnostics> {
        audit_with_target(
            program,
            inventory,
            JavaEditionTarget::V26_2,
            CommandLimitAssumptions::new(100, max_command_forks).unwrap(),
        )
    }

    fn run_scope_program(
        sources: &SourceContext,
        modifiers: &[(u32, OriginId)],
        external_call_count: usize,
    ) -> CoreProgram {
        let mut program = CoreProgram::new();
        let body = program
            .declare_function(Some("body"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut body_builder = FunctionBuilder::new(&program, sources, body).unwrap();
        body_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(body, body_builder.finish().unwrap())
            .unwrap();

        let mut instances = Vec::with_capacity(modifiers.len());
        for &(maximum, origin) in modifiers {
            let query = program
                .declare_entity_query(EntityQueryDecl::from_semantic(
                    StaticEntityQuery::entities(EntityKind::ArmorStand)
                        .limit(maximum)
                        .unwrap(),
                    origin,
                ))
                .unwrap();
            instances.push(RunModifierInstance::AsEntityQuery { query, origin });
        }
        let scope = program
            .declare_run_scope(instances, body, OriginId::UNKNOWN)
            .unwrap();
        let external = program
            .declare_external_op(
                ExternalSemanticBinding::MinecraftRunScope(scope),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();

        let entry = program
            .declare_function(Some("entry"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut entry_builder = FunctionBuilder::new(&program, sources, entry).unwrap();
        for _ in 0..external_call_count {
            entry_builder
                .external(external, vec![], OriginId::UNKNOWN)
                .unwrap();
        }
        entry_builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(entry, entry_builder.finish().unwrap())
            .unwrap();
        program
    }
}
