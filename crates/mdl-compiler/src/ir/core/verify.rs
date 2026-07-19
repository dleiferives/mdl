//! Layered, panic-free Core verification.

use super::analysis::definition_block;
use super::{
    BlockId, CoreProgram, CoreType, Dominance, DominatorTree, EntityQueryStep, FunctionBody,
    FunctionId, MinecraftOperationAttributes, PlacementIndex, RunModifierInstance, TargetFragment,
    TerminatorKind, UnsafeMinecraftCommandFragment, UseIndex, UseSite, ValueDef, ValueId,
};
use crate::diagnostic::{Diagnostic, Diagnostics};
use crate::entity::EntityId;
use crate::source::{OriginId, SourceContext};

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static FUNCTION_VERIFY_CALLS: Cell<usize> = const { Cell::new(0) };
    static PROGRAM_VERIFY_CALLS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_verifier_counters() {
    FUNCTION_VERIFY_CALLS.set(0);
    PROGRAM_VERIFY_CALLS.set(0);
}

#[cfg(test)]
pub(crate) fn verifier_counters() -> (usize, usize) {
    (FUNCTION_VERIFY_CALLS.get(), PROGRAM_VERIFY_CALLS.get())
}

#[derive(Default)]
struct Verifier {
    findings: Vec<Diagnostic>,
}

impl Verifier {
    fn report(&mut self, code: &'static str, message: impl Into<String>, origin: OriginId) {
        self.findings.push(Diagnostic::new(code, message, origin));
    }

    fn finish(self) -> Result<(), Diagnostics> {
        match Diagnostics::from_findings(self.findings) {
            Some(diagnostics) => Err(diagnostics),
            None => Ok(()),
        }
    }
}

/// Verifies one candidate body against its current declaration environment.
///
/// Undefined callees are permitted as long as they are declared. This enables
/// forward, recursive, and mutually recursive construction.
///
/// # Errors
///
/// Returns every safely detectable finding in deterministic layer order.
pub fn verify_function(
    program: &CoreProgram,
    sources: &SourceContext,
    function: FunctionId,
    body: &FunctionBody,
) -> Result<(), Diagnostics> {
    #[cfg(test)]
    FUNCTION_VERIFY_CALLS.with(|calls| calls.set(calls.get() + 1));

    let mut verifier = Verifier::default();
    let declaration = program.function(function);
    let fallback_origin = declaration.map_or(OriginId::UNKNOWN, |item| item.origin);
    if declaration.is_none() {
        verifier.report(
            "core.invalid-function",
            format!("body refers to absent declaration {function:?}"),
            fallback_origin,
        );
    }
    if sources.origin(fallback_origin).is_none() {
        verifier.report(
            "core.invalid-origin",
            format!("function {function:?} has invalid origin {fallback_origin:?}"),
            OriginId::UNKNOWN,
        );
    }

    verify_raw_instruction_ownership(body, &mut verifier, fallback_origin);
    verify_placement(body, &mut verifier, fallback_origin);
    verify_definitions(body, &mut verifier, fallback_origin);
    verify_origins(body, sources, &mut verifier, fallback_origin);
    verify_operations(program, body, &mut verifier, fallback_origin);
    verify_terminators(
        declaration.map(|item| (item.parameters.as_slice(), item.results.as_slice())),
        body,
        &mut verifier,
        fallback_origin,
    );
    verify_ssa(body, &mut verifier, fallback_origin);
    verifier.finish()
}

