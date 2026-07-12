//! Deterministic canonical and malformed-safe Core formatting.

use std::error::Error;
use std::fmt;
use std::fmt::Write;

use super::{
    BlockTarget, CoreOp, CoreProgram, Diagnostics, FunctionBody, FunctionId, TerminatorKind,
    ValueId, verify_program,
};
use crate::entity::EntityId;
use crate::source::SourceContext;

/// Failure to construct a canonical printer for invalid IR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrintError {
    diagnostics: Diagnostics,
}

impl PrintError {
    /// Returns verifier diagnostics preventing canonical output.
    #[must_use]
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }
}

impl fmt::Display for PrintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot print invalid Core IR:\n{}",
            self.diagnostics
        )
    }
}

impl Error for PrintError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.diagnostics)
    }
}

/// Canonical stable output for a fully verified program.
pub struct CanonicalPrinter<'a> {
    program: &'a CoreProgram,
}

impl<'a> CanonicalPrinter<'a> {
    /// Verifies a program before admitting canonical printing.
    ///
    /// # Errors
    ///
    /// Returns all verifier diagnostics when canonical assumptions do not hold.
    pub fn new(program: &'a CoreProgram, sources: &SourceContext) -> Result<Self, PrintError> {
        verify_program(program, sources).map_err(|diagnostics| PrintError { diagnostics })?;
        Ok(Self { program })
    }

    /// Renders byte-stable canonical text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut output = String::new();
        for (index, (function, declaration)) in self.program.functions().enumerate() {
            if index != 0 {
                output.push('\n');
            }
            if let Some(body) = declaration.body() {
                render_function(&mut output, function, declaration, body);
            }
        }
        output
    }
}

impl fmt::Display for CanonicalPrinter<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render())
    }
}

/// Best-effort output that remains available for malformed and detached IR.
pub struct DebugDumper<'a> {
    program: &'a CoreProgram,
}

impl<'a> DebugDumper<'a> {
    /// Creates a dumper without verification.
    #[must_use]
    pub const fn new(program: &'a CoreProgram) -> Self {
        Self { program }
    }

    /// Renders all allocated entities, marking invalid references and detachment.
    #[must_use]
    pub fn render(&self) -> String {
        let mut output = String::new();
        for (function, declaration) in self.program.functions() {
            let _ = writeln!(
                output,
                "function @fn{} {:?}",
                function.index(),
                declaration.name_hint
            );
            let Some(body) = declaration.body() else {
                output.push_str("  <undefined>\n");
                continue;
            };
            let attached_blocks = body.block_order();
            let attached_instructions = attached_blocks
                .iter()
                .filter_map(|block| body.block(*block))
                .flat_map(|block| block.instructions.iter().copied())
                .collect::<Vec<_>>();
            let _ = writeln!(output, "  entry: {}", debug_block(body, body.entry));
            output.push_str("  layout:");
            for block in attached_blocks {
                let _ = write!(output, " {}", debug_block(body, *block));
            }
            output.push('\n');
            output.push_str("  allocated blocks:\n");
            for (block, data) in body.blocks.iter() {
                let detached = if attached_blocks.contains(&block) {
                    ""
                } else {
                    " <detached>"
                };
                let _ = writeln!(
                    output,
                    "    ^bb{}{} origin={:?} params={:?} insts={:?} term={:?}",
                    block.index(),
                    detached,
                    data.origin,
                    data.parameters,
                    data.instructions,
                    data.terminator
                );
            }
            output.push_str("  allocated instructions:\n");
            for (instruction, data) in body.instructions.iter() {
                let detached = if attached_instructions.contains(&instruction) {
                    ""
                } else {
                    " <detached>"
                };
                let operands = data
                    .operands
                    .iter()
                    .map(|value| debug_value(body, *value))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    output,
                    "    !inst{}{} {:?} ({operands}) -> {:?} origin={:?}",
                    instruction.index(),
                    detached,
                    data.op,
                    data.results,
                    data.origin
                );
            }
            output.push_str("  allocated values:\n");
            for (value, data) in body.values.iter() {
                let _ = writeln!(
                    output,
                    "    %v{}: {} = {:?}",
                    value.index(),
                    data.ty,
                    data.definition
                );
            }
        }
        output
    }
}

impl fmt::Display for DebugDumper<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render())
    }
}

