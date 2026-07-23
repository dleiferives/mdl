//! Deterministic canonical and malformed-safe Core formatting.

use std::error::Error;
use std::fmt;
use std::fmt::Write;

use super::{
    BlockTarget, CoreOp, CoreProgram, Criterion, Diagnostics, ExternalSemanticBinding,
    FunctionBody, FunctionId, MinecraftOperationAttributes, Operand, RunModifierInstance,
    TargetFragment, TerminatorKind, ValueId, verify_program,
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
        render_linked_inventories(&mut output, self.program);
        for (index, (function, declaration)) in self.program.functions().enumerate() {
            if index != 0 || !output.is_empty() {
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
        let mut output = CappedDebugOutput::unbounded();
        self.render_into(&mut output);
        output.finish().text
    }

    /// Renders a deterministic UTF-8 prefix without first retaining the complete dump.
    pub(crate) fn render_bounded(&self, byte_limit: usize) -> BoundedDebugRender {
        let mut output = CappedDebugOutput::bounded(byte_limit);
        self.render_into(&mut output);
        output.finish()
    }

    fn render_into(&self, output: &mut CappedDebugOutput) {
        render_linked_inventories_debug(output, self.program);
        for (function, declaration) in self.program.functions() {
            write_debug_function_header(output, function, declaration);
            let Some(body) = declaration.body() else {
                output.push_str("  <undefined>\n");
                continue;
            };
            let layout = body.block_order();
            let mut attached_blocks = vec![false; body.blocks.len()];
            let mut attached_instructions = vec![false; body.instructions.len()];
            for block in layout {
                if let Some(attached) = usize::try_from(block.index())
                    .ok()
                    .and_then(|index| attached_blocks.get_mut(index))
                {
                    *attached = true;
                }
                if let Some(data) = body.block(*block) {
                    for instruction in &data.instructions {
                        if let Some(attached) = usize::try_from(instruction.index())
                            .ok()
                            .and_then(|index| attached_instructions.get_mut(index))
                        {
                            *attached = true;
                        }
                    }
                }
            }
            output.push_str("  entry: ");
            write_debug_block(output, body, body.entry);
            output.push('\n');
            output.push_str("  layout:");
            for block in layout {
                output.push(' ');
                write_debug_block(output, body, *block);
            }
            output.push('\n');
            output.push_str("  allocated blocks:\n");
            for (block, data) in body.blocks.iter() {
                let detached = if attached_blocks
                    .get(usize::try_from(block.index()).unwrap_or(usize::MAX))
                    .copied()
                    .unwrap_or(false)
                {
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
                let detached = if attached_instructions
                    .get(usize::try_from(instruction.index()).unwrap_or(usize::MAX))
                    .copied()
                    .unwrap_or(false)
                {
                    ""
                } else {
                    " <detached>"
                };
                let _ = write!(
                    output,
                    "    !inst{}{} {:?} (",
                    instruction.index(),
                    detached,
                    data.op,
                );
                for (index, operand) in data.operands.iter().copied().enumerate() {
                    if index != 0 {
                        output.push_str(", ");
                    }
                    write_debug_value(output, body, operand);
                }
                let _ = writeln!(output, ") -> {:?} origin={:?}", data.results, data.origin);
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
    }
}

fn render_linked_inventories_debug(output: &mut CappedDebugOutput, program: &CoreProgram) {
    for (query, data) in program.entity_queries() {
        let _ = writeln!(
            output,
            "query {query:?} {} steps={:?}",
            data.semantic(),
            data.steps()
        );
    }
    for (scope, data) in program.run_scopes() {
        let _ = writeln!(
            output,
            "run scope {scope:?} modifiers={:?} callee={:?} bounds={} origin={:?}",
            data.modifiers(),
            data.callee(),
            data.invocation_bounds(),
            data.origin()
        );
    }
    for (fragment, data) in program.target_fragments() {
        let _ = writeln!(output, "fragment {fragment:?} {data:?}");
    }
    for (operation, data) in program.minecraft_operations() {
        let _ = writeln!(output, "minecraft operation {operation:?} {data:?}");
    }
    for (operation, declaration) in program.external_ops() {
        let _ = writeln!(
            output,
            "external {:?} binding={:?} params={:?} results={:?} origin={:?}",
            operation,
            declaration.binding(),
            declaration.parameters(),
            declaration.results(),
            declaration.origin()
        );
    }
}

/// Internal result used to construct public, typed optimizer failure snapshots.
pub(crate) struct BoundedDebugRender {
    pub(crate) text: String,
    pub(crate) total_bytes: u64,
    pub(crate) total_bytes_saturated: bool,
}

struct CappedDebugOutput {
    text: String,
    byte_limit: usize,
    total_bytes: u64,
    total_bytes_saturated: bool,
    retention_closed: bool,
}

impl CappedDebugOutput {
    fn unbounded() -> Self {
        Self {
            text: String::new(),
            byte_limit: usize::MAX,
            total_bytes: 0,
            total_bytes_saturated: false,
            retention_closed: false,
        }
    }

    fn bounded(byte_limit: usize) -> Self {
        Self {
            text: String::with_capacity(byte_limit.min(4_096)),
            byte_limit,
            total_bytes: 0,
            total_bytes_saturated: false,
            retention_closed: false,
        }
    }

    fn push_str(&mut self, value: &str) {
        let _ = self.write_str(value);
    }

    fn push(&mut self, value: char) {
        let mut encoded = [0_u8; 4];
        self.push_str(value.encode_utf8(&mut encoded));
    }

    fn finish(self) -> BoundedDebugRender {
        BoundedDebugRender {
            text: self.text,
            total_bytes: self.total_bytes,
            total_bytes_saturated: self.total_bytes_saturated,
        }
    }
}

impl fmt::Write for CappedDebugOutput {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if let Some(total) = u64::try_from(value.len())
            .ok()
            .and_then(|length| self.total_bytes.checked_add(length))
        {
            self.total_bytes = total;
        } else {
            self.total_bytes = u64::MAX;
            self.total_bytes_saturated = true;
        }

        if self.retention_closed {
            return Ok(());
        }
        let available = self.byte_limit.saturating_sub(self.text.len());
        let mut end = available.min(value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&value[..end]);
        if end != value.len() {
            self.retention_closed = true;
        }
        Ok(())
    }
}

fn write_debug_function_header(
    output: &mut CappedDebugOutput,
    function: FunctionId,
    declaration: &super::Function,
) {
    let _ = writeln!(
        output,
        "function @fn{} {:?} linkage={}",
        function.index(),
        declaration.name_hint,
        declaration.linkage,
    );
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
    if declaration.linkage == super::CoreFunctionLinkage::DatapackExport {
        output.push_str("export ");
    }
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
        CoreOp::I32InClosedRange(range) => {
            let _ = write!(output, " {}...{}", (*range).min(), (*range).max());
        }
        CoreOp::StringConstant(value) => {
            let _ = write!(output, " {value:?}");
        }
        CoreOp::StringEndsWithAscii(value) => {
            let _ = write!(output, " {value}");
        }
        CoreOp::Call(function) => {
            let _ = write!(output, " @fn{}", function.index());
        }
        CoreOp::External(operation) => {
            let _ = write!(output, " @ext{}", operation.index());
        }
        CoreOp::I32AddWrapping
        | CoreOp::I32SubWrapping
        | CoreOp::I32AddOverflowing
        | CoreOp::BoolNot
        | CoreOp::ListI32Empty
        | CoreOp::ListI32Length
        | CoreOp::ListI32Push
        | CoreOp::ListI32LastOrZero
        | CoreOp::ListI32WithoutLast
        | CoreOp::StringLength
        | CoreOp::StringWithoutLastUnit => {}
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "canonical inventory rendering is intentionally exhaustive and centralized"
)]
fn render_linked_inventories(output: &mut String, program: &CoreProgram) {
    for (query, data) in program.entity_queries() {
        let _ = writeln!(output, "query @query{} {}", query.index(), data.semantic());
    }
    for (scope, data) in program.run_scopes() {
        let _ = write!(output, "run_scope @run{} [", scope.index());
        for (index, modifier) in data.modifiers().iter().cloned().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            match modifier {
                RunModifierInstance::AsEntityQuery { query, .. } => {
                    let _ = write!(output, "as @query{}", query.index());
                }
                RunModifierInstance::AtEntityQuery { query, .. } => {
                    let _ = write!(output, "at @query{}", query.index());
                }
                RunModifierInstance::AtExecutor { kind, .. } => {
                    let _ = write!(output, "at_executor {kind}");
                }
                RunModifierInstance::Positioned { position, .. } => {
                    let _ = write!(output, "positioned {position:?}");
                }
                RunModifierInstance::Rotated { rotation, .. } => {
                    let _ = write!(output, "rotated {rotation:?}");
                }
                RunModifierInstance::In { dimension, .. } => {
                    let _ = write!(output, "in {dimension:?}");
                }
                RunModifierInstance::Anchored { anchor, .. } => {
                    let _ = write!(output, "anchored {anchor:?}");
                }
                RunModifierInstance::Align { axes, .. } => {
                    let _ = write!(output, "align {}", axes.as_str());
                }
            }
        }
        let _ = writeln!(
            output,
            "] invoke @fn{} bounds {}",
            data.callee().index(),
            data.invocation_bounds()
        );
    }
    for (fragment, data) in program.target_fragments() {
        match data {
            TargetFragment::UnsafeMinecraftCommand(command) => {
                let _ = writeln!(
                    output,
                    "fragment @frag{} unsafe.minecraft {:?}",
                    fragment.index(),
                    command.as_str()
                );
            }
        }
    }
    for (operation, data) in program.minecraft_operations() {
        let _ = write!(
            output,
            "minecraft_op @mc{} {:?} receiver={} ",
            operation.index(),
            data.key(),
            data.receiver_kind()
        );
        match data.attributes() {
            MinecraftOperationAttributes::Say {
                message,
                message_origin: _,
            } => {
                let _ = writeln!(output, "message={:?}", message.as_str());
            }
            MinecraftOperationAttributes::Teleport { position, .. } => {
                let _ = writeln!(output, "position={position:?}");
            }
            MinecraftOperationAttributes::MoveBy { offset, .. } => {
                let _ = writeln!(output, "offset={offset:?}");
            }
        }
    }
    for (read, data) in program.entity_nbt_reads() {
        let _ = write!(
            output,
            "entity_nbt_read @nbt{} receiver={} result={} path=",
            read.index(),
            data.receiver(),
            data.result_ty()
        );
        for segment in data.segments() {
            match segment {
                super::EntityNbtPathSegment::Key(key) => {
                    let _ = write!(output, ".{key}");
                }
                super::EntityNbtPathSegment::Index(Operand::Const(n)) => {
                    let _ = write!(output, "[{n}]");
                }
                super::EntityNbtPathSegment::Index(Operand::Runtime(v)) => {
                    let _ = write!(output, "[@{}]", v.index());
                }
                super::EntityNbtPathSegment::Match {
                    match_key,
                    value: Operand::Const(n),
                } => {
                    let _ = write!(output, "[{{{match_key}:{n}}}]");
                }
                super::EntityNbtPathSegment::Match {
                    match_key,
                    value: Operand::Runtime(v),
                } => {
                    let _ = write!(output, "[{{{match_key}:@{}}}]", v.index());
                }
            }
        }
        let _ = writeln!(output);
    }
    for (operation, declaration) in program.external_ops() {
        let _ = write!(output, "external @ext{} ", operation.index());
        match declaration.binding() {
            ExternalSemanticBinding::UnsafeTargetFragment(fragment) => {
                let _ = write!(output, "unsafe.fragment @frag{}", fragment.index());
            }
            ExternalSemanticBinding::MinecraftRunScope(scope) => {
                let _ = write!(output, "minecraft.run_scope @run{}", scope.index());
            }
            ExternalSemanticBinding::MinecraftOperation(semantic) => {
                let _ = write!(output, "minecraft.operation @mc{}", semantic.index());
            }
            ExternalSemanticBinding::EntityNbtRead(read) => {
                let _ = write!(output, "minecraft.entity_nbt_read @nbt{}", read.index());
            }
        }
        output.push_str(" : (");
        for (index, ty) in declaration.parameters().iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            let _ = write!(output, "{ty}");
        }
        output.push_str(") -> (");
        for (index, ty) in declaration.results().iter().enumerate() {
            if index != 0 {
                output.push_str(", ");
            }
            let _ = write!(output, "{ty}");
        }
        output.push_str(")\n");
    }
    for (advancement, declaration) in program.advancements() {
        let _ = write!(
            output,
            "advancement @adv{} reward=@fn{} ",
            advancement.index(),
            declaration.reward().index()
        );
        match declaration.criterion() {
            Criterion::InventoryChanged { items } => {
                let _ = write!(output, "inventory_changed items=[");
                for (index, item) in items.iter().enumerate() {
                    if index != 0 {
                        output.push_str(", ");
                    }
                    let _ = write!(output, "{:?}", item.as_str());
                }
                output.push(']');
            }
        }
        let _ = writeln!(output);
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

fn write_debug_block(output: &mut CappedDebugOutput, body: &FunctionBody, block: super::BlockId) {
    if body.block(block).is_some() {
        let _ = write!(output, "^bb{}", block.index());
    } else {
        let _ = write!(output, "<invalid-block ^bb{}>", block.index());
    }
}

fn write_debug_value(output: &mut CappedDebugOutput, body: &FunctionBody, value: ValueId) {
    if body.value(value).is_some() {
        let _ = write!(output, "%v{}", value.index());
    } else {
        let _ = write!(output, "<invalid-value %v{}>", value.index());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::EntityId;
    use crate::ir::core::{BlockId, FunctionBuilder, InstId, Terminator, ValueDef};
    use crate::source::OriginId;

    #[test]
    fn linked_inventories_have_stable_canonical_and_debug_forms() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let fragment = program
            .declare_target_fragment(TargetFragment::unsafe_minecraft_command("say π").unwrap())
            .unwrap();
        program
            .declare_external_op(
                ExternalSemanticBinding::UnsafeTargetFragment(fragment),
                vec![],
                vec![],
                OriginId::UNKNOWN,
            )
            .unwrap();

        assert_eq!(
            CanonicalPrinter::new(&program, &sources).unwrap().render(),
            "fragment @frag0 unsafe.minecraft \"say π\"\nexternal @ext0 unsafe.fragment @frag0 : () -> ()\n"
        );
        let debug = DebugDumper::new(&program).render();
        assert!(debug.contains("fragment TargetFragmentId(0)"));
        assert!(debug.contains("external ExternalOpId(0)"));
    }

    #[test]
    fn bounded_dump_is_prefix_exact_for_dangling_ids() {
        let sources = SourceContext::new();
        let mut program = CoreProgram::new();
        let function = program
            .declare_function(Some("malformed_π_🦀"), vec![], vec![], OriginId::UNKNOWN)
            .unwrap();
        let mut builder = FunctionBuilder::new(&program, &sources, function).unwrap();
        let value = builder.i32_constant(1, OriginId::UNKNOWN).unwrap();
        let ValueDef::InstResult { instruction, .. } =
            builder.body().value(value).unwrap().definition()
        else {
            panic!("constant must be an instruction result");
        };
        builder
            .terminate(Terminator::new(
                TerminatorKind::Return(vec![]),
                OriginId::UNKNOWN,
            ))
            .unwrap();
        program
            .define_function(function, builder.finish().unwrap())
            .unwrap();

        let mut body = program.take_function_body(function).unwrap();
        let entry = body.entry;
        body.entry = BlockId::from_index(u32::MAX);
        body.block_order.push(BlockId::from_index(u32::MAX - 1));
        body.block_mut(entry)
            .unwrap()
            .instructions
            .push(InstId::from_index(u32::MAX));
        body.instruction_mut(instruction)
            .unwrap()
            .operands
            .push(ValueId::from_index(u32::MAX));
        program.restore_function_body(function, body);

        let complete = DebugDumper::new(&program).render();
        assert!(complete.contains("<invalid-block"));
        assert!(complete.contains("<invalid-value"));
        for cap in 0..=complete.len() {
            let bounded = DebugDumper::new(&program).render_bounded(cap);
            let mut expected_end = cap;
            while !complete.is_char_boundary(expected_end) {
                expected_end -= 1;
            }
            assert_eq!(bounded.text, complete[..expected_end]);
            assert_eq!(bounded.total_bytes, u64::try_from(complete.len()).unwrap());
            assert!(!bounded.total_bytes_saturated);
        }
    }

    #[test]
    fn capped_writer_saturates_total_byte_metadata() {
        let mut output = CappedDebugOutput::bounded(1);
        output.total_bytes = u64::MAX - 1;
        output.push_str("abc");
        let rendered = output.finish();

        assert_eq!(rendered.text, "a");
        assert_eq!(rendered.total_bytes, u64::MAX);
        assert!(rendered.total_bytes_saturated);
    }
}