/// Verifies all internal declarations and definitions in a complete program.
///
/// # Errors
///
/// Returns accumulated diagnostics, including every declaration left undefined.
pub fn verify_program(program: &CoreProgram, sources: &SourceContext) -> Result<(), Diagnostics> {
    #[cfg(test)]
    PROGRAM_VERIFY_CALLS.with(|calls| calls.set(calls.get() + 1));

    let mut findings = vec![];
    verify_linked_inventories(program, sources, &mut findings);
    for (function, declaration) in program.functions() {
        if sources.origin(declaration.origin).is_none() {
            findings.push(Diagnostic::new(
                "core.invalid-origin",
                format!(
                    "function {function:?} has invalid origin {:?}",
                    declaration.origin
                ),
                OriginId::UNKNOWN,
            ));
        }
        let Some(body) = declaration.body() else {
            findings.push(Diagnostic::new(
                "core.undefined-function",
                format!("internal function {function:?} has no definition"),
                declaration.origin,
            ));
            continue;
        };
        if let Err(diagnostics) = verify_function(program, sources, function, body) {
            findings.extend(diagnostics.into_findings());
        }
    }
    match Diagnostics::from_findings(findings) {
        Some(diagnostics) => Err(diagnostics),
        None => Ok(()),
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "linked Core inventories share one ordered whole-program provenance and shape audit"
)]
fn verify_linked_inventories(
    program: &CoreProgram,
    sources: &SourceContext,
    findings: &mut Vec<Diagnostic>,
) {
    for (query, declaration) in program.entity_queries() {
        let fallback = declaration.origin().unwrap_or(OriginId::UNKNOWN);
        for step in declaration.steps() {
            let role = match step {
                EntityQueryStep::Entities { .. } => "entity-query root",
                EntityQueryStep::WithTag { .. } => "entity-query tag refinement",
                EntityQueryStep::Limit { .. } => "entity-query limit refinement",
            };
            for (label, origin) in [("step", step.origin()), ("operand", step.value_origin())] {
                if sources.origin(origin).is_none() {
                    findings.push(Diagnostic::new(
                        "core.invalid-origin",
                        format!("{role} {label} in {query:?} has invalid origin {origin:?}"),
                        OriginId::UNKNOWN,
                    ));
                }
            }
        }
        if !declaration.is_well_formed() {
            findings.push(Diagnostic::new(
                "core.invalid-entity-query",
                format!(
                    "entity query {query:?} has malformed steps or mismatched canonical semantics"
                ),
                fallback,
            ));
        }
    }
    for (fragment, data) in program.target_fragments() {
        match data {
            TargetFragment::UnsafeMinecraftCommand(command) => {
                if let Err(error) = UnsafeMinecraftCommandFragment::new(command.as_str()) {
                    findings.push(Diagnostic::new(
                        "core.invalid-target-fragment",
                        format!("target fragment {fragment:?} is malformed: {error}"),
                        OriginId::UNKNOWN,
                    ));
                }
            }
        }
    }
    for (operation, declaration) in program.minecraft_operations() {
        let origins = declaration.origins();
        for (role, origin) in [
            ("call", origins.call()),
            ("member", origins.member()),
            ("receiver", origins.receiver()),
            ("message", declaration.attributes().message_origin()),
        ] {
            if sources.origin(origin).is_none() {
                findings.push(Diagnostic::new(
                    "core.invalid-origin",
                    format!(
                        "Minecraft operation {operation:?} has invalid {role} origin {origin:?}"
                    ),
                    OriginId::UNKNOWN,
                ));
            }
        }
        let attributes_are_valid = match declaration.attributes() {
            MinecraftOperationAttributes::Say { message, .. } => {
                crate::ir::semantic::MessageLiteral::new(message.as_str()).is_ok()
            }
            MinecraftOperationAttributes::Teleport { .. }
            | MinecraftOperationAttributes::MoveBy { .. } => true,
            MinecraftOperationAttributes::BookPage { page_index, .. } => *page_index < 100,
        };
        if !declaration.is_well_formed() || !attributes_are_valid {
            findings.push(Diagnostic::new(
                "core.invalid-minecraft-operation",
                format!("Minecraft operation {operation:?} is malformed"),
                origins.call(),
            ));
        }
    }
    for (scope, declaration) in program.run_scopes() {
        if sources.origin(declaration.origin()).is_none() {
            findings.push(Diagnostic::new(
                "core.invalid-origin",
                format!(
                    "run scope {scope:?} has invalid origin {:?}",
                    declaration.origin()
                ),
                OriginId::UNKNOWN,
            ));
        }
        for modifier in declaration.modifiers().iter().cloned() {
            let role = match modifier {
                RunModifierInstance::AsEntityQuery { .. } => "run-as modifier",
                RunModifierInstance::AtEntityQuery { .. } => "run-at modifier",
                RunModifierInstance::AtExecutor { .. } => "run-at-executor modifier",
                RunModifierInstance::Positioned { .. } => "run-positioned modifier",
                RunModifierInstance::Rotated { .. } => "run-rotated modifier",
                RunModifierInstance::In { .. } => "run-in modifier",
                RunModifierInstance::Anchored { .. } => "run-anchored modifier",
                RunModifierInstance::Align { .. } => "run-align modifier",
            };
            if sources.origin(modifier.origin()).is_none() {
                findings.push(Diagnostic::new(
                    "core.invalid-origin",
                    format!(
                        "{role} in run scope {scope:?} has invalid origin {:?}",
                        modifier.origin()
                    ),
                    OriginId::UNKNOWN,
                ));
            }
        }
        let context_replays = program
            .apply_run_scope_context(
                scope,
                crate::ir::core::CoreExecutionContext::function_entry(),
            )
            .is_some();
        if !declaration.is_well_formed(program) || !context_replays {
            findings.push(Diagnostic::new(
                "core.invalid-run-scope",
                format!("run scope {scope:?} has an invalid modifier, body, or invocation bound"),
                declaration.origin(),
            ));
        }
    }
    for (operation, declaration) in program.external_ops() {
        if sources.origin(declaration.origin()).is_none() {
            findings.push(Diagnostic::new(
                "core.invalid-origin",
                format!(
                    "external declaration {operation:?} has invalid origin {:?}",
                    declaration.origin()
                ),
                OriginId::UNKNOWN,
            ));
        }
        if !declaration.is_well_formed(program) {
            findings.push(Diagnostic::new(
                "core.invalid-external-declaration",
                format!("external declaration {operation:?} has an invalid binding or signature"),
                declaration.origin(),
            ));
        }
    }
}