fn render_function(
    output: &mut String,
    function: FunctionId,
    declaration: &super::Function,
    body: &FunctionBody,
) {
    let _ = write!(output, "func @fn{}(", function.index());
    let entry = body
        .block(body.entry)
        .expect("verified entry exists for canonical output");
    for (index, (parameter, ty)) in entry
        .parameters
        .iter()
        .zip(&declaration.parameters)
        .enumerate()
    {
        if index != 0 {
            output.push_str(", ");
        }
        let _ = write!(output, "%v{}: {ty}", parameter.value.index());
    }
    output.push(')');
    if !declaration.results.is_empty() {
        output.push_str(" -> (");
        for (index, ty) in declaration.results.iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            let _ = write!(output, "{ty}");
        }
        output.push(')');
    }
    if let Some(hint) = &declaration.name_hint {
        let _ = write!(output, " // {hint}");
    }
    output.push_str(" {\n");

    for block in &body.block_order {
        let data = body
            .block(*block)
            .expect("verified attached block exists for canonical output");
        let _ = write!(output, "^bb{}", block.index());
        if *block != body.entry {
            output.push('(');
            for (index, parameter) in data.parameters.iter().enumerate() {
                if index != 0 {
                    output.push_str(", ");
                }
                let ty = body
                    .value(parameter.value)
                    .expect("verified parameter value exists")
                    .ty;
                let _ = write!(output, "%v{}: {ty}", parameter.value.index());
            }
            output.push(')');
        }
        output.push_str(":\n");
        for instruction in &data.instructions {
            let instruction_data = body
                .instruction(*instruction)
                .expect("verified instruction exists for canonical output");
            output.push_str("  ");
            if !instruction_data.results.is_empty() {
                for (index, result) in instruction_data.results.iter().enumerate() {
                    if index != 0 {
                        output.push_str(", ");
                    }
                    let _ = write!(output, "%v{}", result.index());
                }
                output.push_str(" = ");
            }
            render_operation(output, &instruction_data.op);
            if !instruction_data.operands.is_empty() {
                output.push(' ');
                render_values(output, &instruction_data.operands);
            }
            output.push('\n');
        }
        output.push_str("  ");
        render_terminator(
            output,
            &data
                .terminator
                .as_ref()
                .expect("verified attached block is terminated")
                .kind,
        );
        output.push('\n');
    }
    output.push_str("}\n");
}

fn render_operation(output: &mut String, op: &CoreOp) {
    output.push_str(op.name());
    match op {
        CoreOp::BoolConstant(value) => {
            let _ = write!(output, " {value}");
        }
        CoreOp::I32Constant(value) => {
            let _ = write!(output, " {value}");
        }
        CoreOp::I32Compare(predicate) => {
            let _ = write!(output, " {}", predicate.mnemonic());
        }
        CoreOp::Call(function) => {
            let _ = write!(output, " @fn{}", function.index());
        }
        CoreOp::I32AddWrapping | CoreOp::I32AddOverflowing | CoreOp::BoolNot => {}
    }
}

fn render_terminator(output: &mut String, kind: &TerminatorKind) {
    match kind {
        TerminatorKind::Jump(target) => {
            output.push_str("jump ");
            render_target(output, target);
        }
        TerminatorKind::Branch {
            condition,
            then_target,
            else_target,
        } => {
            let _ = write!(output, "branch %v{}, ", condition.index());
            render_target(output, then_target);
            output.push_str(", ");
            render_target(output, else_target);
        }
        TerminatorKind::Return(values) => {
            output.push_str("return");
            if !values.is_empty() {
                output.push(' ');
                render_values(output, values);
            }
        }
        TerminatorKind::Unreachable => output.push_str("unreachable"),
    }
}

fn render_target(output: &mut String, target: &BlockTarget) {
    let _ = write!(output, "^bb{}", target.block.index());
    if !target.arguments.is_empty() {
        output.push('(');
        render_values(output, &target.arguments);
        output.push(')');
    }
}

fn render_values(output: &mut String, values: &[ValueId]) {
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push_str(", ");
        }
        let _ = write!(output, "%v{}", value.index());
    }
}

fn debug_block(body: &FunctionBody, block: super::BlockId) -> String {
    if body.block(block).is_some() {
        format!("^bb{}", block.index())
    } else {
        format!("<invalid-block ^bb{}>", block.index())
    }
}

fn debug_value(body: &FunctionBody, value: ValueId) -> String {
    if body.value(value).is_some() {
        format!("%v{}", value.index())
    } else {
        format!("<invalid-value %v{}>", value.index())
    }
}
