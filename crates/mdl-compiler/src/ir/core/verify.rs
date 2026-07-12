//! Layered, panic-free Core verification.

use std::error::Error;
use std::fmt;

use super::analysis::definition_block;
use super::{
    BlockId, CoreProgram, CoreType, Dominance, DominatorTree, FunctionBody, FunctionId,
    PlacementIndex, TerminatorKind, UseIndex, UseSite, ValueDef, ValueId,
};
use crate::entity::EntityId;
use crate::source::{OriginId, SourceContext};

/// One independently actionable verifier finding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: &'static str,
    message: String,
    origin: OriginId,
}

impl Diagnostic {
    /// Returns the stable diagnostic code.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Returns the human-readable diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the best available provenance for the finding.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} ({:?})",
            self.code, self.message, self.origin
        )
    }
}

/// Accumulated verifier diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostics {
    findings: Vec<Diagnostic>,
}

impl Diagnostics {
    /// Returns all findings in deterministic verifier order.
    #[must_use]
    pub fn findings(&self) -> &[Diagnostic] {
        &self.findings
    }

    /// Returns the number of findings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.findings.len()
    }

    /// Returns whether no findings are present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    /// Returns whether at least one finding has the given stable code.
    #[must_use]
    pub fn contains_code(&self, code: &str) -> bool {
        self.findings.iter().any(|finding| finding.code == code)
    }
}

impl fmt::Display for Diagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, finding) in self.findings.iter().enumerate() {
            if index != 0 {
                formatter.write_str("\n")?;
            }
            write!(formatter, "{finding}")?;
        }
        Ok(())
    }
}

impl Error for Diagnostics {}

#[derive(Default)]
struct Verifier {
    findings: Vec<Diagnostic>,
}

impl Verifier {
    fn report(&mut self, code: &'static str, message: impl Into<String>, origin: OriginId) {
        self.findings.push(Diagnostic {
            code,
            message: message.into(),
            origin,
        });
    }

    fn finish(self) -> Result<(), Diagnostics> {
        if self.findings.is_empty() {
            Ok(())
        } else {
            Err(Diagnostics {
                findings: self.findings,
            })
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
    let mut findings = vec![];
    for (function, declaration) in program.functions() {
        if sources.origin(declaration.origin).is_none() {
            findings.push(Diagnostic {
                code: "core.invalid-origin",
                message: format!(
                    "function {function:?} has invalid origin {:?}",
                    declaration.origin
                ),
                origin: OriginId::UNKNOWN,
            });
        }
        let Some(body) = declaration.body() else {
            findings.push(Diagnostic {
                code: "core.undefined-function",
                message: format!("internal function {function:?} has no definition"),
                origin: declaration.origin,
            });
            continue;
        };
        if let Err(diagnostics) = verify_function(program, sources, function, body) {
            findings.extend(diagnostics.findings);
        }
    }
    if findings.is_empty() {
        Ok(())
    } else {
        Err(Diagnostics { findings })
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
        BlockData, BlockId, BlockTarget, CoreOp, CoreProgram, CoreType, FunctionBody,
        FunctionBuilder, InstData, InstId, Terminator, TerminatorKind, ValueId, verify_function,
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
        assert!(diagnostics.contains_code("core.invalid-instruction"));
        assert!(diagnostics.contains_code("core.invalid-value"));
        assert!(diagnostics.contains_code("core.result-count"));
        assert!(diagnostics.contains_code("core.missing-terminator"));
        assert!(diagnostics.len() >= 6);
    }
}