fn verify_raw_instruction_ownership(
    body: &FunctionBody,
    verifier: &mut Verifier,
    fallback: OriginId,
) {
    let mut owners = vec![None; body.instructions.len()];
    for (block, data) in body.blocks.iter() {
        for instruction in &data.instructions {
            let Ok(index) = usize::try_from(instruction.index()) else {
                verifier.report(
                    "core.invalid-instruction-container",
                    format!("{block:?} contains invalid instruction {instruction:?}"),
                    data.origin,
                );
                continue;
            };
            let Some(owner) = owners.get_mut(index) else {
                verifier.report(
                    "core.invalid-instruction-container",
                    format!("{block:?} contains invalid instruction {instruction:?}"),
                    data.origin,
                );
                continue;
            };
            if let Some(previous) = *owner {
                verifier.report(
                    "core.duplicate-instruction-container",
                    format!(
                        "instruction {instruction:?} occurs more than once in raw block containers ({previous:?} then {block:?})"
                    ),
                    body.instruction(*instruction)
                        .map_or(fallback, |item| item.origin),
                );
            } else {
                *owner = Some(block);
            }
        }
    }
}

fn verify_placement(body: &FunctionBody, verifier: &mut Verifier, origin: OriginId) {
    let mut seen_blocks = vec![false; body.blocks.len()];
    let mut seen_instructions = vec![None; body.instructions.len()];
    for block in &body.block_order {
        let Ok(index) = usize::try_from(block.index()) else {
            verifier.report(
                "core.invalid-block",
                format!("invalid block {block:?}"),
                origin,
            );
            continue;
        };
        let Some(seen) = seen_blocks.get_mut(index) else {
            verifier.report(
                "core.invalid-block",
                format!("invalid block {block:?}"),
                origin,
            );
            continue;
        };
        if *seen {
            verifier.report(
                "core.duplicate-block-placement",
                format!("block {block:?} occurs more than once in layout"),
                body.block(*block).map_or(origin, |data| data.origin),
            );
            continue;
        }
        *seen = true;
        let Some(data) = body.block(*block) else {
            continue;
        };
        for instruction in &data.instructions {
            let Ok(instruction_index) = usize::try_from(instruction.index()) else {
                verifier.report(
                    "core.invalid-instruction",
                    format!("invalid instruction {instruction:?}"),
                    data.origin,
                );
                continue;
            };
            let Some(parent) = seen_instructions.get_mut(instruction_index) else {
                verifier.report(
                    "core.invalid-instruction",
                    format!("invalid instruction {instruction:?}"),
                    data.origin,
                );
                continue;
            };
            if let Some(previous) = parent {
                verifier.report(
                    "core.duplicate-instruction-placement",
                    format!(
                        "instruction {instruction:?} occurs in both {previous:?} and {block:?}"
                    ),
                    body.instruction(*instruction)
                        .map_or(data.origin, |item| item.origin),
                );
            } else {
                *parent = Some(*block);
            }
        }
    }
    if !seen_blocks
        .get(usize::try_from(body.entry.index()).unwrap_or(usize::MAX))
        .copied()
        .unwrap_or(false)
    {
        verifier.report(
            "core.detached-entry",
            format!("entry block {:?} is not attached exactly once", body.entry),
            origin,
        );
    }
}

fn verify_definitions(body: &FunctionBody, verifier: &mut Verifier, origin: OriginId) {
    for (value, data) in body.values.iter() {
        match data.definition {
            ValueDef::BlockParam {
                block,
                parameter_index,
            } => {
                let parameter = body.block(block).and_then(|item| {
                    usize::try_from(parameter_index)
                        .ok()
                        .and_then(|i| item.parameters.get(i))
                });
                if parameter.map(|item| item.value) != Some(value) {
                    verifier.report(
                        "core.bad-value-definition",
                        format!("value {value:?} does not match block parameter {block:?}[{parameter_index}]"),
                        body.block(block).map_or(origin, |item| item.origin),
                    );
                }
            }
            ValueDef::InstResult {
                instruction,
                result_index,
            } => {
                let result = body.instruction(instruction).and_then(|item| {
                    usize::try_from(result_index)
                        .ok()
                        .and_then(|index| item.results.get(index))
                });
                if result.copied() != Some(value) {
                    verifier.report(
                        "core.bad-value-definition",
                        format!(
                            "value {value:?} does not match result {instruction:?}[{result_index}]"
                        ),
                        body.instruction(instruction)
                            .map_or(origin, |item| item.origin),
                    );
                }
            }
        }
    }
    for (block, data) in body.blocks.iter() {
        for (index, parameter) in data.parameters.iter().enumerate() {
            let expected_index = u32::try_from(index).ok();
            let matches = body.value(parameter.value).is_some_and(|value| {
                matches!(
                    value.definition,
                    ValueDef::BlockParam {
                        block: definition_block,
                        parameter_index,
                    } if definition_block == block && Some(parameter_index) == expected_index
                )
            });
            if !matches {
                verifier.report(
                    "core.bad-parameter-value",
                    format!(
                        "parameter {block:?}[{index}] has inconsistent value {:?}",
                        parameter.value
                    ),
                    parameter.origin,
                );
            }
        }
    }
    for (instruction, data) in body.instructions.iter() {
        for (index, result) in data.results.iter().copied().enumerate() {
            let expected_index = u32::try_from(index).ok();
            let matches = body.value(result).is_some_and(|value| {
                matches!(
                    value.definition,
                    ValueDef::InstResult {
                        instruction: definition_instruction,
                        result_index,
                    } if definition_instruction == instruction && Some(result_index) == expected_index
                )
            });
            if !matches {
                verifier.report(
                    "core.bad-result-value",
                    format!("result {instruction:?}[{index}] has inconsistent value {result:?}"),
                    data.origin,
                );
            }
        }
    }
}

fn verify_origins(
    body: &FunctionBody,
    sources: &SourceContext,
    verifier: &mut Verifier,
    fallback: OriginId,
) {
    let mut check = |origin: OriginId, label: String| {
        if sources.origin(origin).is_none() {
            verifier.report("core.invalid-origin", label, fallback);
        }
    };
    for (block, data) in body.blocks.iter() {
        check(
            data.origin,
            format!("block {block:?} has invalid origin {:?}", data.origin),
        );
        for (index, parameter) in data.parameters.iter().enumerate() {
            check(
                parameter.origin,
                format!(
                    "parameter {block:?}[{index}] has invalid origin {:?}",
                    parameter.origin
                ),
            );
        }
        if let Some(terminator) = &data.terminator {
            check(
                terminator.origin,
                format!(
                    "terminator in {block:?} has invalid origin {:?}",
                    terminator.origin
                ),
            );
        }
    }
    for (instruction, data) in body.instructions.iter() {
        check(
            data.origin,
            format!(
                "instruction {instruction:?} has invalid origin {:?}",
                data.origin
            ),
        );
    }
}

fn verify_operations(
    program: &CoreProgram,
    body: &FunctionBody,
    verifier: &mut Verifier,
    fallback: OriginId,
) {
    let placement = PlacementIndex::new(body);
    for (instruction, data) in body.instructions.iter() {
        if !placement.is_instruction_attached(instruction) {
            continue;
        }
        let Some(signature) = data.op.signature(program) else {
            verifier.report(
                "core.invalid-operation",
                format!("{} references an invalid declaration", data.op.name()),
                data.origin,
            );
            continue;
        };
        if data.operands.len() != signature.operands.len() {
            verifier.report(
                "core.operand-count",
                format!(
                    "{instruction:?} expects {} operands but has {}",
                    signature.operands.len(),
                    data.operands.len()
                ),
                data.origin,
            );
        }
        for (index, (operand, expected)) in data.operands.iter().zip(signature.operands).enumerate()
        {
            match body.value(*operand) {
                Some(value) if value.ty != *expected => verifier.report(
                    "core.operand-type",
                    format!(
                        "{instruction:?} operand {index} expects {expected} but has {}",
                        value.ty
                    ),
                    data.origin,
                ),
                None => verifier.report(
                    "core.invalid-value",
                    format!("{instruction:?} operand {index} references invalid {operand:?}"),
                    data.origin,
                ),
                Some(_) => {}
            }
        }
        if data.results.len() != signature.results.len() {
            verifier.report(
                "core.result-count",
                format!(
                    "{instruction:?} expects {} results but has {}",
                    signature.results.len(),
                    data.results.len()
                ),
                data.origin,
            );
        }
        for (index, (result, expected)) in data.results.iter().zip(signature.results).enumerate() {
            match body.value(*result) {
                Some(value) if value.ty != *expected => verifier.report(
                    "core.result-type",
                    format!(
                        "{instruction:?} result {index} expects {expected} but has {}",
                        value.ty
                    ),
                    data.origin,
                ),
                None => verifier.report(
                    "core.invalid-value",
                    format!("{instruction:?} result {index} references invalid {result:?}"),
                    data.origin,
                ),
                Some(_) => {}
            }
        }
    }
    let _ = fallback;
}

fn verify_terminators(
    signature: Option<(&[CoreType], &[CoreType])>,
    body: &FunctionBody,
    verifier: &mut Verifier,
    fallback: OriginId,
) {
    let placement = PlacementIndex::new(body);
    if let Some((parameters, _)) = signature {
        let actual = body.block(body.entry).map(|entry| {
            entry
                .parameters
                .iter()
                .filter_map(|parameter| body.value(parameter.value).map(|value| value.ty))
                .collect::<Vec<_>>()
        });
        if actual.as_deref() != Some(parameters) {
            verifier.report(
                "core.entry-signature",
                "entry block parameters do not match the function declaration",
                body.block(body.entry)
                    .map_or(fallback, |entry| entry.origin),
            );
        }
    }

    for block in &body.block_order {
        if !placement.is_block_attached(*block) {
            continue;
        }
        let Some(data) = body.block(*block) else {
            continue;
        };
        let Some(terminator) = &data.terminator else {
            verifier.report(
                "core.missing-terminator",
                format!("attached block {block:?} has no terminator"),
                data.origin,
            );
            continue;
        };
        verify_one_terminator(
            body,
            &placement,
            *block,
            &terminator.kind,
            terminator.origin,
            signature,
            verifier,
        );
    }
}

fn verify_one_terminator(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    block: BlockId,
    kind: &TerminatorKind,
    origin: OriginId,
    signature: Option<(&[CoreType], &[CoreType])>,
    verifier: &mut Verifier,
) {
    match kind {
        TerminatorKind::Jump(target) => {
            verify_target(body, placement, block, target, verifier, origin);
        }
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => {
            match body.value(*condition) {
                Some(value) if value.ty != CoreType::Bool => verifier.report(
                    "core.branch-condition-type",
                    format!("branch condition {condition:?} has type {}", value.ty),
                    origin,
                ),
                None => verifier.report(
                    "core.invalid-value",
                    format!("branch condition references invalid {condition:?}"),
                    origin,
                ),
                Some(_) => {}
            }
            verify_target(body, placement, block, then_target, verifier, origin);
            verify_target(body, placement, block, else_target, verifier, origin);
        }
        TerminatorKind::Return(values) => {
            if let Some((_, expected)) = signature {
                verify_return(body, values, expected, verifier, origin);
            }
        }
        TerminatorKind::Unreachable => {}
    }
}

fn verify_return(
    body: &FunctionBody,
    values: &[ValueId],
    expected: &[CoreType],
    verifier: &mut Verifier,
    origin: OriginId,
) {
    if values.len() != expected.len() {
        verifier.report(
            "core.return-count",
            format!(
                "return expects {} values but has {}",
                expected.len(),
                values.len()
            ),
            origin,
        );
    }
    for (index, (value, expected)) in values.iter().zip(expected).enumerate() {
        match body.value(*value) {
            Some(actual) if actual.ty != *expected => verifier.report(
                "core.return-type",
                format!(
                    "return value {index} expects {expected} but has {}",
                    actual.ty
                ),
                origin,
            ),
            None => verifier.report(
                "core.invalid-value",
                format!("return references invalid {value:?}"),
                origin,
            ),
            Some(_) => {}
        }
    }
}

fn verify_target(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    source: BlockId,
    target: &super::BlockTarget,
    verifier: &mut Verifier,
    origin: OriginId,
) {
    if target.block == body.entry {
        verifier.report(
            "core.target-entry",
            format!("{source:?} targets the entry block"),
            origin,
        );
    }
    if !placement.is_block_attached(target.block) {
        verifier.report(
            "core.detached-target",
            format!("{source:?} targets detached or invalid {:?}", target.block),
            origin,
        );
        return;
    }
    let Some(destination) = body.block(target.block) else {
        return;
    };
    if target.arguments.len() != destination.parameters.len() {
        verifier.report(
            "core.edge-argument-count",
            format!(
                "edge to {:?} expects {} arguments but has {}",
                target.block,
                destination.parameters.len(),
                target.arguments.len()
            ),
            origin,
        );
    }
    for (index, (argument, parameter)) in target
        .arguments
        .iter()
        .zip(&destination.parameters)
        .enumerate()
    {
        let argument_type = body.value(*argument).map(|value| value.ty);
        let parameter_type = body.value(parameter.value).map(|value| value.ty);
        match (argument_type, parameter_type) {
            (Some(actual), Some(expected)) if actual != expected => verifier.report(
                "core.edge-argument-type",
                format!("edge argument {index} expects {expected} but has {actual}"),
                origin,
            ),
            (None, _) => verifier.report(
                "core.invalid-value",
                format!("edge argument references invalid {argument:?}"),
                origin,
            ),
            _ => {}
        }
    }
}

fn verify_ssa(body: &FunctionBody, verifier: &mut Verifier, fallback: OriginId) {
    let placement = PlacementIndex::new(body);
    let uses = UseIndex::new(body);
    let cfg = super::ControlFlowGraph::new(body);
    let dominators = DominatorTree::new(&cfg);

    for value in body.values.keys() {
        for use_site in uses.uses(value) {
            let use_block = use_site.block();
            let Some(definition_block) = definition_block(body, &placement, value) else {
                verifier.report(
                    "core.detached-definition",
                    format!("attached use references detached definition {value:?}"),
                    use_origin(body, *use_site, fallback),
                );
                continue;
            };
            if definition_block == use_block {
                if !definition_precedes_use(body, &placement, value, *use_site) {
                    verifier.report(
                        "core.use-before-definition",
                        format!("{value:?} is used before its definition in {use_block:?}"),
                        use_origin(body, *use_site, fallback),
                    );
                }
                continue;
            }
            if dominators.dominates(definition_block, use_block) == Dominance::DoesNotDominate {
                verifier.report(
                    "core.dominance",
                    format!("definition of {value:?} in {definition_block:?} does not dominate use in {use_block:?}"),
                    use_origin(body, *use_site, fallback),
                );
            }
        }
    }
}

fn definition_precedes_use(
    body: &FunctionBody,
    placement: &PlacementIndex<'_>,
    value: ValueId,
    use_site: UseSite,
) -> bool {
    let Some(value) = body.value(value) else {
        return false;
    };
    match value.definition {
        ValueDef::BlockParam { .. } => true,
        ValueDef::InstResult { instruction, .. } => {
            let Some((_, definition_position)) = placement.instruction(instruction) else {
                return false;
            };
            match use_site {
                UseSite::InstructionOperand {
                    instruction: user, ..
                } => placement
                    .instruction(user)
                    .is_some_and(|(_, use_position)| definition_position < use_position),
                UseSite::BranchCondition { .. }
                | UseSite::EdgeArgument { .. }
                | UseSite::Return { .. } => true,
            }
        }
    }
}

fn use_origin(body: &FunctionBody, use_site: UseSite, fallback: OriginId) -> OriginId {
    match use_site {
        UseSite::InstructionOperand { instruction, .. } => body
            .instruction(instruction)
            .map_or(fallback, |data| data.origin),
        UseSite::BranchCondition { block }
        | UseSite::EdgeArgument { block, .. }
        | UseSite::Return { block, .. } => body
            .block(block)
            .and_then(|data| data.terminator.as_ref())
            .map_or(fallback, |terminator| terminator.origin),
    }
}

#[cfg(test)]
mod tests {
    use super::verify_program;
    use crate::entity::EntityVec;
    use crate::ir::core::{
        BlockData, BlockId, BlockTarget, CoreOp, CoreProgram, CoreType, ExternalSemanticBinding,
        FunctionBody, FunctionBuilder, InstData, InstId, TargetFragment, Terminator,
        TerminatorKind, UnsafeMinecraftCommandFragment, ValueId, verify_function,
    };
    use crate::source::{OriginId, SourceContext};

    #[test]
    fn verifies_a_block_argument_diamond() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("choose"),
                vec![CoreType::Bool, CoreType::I32, CoreType::I32],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let parameters = builder.body().block(entry).unwrap().parameters.clone();
        let then_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let else_block = builder.create_block(OriginId::UNKNOWN).unwrap();
        let join = builder.create_block(OriginId::UNKNOWN).unwrap();
        let joined = builder
            .append_block_parameter(join, CoreType::I32, OriginId::UNKNOWN)
            .unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Branch {
                    condition: parameters[0].value,
                    then_target: BlockTarget::new(then_block, vec![]),
                    else_target: BlockTarget::new(else_block, vec![]),
                },
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(then_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![parameters[1].value])),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(else_block).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Jump(BlockTarget::new(join, vec![parameters[2].value])),
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

        let body = builder.finish().unwrap();
        program.define_function(function, body).unwrap();
        verify_program(&program, &sources).unwrap();
    }

    #[test]
    fn malformed_ir_accumulates_independent_diagnostics_without_panicking() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("broken"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut blocks = EntityVec::new();
        let block = blocks
            .push(BlockData {
                origin: OriginId::UNKNOWN,
                parameters: vec![],
                instructions: vec![InstId(0), InstId(0), InstId(99)],
                terminator: None,
            })
            .unwrap();
        let mut instructions = EntityVec::new();
        instructions
            .push(InstData {
                op: CoreOp::BoolNot,
                operands: vec![ValueId(99)],
                results: vec![],
                origin: OriginId::UNKNOWN,
            })
            .unwrap();
        let body = FunctionBody {
            blocks,
            instructions,
            values: EntityVec::new(),
            block_order: vec![block, block, BlockId(99)],
            entry: block,
        };

        let diagnostics = verify_function(&program, &sources, function, &body).unwrap_err();
        assert!(diagnostics.contains_code("core.duplicate-block-placement"));
        assert!(diagnostics.contains_code("core.duplicate-instruction-placement"));
        assert!(diagnostics.contains_code("core.duplicate-instruction-container"));
        assert!(diagnostics.contains_code("core.invalid-instruction"));
        assert!(diagnostics.contains_code("core.invalid-value"));
        assert!(diagnostics.contains_code("core.result-count"));
        assert!(diagnostics.contains_code("core.missing-terminator"));
        assert!(diagnostics.len() >= 6);
    }

    #[test]
    fn raw_instruction_ownership_spans_attached_and_detached_containers() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(
                Some("raw-owner"),
                vec![],
                vec![CoreType::I32],
                OriginId::UNKNOWN,
            )
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let entry = builder.entry_block();
        let value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let instruction = builder.body().block(entry).unwrap().instructions[0];
        let detached = builder.create_block(OriginId::UNKNOWN).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![value]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        builder.switch_to_block(detached).unwrap();
        builder
            .terminate(Terminator::new(
                TerminatorKind::Unreachable,
                OriginId::UNKNOWN,
            ))
            .unwrap();
        let mut body = builder.finish().unwrap();
        body.block_order.retain(|block| *block != detached);
        body.block_mut(detached)
            .unwrap()
            .instructions
            .push(instruction);

        let diagnostics = verify_function(&program, &sources, function, &body).unwrap_err();
        assert!(diagnostics.contains_code("core.duplicate-instruction-container"));
        assert!(!diagnostics.contains_code("core.duplicate-instruction-placement"));
    }

    #[test]
    fn linked_inventory_corruption_is_reported_before_target_lowering() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let fragment = program
            .declare_target_fragment(TargetFragment::unsafe_minecraft_command("say valid").unwrap())
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
            .declare_function(Some("raw"), vec![], vec![], OriginId::UNKNOWN)
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
        verify_program(&program, &sources).unwrap();

        *program.target_fragments.get_mut(fragment).unwrap() =
            TargetFragment::UnsafeMinecraftCommand(UnsafeMinecraftCommandFragment::from_unchecked(
                "say\nbroken",
            ));
        program.external_ops.get_mut(external).unwrap().results = vec![CoreType::Bool];

        let diagnostics = verify_program(&program, &sources).unwrap_err();
        assert!(diagnostics.contains_code("core.invalid-target-fragment"));
        assert!(diagnostics.contains_code("core.invalid-external-declaration"));
        assert!(diagnostics.contains_code("core.invalid-operation"));
    }
}
